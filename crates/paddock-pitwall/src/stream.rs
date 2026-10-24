//! A streamed chat answer: server-sent events read as they arrive. [`ChatStream`] is an iterator over the text deltas;
//! dropping it closes the connection, which is how Pitwall hears a cancel. A stream is never retried after its first delta.

use crate::error::PitwallError;
use crate::loopback::Response;
use crate::wire::{Provenance, Usage};
use serde_json::Value;
use std::collections::VecDeque;

/// Largest single event line.
pub const MAX_EVENT: usize = 4 * 1024 * 1024;
/// Longest finish reason kept.
const MAX_FINISH: usize = 32;
/// Longest error message kept from an error event.
const MAX_MESSAGE: usize = 500;

/// The deltas of one streamed answer.
pub struct ChatStream {
    resp: Response,
    line: Vec<u8>,
    ready: VecDeque<String>,
    provenance: Provenance,
    finish_reason: Option<String>,
    usage: Usage,
    done: bool,
}

impl ChatStream {
    pub(crate) fn new(resp: Response, provenance: Provenance) -> ChatStream {
        ChatStream {
            resp,
            line: Vec::new(),
            ready: VecDeque::new(),
            provenance,
            finish_reason: None,
            usage: Usage::default(),
            done: false,
        }
    }

    /// Who answers (known from the response head, before the first delta).
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// `stop`, `length`, `tool_calls`, ...; `None` until the stream says it.
    pub fn finish_reason(&self) -> Option<&str> {
        self.finish_reason.as_deref()
    }

    /// Token counts, once the stream reported them.
    pub fn usage(&self) -> Usage {
        self.usage
    }

    /// Handles one event line; returns true at `[DONE]`.
    fn event(&mut self, text: &str) -> Result<bool, PitwallError> {
        let Some(data) = text.strip_prefix("data:") else {
            return Ok(false);
        };
        let data = data.trim_start();
        if data == "[DONE]" {
            return Ok(true);
        }
        let v: Value = serde_json::from_str(data)
            .map_err(|e| PitwallError::Protocol(format!("a stream event is not JSON: {e}")))?;
        if let Some(err) = v.get("error") {
            let message: String = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("no details")
                .chars()
                .take(MAX_MESSAGE)
                .collect();
            let code: String = err
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .chars()
                .take(64)
                .collect();
            return Err(PitwallError::Server {
                status: 500,
                code,
                message,
            });
        }
        if let Some(choice) = v.pointer("/choices/0") {
            if let Some(t) = choice
                .pointer("/delta/content")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
            {
                self.ready.push_back(t.to_string());
            }
            if let Some(f) = choice.get("finish_reason").and_then(Value::as_str) {
                self.finish_reason = Some(f.chars().take(MAX_FINISH).collect());
            }
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            self.usage = Usage::from_value(u);
        }
        Ok(false)
    }

    /// Reads until a delta is ready; `Ok(None)` at the end.
    fn advance(&mut self) -> Result<Option<String>, PitwallError> {
        loop {
            if let Some(t) = self.ready.pop_front() {
                return Ok(Some(t));
            }
            if self.done {
                return Ok(None);
            }
            let piece = self.resp.next_piece()?;
            if piece.is_empty() {
                // The body ended without `[DONE]`: an answer that never finished is a dropped connection.
                self.done = true;
                return if self.finish_reason.is_some() {
                    Ok(None)
                } else {
                    Err(PitwallError::Stopped)
                };
            }
            for &b in &piece {
                if b != b'\n' {
                    if self.line.len() >= MAX_EVENT {
                        return Err(PitwallError::Protocol("an event is too large".into()));
                    }
                    self.line.push(b);
                    continue;
                }
                let text = String::from_utf8_lossy(&self.line)
                    .trim_end_matches('\r')
                    .to_string();
                self.line.clear();
                if self.event(&text)? {
                    self.done = true;
                    break;
                }
            }
        }
    }
}

impl Iterator for ChatStream {
    type Item = Result<String, PitwallError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.advance() {
            Ok(Some(t)) => Some(Ok(t)),
            Ok(None) => None,
            Err(e) => {
                // After an error nothing more is read (the connection is closed on drop).
                self.done = true;
                self.ready.clear();
                Some(Err(e))
            }
        }
    }
}
