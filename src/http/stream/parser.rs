const MAX_DATA_BYTES: usize = 1024 * 1024;
const MAX_LINE_BYTES: usize = 5 * 1024 * 1024;

pub struct SseFrame {
    pub event: String,
    pub data: String,
    pub id: Option<String>,
    pub retry_ms: Option<u64>,
    pub truncated: bool,
}

#[derive(Default)]
pub struct SseParser {
    buffer: Vec<u8>,
    event: Option<String>,
    data: String,
    id: Option<String>,
    retry_ms: Option<u64>,
    truncated: bool,
    dirty: bool,
}

impl SseParser {
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<SseFrame>, String> {
        let mut frames = Vec::new();
        let mut remaining = bytes;
        loop {
            let Some(newline) = remaining.iter().position(|byte| *byte == b'\n') else {
                self.extend_line(remaining)?;
                break;
            };
            self.extend_line(&remaining[..newline])?;
            remaining = &remaining[newline + 1..];
            let end = if self.buffer.last() == Some(&b'\r') {
                self.buffer.len() - 1
            } else {
                self.buffer.len()
            };
            let line = String::from_utf8_lossy(&self.buffer[..end]).into_owned();
            self.buffer.clear();
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
        Ok(frames)
    }

    fn extend_line(&mut self, bytes: &[u8]) -> Result<(), String> {
        let length = self
            .buffer
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| "SSE 单行超过 5 MB 上限".to_string())?;
        if length > MAX_LINE_BYTES {
            return Err("SSE 单行超过 5 MB 上限".into());
        }
        self.buffer.extend_from_slice(bytes);
        Ok(())
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
        assert!(parser.feed(b"event: ans").unwrap().is_empty());
        let frames = parser
            .feed(b"wer\nid: 7\ndata: hello\ndata: world\n\n")
            .unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event, "answer");
        assert_eq!(frames[0].id.as_deref(), Some("7"));
        assert_eq!(frames[0].data, "hello\nworld");
    }

    #[test]
    fn preserves_a_multibyte_character_split_across_chunks() {
        let payload = "data: 中\n\n".as_bytes();
        let mut parser = SseParser::default();

        assert!(parser.feed(&payload[..7]).unwrap().is_empty());
        let frames = parser.feed(&payload[7..]).unwrap();

        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].data, "中");
    }

    #[test]
    fn rejects_an_sse_line_larger_than_five_megabytes() {
        let mut parser = SseParser::default();
        let oversized = vec![b'x'; 5 * 1024 * 1024 + 1];

        let error = match parser.feed(&oversized) {
            Err(error) => error,
            Ok(_) => panic!("oversized line must fail"),
        };

        assert!(error.contains("5 MB"), "{error}");
    }

    #[test]
    fn truncates_oversized_multibyte_data_at_a_character_boundary() {
        let payload = format!("data: {}\n\n", "中".repeat(MAX_DATA_BYTES));
        let mut parser = SseParser::default();
        let frames = parser.feed(payload.as_bytes()).unwrap();
        assert!(frames[0].truncated);
        assert!(frames[0].data.len() <= MAX_DATA_BYTES);
        assert!(frames[0].data.is_char_boundary(frames[0].data.len()));
    }
}
