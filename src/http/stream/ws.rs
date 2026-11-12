use std::{
    str::FromStr,
    sync::{Arc, Mutex},
};

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        handshake::client::Request,
        http::{HeaderName, HeaderValue},
        Message,
    },
};

use super::{buffer::SessionBuffer, session::HttpStreamState};
use crate::http::{Direction, StreamMessage, WsSpec};

const MAX_TEXT_BYTES: usize = 1024 * 1024;
const BINARY_PREVIEW_BYTES: usize = 4 * 1024;

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
    let connection = connect_async(request);
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
}
