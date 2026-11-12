mod buffer;
mod parser;
mod session;
mod sse;

pub use session::{HttpStreamState, StreamEvent};

use std::sync::Arc;

use crate::http::SseSpec;

pub fn open_sse(state: Arc<HttpStreamState>, session_id: String, spec: SseSpec) -> u64 {
    state.open_sse(session_id, spec)
}
