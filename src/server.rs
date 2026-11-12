use std::{io, path::PathBuf, sync::Arc};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::Notify,
};
use uuid::Uuid;

use crate::{
    negotiate, write_endpoint, Endpoint, ErrorEnvelope, ServiceError, ServiceHandshake,
    ServiceReply,
};

const MAX_FRAME_BYTES: usize = 1024 * 1024;

pub struct ServiceServer {
    listener: TcpListener,
    endpoint_path: PathBuf,
    endpoint: Endpoint,
    stop: Arc<Notify>,
}

impl ServiceServer {
    pub async fn bind(
        endpoint_path: PathBuf,
        service_version: impl Into<String>,
    ) -> Result<Self, ServiceError> {
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
            endpoint,
            stop: Arc::new(Notify::new()),
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
                    tokio::spawn(async move {
                        let _ = handle_connection(stream, &token).await;
                    });
                }
                _ = self.stop.notified() => break,
            }
        }
        let _ = std::fs::remove_file(&self.endpoint_path);
        Ok(())
    }
}

async fn handle_connection(stream: TcpStream, token: &str) -> io::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut frame = Vec::new();
    let read = reader.read_until(b'\n', &mut frame).await?;
    if read == 0 || frame.len() > MAX_FRAME_BYTES {
        return Ok(());
    }
    let reply = match serde_json::from_slice::<ServiceHandshake>(&frame) {
        Ok(ServiceHandshake::Request(request)) => match negotiate(&request, token) {
            Ok(response) => ServiceReply::Handshake(response),
            Err(error) => ServiceReply::Error {
                request_id: request.request_id,
                error: ErrorEnvelope::from(error),
            },
        },
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
    write_half.write_all(&bytes).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read_endpoint, HandshakeRequest};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn binds_loopback_and_answers_handshake() {
        let directory = tempfile::tempdir().unwrap();
        let endpoint_path = directory.path().join("service.endpoint.json");
        let server = ServiceServer::bind(endpoint_path.clone(), "0.1.0-alpha.0")
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
}
