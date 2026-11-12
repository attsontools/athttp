use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

use serde::Serialize;
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{tcp::OwnedWriteHalf, TcpStream},
    sync::{broadcast, oneshot, Mutex as AsyncMutex},
    time::{timeout, Duration},
};
use uuid::Uuid;

use crate::{
    http::StreamMessage, read_endpoint, HandshakeRequest, ServiceHandshake, ServiceReply,
    ServiceRequest, APP_ID, DATA_API, HOST_API, SERVICE_API,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceClientError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl ServiceClientError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
        }
    }
}

impl std::fmt::Display for ServiceClientError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ServiceClientError {}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceEvent {
    pub session_id: String,
    pub generation: u64,
    pub message: StreamMessage,
}

type PendingResult = Result<Value, ServiceClientError>;

#[derive(Default)]
struct ClientState {
    pending: HashMap<String, oneshot::Sender<PendingResult>>,
    disconnected: Option<ServiceClientError>,
}

type SharedClientState = Arc<Mutex<ClientState>>;

pub struct ServiceClient {
    writer: Arc<AsyncMutex<OwnedWriteHalf>>,
    state: SharedClientState,
    events: broadcast::Sender<ServiceEvent>,
}

impl ServiceClient {
    pub async fn connect(
        endpoint_path: &Path,
        data_directory: &Path,
        client_id: impl Into<String>,
    ) -> Result<Self, ServiceClientError> {
        let endpoint = read_endpoint(endpoint_path)
            .map_err(|error| ServiceClientError::new(error.code, error.message))?;
        let stream = TcpStream::connect(("127.0.0.1", endpoint.port))
            .await
            .map_err(|error| {
                let code = if error.kind() == std::io::ErrorKind::ConnectionRefused {
                    "HTTP_SERVICE_NOT_RUNNING"
                } else {
                    "HTTP_SERVICE_CONNECT_FAILED"
                };
                ServiceClientError::new(code, format!("cannot connect to HTTP service: {error}"))
            })?;
        let (reader, mut writer) = stream.into_split();
        let request_id = Uuid::new_v4().to_string();
        let handshake = ServiceHandshake::request(HandshakeRequest {
            protocol_version: 1,
            app_id: APP_ID.into(),
            client_id: client_id.into(),
            request_id: request_id.clone(),
            host_api: HOST_API.into(),
            data_api: DATA_API.into(),
            service_api: SERVICE_API.into(),
            data_directory: data_directory.to_string_lossy().to_string(),
            token: endpoint.token,
        });
        write_json_line(&mut writer, &handshake).await?;

        let mut reader = BufReader::new(reader);
        let mut line = Vec::new();
        timeout(Duration::from_secs(10), reader.read_until(b'\n', &mut line))
            .await
            .map_err(|_| {
                ServiceClientError::new(
                    "HTTP_SERVICE_HANDSHAKE_TIMEOUT",
                    "HTTP service handshake timed out",
                )
            })?
            .map_err(|error| {
                ServiceClientError::new(
                    "HTTP_SERVICE_HANDSHAKE_FAILED",
                    format!("cannot read HTTP service handshake: {error}"),
                )
            })?;
        let reply = serde_json::from_slice::<ServiceReply>(&line).map_err(|error| {
            ServiceClientError::new(
                "HTTP_SERVICE_HANDSHAKE_FAILED",
                format!("cannot decode HTTP service handshake: {error}"),
            )
        })?;
        match reply {
            ServiceReply::Handshake(response) if response.request_id == request_id => {}
            ServiceReply::Error { error, .. } => {
                return Err(ServiceClientError {
                    code: error.code,
                    message: error.message,
                    retryable: error.retryable,
                });
            }
            _ => {
                return Err(ServiceClientError::new(
                    "HTTP_SERVICE_HANDSHAKE_FAILED",
                    "HTTP service returned an unexpected handshake response",
                ));
            }
        }

        let state = Arc::new(Mutex::new(ClientState::default()));
        let (events, _) = broadcast::channel(256);
        tokio::spawn(read_replies(reader, Arc::clone(&state), events.clone()));
        Ok(Self {
            writer: Arc::new(AsyncMutex::new(writer)),
            state,
            events,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServiceEvent> {
        self.events.subscribe()
    }

    pub async fn request(
        &self,
        method: impl Into<String>,
        params: Value,
    ) -> Result<Value, ServiceClientError> {
        let request_id = Uuid::new_v4().to_string();
        let request = ServiceRequest::Request {
            request_id: request_id.clone(),
            method: method.into(),
            params,
        };
        let (sender, receiver) = oneshot::channel();
        {
            let mut state = self.state.lock().expect("client state mutex poisoned");
            if let Some(error) = &state.disconnected {
                return Err(error.clone());
            }
            state.pending.insert(request_id.clone(), sender);
        }
        let _pending_guard = PendingGuard {
            request_id: request_id.clone(),
            state: Arc::clone(&self.state),
        };
        if let Err(error) = write_json_line(&mut *self.writer.lock().await, &request).await {
            return Err(error);
        }
        match timeout(REQUEST_TIMEOUT, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(disconnected()),
            Err(_) => Err(ServiceClientError::new(
                "HTTP_SERVICE_REQUEST_TIMEOUT",
                "HTTP service request timed out",
            )),
        }
    }
}

struct PendingGuard {
    request_id: String,
    state: SharedClientState,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        self.state
            .lock()
            .expect("client state mutex poisoned")
            .pending
            .remove(&self.request_id);
    }
}

async fn read_replies(
    mut reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
    state: SharedClientState,
    events: broadcast::Sender<ServiceEvent>,
) {
    let mut line = Vec::new();
    let disconnect_error = loop {
        line.clear();
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) => break disconnected(),
            Err(error) => {
                break ServiceClientError::new(
                    "HTTP_SERVICE_DISCONNECTED",
                    format!("cannot read HTTP service response: {error}"),
                );
            }
            Ok(_) => {}
        }
        let reply = match serde_json::from_slice::<ServiceReply>(&line) {
            Ok(reply) => reply,
            Err(error) => {
                break ServiceClientError::new(
                    "HTTP_SERVICE_RESPONSE_INVALID",
                    format!("cannot decode HTTP service response: {error}"),
                );
            }
        };
        match reply {
            ServiceReply::Response { request_id, result } => {
                if let Some(sender) = state
                    .lock()
                    .expect("client state mutex poisoned")
                    .pending
                    .remove(&request_id)
                {
                    let _ = sender.send(Ok(result));
                }
            }
            ServiceReply::Error { request_id, error } => {
                if let Some(sender) = state
                    .lock()
                    .expect("client state mutex poisoned")
                    .pending
                    .remove(&request_id)
                {
                    let _ = sender.send(Err(ServiceClientError {
                        code: error.code,
                        message: error.message,
                        retryable: error.retryable,
                    }));
                }
            }
            ServiceReply::Event {
                session_id,
                generation,
                message,
            } => {
                let _ = events.send(ServiceEvent {
                    session_id,
                    generation,
                    message,
                });
            }
            ServiceReply::Handshake(_) => {
                break ServiceClientError::new(
                    "HTTP_SERVICE_RESPONSE_INVALID",
                    "unexpected handshake response after connection was established",
                );
            }
        }
    };
    let senders = {
        let mut state = state.lock().expect("client state mutex poisoned");
        state.disconnected = Some(disconnect_error.clone());
        state
            .pending
            .drain()
            .map(|(_, sender)| sender)
            .collect::<Vec<_>>()
    };
    for sender in senders {
        let _ = sender.send(Err(disconnect_error.clone()));
    }
}

async fn write_json_line<T: serde::Serialize>(
    writer: &mut OwnedWriteHalf,
    value: &T,
) -> Result<(), ServiceClientError> {
    let mut bytes = serde_json::to_vec(value).map_err(|error| {
        ServiceClientError::new(
            "HTTP_SERVICE_REQUEST_INVALID",
            format!("cannot encode HTTP service request: {error}"),
        )
    })?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await.map_err(|error| {
        ServiceClientError::new(
            "HTTP_SERVICE_DISCONNECTED",
            format!("cannot write HTTP service request: {error}"),
        )
    })
}

fn disconnected() -> ServiceClientError {
    ServiceClientError::new(
        "HTTP_SERVICE_DISCONNECTED",
        "HTTP service connection was closed",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_a_pending_request_guard_removes_the_request() {
        let state = Arc::new(Mutex::new(ClientState::default()));
        let (sender, _receiver) = oneshot::channel();
        state
            .lock()
            .unwrap()
            .pending
            .insert("request-1".into(), sender);
        let guard = PendingGuard {
            request_id: "request-1".into(),
            state: Arc::clone(&state),
        };

        drop(guard);

        assert!(state.lock().unwrap().pending.is_empty());
    }
}
