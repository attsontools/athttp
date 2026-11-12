use std::{io, path::PathBuf, sync::Arc};

use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::Notify,
    time::{timeout, Duration},
};
use uuid::Uuid;

use crate::{
    http::{self, HttpService},
    negotiate, write_endpoint, Endpoint, ErrorEnvelope, ServiceError, ServiceHandshake,
    ServiceReply, ServiceRequest,
};

const MAX_FRAME_BYTES: usize = 1024 * 1024;

pub struct ServiceServer {
    listener: TcpListener,
    endpoint_path: PathBuf,
    data_directory: String,
    endpoint: Endpoint,
    stop: Arc<Notify>,
    service: Arc<HttpService>,
}

impl ServiceServer {
    pub async fn bind(
        endpoint_path: PathBuf,
        data_directory: impl Into<String>,
        service_version: impl Into<String>,
    ) -> Result<Self, ServiceError> {
        let data_directory = data_directory.into();
        let service = HttpService::new(PathBuf::from(&data_directory))
            .map_err(|error| ServiceError::new("HTTP_DATA_MIGRATION_FAILED", error))?;
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(|error| {
            ServiceError::new(
                "HTTP_SERVICE_BIND_FAILED",
                format!("cannot bind local service endpoint: {error}"),
            )
        })?;
        let address = listener.local_addr().map_err(|error| {
            ServiceError::new(
                "HTTP_SERVICE_BIND_FAILED",
                format!("cannot read local service endpoint: {error}"),
            )
        })?;
        let endpoint = Endpoint {
            port: address.port(),
            token: Uuid::new_v4().to_string(),
            service_version: service_version.into(),
        };
        write_endpoint(&endpoint_path, &endpoint)
            .map_err(|error| ServiceError::new(error.code, error.message))?;
        Ok(Self {
            listener,
            endpoint_path,
            data_directory,
            endpoint,
            stop: Arc::new(Notify::new()),
            service: Arc::new(service),
        })
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    pub fn stop_handle(&self) -> Arc<Notify> {
        Arc::clone(&self.stop)
    }

    pub async fn run(self) -> Result<(), ServiceError> {
        loop {
            tokio::select! {
                result = self.listener.accept() => {
                    let (stream, _) = result.map_err(|error| ServiceError::new(
                        "HTTP_SERVICE_ACCEPT_FAILED",
                        format!("cannot accept local service client: {error}"),
                    ))?;
                    let token = self.endpoint.token.clone();
                    let data_directory = self.data_directory.clone();
                    let service = Arc::clone(&self.service);
                    tokio::spawn(async move {
                        if let Err(error) = handle_connection(stream, &token, &data_directory, service).await {
                            eprintln!("[athttp-service] client connection failed: {error}");
                        }
                    });
                }
                _ = self.stop.notified() => break,
            }
        }
        match std::fs::remove_file(&self.endpoint_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(ServiceError::new(
                "HTTP_SERVICE_ENDPOINT_CLEANUP_FAILED",
                format!("cannot remove service endpoint: {error}"),
            )),
        }
    }
}

async fn handle_connection(
    stream: TcpStream,
    token: &str,
    data_directory: &str,
    service: Arc<HttpService>,
) -> io::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut frame = Vec::new();
    let read = read_frame(&mut reader, &mut frame, "handshake timed out").await?;
    if read == 0 || frame.len() > MAX_FRAME_BYTES || frame.last() != Some(&b'\n') {
        return Ok(());
    }
    let reply = match serde_json::from_slice::<ServiceHandshake>(&frame) {
        Ok(ServiceHandshake::Request(request)) => {
            match negotiate(&request, token, data_directory) {
                Ok(response) => ServiceReply::Handshake(response),
                Err(error) => ServiceReply::Error {
                    request_id: request.request_id,
                    error: ErrorEnvelope::from(error),
                },
            }
        }
        _ => ServiceReply::Error {
            request_id: String::new(),
            error: ErrorEnvelope::from(ServiceError::new(
                "HTTP_SERVICE_REQUEST_INVALID",
                "expected a handshake request",
            )),
        },
    };
    let mut bytes = serde_json::to_vec(&reply).map_err(io::Error::other)?;
    bytes.push(b'\n');
    write_half.write_all(&bytes).await?;
    if !matches!(reply, ServiceReply::Handshake(_)) {
        return Ok(());
    }

    loop {
        frame.clear();
        let read = read_frame(&mut reader, &mut frame, "request timed out").await?;
        if read == 0 {
            return Ok(());
        }
        if frame.len() > MAX_FRAME_BYTES || frame.last() != Some(&b'\n') {
            return Ok(());
        }
        let request = match serde_json::from_slice::<ServiceRequest>(&frame) {
            Ok(ServiceRequest::Request {
                request_id,
                method,
                params,
            }) => (request_id, method, params),
            Err(_) => {
                let error = ServiceReply::Error {
                    request_id: String::new(),
                    error: ErrorEnvelope::from(ServiceError::new(
                        "HTTP_SERVICE_REQUEST_INVALID",
                        "expected a service request",
                    )),
                };
                write_reply(&mut write_half, error).await?;
                continue;
            }
        };
        let (request_id, method, params) = request;
        let reply = dispatch_request(&service, &method, params, &request_id).await;
        write_reply(&mut write_half, reply).await?;
    }
}

async fn read_frame(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    frame: &mut Vec<u8>,
    timeout_message: &'static str,
) -> io::Result<usize> {
    let mut limited = reader.take((MAX_FRAME_BYTES + 1) as u64);
    timeout(Duration::from_secs(10), limited.read_until(b'\n', frame))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, timeout_message))?
}

async fn dispatch_request(
    service: &HttpService,
    method: &str,
    params: serde_json::Value,
    request_id: &str,
) -> ServiceReply {
    let result = match method {
        "http.send" => {
            let params = match serde_json::from_value::<http::HttpSendParams>(params) {
                Ok(params) => params,
                Err(error) => {
                    return ServiceReply::Error {
                        request_id: request_id.into(),
                        error: ErrorEnvelope::from(ServiceError::new(
                            "HTTP_SERVICE_REQUEST_INVALID",
                            format!("invalid http.send params: {error}"),
                        )),
                    }
                }
            };
            http::send(
                params.request,
                params.cancel_token_id,
                Some(service.cancel_state()),
            )
            .await
            .and_then(|response| serde_json::to_value(response).map_err(|error| error.to_string()))
            .map_err(|error| {
                let code = if error == "请求已取消" {
                    "HTTP_REQUEST_CANCELLED"
                } else {
                    "HTTP_REQUEST_FAILED"
                };
                ServiceError::new(code, error)
            })
        }
        "http.cancel" => {
            let token_id = params
                .get("cancelTokenId")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if token_id.is_empty() {
                return ServiceReply::Error {
                    request_id: request_id.into(),
                    error: ErrorEnvelope::from(ServiceError::new(
                        "HTTP_SERVICE_REQUEST_INVALID",
                        "cancelTokenId is required",
                    )),
                };
            }
            Ok(serde_json::json!({
                "cancelled": service.cancel_state().cancel(token_id)
            }))
        }
        "data.tabs.list" => service
            .store()
            .list_tabs()
            .and_then(|rows| serde_json::to_value(rows).map_err(|error| error.to_string()))
            .map_err(|error| ServiceError::new("HTTP_DATA_READ_FAILED", error)),
        "data.tabs.upsert" => {
            let row = params
                .get("row")
                .cloned()
                .ok_or_else(|| "row is required".to_string())
                .and_then(|value| {
                    serde_json::from_value::<http::HttpTabRow>(value)
                        .map_err(|error| format!("invalid tab row: {error}"))
                });
            match row {
                Ok(row) => service
                    .store()
                    .upsert_tab(row)
                    .map(|()| serde_json::Value::Null)
                    .map_err(|error| ServiceError::new("HTTP_DATA_WRITE_FAILED", error)),
                Err(error) => Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error)),
            }
        }
        "data.tabs.delete" => match required_string(&params, "id") {
            Ok(id) => service
                .store()
                .delete_tab(id)
                .map(|()| serde_json::Value::Null)
                .map_err(|error| ServiceError::new("HTTP_DATA_WRITE_FAILED", error)),
            Err(error) => Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error)),
        },
        "data.tabs.setActive" => match required_string(&params, "id") {
            Ok(id) => service
                .store()
                .set_active_tab(id)
                .map(|()| serde_json::Value::Null)
                .map_err(|error| ServiceError::new("HTTP_DATA_WRITE_FAILED", error)),
            Err(error) => Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error)),
        },
        "data.history.list" => {
            let limit = params
                .get("limit")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(500)
                .min(u32::MAX as u64) as u32;
            service
                .store()
                .list_history(limit)
                .and_then(|rows| serde_json::to_value(rows).map_err(|error| error.to_string()))
                .map_err(|error| ServiceError::new("HTTP_DATA_READ_FAILED", error))
        }
        "data.history.insert" => match required_row::<http::HttpHistoryRow>(&params) {
            Ok(row) => service
                .store()
                .insert_history(row)
                .map(|()| serde_json::Value::Null)
                .map_err(|error| ServiceError::new("HTTP_DATA_WRITE_FAILED", error)),
            Err(error) => Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error)),
        },
        "data.history.delete" => match required_string(&params, "id") {
            Ok(id) => service
                .store()
                .delete_history(id)
                .map(|()| serde_json::Value::Null)
                .map_err(|error| ServiceError::new("HTTP_DATA_WRITE_FAILED", error)),
            Err(error) => Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error)),
        },
        "data.history.clear" => service
            .store()
            .clear_history()
            .map(|()| serde_json::Value::Null)
            .map_err(|error| ServiceError::new("HTTP_DATA_WRITE_FAILED", error)),
        _ => Err(ServiceError::new(
            "HTTP_SERVICE_METHOD_NOT_FOUND",
            format!("unknown service method: {method}"),
        )),
    };
    match result {
        Ok(value) => ServiceReply::Response {
            request_id: request_id.into(),
            result: serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
        },
        Err(error) => ServiceReply::Error {
            request_id: request_id.into(),
            error: ErrorEnvelope::from(error),
        },
    }
}

fn required_string<'a>(params: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{key} is required"))
}

fn required_row<T: serde::de::DeserializeOwned>(params: &serde_json::Value) -> Result<T, String> {
    let value = params
        .get("row")
        .cloned()
        .ok_or_else(|| "row is required".to_string())?;
    serde_json::from_value(value).map_err(|error| format!("invalid row: {error}"))
}

async fn write_reply(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    reply: ServiceReply,
) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(&reply).map_err(io::Error::other)?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read_endpoint, HandshakeRequest};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    async fn connect_and_handshake(
        endpoint: &Endpoint,
        data_directory: &str,
        client_id: &str,
    ) -> BufReader<TcpStream> {
        let mut stream = TcpStream::connect(("127.0.0.1", endpoint.port))
            .await
            .unwrap();
        let handshake = ServiceHandshake::request(HandshakeRequest {
            protocol_version: 1,
            app_id: "com.attson.http".into(),
            client_id: client_id.into(),
            request_id: format!("handshake-{client_id}"),
            host_api: "2.0.0".into(),
            data_api: "1.0.0".into(),
            service_api: "1.0.0".into(),
            data_directory: data_directory.into(),
            token: endpoint.token.clone(),
        });
        let mut bytes = serde_json::to_vec(&handshake).unwrap();
        bytes.push(b'\n');
        stream.write_all(&bytes).await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        reader.read_until(b'\n', &mut line).await.unwrap();
        let reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(reply["kind"], "handshake");
        reader
    }

    #[tokio::test]
    async fn binds_loopback_and_answers_handshake() {
        let directory = tempfile::tempdir().unwrap();
        let endpoint_path = directory.path().join("service.endpoint.json");
        let server = ServiceServer::bind(
            endpoint_path.clone(),
            directory.path().display().to_string(),
            "0.1.0-alpha.0",
        )
        .await
        .unwrap();
        let endpoint = server.endpoint().clone();
        let stop = server.stop_handle();
        let task = tokio::spawn(server.run());

        let mut stream = TcpStream::connect(("127.0.0.1", endpoint.port))
            .await
            .unwrap();
        let request = ServiceHandshake::request(HandshakeRequest {
            protocol_version: 1,
            app_id: "com.attson.http".into(),
            client_id: "test-client".into(),
            request_id: "req-1".into(),
            host_api: "2.0.0".into(),
            data_api: "1.0.0".into(),
            service_api: "1.0.0".into(),
            data_directory: directory.path().display().to_string(),
            token: endpoint.token.clone(),
        });
        let mut bytes = serde_json::to_vec(&request).unwrap();
        bytes.push(b'\n');
        stream.write_all(&bytes).await.unwrap();
        let mut line = Vec::new();
        BufReader::new(stream)
            .read_until(b'\n', &mut line)
            .await
            .unwrap();
        let reply: ServiceReply = serde_json::from_slice(&line).unwrap();
        assert!(
            matches!(reply, ServiceReply::Handshake(response) if response.request_id == "req-1")
        );

        stop.notify_one();
        task.await.unwrap().unwrap();
        assert!(read_endpoint(&endpoint_path).is_err());
    }

    #[tokio::test]
    async fn dispatches_http_send_after_handshake() {
        let upstream = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move {
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut request = Vec::new();
            BufReader::new(&mut stream)
                .read_until(b'\n', &mut request)
                .await
                .unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nX-Test: ok\r\n\r\nhi")
                .await
                .unwrap();
        });

        let directory = tempfile::tempdir().unwrap();
        let endpoint_path = directory.path().join("service.endpoint.json");
        let server = ServiceServer::bind(
            endpoint_path,
            directory.path().display().to_string(),
            "0.1.0-alpha.0",
        )
        .await
        .unwrap();
        let endpoint = server.endpoint().clone();
        let stop = server.stop_handle();
        let task = tokio::spawn(server.run());

        let mut stream = TcpStream::connect(("127.0.0.1", endpoint.port))
            .await
            .unwrap();
        let handshake = ServiceHandshake::request(HandshakeRequest {
            protocol_version: 1,
            app_id: "com.attson.http".into(),
            client_id: "test-client".into(),
            request_id: "handshake-1".into(),
            host_api: "2.0.0".into(),
            data_api: "1.0.0".into(),
            service_api: "1.0.0".into(),
            data_directory: directory.path().display().to_string(),
            token: endpoint.token.clone(),
        });
        let mut handshake_bytes = serde_json::to_vec(&handshake).unwrap();
        handshake_bytes.push(b'\n');
        stream.write_all(&handshake_bytes).await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        reader.read_until(b'\n', &mut line).await.unwrap();
        let handshake_reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(handshake_reply["kind"], "handshake");

        let request = serde_json::json!({
            "kind": "request",
            "requestId": "http-1",
            "method": "http.send",
            "params": {
                "request": {
                    "method": "GET",
                    "url": format!("http://{upstream_address}/hello"),
                    "timeoutSeconds": 5
                },
                "cancelTokenId": "cancel-http-1"
            }
        });
        let mut request_bytes = serde_json::to_vec(&request).unwrap();
        request_bytes.push(b'\n');
        reader.get_mut().write_all(&request_bytes).await.unwrap();
        line.clear();
        reader.read_until(b'\n', &mut line).await.unwrap();
        let reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(reply["kind"], "response", "reply: {reply}");
        assert_eq!(reply["requestId"], "http-1");
        assert_eq!(reply["result"]["status"], 200);
        assert_eq!(reply["result"]["body"], "hi");

        stop.notify_one();
        task.await.unwrap().unwrap();
        upstream_task.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_business_request_before_handshake() {
        let directory = tempfile::tempdir().unwrap();
        let server = ServiceServer::bind(
            directory.path().join("service.endpoint.json"),
            directory.path().display().to_string(),
            "0.1.0-alpha.0",
        )
        .await
        .unwrap();
        let endpoint = server.endpoint().clone();
        let stop = server.stop_handle();
        let task = tokio::spawn(server.run());

        let mut stream = TcpStream::connect(("127.0.0.1", endpoint.port))
            .await
            .unwrap();
        stream
            .write_all(
                b"{\"kind\":\"request\",\"requestId\":\"http-1\",\"method\":\"http.send\",\"params\":{}}\n",
            )
            .await
            .unwrap();
        let mut line = Vec::new();
        BufReader::new(stream)
            .read_until(b'\n', &mut line)
            .await
            .unwrap();
        let reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(reply["kind"], "error");
        assert_eq!(reply["error"]["code"], "HTTP_SERVICE_REQUEST_INVALID");

        stop.notify_one();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn cancels_an_in_flight_http_request_from_another_client() {
        let upstream = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move {
            let (_stream, _) = upstream.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(2)).await;
        });
        let directory = tempfile::tempdir().unwrap();
        let data_directory = directory.path().display().to_string();
        let server = ServiceServer::bind(
            directory.path().join("service.endpoint.json"),
            data_directory.clone(),
            "0.1.0-alpha.0",
        )
        .await
        .unwrap();
        let endpoint = server.endpoint().clone();
        let stop = server.stop_handle();
        let task = tokio::spawn(server.run());
        let mut sender = connect_and_handshake(&endpoint, &data_directory, "sender").await;
        let mut canceller = connect_and_handshake(&endpoint, &data_directory, "canceller").await;

        let send_request = serde_json::json!({
            "kind": "request",
            "requestId": "http-slow",
            "method": "http.send",
            "params": {
                "request": {
                    "method": "GET",
                    "url": format!("http://{upstream_address}/slow"),
                    "timeoutSeconds": 5
                },
                "cancelTokenId": "cancel-slow"
            }
        });
        let mut bytes = serde_json::to_vec(&send_request).unwrap();
        bytes.push(b'\n');
        sender.get_mut().write_all(&bytes).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        let cancel_request = serde_json::json!({
            "kind": "request",
            "requestId": "cancel-1",
            "method": "http.cancel",
            "params": { "cancelTokenId": "cancel-slow" }
        });
        bytes = serde_json::to_vec(&cancel_request).unwrap();
        bytes.push(b'\n');
        canceller.get_mut().write_all(&bytes).await.unwrap();
        let mut line = Vec::new();
        canceller.read_until(b'\n', &mut line).await.unwrap();
        let cancel_reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(cancel_reply["result"]["cancelled"], true);

        line.clear();
        sender.read_until(b'\n', &mut line).await.unwrap();
        let send_reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(send_reply["kind"], "error");
        assert_eq!(send_reply["requestId"], "http-slow");
        assert_eq!(send_reply["error"]["code"], "HTTP_REQUEST_CANCELLED");

        stop.notify_one();
        task.await.unwrap().unwrap();
        upstream_task.abort();
    }

    #[tokio::test]
    async fn shares_tabs_between_two_service_clients() {
        let directory = tempfile::tempdir().unwrap();
        let data_directory = directory.path().display().to_string();
        let server = ServiceServer::bind(
            directory.path().join("service.endpoint.json"),
            data_directory.clone(),
            "0.1.0-alpha.0",
        )
        .await
        .unwrap();
        let endpoint = server.endpoint().clone();
        let stop = server.stop_handle();
        let task = tokio::spawn(server.run());
        let mut writer = connect_and_handshake(&endpoint, &data_directory, "writer").await;
        let mut reader = connect_and_handshake(&endpoint, &data_directory, "reader").await;

        let upsert = serde_json::json!({
            "kind": "request",
            "requestId": "tab-upsert-1",
            "method": "data.tabs.upsert",
            "params": {
                "row": {
                    "id": "tab-1",
                    "title": "Shared request",
                    "orderIndex": 0,
                    "isActive": true,
                    "specJson": "{}",
                    "updatedAt": 1_797_000_000_000_i64,
                    "kind": "http"
                }
            }
        });
        let mut bytes = serde_json::to_vec(&upsert).unwrap();
        bytes.push(b'\n');
        writer.get_mut().write_all(&bytes).await.unwrap();
        let mut line = Vec::new();
        writer.read_until(b'\n', &mut line).await.unwrap();
        let upsert_reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(upsert_reply["kind"], "response", "{upsert_reply}");

        let list = serde_json::json!({
            "kind": "request",
            "requestId": "tab-list-1",
            "method": "data.tabs.list",
            "params": {}
        });
        bytes = serde_json::to_vec(&list).unwrap();
        bytes.push(b'\n');
        reader.get_mut().write_all(&bytes).await.unwrap();
        line.clear();
        reader.read_until(b'\n', &mut line).await.unwrap();
        let list_reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(list_reply["kind"], "response", "{list_reply}");
        assert_eq!(list_reply["result"][0]["id"], "tab-1");
        assert_eq!(list_reply["result"][0]["title"], "Shared request");

        stop.notify_one();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn activates_and_deletes_tabs_through_data_api() {
        let directory = tempfile::tempdir().unwrap();
        let data_directory = directory.path().display().to_string();
        let server = ServiceServer::bind(
            directory.path().join("service.endpoint.json"),
            data_directory.clone(),
            "0.1.0-alpha.0",
        )
        .await
        .unwrap();
        server
            .service
            .store()
            .upsert_tab(http::HttpTabRow {
                id: "tab-1".into(),
                title: "One".into(),
                order_index: 0,
                is_active: true,
                spec_json: "{}".into(),
                updated_at: 1,
                kind: "http".into(),
            })
            .unwrap();
        server
            .service
            .store()
            .upsert_tab(http::HttpTabRow {
                id: "tab-2".into(),
                title: "Two".into(),
                order_index: 1,
                is_active: false,
                spec_json: "{}".into(),
                updated_at: 2,
                kind: "http".into(),
            })
            .unwrap();
        let endpoint = server.endpoint().clone();
        let stop = server.stop_handle();
        let task = tokio::spawn(server.run());
        let mut client = connect_and_handshake(&endpoint, &data_directory, "tab-manager").await;

        for request in [
            serde_json::json!({
                "kind": "request",
                "requestId": "activate-2",
                "method": "data.tabs.setActive",
                "params": { "id": "tab-2" }
            }),
            serde_json::json!({
                "kind": "request",
                "requestId": "delete-1",
                "method": "data.tabs.delete",
                "params": { "id": "tab-1" }
            }),
        ] {
            let mut bytes = serde_json::to_vec(&request).unwrap();
            bytes.push(b'\n');
            client.get_mut().write_all(&bytes).await.unwrap();
            let mut line = Vec::new();
            client.read_until(b'\n', &mut line).await.unwrap();
            let reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
            assert_eq!(reply["kind"], "response", "{reply}");
        }

        let rows = task_store_rows(&endpoint, &data_directory).await;
        assert_eq!(rows.as_array().unwrap().len(), 1);
        assert_eq!(rows[0]["id"], "tab-2");
        assert_eq!(rows[0]["isActive"], true);

        stop.notify_one();
        task.await.unwrap().unwrap();
    }

    async fn task_store_rows(endpoint: &Endpoint, data_directory: &str) -> serde_json::Value {
        let mut client = connect_and_handshake(endpoint, data_directory, "tab-list-check").await;
        let request = serde_json::json!({
            "kind": "request",
            "requestId": "list-check",
            "method": "data.tabs.list",
            "params": {}
        });
        let mut bytes = serde_json::to_vec(&request).unwrap();
        bytes.push(b'\n');
        client.get_mut().write_all(&bytes).await.unwrap();
        let mut line = Vec::new();
        client.read_until(b'\n', &mut line).await.unwrap();
        serde_json::from_slice::<serde_json::Value>(&line).unwrap()["result"].clone()
    }

    #[tokio::test]
    async fn migrates_and_backs_up_legacy_tabs_without_losing_data() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("http.sqlite3");
        let connection = rusqlite::Connection::open(&database_path).unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE http_tabs (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    order_index INTEGER NOT NULL,
                    is_active INTEGER NOT NULL DEFAULT 0,
                    spec_json TEXT NOT NULL,
                    updated_at INTEGER NOT NULL
                );
                INSERT INTO http_tabs VALUES ('legacy-tab', 'Legacy', 0, 1, '{}', 10);
                "#,
            )
            .unwrap();
        drop(connection);

        let data_directory = directory.path().display().to_string();
        let server = ServiceServer::bind(
            directory.path().join("service.endpoint.json"),
            data_directory.clone(),
            "0.1.0-alpha.0",
        )
        .await
        .unwrap();
        let endpoint = server.endpoint().clone();
        let stop = server.stop_handle();
        let task = tokio::spawn(server.run());
        let rows = task_store_rows(&endpoint, &data_directory).await;

        assert_eq!(rows[0]["id"], "legacy-tab");
        assert_eq!(rows[0]["kind"], "http");
        assert!(directory
            .path()
            .join("http.sqlite3.pre-service-v1.bak")
            .exists());

        stop.notify_one();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn shares_and_clears_history_through_data_api() {
        let directory = tempfile::tempdir().unwrap();
        let data_directory = directory.path().display().to_string();
        let server = ServiceServer::bind(
            directory.path().join("service.endpoint.json"),
            data_directory.clone(),
            "0.1.0-alpha.0",
        )
        .await
        .unwrap();
        let endpoint = server.endpoint().clone();
        let stop = server.stop_handle();
        let task = tokio::spawn(server.run());
        let mut client = connect_and_handshake(&endpoint, &data_directory, "history-client").await;

        let insert = serde_json::json!({
            "kind": "request",
            "requestId": "history-insert",
            "method": "data.history.insert",
            "params": { "row": {
                "id": "history-1",
                "method": "POST",
                "url": "https://example.com/users",
                "status": 201,
                "elapsedMs": 42,
                "bodyBytes": 12,
                "specJson": "{}",
                "respSummary": "created",
                "createdAt": 1_797_000_000_000_i64
            }}
        });
        let mut bytes = serde_json::to_vec(&insert).unwrap();
        bytes.push(b'\n');
        client.get_mut().write_all(&bytes).await.unwrap();
        let mut line = Vec::new();
        client.read_until(b'\n', &mut line).await.unwrap();
        let insert_reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(insert_reply["kind"], "response", "{insert_reply}");

        let mut other = connect_and_handshake(&endpoint, &data_directory, "history-reader").await;
        let list = serde_json::json!({
            "kind": "request",
            "requestId": "history-list",
            "method": "data.history.list",
            "params": { "limit": 10 }
        });
        bytes = serde_json::to_vec(&list).unwrap();
        bytes.push(b'\n');
        other.get_mut().write_all(&bytes).await.unwrap();
        line.clear();
        other.read_until(b'\n', &mut line).await.unwrap();
        let list_reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(list_reply["result"][0]["id"], "history-1", "{list_reply}");

        let clear = serde_json::json!({
            "kind": "request",
            "requestId": "history-clear",
            "method": "data.history.clear",
            "params": {}
        });
        bytes = serde_json::to_vec(&clear).unwrap();
        bytes.push(b'\n');
        other.get_mut().write_all(&bytes).await.unwrap();
        line.clear();
        other.read_until(b'\n', &mut line).await.unwrap();
        let clear_reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(clear_reply["kind"], "response", "{clear_reply}");

        bytes = serde_json::to_vec(&list).unwrap();
        bytes.push(b'\n');
        other.get_mut().write_all(&bytes).await.unwrap();
        line.clear();
        other.read_until(b'\n', &mut line).await.unwrap();
        let empty_reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(empty_reply["result"], serde_json::json!([]));

        stop.notify_one();
        task.await.unwrap().unwrap();
    }
}
