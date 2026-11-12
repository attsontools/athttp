mod cancel;
mod models;
mod send;

pub use cancel::HttpCancelState;
pub use models::{HttpRequestSpec, HttpResponseInfo, HttpSendParams};
pub use send::send;

use std::sync::Arc;

#[derive(Clone, Default)]
pub struct HttpService {
    cancel_state: Arc<HttpCancelState>,
}

impl HttpService {
    pub fn cancel_state(&self) -> &HttpCancelState {
        &self.cancel_state
    }
}
