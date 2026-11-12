const MAX_DATA_BYTES: usize = 1024 * 1024;

pub struct SseFrame {
    pub event: String,
    pub data: String,
    pub id: Option<String>,
    pub retry_ms: Option<u64>,
    pub truncated: bool,
}

#[derive(Default)]
pub struct SseParser {
    buffer: String,
    event: Option<String>,
    data: String,
    id: Option<String>,
    retry_ms: Option<u64>,
    truncated: bool,
    dirty: bool,
}

impl SseParser {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<SseFrame> {
        self.buffer.push_str(&String::from_utf8_lossy(bytes));
        let mut frames = Vec::new();
        while let Some(newline) = self.buffer.find('\n') {
            let end = if newline > 0 && self.buffer.as_bytes()[newline - 1] == b'\r' {
                newline - 1
            } else {
                newline
            };
            let line = self.buffer[..end].to_string();
            self.buffer.drain(..=newline);
            if line.is_empty() {
                if self.dirty {
                    frames.push(self.take());
                }
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = line
                .split_once(':')
                .map(|(f, v)| (f, v.strip_prefix(' ').unwrap_or(v)))
                .unwrap_or((&line, ""));
            match field {
                "event" => {
                    self.event = Some(value.into());
                    self.dirty = true;
                }
                "data" => {
                    if !self.data.is_empty() && self.data.len() < MAX_DATA_BYTES {
                        self.data.push('\n');
                    }
                    let room = MAX_DATA_BYTES.saturating_sub(self.data.len());
                    let mut end = room.min(value.len());
                    while end > 0 && !value.is_char_boundary(end) {
                        end -= 1;
                    }
                    self.data.push_str(&value[..end]);
                    self.truncated |= end < value.len();
                    self.dirty = true;
                }
                "id" => {
                    self.id = Some(value.into());
                    self.dirty = true;
                }
                "retry" => {
                    if let Ok(value) = value.parse() {
                        self.retry_ms = Some(value);
                        self.dirty = true;
                    }
                }
                _ => {}
            }
        }
        frames
    }

    fn take(&mut self) -> SseFrame {
        let frame = SseFrame {
            event: self.event.take().unwrap_or_else(|| "message".into()),
            data: std::mem::take(&mut self.data),
            id: self.id.take(),
            retry_ms: self.retry_ms.take(),
            truncated: self.truncated,
        };
        self.truncated = false;
        self.dirty = false;
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chunked_multiline_sse_frames() {
        let mut parser = SseParser::default();
        assert!(parser.feed(b"event: ans").is_empty());
        let frames = parser.feed(b"wer\nid: 7\ndata: hello\ndata: world\n\n");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event, "answer");
        assert_eq!(frames[0].id.as_deref(), Some("7"));
        assert_eq!(frames[0].data, "hello\nworld");
    }

    #[test]
    fn truncates_oversized_multibyte_data_at_a_character_boundary() {
        let payload = format!("data: {}\n\n", "中".repeat(MAX_DATA_BYTES));
        let mut parser = SseParser::default();
        let frames = parser.feed(payload.as_bytes());
        assert!(frames[0].truncated);
        assert!(frames[0].data.len() <= MAX_DATA_BYTES);
        assert!(frames[0].data.is_char_boundary(frames[0].data.len()));
    }
}
