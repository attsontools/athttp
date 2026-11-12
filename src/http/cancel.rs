use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::oneshot;

#[derive(Default)]
pub struct HttpCancelState {
    inner: Mutex<HashMap<String, oneshot::Sender<()>>>,
}

impl HttpCancelState {
    pub fn register(&self, id: String) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        if let Ok(mut map) = self.inner.lock() {
            map.insert(id, tx);
        }
        rx
    }

    pub fn unregister(&self, id: &str) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(id);
        }
    }

    pub fn cancel(&self, id: &str) -> bool {
        let sender = self.inner.lock().ok().and_then(|mut map| map.remove(id));
        sender.map(|tx| tx.send(()).is_ok()).unwrap_or(false)
    }
}
