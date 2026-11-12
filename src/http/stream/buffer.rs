use std::collections::VecDeque;

use crate::http::StreamMessage;

pub const MAX_MESSAGES: usize = 2000;
pub const MAX_BYTES: usize = 5 * 1024 * 1024;

pub struct SessionBuffer {
    messages: VecDeque<StreamMessage>,
    bytes: usize,
    dropped: usize,
}

impl SessionBuffer {
    pub fn new() -> Self {
        Self {
            messages: VecDeque::new(),
            bytes: 0,
            dropped: 0,
        }
    }

    pub fn push(&mut self, message: StreamMessage) {
        self.bytes += message.approx_size();
        self.messages.push_back(message);
        while self.messages.len() > MAX_MESSAGES || self.bytes > MAX_BYTES {
            if let Some(message) = self.messages.pop_front() {
                self.bytes = self.bytes.saturating_sub(message.approx_size());
                self.dropped += 1;
            }
        }
    }

    pub fn take_truncated(&mut self, at_ms: u64) -> Option<StreamMessage> {
        if self.dropped == 0 {
            return None;
        }
        let dropped = std::mem::take(&mut self.dropped);
        Some(StreamMessage::BufferTruncated { at_ms, dropped })
    }

    pub fn snapshot(&self) -> Vec<StreamMessage> {
        self.messages.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(data: String) -> StreamMessage {
        StreamMessage::SseEvent {
            at_ms: 0,
            event: "message".into(),
            data,
            id: None,
            retry_ms: None,
            truncated: false,
        }
    }

    #[test]
    fn keeps_only_the_latest_two_thousand_messages() {
        let mut buffer = SessionBuffer::new();
        for index in 0..(MAX_MESSAGES + 3) {
            buffer.push(event(index.to_string()));
        }
        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.len(), 2000);
        assert!(matches!(&snapshot[0], StreamMessage::SseEvent { data, .. } if data == "3"));
        assert!(matches!(
            buffer.take_truncated(42),
            Some(StreamMessage::BufferTruncated { dropped: 3, .. })
        ));
    }

    #[test]
    fn keeps_buffer_payload_within_five_megabytes() {
        let mut buffer = SessionBuffer::new();
        let payload = "x".repeat(1024 * 1024 - 39);
        for _ in 0..8 {
            buffer.push(event(payload.clone()));
        }
        assert_eq!(buffer.snapshot().len(), 5);
    }
}
