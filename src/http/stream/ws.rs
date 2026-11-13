use std::{
    fmt,
    str::FromStr,
    sync::{Arc, Mutex},
};

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{verify_tls12_signature, verify_tls13_signature, WebPkiSupportedAlgorithms},
    pki_types::{CertificateDer, ServerName, UnixTime},
    ClientConfig, DigitallySignedStruct, Error as TlsError, SignatureScheme,
};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::{
    connect_async_tls_with_config,
    tungstenite::{
        client::IntoClientRequest,
        handshake::client::Request,
        http::{HeaderName, HeaderValue},
        Message,
    },
    Connector,
};

use super::{buffer::SessionBuffer, session::HttpStreamState};
use crate::http::{Direction, StreamMessage, WsSpec};

const MAX_TEXT_BYTES: usize = 1024 * 1024;
const BINARY_PREVIEW_BYTES: usize = 4 * 1024;

struct AcceptInvalidCertificates {
    algorithms: WebPkiSupportedAlgorithms,
}

impl fmt::Debug for AcceptInvalidCertificates {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("AcceptInvalidCertificates").finish()
    }
}

impl ServerCertVerifier for AcceptInvalidCertificates {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

fn invalid_certificate_connector() -> Result<Connector, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(AcceptInvalidCertificates {
        algorithms: provider.signature_verification_algorithms,
    });
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("TLS config failed: {error}"))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(Connector::Rustls(Arc::new(config)))
}

pub async fn run_ws(
    state: Arc<HttpStreamState>,
    session_id: String,
    spec: WsSpec,
    mut cancel: oneshot::Receiver<()>,
    mut sends: mpsc::UnboundedReceiver<String>,
    buffer: Arc<Mutex<SessionBuffer>>,
    generation: u64,
) {
    if let Err(error) = run(
        &state,
        &session_id,
        spec,
        &mut cancel,
        &mut sends,
        &buffer,
        generation,
    )
    .await
    {
        state.publish(
            &session_id,
            generation,
            &buffer,
            StreamMessage::Error {
                at_ms: now_ms(),
                message: error.clone(),
            },
        );
        state.publish(
            &session_id,
            generation,
            &buffer,
            StreamMessage::Closed {
                at_ms: now_ms(),
                code: None,
                reason: error,
            },
        );
    }
}

async fn run(
    state: &HttpStreamState,
    session_id: &str,
    spec: WsSpec,
    cancel: &mut oneshot::Receiver<()>,
    sends: &mut mpsc::UnboundedReceiver<String>,
    buffer: &Arc<Mutex<SessionBuffer>>,
    generation: u64,
) -> Result<(), String> {
    let mut url =
        reqwest::Url::parse(spec.url.trim()).map_err(|error| format!("URL 非法：{error}"))?;
    if !matches!(url.scheme(), "ws" | "wss") {
        return Err("WebSocket 需要 ws/wss URL".into());
    }
    for item in spec
        .query_params
        .iter()
        .filter(|item| item.enabled && !item.key.trim().is_empty())
    {
        url.query_pairs_mut().append_pair(&item.key, &item.value);
    }
    let mut request: Request = url
        .as_str()
        .into_client_request()
        .map_err(|error| format!("bad request: {error}"))?;
    for item in spec
        .headers
        .iter()
        .filter(|item| item.enabled && !item.key.trim().is_empty())
    {
        request.headers_mut().insert(
            HeaderName::from_str(&item.key).map_err(|error| error.to_string())?,
            HeaderValue::from_str(&item.value).map_err(|error| error.to_string())?,
        );
    }
    match spec.auth.r#type.as_str() {
        "bearer" if !spec.auth.bearer_token.is_empty() => {
            request.headers_mut().insert(
                "authorization",
                HeaderValue::from_str(&format!("Bearer {}", spec.auth.bearer_token))
                    .map_err(|error| error.to_string())?,
            );
        }
        "basic" => {
            let encoded = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", spec.auth.basic_user, spec.auth.basic_pass));
            request.headers_mut().insert(
                "authorization",
                HeaderValue::from_str(&format!("Basic {encoded}"))
                    .map_err(|error| error.to_string())?,
            );
        }
        _ => {}
    }
    if !spec.subprotocols.is_empty() {
        request.headers_mut().insert(
            "sec-websocket-protocol",
            HeaderValue::from_str(&spec.subprotocols.join(", "))
                .map_err(|error| error.to_string())?,
        );
    }
    let connector = if url.scheme() == "wss" && !spec.verify_ssl {
        Some(invalid_certificate_connector()?)
    } else {
        None
    };
    let connection = connect_async_tls_with_config(request, None, false, connector);
    tokio::pin!(connection);
    let (socket, response) = tokio::select! {
        result = &mut connection => result.map_err(|error| format!("connect failed: {error}"))?,
        _ = &mut *cancel => return Ok(()),
    };
    let headers = response
        .headers()
        .iter()
        .map(|(key, value)| {
            (
                key.to_string(),
                value.to_str().unwrap_or("<binary>").to_string(),
            )
        })
        .collect();
    state.publish(
        session_id,
        generation,
        buffer,
        StreamMessage::Open {
            at_ms: now_ms(),
            status: None,
            headers,
        },
    );
    let (mut sink, mut stream) = socket.split();
    loop {
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    let (text, truncated) = truncate(text.to_string());
                    state.publish(session_id, generation, buffer, StreamMessage::WsText {
                        at_ms: now_ms(), direction: Direction::In, text, truncated,
                    });
                }
                Some(Ok(Message::Binary(bytes))) => {
                    let preview = &bytes[..bytes.len().min(BINARY_PREVIEW_BYTES)];
                    state.publish(session_id, generation, buffer, StreamMessage::WsBinary {
                        at_ms: now_ms(), direction: Direction::In, bytes_len: bytes.len(),
                        preview_b64: base64::engine::general_purpose::STANDARD.encode(preview),
                    });
                }
                Some(Ok(Message::Close(frame))) => {
                    let (code, reason) = frame.map(|frame| (Some(u16::from(frame.code)), frame.reason.to_string())).unwrap_or((None, String::new()));
                    state.publish(session_id, generation, buffer, StreamMessage::Closed { at_ms: now_ms(), code, reason });
                    return Ok(());
                }
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(format!("read error: {error}")),
                None => {
                    state.publish(session_id, generation, buffer, StreamMessage::Closed { at_ms: now_ms(), code: None, reason: "peer closed".into() });
                    return Ok(());
                }
            },
            outgoing = sends.recv() => match outgoing {
                Some(text) => {
                    sink.send(Message::Text(text.clone().into())).await.map_err(|error| format!("send failed: {error}"))?;
                    let (text, truncated) = truncate(text);
                    state.publish(session_id, generation, buffer, StreamMessage::WsText {
                        at_ms: now_ms(), direction: Direction::Out, text, truncated,
                    });
                }
                None => return Ok(()),
            },
            _ = &mut *cancel => {
                let _ = sink.close().await;
                return Ok(());
            }
        }
    }
}

fn truncate(mut text: String) -> (String, bool) {
    if text.len() <= MAX_TEXT_BYTES {
        return (text, false);
    }
    let mut end = MAX_TEXT_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (text, true)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use rustls::{
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
        ServerConfig,
    };
    use tokio::{net::TcpListener, task::JoinHandle, time::Duration};
    use tokio_rustls::TlsAcceptor;

    const TEST_CERT_DER: &str = "MIIBmjCCAT+gAwIBAgIUQXe1oiXane9Xx3/AmuTCDv7wljIwCgYIKoZIzj0EAwIwFDESMBAGA1UEAwwJbG9jYWxob3N0MB4XDTI2MTExMzA2MzA1NVoXDTM2MTExMDA2MzA1NVowFDESMBAGA1UEAwwJbG9jYWxob3N0MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE94ONbd1mLHGdj+Tj7zLLYPKeiNiQx2AT9V7ohNkhtNWvM5jrDCP8BAV/c5K96sX/tYiQr5n9fnPA9O4N6F1yVaNvMG0wHQYDVR0OBBYEFDPyBZ5nr0jaY0om9s8EdMpCb5QjMB8GA1UdIwQYMBaAFDPyBZ5nr0jaY0om9s8EdMpCb5QjMA8GA1UdEwEB/wQFMAMBAf8wGgYDVR0RBBMwEYIJbG9jYWxob3N0hwR/AAABMAoGCCqGSM49BAMCA0kAMEYCIQC9HyGKAFWLLl4TQuGkWQN3AOOKM8SrwTTIN+tLITow4QIhANpFlC4s6HtkQihRbWaBqiicyavEC6xMXGMd77BC7pOZ";
    const TEST_KEY_DER: &str = "MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgIYk2xGvrFrtmcwKCZCwvQ5FAPfhBq7x3lXirn75brB6hRANCAAT3g41t3WYscZ2P5OPvMstg8p6I2JDHYBP1XuiE2SG01a8zmOsMI/wEBX9zkr3qxf+1iJCvmf1+c8D07g3oXXJV";

    async fn spawn_self_signed_wss() -> (String, JoinHandle<()>) {
        let cert = CertificateDer::from(
            base64::engine::general_purpose::STANDARD
                .decode(TEST_CERT_DER)
                .unwrap(),
        );
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            base64::engine::general_purpose::STANDARD
                .decode(TEST_KEY_DER)
                .unwrap(),
        ));
        let config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(vec![cert], key)
                .unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let Ok(tls) = acceptor.accept(tcp).await else {
                return;
            };
            let Ok(mut socket) = tokio_tungstenite::accept_async(tls).await else {
                return;
            };
            let _ = socket.close(None).await;
        });
        (format!("wss://{address}"), server)
    }

    fn ws_spec(url: String, verify_ssl: bool) -> WsSpec {
        WsSpec {
            url,
            headers: vec![],
            query_params: vec![],
            auth: Default::default(),
            verify_ssl,
            subprotocols: vec![],
            ping_interval_seconds: None,
        }
    }

    async fn wait_for_event(
        receiver: &mut tokio::sync::broadcast::Receiver<super::super::session::StreamEvent>,
        session_id: &str,
        predicate: impl Fn(&StreamMessage) -> bool,
    ) -> StreamMessage {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let event = receiver.recv().await.unwrap();
                if event.session_id == session_id && predicate(&event.message) {
                    return event.message;
                }
            }
        })
        .await
        .expect("timed out waiting for WebSocket event")
    }

    #[test]
    fn truncates_oversized_text_at_a_character_boundary() {
        let (ascii, ascii_truncated) = truncate("x".repeat(MAX_TEXT_BYTES + 10));
        assert!(ascii_truncated);
        assert_eq!(ascii.len(), MAX_TEXT_BYTES);

        let (multibyte, multibyte_truncated) = truncate("中".repeat(MAX_TEXT_BYTES));
        assert!(multibyte_truncated);
        assert!(multibyte.len() <= MAX_TEXT_BYTES);
        assert_eq!(multibyte.len() % 3, 0);
    }

    #[tokio::test]
    async fn rejects_a_self_signed_wss_certificate_when_verification_is_enabled() {
        let (url, server) = spawn_self_signed_wss().await;
        let state = Arc::new(HttpStreamState::default());
        let mut events = state.subscribe();

        state.open_ws("strict".into(), ws_spec(url, true));

        let error = wait_for_event(&mut events, "strict", |message| {
            matches!(message, StreamMessage::Error { .. })
        })
        .await;
        let StreamMessage::Error { message, .. } = error else {
            unreachable!();
        };
        assert!(message.to_lowercase().contains("certificate"), "{message}");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn connects_to_self_signed_wss_when_verification_is_disabled() {
        let (url, server) = spawn_self_signed_wss().await;
        let state = Arc::new(HttpStreamState::default());
        let mut events = state.subscribe();

        state.open_ws("insecure".into(), ws_spec(url, false));

        let opened = wait_for_event(&mut events, "insecure", |message| {
            matches!(message, StreamMessage::Open { .. })
        })
        .await;
        assert!(matches!(opened, StreamMessage::Open { .. }));
        server.await.unwrap();
    }
}
