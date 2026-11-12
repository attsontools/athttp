use std::{
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, Mutex},
};

use tokio::sync::{broadcast, oneshot};

use super::{buffer::SessionBuffer, sse::run_sse};
use crate::http::{SseSpec, StreamMessage};

#[derive(Debug, Clone)]
pub struct StreamEvent {
    pub session_id: String,
    pub generation: u64,
    pub message: StreamMessage,
}

struct SessionHandle {
    cancel: oneshot::Sender<()>,
    buffer: Arc<Mutex<SessionBuffer>>,
    generation: u64,
}

pub struct HttpStreamState {
    sessions: Mutex<HashMap<String, SessionHandle>>,
    next_generation: AtomicU64,
    events: broadcast::Sender<StreamEvent>,
}

impl Default for HttpStreamState {
    fn default() -> Self {
        let (events, _) = broadcast::channel(2048);
        Self {
            sessions: Mutex::new(HashMap::new()),
            next_generation: AtomicU64::new(1),
            events,
        }
    }
}

impl HttpStreamState {
    pub fn subscribe(&self) -> broadcast::Receiver<StreamEvent> {
        self.events.subscribe()
    }

    pub fn open_sse(self: &Arc<Self>, session_id: String, spec: SseSpec) -> u64 {
        self.close(&session_id);
        let generation = self.next_generation.fetch_add(1, Ordering::SeqCst);
        let buffer = Arc::new(Mutex::new(SessionBuffer::new()));
        let (cancel, receiver) = oneshot::channel();
        self.sessions.lock().unwrap().insert(
            session_id.clone(),
            SessionHandle {
                cancel,
                buffer: Arc::clone(&buffer),
                generation,
            },
        );
        let state = Arc::clone(self);
        tokio::spawn(async move {
            run_sse(state, session_id, spec, receiver, buffer, generation).await;
        });
        generation
    }

    pub fn publish(
        &self,
        session_id: &str,
        generation: u64,
        buffer: &Arc<Mutex<SessionBuffer>>,
        message: StreamMessage,
    ) {
        if self.generation(session_id) != Some(generation) {
            return;
        }
        let _ = self.events.send(StreamEvent {
            session_id: session_id.into(),
            generation,
            message: message.clone(),
        });
        if let Ok(mut buffer) = buffer.lock() {
            buffer.push(message);
            if let Some(meta) = buffer.take_truncated(now_ms()) {
                let _ = self.events.send(StreamEvent {
                    session_id: session_id.into(),
                    generation,
                    message: meta.clone(),
                });
                buffer.push(meta);
            }
        }
    }

    pub fn close(&self, session_id: &str) -> bool {
        let session = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut sessions| sessions.remove(session_id));
        if let Some(session) = session {
            let _ = session.cancel.send(());
            true
        } else {
            false
        }
    }

    pub fn snapshot(&self, session_id: &str) -> Vec<StreamMessage> {
        let buffer = self
            .sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.get(session_id).map(|s| Arc::clone(&s.buffer)));
        buffer
            .and_then(|buffer| buffer.lock().ok().map(|buffer| buffer.snapshot()))
            .unwrap_or_default()
    }

    fn generation(&self, session_id: &str) -> Option<u64> {
        self.sessions
            .lock()
            .ok()
            .and_then(|sessions| sessions.get(session_id).map(|s| s.generation))
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
