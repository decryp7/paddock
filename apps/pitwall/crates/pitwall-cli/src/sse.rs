//! A parser for the server-sent events of a streamed OpenAI chat completion. Pure and bounded: bytes in, events out.

/// The longest event kept while waiting for its end (a hostile or broken stream cannot grow memory without limit).
pub const MAX_EVENT_BYTES: usize = 1024 * 1024;

/// What one event means to the CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Text to print.
    Delta(String),
    /// The server reported an error inside the stream.
    Error(String),
    /// `[DONE]`.
    Done,
}

/// Incremental parser: [`Parser::feed`] takes whatever bytes arrived.
#[derive(Debug, Default)]
pub struct Parser {
    buf: Vec<u8>,
}

impl Parser {
    /// Adds bytes and returns the events they complete. An oversized event is dropped and reported as an error.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Event> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(end) = find_blank_line(&self.buf) {
            let block: Vec<u8> = self.buf.drain(..end.0 + end.1).take(end.0).collect();
            out.extend(event(&block));
        }
        if self.buf.len() > MAX_EVENT_BYTES {
            self.buf.clear();
            out.push(Event::Error("the server sent an event larger than 1 MB".into()));
        }
        out
    }
}

/// The position and length of the first blank line (`\n\n` or `\r\n\r\n`).
fn find_blank_line(b: &[u8]) -> Option<(usize, usize)> {
    (0..b.len()).find_map(|i| {
        if b[i..].starts_with(b"\n\n") {
            Some((i, 2))
        } else if b[i..].starts_with(b"\r\n\r\n") {
            Some((i, 4))
        } else {
            None
        }
    })
}

fn event(block: &[u8]) -> Option<Event> {
    let text = String::from_utf8_lossy(block);
    let data: Vec<&str> = text
        .lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .map(|d| d.strip_prefix(' ').unwrap_or(d))
        .collect();
    if data.is_empty() {
        return None;
    }
    let data = data.join("\n");
    if data.trim() == "[DONE]" {
        return Some(Event::Done);
    }
    let v: serde_json::Value = serde_json::from_str(&data).ok()?;
    if let Some(e) = v.get("error") {
        let m = e
            .get("message")
            .and_then(|m| m.as_str())
            .or_else(|| e.as_str())
            .unwrap_or("the model reported an error");
        return Some(Event::Error(m.to_string()));
    }
    let delta = v.get("choices")?.get(0)?.get("delta")?.get("content")?.as_str()?;
    (!delta.is_empty()).then(|| Event::Delta(delta.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deltas_arrive_across_chunk_boundaries() {
        let mut p = Parser::default();
        assert!(p.feed(b"data: {\"choices\":[{\"delta\":{\"con").is_empty());
        let e = p.feed(b"tent\":\"Hi\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\" you\"}}]}\r\n\r\ndata: [DONE]\n\n");
        assert_eq!(e, vec![Event::Delta("Hi".into()), Event::Delta(" you".into()), Event::Done]);
    }

    #[test]
    fn noise_is_ignored() {
        let mut p = Parser::default();
        let e = p.feed(b": keep-alive\n\ndata: not json\n\ndata: {\"choices\":[{\"delta\":{}}]}\n\ndata: {\"choices\":[]}\n\ndata:{\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n");
        assert_eq!(e, vec![Event::Delta("x".into())]);
    }

    #[test]
    fn errors_in_the_stream() {
        let mut p = Parser::default();
        assert_eq!(
            p.feed(b"data: {\"error\":{\"message\":\"out of memory\"}}\n\ndata: {\"error\":\"plain\"}\n\ndata: {\"error\":{}}\n\n"),
            vec![
                Event::Error("out of memory".into()),
                Event::Error("plain".into()),
                Event::Error("the model reported an error".into())
            ]
        );
    }

    #[test]
    fn an_endless_event_is_cut_off() {
        let mut p = Parser::default();
        let big = vec![b'a'; MAX_EVENT_BYTES + 1];
        let e = p.feed(&big);
        assert!(matches!(e.as_slice(), [Event::Error(m)] if m.contains("1 MB")));
        assert!(p.feed(b"data: [DONE]\n\n") == vec![Event::Done]);
    }
}
