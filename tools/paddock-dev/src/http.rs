//! The HTTP port of the Gitea readers and writers: what the planner's run reader, the CI-run clients and the queue ask of the
//! network, so each can be driven by a fake in a test (ports and adapters). The real adapter ([`crate::wait::CurlHttp`]) lives in the
//! host layer; this trait is core because the repo layer's planner ([`crate::plan_since`]) reads runs through it too.

use serde_json::Value;

/// A GET that returns JSON. The token travels in a header and appears in no message.
pub trait Http {
    /// Fetches `url`; errors are texts without the token.
    fn get_json(&mut self, url: &str, token: &str) -> Result<Value, String>;
    /// Fetches `url` as text (a job log); errors are texts without the token.
    fn get_text(&mut self, url: &str, token: &str) -> Result<String, String> {
        let _ = (url, token);
        Err("this reader fetches JSON only".into())
    }
    /// Fetches `url` as bytes, at most `max_bytes` (an artifact's zip); errors are texts without the token.
    fn get_bytes(&mut self, url: &str, token: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
        let _ = (url, token, max_bytes);
        Err("this reader fetches JSON only".into())
    }
    /// The `Location` a GET of `url` answers with, `None` for an answer that is no redirect (the recorder of an artifact download
    /// keeps it); errors are texts without the token.
    fn get_redirect(&mut self, url: &str, token: &str) -> Result<Option<String>, String> {
        let _ = (url, token);
        Err("this reader fetches JSON only".into())
    }
    /// Sends `body` (or nothing) with `method` (`POST`, `PUT`, `PATCH`, `DELETE`) and returns the JSON answer (`null` for an
    /// empty one); errors are texts without the token. Only writers (`./dev pr`) use it.
    fn send_json(&mut self, method: &str, url: &str, token: &str, body: Option<&Value>) -> Result<Value, String> {
        let _ = (method, url, token, body);
        Err("this reader cannot write".into())
    }

    /// Posts `body` as JSON to `url` (a `send_json` with `POST`); the answer may be empty (`Null`, Gitea's 204).
    fn post_json(&mut self, url: &str, token: &str, body: &Value) -> Result<Value, String> {
        self.send_json("POST", url, token, Some(body))
    }
}
