mod cancel;
mod models;
mod openapi;
mod send;
mod storage;
pub mod stream;

pub use cancel::HttpCancelState;
pub use models::{
    HttpCollectionFolderRow, HttpCollectionRequestRow, HttpCollectionRow, HttpEnvRow,
    HttpEnvVarRow, HttpHistoryRow, HttpRequestSpec, HttpResponseInfo, HttpSendParams, HttpTabRow,
    SseSpec, StreamMessage,
};
pub use openapi::{fetch_openapi, OpenApiImportParams};
pub use send::send;
pub use storage::HttpStore;

use std::{path::PathBuf, sync::Arc};

#[derive(Clone)]
pub struct HttpService {
    cancel_state: Arc<HttpCancelState>,
    store: Arc<HttpStore>,
    stream_state: Arc<stream::HttpStreamState>,
}

impl HttpService {
    pub fn new(data_directory: PathBuf) -> Result<Self, String> {
        Ok(Self {
            cancel_state: Arc::new(HttpCancelState::default()),
            store: Arc::new(HttpStore::new(data_directory)?),
            stream_state: Arc::new(stream::HttpStreamState::default()),
        })
    }

    pub fn cancel_state(&self) -> &HttpCancelState {
        &self.cancel_state
    }

    pub fn store(&self) -> &HttpStore {
        &self.store
    }

    pub fn stream_state(&self) -> &Arc<stream::HttpStreamState> {
        &self.stream_state
    }
}
