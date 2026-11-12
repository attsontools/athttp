mod cancel;
mod models;
mod send;
mod storage;

pub use cancel::HttpCancelState;
pub use models::{HttpHistoryRow, HttpRequestSpec, HttpResponseInfo, HttpSendParams, HttpTabRow};
pub use send::send;
pub use storage::HttpStore;

use std::{path::PathBuf, sync::Arc};

#[derive(Clone)]
pub struct HttpService {
    cancel_state: Arc<HttpCancelState>,
    store: Arc<HttpStore>,
}

impl HttpService {
    pub fn new(data_directory: PathBuf) -> Result<Self, String> {
        Ok(Self {
            cancel_state: Arc::new(HttpCancelState::default()),
            store: Arc::new(HttpStore::new(data_directory)?),
        })
    }

    pub fn cancel_state(&self) -> &HttpCancelState {
        &self.cancel_state
    }

    pub fn store(&self) -> &HttpStore {
        &self.store
    }
}
