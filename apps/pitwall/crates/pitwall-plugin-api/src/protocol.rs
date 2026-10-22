//! The plugin contract on the wire: the host's [`Request`]s, the plugin's [`Reply`]s and the items of a streamed
//! generation ([`GenerateItem`]), all postcard bodies inside `pitwall-ipc` frames.
//!
//! **Versioning.** [`CONTRACT_VERSION`] is exchanged in the handshake. Within a major version changes are additive
//! only: a new variant goes at the **end** of its enum, a new manifest key is optional. Anything else (a changed
//! field, a removed variant, a reordering) is a new major version. postcard is not self-describing, so wire types
//! never use `untagged`, `flatten` or `skip_serializing_if`; JSON values cross as [`Json`] (JSON text).

use crate::manifest::Kind;
use pitwall_ipc::{ContractVersion, Json};
use serde::{Deserialize, Serialize};

/// The plugin contract this SDK speaks: 1.0.
pub const CONTRACT_VERSION: ContractVersion = ContractVersion::new(1, 0);

/// A request of the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    /// What the plugin is and offers (sent once after connecting).
    Describe,
    /// The user's settings for the plugin (sent after every start, before anything else but [`Request::Describe`]).
    Configure {
        /// A JSON object (empty when the user set nothing).
        settings: Json,
    },
    /// Filter: a chat/completion request body (OpenAI-compatible JSON) before it reaches the model.
    FilterRequest {
        /// The request body.
        body: Json,
    },
    /// Filter: a response body after the model answered.
    FilterResponse {
        /// The response body.
        body: Json,
    },
    /// Endpoint: an HTTP request to one of the plugin's endpoints.
    Endpoint(EndpointRequest),
    /// Tool: invoke one of the plugin's tools.
    InvokeTool {
        /// The tool's name, as described.
        tool: String,
        /// Its arguments (a JSON object).
        args: Json,
    },
    /// Engine: load a model; answered with [`Reply::Loaded`].
    EngineLoad {
        /// The model to load (a path or id the engine understands).
        model: String,
        /// Engine options (JSON object).
        options: Json,
    },
    /// Engine: unload a loaded model.
    EngineUnload {
        /// The handle [`Reply::Loaded`] gave.
        handle: String,
    },
    /// Engine: generate; a streamed call whose items are [`GenerateItem`]s, answered with [`Reply::Generated`].
    EngineGenerate {
        /// The handle of a loaded model.
        handle: String,
        /// The generation request (OpenAI-compatible JSON).
        request: Json,
    },
}

/// A plugin's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Reply {
    /// To [`Request::Describe`].
    Described(Description),
    /// To [`Request::Configure`].
    Configured,
    /// To [`Request::FilterRequest`].
    FilteredRequest(FilterVerdict),
    /// To [`Request::FilterResponse`]: the (possibly changed) response.
    FilteredResponse(Json),
    /// To [`Request::Endpoint`].
    Endpoint(EndpointResponse),
    /// To [`Request::InvokeTool`]: the tool's result.
    Tool(Json),
    /// To [`Request::EngineLoad`].
    Loaded {
        /// Names the loaded model in later requests.
        handle: String,
    },
    /// To [`Request::EngineUnload`].
    Unloaded,
    /// The end of [`Request::EngineGenerate`]: a summary (finish reason, token counts) as JSON.
    Generated(Json),
}

/// What a filter decided about a request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FilterVerdict {
    /// Pass the (possibly changed) request on.
    Forward(Json),
    /// Refuse it; the client gets `status` and `message`.
    Reject {
        /// An HTTP status (400–599).
        status: u16,
        /// Why, for the client.
        message: String,
    },
}

impl FilterVerdict {
    /// Forward `body`.
    pub fn forward(body: serde_json::Value) -> FilterVerdict {
        FilterVerdict::Forward(Json(body))
    }

    /// Reject with `status` (clamped to 400–599) and `message`.
    pub fn reject(status: u16, message: impl Into<String>) -> FilterVerdict {
        FilterVerdict::Reject {
            status: status.clamp(400, 599),
            message: message.into(),
        }
    }
}

/// An HTTP request to a plugin endpoint (the host already routed it: `path` is below the plugin's prefix).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointRequest {
    /// `GET`, `POST`, ...
    pub method: String,
    /// The path below `/plugins/<id>` (starts with `/`).
    pub path: String,
    /// The query string without `?` (may be empty).
    pub query: String,
    /// The request's content type, if any.
    pub content_type: Option<String>,
    /// The body (bounded by the host).
    pub body: Vec<u8>,
}

/// A plugin endpoint's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointResponse {
    /// HTTP status.
    pub status: u16,
    /// Content type of the body.
    pub content_type: String,
    /// The body.
    pub body: Vec<u8>,
}

impl EndpointResponse {
    /// A JSON response.
    pub fn json(status: u16, value: &serde_json::Value) -> EndpointResponse {
        EndpointResponse {
            status,
            content_type: "application/json".into(),
            body: value.to_string().into_bytes(),
        }
    }

    /// A plain-text response.
    pub fn text(status: u16, text: impl Into<String>) -> EndpointResponse {
        EndpointResponse {
            status,
            content_type: "text/plain; charset=utf-8".into(),
            body: text.into().into_bytes(),
        }
    }
}

/// One item of a streamed generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GenerateItem {
    /// Generated text.
    Text(String),
}

/// What a plugin is and offers, as it says itself (checked against its manifest by the host).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Description {
    /// Its id (must equal the manifest's).
    pub id: String,
    /// Its version.
    pub version: String,
    /// The contract it was built for.
    pub contract: ContractVersion,
    /// The kinds it implements (sorted; must equal the manifest's).
    pub kinds: Vec<Kind>,
    /// Its tools (kind tool).
    pub tools: Vec<ToolInfo>,
    /// Its endpoints (kind endpoint).
    pub endpoints: Vec<EndpointInfo>,
}

/// A tool a plugin offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Its name (unique within the plugin).
    pub name: String,
    /// What it does (shown to the model and the user).
    pub description: String,
    /// JSON Schema of its arguments.
    pub parameters: Json,
}

/// An endpoint a plugin serves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointInfo {
    /// `GET`, `POST`, ...
    pub method: String,
    /// The path below `/plugins/<id>` (starts with `/`).
    pub path: String,
    /// What it does.
    pub description: String,
}

/// Typed error codes of the plugin contract (beside `pitwall_ipc::peer::code`).
pub mod code {
    /// The request is not valid for this plugin (bad arguments, unknown tool or handle).
    pub const INVALID: i32 = pitwall_ipc::peer::code::INVALID;
    /// The plugin does not implement this kind of request.
    pub const UNSUPPORTED: i32 = pitwall_ipc::peer::code::UNIMPLEMENTED;
    /// The plugin failed in a way it did not classify.
    pub const FAILED: i32 = pitwall_ipc::peer::code::INTERNAL;
    /// Something the request names does not exist.
    pub const NOT_FOUND: i32 = 1404;
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitwall_ipc::wire;
    use serde_json::json;

    #[test]
    fn every_message_roundtrips_through_postcard() {
        let requests = vec![
            Request::Describe,
            Request::Configure {
                settings: json!({"audit": true}).into(),
            },
            Request::FilterRequest {
                body: json!({"messages": [{"role": "user", "content": "hé"}]}).into(),
            },
            Request::FilterResponse { body: json!({}).into() },
            Request::Endpoint(EndpointRequest {
                method: "POST".into(),
                path: "/x".into(),
                query: "a=1".into(),
                content_type: Some("application/json".into()),
                body: b"{}".to_vec(),
            }),
            Request::InvokeTool {
                tool: "t".into(),
                args: json!({"n": 1}).into(),
            },
            Request::EngineLoad {
                model: "m.gguf".into(),
                options: json!({}).into(),
            },
            Request::EngineUnload { handle: "h".into() },
            Request::EngineGenerate {
                handle: "h".into(),
                request: json!({"prompt": "p"}).into(),
            },
        ];
        for r in requests {
            assert_eq!(wire::decode::<Request>(&wire::encode(&r).unwrap()).unwrap(), r);
        }
        let replies = vec![
            Reply::Described(Description {
                id: "x".into(),
                version: "1".into(),
                contract: CONTRACT_VERSION,
                kinds: vec![Kind::Tool],
                tools: vec![ToolInfo {
                    name: "t".into(),
                    description: "d".into(),
                    parameters: json!({"type": "object"}).into(),
                }],
                endpoints: vec![EndpointInfo {
                    method: "GET".into(),
                    path: "/s".into(),
                    description: "d".into(),
                }],
            }),
            Reply::Configured,
            Reply::FilteredRequest(FilterVerdict::forward(json!({"a": 1}))),
            Reply::FilteredRequest(FilterVerdict::reject(1, "no")),
            Reply::FilteredResponse(json!({}).into()),
            Reply::Endpoint(EndpointResponse::json(200, &json!({"ok": true}))),
            Reply::Endpoint(EndpointResponse::text(404, "nope")),
            Reply::Tool(json!([1, 2]).into()),
            Reply::Loaded { handle: "h".into() },
            Reply::Unloaded,
            Reply::Generated(json!({"finish": "stop"}).into()),
        ];
        for r in replies {
            assert_eq!(wire::decode::<Reply>(&wire::encode(&r).unwrap()).unwrap(), r);
        }
        let item = GenerateItem::Text("tok".into());
        assert_eq!(wire::decode::<GenerateItem>(&wire::encode(&item).unwrap()).unwrap(), item);
        assert_eq!(
            FilterVerdict::reject(700, "x"),
            FilterVerdict::Reject {
                status: 599,
                message: "x".into()
            }
        );
    }

    /// The contract is additive within a major version: these are the variant indexes plugins built for 1.0 rely
    /// on. A failure here means a variant was inserted or reordered (a breaking change).
    #[test]
    fn variant_order_is_stable() {
        let index = |bytes: Vec<u8>| bytes[0];
        assert_eq!(index(wire::encode(&Request::Describe).unwrap()), 0);
        assert_eq!(index(wire::encode(&Request::EngineUnload { handle: String::new() }).unwrap()), 7);
        assert_eq!(index(wire::encode(&Reply::Unloaded).unwrap()), 7);
        assert_eq!(index(wire::encode(&Kind::Filter).unwrap()), 3);
    }
}
