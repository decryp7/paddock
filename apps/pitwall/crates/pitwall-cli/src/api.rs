//! The thin edge to the server: every call goes through `pitwall_loopback`, whose client can only reach 127.0.0.1.

use crate::exit::{from_loopback, CliError};
use pitwall_core::mgmt::CommandError;
use pitwall_loopback::{Client, LoopbackPort};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::time::Duration;

/// A client of the local server.
pub struct Api {
    client: Client,
    port: u16,
}

/// How long a streamed chat may be silent before the CLI gives up.
pub const STREAM_IDLE: Duration = Duration::from_secs(300);

impl Api {
    /// A client of `port`, authenticating with `bearer` (the admin key for management calls, an app key for chat).
    pub fn new(port: u16, bearer: Option<String>) -> Result<Api, CliError> {
        let p = LoopbackPort::new(port).ok_or_else(|| CliError::failure(format!("{port} is not a usable port (use 1024 to 65535).")))?;
        Ok(Api {
            client: Client::new(p, bearer),
            port,
        })
    }

    /// The port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// A management call (`POST /mgmt/v1/rpc/<method>`) answered as JSON.
    pub async fn rpc_json(&self, method: &str, args: Value, timeout: Duration) -> Result<Value, CliError> {
        let (status, body) = self
            .client
            .json("POST", &format!("/mgmt/v1/rpc/{method}"), Some(&args), timeout)
            .await
            .map_err(|e| from_loopback(&e, self.port))?;
        if status >= 400 {
            return Err(match serde_json::from_value::<CommandError>(body) {
                Ok(e) => e.into(),
                Err(_) => CliError::failure(format!(
                    "the server answered HTTP {status} on port {}. Is that Pitwall? Pass --port <n> if it listens elsewhere.",
                    self.port
                )),
            });
        }
        Ok(body)
    }

    /// A management call decoded into its DTO.
    pub async fn rpc<T: DeserializeOwned>(&self, method: &str, args: Value, timeout: Duration) -> Result<T, CliError> {
        decode(self.rpc_json(method, args, timeout).await?)
    }

    /// Streams a chat completion: `on_chunk` gets every piece of the body. Fails with the server's message on an
    /// error status.
    pub async fn chat_stream(&self, body: &Value, head_timeout: Duration, mut on_chunk: impl FnMut(&[u8]) -> bool) -> Result<(), CliError> {
        let bytes = serde_json::to_vec(body).map_err(|e| CliError::failure(e.to_string()))?;
        let mut resp = self
            .client
            .send("POST", "/v1/chat/completions", Some(bytes), head_timeout)
            .await
            .map_err(|e| from_loopback(&e, self.port))?;
        if resp.status >= 400 {
            let status = resp.status;
            let raw = resp.bytes(64 * 1024).await.unwrap_or_default();
            return Err(chat_error(status, &raw));
        }
        loop {
            match tokio::time::timeout(STREAM_IDLE, resp.chunk()).await {
                Err(_) => return Err(CliError::failure("the model stopped answering (no output for 5 minutes).")),
                Ok(Err(e)) => return Err(from_loopback(&e, self.port)),
                Ok(Ok(None)) => return Ok(()),
                Ok(Ok(Some(c))) => {
                    if !on_chunk(&c) {
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Decodes a DTO; a mismatch means the CLI and the server are different versions.
pub fn decode<T: DeserializeOwned>(v: Value) -> Result<T, CliError> {
    serde_json::from_value(v).map_err(|e| {
        CliError::failure(format!(
            "the server's answer was not what this pitwall expects ({e}). Update pitwall and Pitwall to the same version."
        ))
    })
}

/// The error of a failed chat request (OpenAI-shaped body), with a next step.
pub fn chat_error(status: u16, raw: &[u8]) -> CliError {
    let v: Value = serde_json::from_slice(raw).unwrap_or(Value::Null);
    let message = v
        .pointer("/error/message")
        .and_then(Value::as_str)
        .map_or_else(|| format!("the server answered HTTP {status}"), str::to_string);
    let message = message.trim_end_matches('.').to_string();
    match status {
        401 => CliError::failure(format!(
            "{message}. Set PITWALL_API_KEY to a key made with `pitwall keys create <app> --new` (the admin key is not used for chat)."
        )),
        403 => CliError::failure(format!(
            "{message}. This app key may not use that model; check the app's permissions in Pitwall."
        )),
        404 => CliError::failure(format!(
            "{message}. List what is installed with `pitwall models` and what is aliased with `pitwall presets`."
        )),
        429 => CliError {
            exit: crate::exit::Exit::Refused,
            message: format!("{message}. A limit for this app was reached; try again later."),
        },
        503 => CliError {
            exit: crate::exit::Exit::Refused,
            message: format!("{message}. Unload a model with `pitwall unload <model>` or pick a smaller one."),
        },
        _ => CliError::failure(format!("{message}.")),
    }
}

/// The JSON body of a streamed chat.
pub fn chat_body(model: &str, prompt: &str, system: Option<&str>, max_tokens: Option<u32>) -> Value {
    let mut messages = Vec::new();
    if let Some(s) = system {
        messages.push(json!({"role": "system", "content": s}));
    }
    messages.push(json!({"role": "user", "content": prompt}));
    let mut body = json!({"model": model, "stream": true, "messages": messages});
    if let (Some(n), Some(o)) = (max_tokens, body.as_object_mut()) {
        o.insert("max_tokens".into(), json!(n));
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exit::Exit;

    #[test]
    fn chat_body_shape() {
        let b = chat_body("fast", "hi", Some("be brief"), Some(8));
        assert_eq!(b["model"], "fast");
        assert_eq!(b["stream"], true);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][1]["content"], "hi");
        assert_eq!(b["max_tokens"], 8);
        let b = chat_body("m", "hi", None, None);
        assert_eq!(b["messages"].as_array().map(Vec::len), Some(1));
        assert!(b.get("max_tokens").is_none());
    }

    #[test]
    fn chat_errors_map_to_exit_codes_and_steps() {
        let body = br#"{"error":{"message":"not enough memory."}}"#;
        let e = chat_error(503, body);
        assert_eq!(e.exit, Exit::Refused);
        assert!(e.message.starts_with("not enough memory. ") && e.message.contains("pitwall unload"));
        assert_eq!(chat_error(429, body).exit, Exit::Refused);
        assert!(chat_error(401, body).message.contains("PITWALL_API_KEY"));
        assert!(chat_error(403, body).message.contains("permissions"));
        assert!(chat_error(404, body).message.contains("pitwall models"));
        let e = chat_error(500, b"<html>");
        assert_eq!(e.exit, Exit::Error);
        assert!(e.message.contains("HTTP 500"));
    }

    #[test]
    fn decode_reports_a_version_mismatch() {
        assert!(decode::<u32>(json!("x")).unwrap_err().message.contains("same version"));
        assert_eq!(decode::<u32>(json!(4)).unwrap(), 4);
    }

    #[test]
    fn a_bad_port_is_refused_before_any_call() {
        assert!(Api::new(0, None).is_err());
        assert_eq!(Api::new(11435, None).map(|a| a.port()).ok(), Some(11435));
    }
}
