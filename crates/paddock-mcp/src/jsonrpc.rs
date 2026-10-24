//! JSON-RPC 2.0 as MCP uses it: requests with an id, notifications without, responses with a result or an error. Batches are
//! not part of MCP (2025-06-18) and are refused.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// A request id: a number or a string (MCP forbids `null`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
    /// A number.
    Num(i64),
    /// A string.
    Str(String),
}

/// JSON-RPC error codes.
pub mod code {
    /// The line is not JSON.
    pub const PARSE_ERROR: i64 = -32700;
    /// Not a request object (or a batch, or too large).
    pub const INVALID_REQUEST: i64 = -32600;
    /// No such method.
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// Wrong parameters (also an unknown tool).
    pub const INVALID_PARAMS: i64 = -32602;
    /// The server failed.
    pub const INTERNAL_ERROR: i64 = -32603;
    /// MCP: the resource does not exist.
    pub const RESOURCE_NOT_FOUND: i64 = -32002;
}

/// An error object (`{"code", "message", "data"?}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    /// One of [`code`].
    pub code: i64,
    /// What went wrong, for a person.
    pub message: String,
    /// More detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    /// An error without data.
    pub fn new(code: i64, message: impl Into<String>) -> RpcError {
        RpcError {
            code,
            message: message.into(),
            data: None,
        }
    }
}

/// A message from the client.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// Wants an answer.
    Request {
        /// The id to answer with.
        id: Id,
        /// The method (`tools/call`).
        method: String,
        /// The parameters (an object, empty when absent).
        params: Map<String, Value>,
    },
    /// Wants none.
    Notification {
        /// The method (`notifications/initialized`).
        method: String,
        /// The parameters.
        params: Map<String, Value>,
    },
    /// A response to something the server sent; this server sends no requests, so it is ignored.
    Response,
}

/// Reads one line. The error is the response to send (with the id when it could be read).
pub fn parse(line: &str) -> Result<Incoming, (Option<Id>, RpcError)> {
    let value: Value = serde_json::from_str(line).map_err(|e| {
        (
            None,
            RpcError::new(code::PARSE_ERROR, format!("not JSON: {e}")),
        )
    })?;
    let Value::Object(obj) = value else {
        let what = if value.is_array() {
            "batches are not supported by MCP; send one message per line"
        } else {
            "a message is a JSON object"
        };
        return Err((None, RpcError::new(code::INVALID_REQUEST, what)));
    };
    let id = match obj.get("id") {
        None => None,
        Some(Value::Number(n)) if n.is_i64() => Some(Id::Num(n.as_i64().unwrap_or_default())),
        Some(Value::String(s)) => Some(Id::Str(s.clone())),
        Some(_) => {
            return Err((
                None,
                RpcError::new(code::INVALID_REQUEST, "id must be a string or an integer"),
            ));
        }
    };
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err((
            id,
            RpcError::new(code::INVALID_REQUEST, "jsonrpc must be \"2.0\""),
        ));
    }
    if obj.contains_key("result") || obj.contains_key("error") {
        return Ok(Incoming::Response);
    }
    let Some(method) = obj
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return Err((
            id,
            RpcError::new(code::INVALID_REQUEST, "method must be a string"),
        ));
    };
    let params = match obj.get("params") {
        None => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => {
            return Err((
                id,
                RpcError::new(code::INVALID_PARAMS, "params must be an object"),
            ));
        }
    };
    Ok(match id {
        Some(id) => Incoming::Request { id, method, params },
        None => Incoming::Notification { method, params },
    })
}

/// A success response line.
pub fn result_line(id: &Id, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

/// An error response line; `id` is `null` when the request's id could not be read.
pub fn error_line(id: Option<&Id>, error: &RpcError) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": error}).to_string()
}

/// A request line (client side).
pub fn request_line(id: &Id, method: &str, params: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
}

/// A notification line (client side).
pub fn notification_line(method: &str, params: Option<Value>) -> String {
    match params {
        Some(p) => json!({"jsonrpc": "2.0", "method": method, "params": p}).to_string(),
        None => json!({"jsonrpc": "2.0", "method": method}).to_string(),
    }
}

/// A response read by a client: the id and either the result or the error.
pub fn parse_response(line: &str) -> Result<(Option<Id>, Result<Value, RpcError>), String> {
    let v: Value = serde_json::from_str(line).map_err(|e| format!("not JSON: {e}"))?;
    let id = v
        .get("id")
        .and_then(|i| serde_json::from_value::<Id>(i.clone()).ok());
    if let Some(e) = v.get("error") {
        let e: RpcError =
            serde_json::from_value(e.clone()).map_err(|e| format!("bad error object: {e}"))?;
        return Ok((id, Err(e)));
    }
    match v.get("result") {
        Some(r) => Ok((id, Ok(r.clone()))),
        None => Err("a response has a result or an error".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_notifications_and_responses_are_told_apart() {
        let r = parse(r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#).unwrap();
        assert_eq!(
            r,
            Incoming::Request {
                id: Id::Num(7),
                method: "ping".into(),
                params: Map::new()
            }
        );
        let r = parse(r#"{"jsonrpc":"2.0","id":"a","method":"x","params":{"k":1}}"#).unwrap();
        assert!(
            matches!(r, Incoming::Request { id: Id::Str(ref s), ref params, .. } if s == "a" && params["k"] == 1)
        );
        assert!(matches!(
            parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).unwrap(),
            Incoming::Notification { .. }
        ));
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#).unwrap(),
            Incoming::Response
        );
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","id":1,"error":{"code":1,"message":"m"}}"#).unwrap(),
            Incoming::Response
        );
    }

    #[test]
    fn malformed_messages_get_the_standard_errors() {
        let code_of = |s: &str| parse(s).unwrap_err().1.code;
        assert_eq!(code_of("{nope"), code::PARSE_ERROR);
        assert_eq!(code_of("[{\"jsonrpc\":\"2.0\"}]"), code::INVALID_REQUEST);
        assert_eq!(code_of("3"), code::INVALID_REQUEST);
        assert_eq!(
            code_of(r#"{"jsonrpc":"1.0","id":1,"method":"x"}"#),
            code::INVALID_REQUEST
        );
        assert_eq!(
            code_of(r#"{"jsonrpc":"2.0","id":1}"#),
            code::INVALID_REQUEST
        );
        assert_eq!(
            code_of(r#"{"jsonrpc":"2.0","id":null,"method":"x"}"#),
            code::INVALID_REQUEST
        );
        assert_eq!(
            code_of(r#"{"jsonrpc":"2.0","id":1.5,"method":"x"}"#),
            code::INVALID_REQUEST
        );
        assert_eq!(
            code_of(r#"{"jsonrpc":"2.0","id":1,"method":"x","params":[1]}"#),
            code::INVALID_PARAMS
        );
        let (id, _) = parse(r#"{"jsonrpc":"2.0","id":9,"method":"x","params":3}"#).unwrap_err();
        assert_eq!(id, Some(Id::Num(9)), "the id is kept when it could be read");
    }

    #[test]
    fn lines_are_one_json_object_each() {
        let ok = result_line(&Id::Num(1), json!({"a": 1}));
        assert_eq!(
            serde_json::from_str::<Value>(&ok).unwrap(),
            json!({"jsonrpc":"2.0","id":1,"result":{"a":1}})
        );
        let err = error_line(None, &RpcError::new(code::PARSE_ERROR, "x"));
        assert_eq!(
            serde_json::from_str::<Value>(&err).unwrap()["id"],
            Value::Null
        );
        assert!(!ok.contains('\n') && !err.contains('\n'));
        assert_eq!(
            parse_response(&ok).unwrap(),
            (Some(Id::Num(1)), Ok(json!({"a": 1})))
        );
        let (id, e) = parse_response(&error_line(
            Some(&Id::Str("q".into())),
            &RpcError::new(code::METHOD_NOT_FOUND, "m"),
        ))
        .unwrap();
        assert_eq!(
            (id, e.unwrap_err().code),
            (Some(Id::Str("q".into())), code::METHOD_NOT_FOUND)
        );
        assert!(parse_response("{}").is_err() && parse_response("x").is_err());
        assert!(parse_response(r#"{"id":1,"error":3}"#).is_err());
        let req = request_line(&Id::Num(2), "ping", json!({}));
        assert!(matches!(parse(&req).unwrap(), Incoming::Request { .. }));
        assert!(matches!(
            parse(&notification_line("n", Some(json!({"a":1})))).unwrap(),
            Incoming::Notification { .. }
        ));
        assert!(matches!(
            parse(&notification_line("n", None)).unwrap(),
            Incoming::Notification { .. }
        ));
    }
}
