//! Server-sent events: an incremental, bounded decoder for engine streams (llama-server's `/v1/chat/completions`)
//! and the encoders the OpenAI and Anthropic front-ends write with. NDJSON (Ollama) is one JSON value per line.

/// Longest event Pitwall accepts from an engine (one token's chunk is a few hundred bytes).
pub const MAX_EVENT_BYTES: usize = 4 * 1024 * 1024;

/// One decoded event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// The `event:` field, if any.
    pub event: Option<String>,
    /// The `data:` lines, joined with `\n`.
    pub data: String,
}

/// Why a stream cannot be decoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SseError {
    /// An event grew past [`MAX_EVENT_BYTES`].
    #[error("the engine sent an event larger than {MAX_EVENT_BYTES} bytes")]
    TooLarge,
}

/// Feeds bytes in, takes complete events out.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
    pending: usize,
}

impl Decoder {
    /// A new decoder.
    pub fn new() -> Decoder {
        Decoder::default()
    }

    /// Feeds a chunk and returns the events it completed.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<Event>, SseError> {
        self.buf.extend_from_slice(chunk);
        let mut out = vec![];
        while let Some(nl) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=nl).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if !self.data.is_empty() || self.event.is_some() {
                    out.push(Event {
                        event: self.event.take(),
                        data: std::mem::take(&mut self.data).join("\n"),
                    });
                    self.pending = 0;
                }
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = line
                .split_once(':')
                .map_or((line, ""), |(f, v)| (f, v.strip_prefix(' ').unwrap_or(v)));
            self.pending += value.len();
            match field {
                "data" => self.data.push(value.to_string()),
                "event" => self.event = Some(value.to_string()),
                _ => {}
            }
            if self.pending > MAX_EVENT_BYTES {
                return Err(SseError::TooLarge);
            }
        }
        if self.buf.len() > MAX_EVENT_BYTES {
            return Err(SseError::TooLarge);
        }
        Ok(out)
    }
}

/// `data: <json>\n\n`.
pub fn data(json: &str) -> String {
    format!("data: {json}\n\n")
}

/// `event: <name>\ndata: <json>\n\n`.
pub fn named(name: &str, json: &str) -> String {
    format!("event: {name}\ndata: {json}\n\n")
}

/// Splits NDJSON input into complete lines, keeping the unfinished rest.
#[derive(Debug, Default)]
pub struct Lines {
    buf: Vec<u8>,
}

impl Lines {
    /// Feeds a chunk; returns complete non-empty lines.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<String>, SseError> {
        self.buf.extend_from_slice(chunk);
        let mut out = vec![];
        while let Some(nl) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=nl).collect();
            let s = String::from_utf8_lossy(&line).trim().to_string();
            if !s.is_empty() {
                out.push(s);
            }
        }
        if self.buf.len() > MAX_EVENT_BYTES {
            return Err(SseError::TooLarge);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn decodes_across_chunks() {
        let mut d = Decoder::new();
        assert!(d.feed(b"data: {\"a\"").unwrap().is_empty());
        let e = d
            .feed(b":1}\r\n\r\n: comment\nevent: done\ndata: x\ndata:y\nid: 3\nnofield\n\n\n")
            .unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(
            e[0],
            Event {
                event: None,
                data: "{\"a\":1}".into()
            }
        );
        assert_eq!(
            e[1],
            Event {
                event: Some("done".into()),
                data: "x\ny".into()
            }
        );
        let e = d.feed(b"event: only\n\n").unwrap();
        assert_eq!(e[0].data, "");
    }

    #[test]
    fn bounded() {
        let mut d = Decoder::new();
        assert_eq!(d.feed(&vec![b'a'; MAX_EVENT_BYTES + 1]), Err(SseError::TooLarge));
        let mut d = Decoder::new();
        let line = format!("data: {}\n", "a".repeat(MAX_EVENT_BYTES / 2 + 1));
        d.feed(line.as_bytes()).unwrap();
        assert_eq!(d.feed(line.as_bytes()), Err(SseError::TooLarge));
        let mut l = Lines::default();
        assert_eq!(l.feed(&vec![b'a'; MAX_EVENT_BYTES + 1]), Err(SseError::TooLarge));
        assert!(SseError::TooLarge.to_string().contains("larger"));
    }

    #[test]
    fn encoders_and_lines() {
        assert_eq!(data("{}"), "data: {}\n\n");
        assert_eq!(named("ping", "{}"), "event: ping\ndata: {}\n\n");
        let mut l = Lines::default();
        assert_eq!(l.feed(b"{\"a\":1}\n\n{\"b\"").unwrap(), vec!["{\"a\":1}"]);
        assert_eq!(l.feed(b":2}\n").unwrap(), vec!["{\"b\":2}"]);
    }

    proptest! {
        #[test]
        fn never_panics(chunks in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..200), 0..20)) {
            let mut d = Decoder::new();
            let mut l = Lines::default();
            for c in chunks {
                let _ = d.feed(&c);
                let _ = l.feed(&c);
            }
        }
    }
}
