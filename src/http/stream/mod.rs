mod buffer;
mod parser;
mod session;
mod sse;
mod ws;

pub use session::{HttpStreamState, StreamEvent};

use std::sync::Arc;

use crate::http::{SseSpec, WsSpec};

pub fn open_sse(state: Arc<HttpStreamState>, session_id: String, spec: SseSpec) -> u64 {
    state.open_sse(session_id, spec)
}

pub fn open_ws(state: Arc<HttpStreamState>, session_id: String, spec: WsSpec) -> u64 {
    state.open_ws(session_id, spec)
}
