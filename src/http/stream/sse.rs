use std::{
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::Engine;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use tokio::sync::oneshot;

use super::{buffer::SessionBuffer, parser::SseParser, session::HttpStreamState};
use crate::http::{SseSpec, StreamMessage};

pub async fn run_sse(
    state: Arc<HttpStreamState>,
    session_id: String,
    spec: SseSpec,
    mut cancel: oneshot::Receiver<()>,
    buffer: Arc<Mutex<SessionBuffer>>,
    generation: u64,
) {
    if let Err(error) = run(&state, &session_id, spec, &mut cancel, &buffer, generation).await {
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
    spec: SseSpec,
    cancel: &mut oneshot::Receiver<()>,
    buffer: &Arc<Mutex<SessionBuffer>>,
    generation: u64,
) -> Result<(), String> {
    let mut url = reqwest::Url::parse(spec.url.trim()).map_err(|e| format!("URL 非法：{e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("SSE 需要 http/https URL".into());
    }
    for item in spec
        .query_params
        .iter()
        .filter(|item| item.enabled && !item.key.trim().is_empty())
    {
        url.query_pairs_mut().append_pair(&item.key, &item.value);
    }
    let mut headers = HeaderMap::new();
    for item in spec
        .headers
        .iter()
        .filter(|item| item.enabled && !item.key.trim().is_empty())
    {
        headers.insert(
            HeaderName::from_str(&item.key).map_err(|e| e.to_string())?,
            HeaderValue::from_str(&item.value).map_err(|e| e.to_string())?,
        );
    }
    if let Some(id) = &spec.last_event_id {
        headers.insert(
            "last-event-id",
            HeaderValue::from_str(id).map_err(|e| e.to_string())?,
        );
    }
    match spec.auth.r#type.as_str() {
        "bearer" if !spec.auth.bearer_token.is_empty() => {
            headers.insert(
                "authorization",
                HeaderValue::from_str(&format!("Bearer {}", spec.auth.bearer_token))
                    .map_err(|e| e.to_string())?,
            );
        }
        "basic" => {
            let encoded = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", spec.auth.basic_user, spec.auth.basic_pass));
            headers.insert(
                "authorization",
                HeaderValue::from_str(&format!("Basic {encoded}")).unwrap(),
            );
        }
        _ => {}
    }
    let mut client = reqwest::Client::builder().danger_accept_invalid_certs(!spec.verify_ssl);
    if let Some(seconds) = spec.timeout_seconds {
        client = client.timeout(Duration::from_secs(seconds.clamp(1, 300) as u64));
    }
    let client = client.build().map_err(|e| e.to_string())?;
    let response = tokio::select! { response = client.get(url).headers(headers).send() => response.map_err(|e| e.to_string())?, _ = &mut *cancel => return Ok(()) };
    let status = response.status();
    let response_headers = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("<binary>").into()))
        .collect();
    state.publish(
        session_id,
        generation,
        buffer,
        StreamMessage::Open {
            at_ms: now_ms(),
            status: Some(status.as_u16()),
            headers: response_headers,
        },
    );
    if !status.is_success() {
        return Err(format!("HTTP {}", status.as_u16()));
    }
    let mut response = response;
    let mut parser = SseParser::default();
    loop {
        tokio::select! {
            chunk = response.chunk() => match chunk.map_err(|e| e.to_string())? {
                Some(bytes) => for frame in parser.feed(&bytes) { state.publish(session_id, generation, buffer, StreamMessage::SseEvent { at_ms: now_ms(), event: frame.event, data: frame.data, id: frame.id, retry_ms: frame.retry_ms, truncated: frame.truncated }); },
                None => { state.publish(session_id, generation, buffer, StreamMessage::Closed { at_ms: now_ms(), code: None, reason: "server closed stream".into() }); return Ok(()); }
            },
            _ = &mut *cancel => { state.publish(session_id, generation, buffer, StreamMessage::Closed { at_ms: now_ms(), code: Some(1000), reason: "client".into() }); return Ok(()); }
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
