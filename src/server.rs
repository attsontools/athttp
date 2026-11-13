use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::Notify,
    task::JoinSet,
    time::{timeout, Duration},
};
use uuid::Uuid;

use crate::{
    http::{self, HttpService},
    negotiate, write_endpoint, Endpoint, ErrorEnvelope, ServiceError, ServiceHandshake,
    ServiceReply, ServiceRequest,
};

const MAX_HANDSHAKE_FRAME_BYTES: usize = 1024 * 1024;
const MAX_BUSINESS_FRAME_BYTES: usize = 8 * 1024 * 1024;

pub struct ServiceServer {
    listener: TcpListener,
    endpoint_path: PathBuf,
    data_directory: String,
    endpoint: Endpoint,
    stop: Arc<Notify>,
    service: Arc<HttpService>,
    lease: Option<ServiceLease>,
}

impl ServiceServer {
    pub async fn bind(
        endpoint_path: PathBuf,
        data_directory: impl Into<String>,
        service_version: impl Into<String>,
    ) -> Result<Self, ServiceError> {
        let data_directory = data_directory.into();
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
        let token = Uuid::new_v4().to_string();
        let lease = ServiceLease::acquire(Path::new(&data_directory), &token)?;
        let service = HttpService::new(PathBuf::from(&data_directory))
            .map_err(|error| ServiceError::new("HTTP_DATA_MIGRATION_FAILED", error))?;
        let endpoint = Endpoint {
            port: address.port(),
            token,
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
            lease: Some(lease),
        })
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    pub fn stop_handle(&self) -> Arc<Notify> {
        Arc::clone(&self.stop)
    }

    pub async fn run(mut self) -> Result<(), ServiceError> {
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
        let endpoint_result = match std::fs::remove_file(&self.endpoint_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(ServiceError::new(
                "HTTP_SERVICE_ENDPOINT_CLEANUP_FAILED",
                format!("cannot remove service endpoint: {error}"),
            )),
        };
        let lease_result = self
            .lease
            .take()
            .expect("service lease exists while server is running")
            .release();
        endpoint_result?;
        lease_result
    }
}

struct ServiceLease {
    path: Option<PathBuf>,
    file: Option<File>,
}

impl ServiceLease {
    fn acquire(data_directory: &Path, token: &str) -> Result<Self, ServiceError> {
        fs::create_dir_all(data_directory).map_err(|error| {
            ServiceError::new(
                "HTTP_DATA_MIGRATION_FAILED",
                format!("cannot create HTTP data directory: {error}"),
            )
        })?;
        let path = data_directory.join("service.lock");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                let code = if error.kind() == io::ErrorKind::AlreadyExists {
                    "HTTP_SERVICE_ALREADY_RUNNING"
                } else {
                    "HTTP_SERVICE_LEASE_FAILED"
                };
                ServiceError::new(code, format!("cannot acquire HTTP service lease: {error}"))
            })?;
        let mut lease = Self {
            path: Some(path),
            file: Some(file),
        };
        set_lease_permissions(lease.file.as_ref().expect("lease file is open"))?;
        writeln!(
            lease.file.as_mut().expect("lease file is open"),
            "{token}\n{}",
            std::process::id()
        )
        .map_err(|error| {
            ServiceError::new(
                "HTTP_SERVICE_LEASE_FAILED",
                format!("cannot write HTTP service lease: {error}"),
            )
        })?;
        Ok(lease)
    }

    fn release(self) -> Result<(), ServiceError> {
        self.release_with(|path| fs::remove_file(path))
    }

    fn release_with(
        mut self,
        remove: impl FnOnce(&Path) -> io::Result<()>,
    ) -> Result<(), ServiceError> {
        self.file.take();
        let path = self
            .path
            .take()
            .expect("service lease path exists until release");
        remove(&path).map_err(|error| {
            ServiceError::new(
                "HTTP_SERVICE_LEASE_CLEANUP_FAILED",
                format!("cannot remove HTTP service lease: {error}"),
            )
        })
    }
}

impl Drop for ServiceLease {
    fn drop(&mut self) {
        self.file.take();
        if let Some(path) = self.path.take() {
            let _ = fs::remove_file(path);
        }
    }
}

fn set_lease_permissions(file: &File) -> Result<(), ServiceError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = file
            .metadata()
            .map_err(|error| {
                ServiceError::new(
                    "HTTP_SERVICE_LEASE_FAILED",
                    format!("cannot inspect HTTP service lease: {error}"),
                )
            })?
            .permissions();
        permissions.set_mode(0o600);
        file.set_permissions(permissions).map_err(|error| {
            ServiceError::new(
                "HTTP_SERVICE_LEASE_FAILED",
                format!("cannot restrict HTTP service lease: {error}"),
            )
        })?;
    }
    Ok(())
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
    let read = timeout(
        Duration::from_secs(10),
        read_frame(&mut reader, &mut frame, MAX_HANDSHAKE_FRAME_BYTES),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "handshake timed out"))??;
    if read == 0 || frame.len() > MAX_HANDSHAKE_FRAME_BYTES || frame.last() != Some(&b'\n') {
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

    let mut events = service.stream_state().subscribe();
    let mut requests = JoinSet::new();
    loop {
        frame.clear();
        tokio::select! {
            read = read_frame(&mut reader, &mut frame, MAX_BUSINESS_FRAME_BYTES) => {
                let read = read?;
                if read == 0 {
                    return Ok(());
                }
                if frame.len() > MAX_BUSINESS_FRAME_BYTES || frame.last() != Some(&b'\n') {
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
                let service = Arc::clone(&service);
                requests.spawn(async move {
                    let (request_id, method, params) = request;
                    dispatch_request(&service, &method, params, &request_id).await
                });
            }
            event = events.recv() => {
                match event {
                    Ok(event) => write_reply(&mut write_half, ServiceReply::Event {
                        session_id: event.session_id,
                        generation: event.generation,
                        message: event.message,
                    }).await?,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
                }
            }
            completed = requests.join_next(), if !requests.is_empty() => {
                match completed {
                    Some(Ok(reply)) => write_reply(&mut write_half, reply).await?,
                    Some(Err(error)) => {
                        return Err(io::Error::other(format!(
                            "service request task failed: {error}"
                        )));
                    }
                    None => {}
                }
            }
        }
    }
}

async fn read_frame(
    reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
    frame: &mut Vec<u8>,
    max_bytes: usize,
) -> io::Result<usize> {
    let mut limited = reader.take((max_bytes + 1) as u64);
    limited.read_until(b'\n', frame).await
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
        "data.envs.list" => service
            .store()
            .list_envs()
            .and_then(|rows| serde_json::to_value(rows).map_err(|error| error.to_string()))
            .map_err(|error| ServiceError::new("HTTP_DATA_READ_FAILED", error)),
        "data.envs.upsert" => match required_row::<http::HttpEnvRow>(&params) {
            Ok(row) => service
                .store()
                .upsert_env(row)
                .map(|()| serde_json::Value::Null)
                .map_err(|error| ServiceError::new("HTTP_DATA_WRITE_FAILED", error)),
            Err(error) => Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error)),
        },
        "data.envs.delete" => mutate_id(&params, |id| service.store().delete_env(id)),
        "data.envs.setActive" => mutate_id(&params, |id| service.store().set_active_env(id)),
        "data.envs.vars.list" => match params.get("envId").and_then(serde_json::Value::as_str) {
            Some(env_id) => service
                .store()
                .list_env_vars(env_id)
                .and_then(|rows| serde_json::to_value(rows).map_err(|error| error.to_string()))
                .map_err(|error| ServiceError::new("HTTP_DATA_READ_FAILED", error)),
            None => Err(ServiceError::new(
                "HTTP_SERVICE_REQUEST_INVALID",
                "envId is required",
            )),
        },
        "data.envs.vars.upsert" => match required_row::<http::HttpEnvVarRow>(&params) {
            Ok(row) => service
                .store()
                .upsert_env_var(row)
                .map(|()| serde_json::Value::Null)
                .map_err(|error| ServiceError::new("HTTP_DATA_WRITE_FAILED", error)),
            Err(error) => Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error)),
        },
        "data.envs.vars.delete" => mutate_id(&params, |id| service.store().delete_env_var(id)),
        "data.collections.list" => serialize_read(service.store().list_collections()),
        "data.collections.folders.list" => {
            serialize_read(service.store().list_collection_folders())
        }
        "data.collections.requests.list" => {
            serialize_read(service.store().list_collection_requests())
        }
        "data.collections.upsert" => match required_row::<http::HttpCollectionRow>(&params) {
            Ok(row) => mutate_data(service.store().upsert_collection(row)),
            Err(error) => Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error)),
        },
        "data.collections.folders.upsert" => {
            match required_row::<http::HttpCollectionFolderRow>(&params) {
                Ok(row) => mutate_data(service.store().upsert_collection_folder(row)),
                Err(error) => Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error)),
            }
        }
        "data.collections.requests.upsert" => {
            match required_row::<http::HttpCollectionRequestRow>(&params) {
                Ok(row) => mutate_data(service.store().upsert_collection_request(row)),
                Err(error) => Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error)),
            }
        }
        "data.collections.delete" => mutate_id(&params, |id| service.store().delete_collection(id)),
        "data.collections.folders.delete" => {
            mutate_id(&params, |id| service.store().delete_collection_folder(id))
        }
        "data.collections.requests.delete" => {
            mutate_id(&params, |id| service.store().delete_collection_request(id))
        }
        "data.openapi.import" => {
            match serde_json::from_value::<http::OpenApiImportParams>(params) {
                Ok(params) => http::fetch_openapi(params)
                    .await
                    .and_then(|document| {
                        serde_json::to_value(document).map_err(|error| error.to_string())
                    })
                    .map_err(|error| ServiceError::new("HTTP_OPENAPI_IMPORT_FAILED", error)),
                Err(error) => Err(ServiceError::new(
                    "HTTP_SERVICE_REQUEST_INVALID",
                    format!("invalid OpenAPI import params: {error}"),
                )),
            }
        }
        "stream.open" => {
            let session_id = required_string(&params, "sessionId");
            let kind = required_string(&params, "kind");
            match (session_id, kind) {
                (Ok(session_id), Ok("sse")) => {
                    let spec = params
                        .get("spec")
                        .cloned()
                        .ok_or_else(|| "spec is required".to_string())
                        .and_then(|value| {
                            serde_json::from_value::<http::SseSpec>(value)
                                .map_err(|error| error.to_string())
                        });
                    match spec {
                        Ok(spec) => Ok(serde_json::json!({
                            "generation": http::stream::open_sse(Arc::clone(service.stream_state()), session_id.into(), spec)
                        })),
                        Err(error) => Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error)),
                    }
                }
                (Ok(session_id), Ok("ws")) => {
                    let spec = params
                        .get("spec")
                        .cloned()
                        .ok_or_else(|| "spec is required".to_string())
                        .and_then(|value| {
                            serde_json::from_value::<http::WsSpec>(value)
                                .map_err(|error| error.to_string())
                        });
                    match spec {
                        Ok(spec) => Ok(serde_json::json!({
                            "generation": http::stream::open_ws(Arc::clone(service.stream_state()), session_id.into(), spec)
                        })),
                        Err(error) => Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error)),
                    }
                }
                (Ok(_), Ok(kind)) => Err(ServiceError::new(
                    "HTTP_SERVICE_REQUEST_INVALID",
                    format!("unsupported stream kind: {kind}"),
                )),
                (Err(error), _) | (_, Err(error)) => {
                    Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error))
                }
            }
        }
        "stream.close" => match required_string(&params, "sessionId") {
            Ok(session_id) => {
                Ok(serde_json::json!({ "closed": service.stream_state().close(session_id) }))
            }
            Err(error) => Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error)),
        },
        "stream.snapshot" => match required_string(&params, "sessionId") {
            Ok(session_id) => serialize_read::<Vec<http::StreamMessage>>(Ok(service
                .stream_state()
                .snapshot(session_id))),
            Err(error) => Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error)),
        },
        "stream.send" => match (
            required_string(&params, "sessionId"),
            required_string(&params, "text"),
        ) {
            (Ok(session_id), Ok(text)) => service
                .stream_state()
                .send(session_id, text.into())
                .map(|()| serde_json::Value::Null)
                .map_err(|error| ServiceError::new("HTTP_STREAM_SEND_FAILED", error)),
            (Err(error), _) | (_, Err(error)) => {
                Err(ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error))
            }
        },
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

fn mutate_id(
    params: &serde_json::Value,
    operation: impl FnOnce(&str) -> Result<(), String>,
) -> Result<serde_json::Value, ServiceError> {
    let id = required_string(params, "id")
        .map_err(|error| ServiceError::new("HTTP_SERVICE_REQUEST_INVALID", error))?;
    operation(id)
        .map(|()| serde_json::Value::Null)
        .map_err(|error| ServiceError::new("HTTP_DATA_WRITE_FAILED", error))
}

fn mutate_data(result: Result<(), String>) -> Result<serde_json::Value, ServiceError> {
    result
        .map(|()| serde_json::Value::Null)
        .map_err(|error| ServiceError::new("HTTP_DATA_WRITE_FAILED", error))
}

fn serialize_read<T: serde::Serialize>(
    result: Result<T, String>,
) -> Result<serde_json::Value, ServiceError> {
    result
        .and_then(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        .map_err(|error| ServiceError::new("HTTP_DATA_READ_FAILED", error))
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
    use futures_util::{SinkExt, StreamExt};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    #[test]
    fn releasing_a_lease_does_not_remove_a_new_owners_file() {
        let directory = tempfile::tempdir().unwrap();
        let lease = ServiceLease::acquire(directory.path(), "first-owner").unwrap();
        let lease_path = lease.path.clone().unwrap();

        lease
            .release_with(|path| {
                fs::remove_file(path)?;
                fs::write(path, "second-owner")
            })
            .unwrap();

        assert_eq!(fs::read_to_string(lease_path).unwrap(), "second-owner");
    }

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

    async fn call(
        client: &mut BufReader<TcpStream>,
        request_id: &str,
        method: &str,
        params: serde_json::Value,
    ) -> serde_json::Value {
        let request = serde_json::json!({
            "kind": "request",
            "requestId": request_id,
            "method": method,
            "params": params
        });
        let mut bytes = serde_json::to_vec(&request).unwrap();
        bytes.push(b'\n');
        client.get_mut().write_all(&bytes).await.unwrap();
        let mut line = Vec::new();
        client.read_until(b'\n', &mut line).await.unwrap();
        serde_json::from_slice(&line).unwrap()
    }

    async fn call_response(
        client: &mut BufReader<TcpStream>,
        request_id: &str,
        method: &str,
        params: serde_json::Value,
    ) -> serde_json::Value {
        let request = serde_json::json!({
            "kind": "request", "requestId": request_id, "method": method, "params": params
        });
        let mut bytes = serde_json::to_vec(&request).unwrap();
        bytes.push(b'\n');
        client.get_mut().write_all(&bytes).await.unwrap();
        loop {
            let mut line = Vec::new();
            tokio::time::timeout(Duration::from_secs(2), client.read_until(b'\n', &mut line))
                .await
                .unwrap()
                .unwrap();
            let envelope: serde_json::Value = serde_json::from_slice(&line).unwrap();
            if envelope["requestId"] == request_id {
                return envelope;
            }
        }
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
    async fn cancels_an_in_flight_http_request_from_the_same_client() {
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
        let mut client = connect_and_handshake(&endpoint, &data_directory, "single-client").await;

        for request in [
            serde_json::json!({
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
            }),
            serde_json::json!({
                "kind": "request",
                "requestId": "cancel-1",
                "method": "http.cancel",
                "params": { "cancelTokenId": "cancel-slow" }
            }),
        ] {
            let mut bytes = serde_json::to_vec(&request).unwrap();
            bytes.push(b'\n');
            client.get_mut().write_all(&bytes).await.unwrap();
            if request["requestId"] == "http-slow" {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }

        let replies = tokio::time::timeout(Duration::from_millis(500), async {
            let mut replies = std::collections::HashMap::new();
            for _ in 0..2 {
                let mut line = Vec::new();
                client.read_until(b'\n', &mut line).await.unwrap();
                let reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
                replies.insert(reply["requestId"].as_str().unwrap().to_string(), reply);
            }
            replies
        })
        .await
        .expect("same-client cancellation must not wait for the HTTP request");

        assert_eq!(replies["cancel-1"]["result"]["cancelled"], true);
        assert_eq!(
            replies["http-slow"]["error"]["code"],
            "HTTP_REQUEST_CANCELLED"
        );

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

    #[tokio::test]
    async fn shares_environments_and_variables_through_data_api() {
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
        let mut writer = connect_and_handshake(&endpoint, &data_directory, "env-writer").await;

        for request in [
            serde_json::json!({
                "kind": "request", "requestId": "env-upsert", "method": "data.envs.upsert",
                "params": { "row": { "id": "env-1", "name": "Production", "isActive": true,
                    "orderIndex": 0, "updatedAt": 100 } }
            }),
            serde_json::json!({
                "kind": "request", "requestId": "var-upsert", "method": "data.envs.vars.upsert",
                "params": { "row": { "id": "var-1", "envId": "env-1", "key": "apiHost",
                    "value": "https://api.example.com", "enabled": true, "orderIndex": 0,
                    "updatedAt": 101 } }
            }),
            serde_json::json!({
                "kind": "request", "requestId": "global-var-upsert",
                "method": "data.envs.vars.upsert",
                "params": { "row": { "id": "global-var-1", "envId": "", "key": "token",
                    "value": "global-token", "enabled": true, "orderIndex": 0,
                    "updatedAt": 102 } }
            }),
        ] {
            let mut bytes = serde_json::to_vec(&request).unwrap();
            bytes.push(b'\n');
            writer.get_mut().write_all(&bytes).await.unwrap();
            let mut line = Vec::new();
            writer.read_until(b'\n', &mut line).await.unwrap();
            let reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
            assert_eq!(reply["kind"], "response", "{reply}");
        }

        let mut reader = connect_and_handshake(&endpoint, &data_directory, "env-reader").await;
        for (method, params, expected_id) in [
            ("data.envs.list", serde_json::json!({}), "env-1"),
            (
                "data.envs.vars.list",
                serde_json::json!({ "envId": "env-1" }),
                "var-1",
            ),
            (
                "data.envs.vars.list",
                serde_json::json!({ "envId": "" }),
                "global-var-1",
            ),
        ] {
            let request = serde_json::json!({
                "kind": "request", "requestId": format!("{method}-list"), "method": method,
                "params": params
            });
            let mut bytes = serde_json::to_vec(&request).unwrap();
            bytes.push(b'\n');
            reader.get_mut().write_all(&bytes).await.unwrap();
            let mut line = Vec::new();
            reader.read_until(b'\n', &mut line).await.unwrap();
            let reply: serde_json::Value = serde_json::from_slice(&line).unwrap();
            assert_eq!(reply["result"][0]["id"], expected_id, "{reply}");
        }

        let invalid = call(
            &mut reader,
            "empty-tab-id",
            "data.tabs.delete",
            serde_json::json!({ "id": "" }),
        )
        .await;
        assert_eq!(invalid["error"]["code"], "HTTP_SERVICE_REQUEST_INVALID");

        stop.notify_one();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn shares_collection_tree_and_cascades_deletion_through_data_api() {
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
        let mut writer =
            connect_and_handshake(&endpoint, &data_directory, "collection-writer").await;

        for (id, method, row) in [
            (
                "collection-upsert",
                "data.collections.upsert",
                serde_json::json!({ "id": "collection-1", "name": "Users API",
                    "orderIndex": 0, "updatedAt": 100, "sourceUrl": "https://example.com/openapi.json",
                    "sourceHeadersJson": "[]", "syncIntervalSecs": 1800,
                    "lastSyncedAt": 99, "lastSyncError": null, "baseUrl": "https://api.example.com" }),
            ),
            (
                "folder-upsert",
                "data.collections.folders.upsert",
                serde_json::json!({ "id": "folder-1", "collectionId": "collection-1",
                    "parentId": null, "name": "users", "orderIndex": 0, "updatedAt": 101 }),
            ),
            (
                "request-upsert",
                "data.collections.requests.upsert",
                serde_json::json!({ "id": "request-1", "collectionId": "collection-1",
                    "folderId": "folder-1", "name": "List users", "method": "GET",
                    "specJson": "{}", "orderIndex": 0, "updatedAt": 102,
                    "sourceKey": "GET /users" }),
            ),
        ] {
            let reply = call(&mut writer, id, method, serde_json::json!({ "row": row })).await;
            assert_eq!(reply["kind"], "response", "{reply}");
        }

        let mut reader =
            connect_and_handshake(&endpoint, &data_directory, "collection-reader").await;
        for (method, expected_id) in [
            ("data.collections.list", "collection-1"),
            ("data.collections.folders.list", "folder-1"),
            ("data.collections.requests.list", "request-1"),
        ] {
            let reply = call(&mut reader, method, method, serde_json::json!({})).await;
            assert_eq!(reply["result"][0]["id"], expected_id, "{reply}");
        }

        let deleted = call(
            &mut writer,
            "collection-delete",
            "data.collections.delete",
            serde_json::json!({ "id": "collection-1" }),
        )
        .await;
        assert_eq!(deleted["kind"], "response", "{deleted}");
        for method in [
            "data.collections.list",
            "data.collections.folders.list",
            "data.collections.requests.list",
        ] {
            let reply = call(&mut reader, method, method, serde_json::json!({})).await;
            assert_eq!(reply["result"], serde_json::json!([]), "{reply}");
        }

        stop.notify_one();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn imports_openapi_document_over_service_rpc() {
        let upstream = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move {
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut request = Vec::new();
            BufReader::new(&mut stream)
                .read_until(b'\n', &mut request)
                .await
                .unwrap();
            let body = r#"{"openapi":"3.1.0"}"#;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
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
        let mut client = connect_and_handshake(&endpoint, &data_directory, "openapi-client").await;

        let reply = call(
            &mut client,
            "openapi-import",
            "data.openapi.import",
            serde_json::json!({
                "url": format!("http://{upstream_address}/openapi.json"),
                "headers": [{ "key": "X-Test", "value": "yes" }]
            }),
        )
        .await;
        assert_eq!(reply["kind"], "response", "{reply}");
        assert_eq!(reply["result"], r#"{"openapi":"3.1.0"}"#);

        stop.notify_one();
        task.await.unwrap().unwrap();
        upstream_task.await.unwrap();
    }

    #[tokio::test]
    async fn rejects_non_http_openapi_sources() {
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
        let mut client =
            connect_and_handshake(&endpoint, &data_directory, "openapi-security").await;

        let reply = call(
            &mut client,
            "openapi-file",
            "data.openapi.import",
            serde_json::json!({ "url": "file:///etc/passwd", "headers": [] }),
        )
        .await;
        assert_eq!(reply["kind"], "error");
        assert_eq!(reply["error"]["code"], "HTTP_OPENAPI_IMPORT_FAILED");

        stop.notify_one();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn emits_sse_events_after_stream_open() {
        let upstream = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move {
            let (mut stream, _) = upstream.accept().await.unwrap();
            let mut request = Vec::new();
            BufReader::new(&mut stream)
                .read_until(b'\n', &mut request)
                .await
                .unwrap();
            let body = "event: answer\nid: 7\ndata: hello\n\n";
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
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
        let mut client = connect_and_handshake(&endpoint, &data_directory, "sse-client").await;
        let request = serde_json::json!({
            "kind": "request",
            "requestId": "stream-open-1",
            "method": "stream.open",
            "params": {
                "sessionId": "sse-1",
                "kind": "sse",
                "spec": { "url": format!("http://{upstream_address}/events") }
            }
        });
        let mut bytes = serde_json::to_vec(&request).unwrap();
        bytes.push(b'\n');
        client.get_mut().write_all(&bytes).await.unwrap();

        let mut saw_response = false;
        let mut saw_event = false;
        for _ in 0..5 {
            let mut line = Vec::new();
            tokio::time::timeout(Duration::from_secs(2), client.read_until(b'\n', &mut line))
                .await
                .unwrap()
                .unwrap();
            let envelope: serde_json::Value = serde_json::from_slice(&line).unwrap();
            if envelope["kind"] == "response" && envelope["requestId"] == "stream-open-1" {
                saw_response = true;
            }
            if envelope["kind"] == "event"
                && envelope["sessionId"] == "sse-1"
                && envelope["message"]["kind"] == "sseEvent"
            {
                assert_eq!(envelope["message"]["event"], "answer");
                assert_eq!(envelope["message"]["id"], "7");
                assert_eq!(envelope["message"]["data"], "hello");
                saw_event = true;
            }
            if saw_response && saw_event {
                break;
            }
        }
        assert!(saw_response, "stream.open response was not received");
        assert!(saw_event, "SSE event envelope was not received");

        let snapshot = call_response(
            &mut client,
            "stream-snapshot-1",
            "stream.snapshot",
            serde_json::json!({ "sessionId": "sse-1" }),
        )
        .await;
        assert!(snapshot["result"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["kind"] == "sseEvent" && message["data"] == "hello"));
        let closed = call_response(
            &mut client,
            "stream-close-1",
            "stream.close",
            serde_json::json!({ "sessionId": "sse-1" }),
        )
        .await;
        assert_eq!(closed["kind"], "response");
        let empty = call_response(
            &mut client,
            "stream-snapshot-2",
            "stream.snapshot",
            serde_json::json!({ "sessionId": "sse-1" }),
        )
        .await;
        assert_eq!(empty["result"], serde_json::json!([]));

        stop.notify_one();
        task.await.unwrap().unwrap();
        upstream_task.await.unwrap();
    }

    #[tokio::test]
    async fn sends_and_receives_websocket_messages_over_stream_rpc() {
        let upstream = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move {
            let (stream, _) = upstream.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            if let Some(Ok(message)) = socket.next().await {
                socket.send(message).await.unwrap();
            }
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
        let mut client = connect_and_handshake(&endpoint, &data_directory, "ws-client").await;

        let opened = call_response(
            &mut client,
            "ws-open",
            "stream.open",
            serde_json::json!({
                "sessionId": "ws-1", "kind": "ws",
                "spec": { "url": format!("ws://{upstream_address}/echo") }
            }),
        )
        .await;
        assert_eq!(opened["kind"], "response", "{opened}");
        let sent = call_response(
            &mut client,
            "ws-send",
            "stream.send",
            serde_json::json!({ "sessionId": "ws-1", "text": "hello" }),
        )
        .await;
        assert_eq!(sent["kind"], "response", "{sent}");

        let mut snapshot = serde_json::json!([]);
        for attempt in 0..10 {
            let reply = call_response(
                &mut client,
                &format!("ws-snapshot-{attempt}"),
                "stream.snapshot",
                serde_json::json!({ "sessionId": "ws-1" }),
            )
            .await;
            snapshot = reply["result"].clone();
            if snapshot.as_array().unwrap().iter().any(|message| {
                message["kind"] == "wsText"
                    && message["direction"] == "in"
                    && message["text"] == "hello"
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(snapshot.as_array().unwrap().iter().any(|message| {
            message["kind"] == "wsText"
                && message["direction"] == "in"
                && message["text"] == "hello"
        }));

        stop.notify_one();
        task.await.unwrap().unwrap();
        upstream_task.await.unwrap();
    }
}
