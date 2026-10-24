//! The platform wire (ADR 0001, "Wire (extends ADR 0017 §1)"): what the app asks a plugin ([`PlatformRequest`]) and
//! what the plugin asks the app back while it answers ([`HostCall`]). Each message is the body of one `paddock-ipc`
//! frame (postcard); values that are JSON by nature (arguments, results, view documents, settings) cross as [`Json`].
//!
//! The platform knows nothing of an app's domain: app contracts ride in [`PlatformRequest::Extension`], whose `body` the
//! platform routes by namespace and never decodes. Wire rules: no `untagged`, `flatten`, internal tags or
//! `skip_serializing_if` in a postcard type; adding a variant at the end of an enum is a minor change.

use crate::tools::{FailureReason, ToolInfo, ToolTask};
use paddock_ipc::Json;
pub use paddock_ipc::frame::ProtocolVersion;
use paddock_settings::SettingsSection;
use serde::{Deserialize, Serialize};

/// The protocol this crate speaks. Protocol 1 was Debrief's own plugin protocol (ADR 0003 phase 3): a host refuses it
/// with [`crate::error::Refusal::Legacy`]; the shim that serves such plugins is the app's.
///
/// - **minor**: a request, call, reply or description field was added at the end (older plugins keep working);
/// - **major**: anything else (a plugin built for another major version must be rebuilt).
pub const PLATFORM_PROTOCOL: ProtocolVersion = ProtocolVersion::new(2, 0);

/// Largest piece of a host file one [`HostCall::ReadFile`] returns (1 MiB).
pub const READ_CHUNK: u32 = 1 << 20;
/// Largest piece of an output file in one [`HostCall::Output`].
pub const OUTPUT_CHUNK: usize = 256 << 10;
/// Largest output file the host accepts, whatever the plugin declares.
pub const MAX_OUTPUT_BYTES: u64 = 8 << 30;
/// Output files one plugin may have open at once.
pub const MAX_OUTPUTS: usize = 8;
/// Longest name of an output file.
pub const MAX_OUTPUT_NAME: usize = 200;
/// Longest forwarded log line (bytes); longer lines are cut at a character boundary.
pub const MAX_LOG_LINE: usize = 4096;
/// Most lines in one [`HostCall::Log`]; the host drops the rest.
pub const MAX_LOG_BATCH: usize = 64;
/// Code of a [`paddock_ipc::RemoteError`] the app answers a [`HostCall`] with when the plugin lacks a grant.
pub const DENIED: &str = "denied";

/// App to plugin. The reply of each request is documented on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlatformRequest {
    /// Asks what the plugin is and contributes. Reply: [`Json`] of a [`Described`].
    Describe,
    /// Gives the plugin's settings (validated by the app against the section the plugin described). Reply: `()`.
    ApplySettings {
        /// The values, a JSON object.
        json: Json,
    },
    /// Runs a tool operation. Reply: `Result<Json, Fail>`.
    Invoke {
        /// Tool id.
        tool: String,
        /// Operation name.
        op: String,
        /// Arguments, validated by the app against the operation's schema.
        args: Json,
    },
    /// Renders a view. Reply: `Result<Json, Fail>` where the JSON is a `paddock_view::ViewDoc`.
    RenderView {
        /// View id.
        view: String,
        /// What the app tells the view (selection, page, ...), a JSON object.
        ctx: Json,
    },
    /// Delivers a user action on a view. Reply: `Result<Json, Fail>` where the JSON is a `paddock_view::ViewPatch`.
    ViewAction {
        /// View id.
        view: String,
        /// Action id from the document.
        action: String,
        /// The action's arguments.
        args: Json,
    },
    /// Calls an MCP tool the plugin contributed (read-only). Reply: `Result<Json, Fail>`.
    McpTool {
        /// MCP tool name.
        tool: String,
        /// Arguments, validated by the app against the tool's schema.
        args: Json,
        /// What the call may use.
        budget: McpBudget,
    },
    /// Stops a background task. Reply: `Result<(), Fail>`.
    Stop {
        /// Task id, as announced with [`HostCall::TaskStarted`].
        task: String,
    },
    /// An app contract's request. The platform never decodes `body`. Reply: the contract's postcard-encoded reply, as it is.
    Extension {
        /// Namespace (`debrief.import`).
        ns: String,
        /// The minor version of the namespace the body is encoded for.
        minor: u16,
        /// The contract's postcard-encoded request.
        body: Vec<u8>,
    },
}

/// What an MCP tool call may use (the app's server enforces it again).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpBudget {
    /// Largest answer in bytes.
    pub max_bytes: u64,
    /// Longest time in milliseconds.
    pub max_ms: u64,
}

/// Plugin to app, while a request is being answered. The reply of each call is documented on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HostCall {
    /// Reads a range of a file the app opened for the plugin. Reply: the bytes (`Vec<u8>`), at most `len`.
    ReadFile {
        /// The host file's id.
        id: u64,
        /// Where to start.
        offset: u64,
        /// How many bytes, at most [`READ_CHUNK`].
        len: u32,
    },
    /// Progress of a task. Reply: `()`.
    Report {
        /// Task id.
        task: String,
        /// 0 to 1, or `None` when unknown.
        progress: Option<f32>,
        /// What it is doing now.
        note: String,
    },
    /// A background task started. Reply: `()`.
    TaskStarted {
        /// The tool that started it.
        tool: String,
        /// The task.
        task: ToolTask,
    },
    /// A background task ended. Reply: `()`.
    TaskFinished {
        /// Task id.
        task: String,
    },
    /// One piece of an output file the app writes into its own folder. Reply: `Option<SavedOutput>` (set after `last`).
    Output {
        /// The file name (see [`check_output_name`]).
        name: String,
        /// The bytes, at most [`OUTPUT_CHUNK`].
        bytes: Vec<u8>,
        /// Whether this is the end of the file.
        last: bool,
    },
    /// Runs a named elevated operation through the app. Reply: `Result<Json, Fail>`.
    Elevated {
        /// The tool.
        tool: String,
        /// The operation, one the manifest lists under `Elevated`.
        op: String,
        /// Its arguments.
        args: Json,
        /// How long the app waits.
        timeout_ms: u64,
    },
    /// Lines of the plugin's own log. Reply: `()`.
    Log {
        /// At most [`MAX_LOG_BATCH`] lines of at most [`MAX_LOG_LINE`] bytes.
        lines: Vec<String>,
        /// Lines the plugin dropped before these.
        dropped: u64,
    },
}

/// A typed failure of a call: the plugin answered, the process is fine, and it does not count as a fault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fail {
    /// What the app can branch on.
    pub reason: Option<FailureReason>,
    /// A message the user can act on.
    pub message: String,
}

impl Fail {
    /// A failure without a reason.
    pub fn new(message: impl Into<String>) -> Fail {
        Fail {
            reason: None,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Fail {}

/// The result of an output file's last piece.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedOutput {
    /// The name it was saved under.
    pub name: String,
    /// Its size.
    pub bytes: u64,
}

/// A view a plugin contributes, as the app lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewInfo {
    /// Stable id.
    pub id: String,
    /// The title in the app's lists.
    pub title: String,
    /// Whether any of its actions changes something (the app badges it and never exposes it over MCP).
    pub writes: bool,
}

/// An MCP tool a plugin contributes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolInfo {
    /// The tool name MCP clients see (the app prefixes the plugin id).
    pub name: String,
    /// One line for the model.
    pub description: String,
    /// Its arguments.
    pub args: crate::tools::Schema,
}

/// A namespace the plugin speaks and the minor version it was built for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionInfo {
    /// The namespace (`debrief.import`).
    pub ns: String,
    /// The minor version.
    pub minor: u16,
}

/// What a plugin says about itself on [`PlatformRequest::Describe`]. The app compares it with the manifest it verified:
/// a plugin that contributes more than it declared is refused.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Described {
    /// The plugin's id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Its version (semver).
    pub version: String,
    /// The platform protocol it was built for.
    pub protocol: ProtocolVersion,
    /// Memory it needs at most, in bytes.
    pub memory_limit: Option<u64>,
    /// Its tools.
    pub tools: Vec<ToolInfo>,
    /// Its settings section.
    pub settings: Option<SettingsSection>,
    /// Its views.
    pub views: Vec<ViewInfo>,
    /// Its MCP tools.
    pub mcp_tools: Vec<McpToolInfo>,
    /// The namespaces it speaks.
    pub extensions: Vec<ExtensionInfo>,
}

/// Checks the name of an output file or folder: letters, digits, `-`, `_` and `.`, no reserved Windows names.
pub fn check_output_name(what: &str, name: &str) -> Result<(), String> {
    let plain = name
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'));
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let numbered = stem.len() == 4
        && (stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.as_bytes()[3].is_ascii_digit();
    let reserved = numbered || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL");
    if name.is_empty()
        || name.len() > MAX_OUTPUT_NAME
        || !plain
        || name.starts_with('.')
        || name.ends_with('.')
        || reserved
    {
        return Err(format!(
            "invalid output {what} {name:?}: use one name of letters, digits, '-', '_' and '.' (at most {MAX_OUTPUT_NAME})"
        ));
    }
    Ok(())
}

/// `line` cut to at most `max` bytes at a character boundary.
pub fn cut_line(line: &str, max: usize) -> &str {
    if line.len() <= max {
        return line;
    }
    let mut end = max;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    &line[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{OpSpec, Schema};
    use paddock_ipc::wire::{decode, encode};
    use serde_json::json;

    #[test]
    fn requests_and_calls_roundtrip_through_postcard() {
        let requests = vec![
            PlatformRequest::Describe,
            PlatformRequest::ApplySettings {
                json: Json(json!({"a": [1, 2.5, "x"]})),
            },
            PlatformRequest::Invoke {
                tool: "t".into(),
                op: "o".into(),
                args: Json(json!({"n": 1})),
            },
            PlatformRequest::RenderView {
                view: "v".into(),
                ctx: Json(json!({})),
            },
            PlatformRequest::ViewAction {
                view: "v".into(),
                action: "a".into(),
                args: Json(json!(null)),
            },
            PlatformRequest::McpTool {
                tool: "m".into(),
                args: Json(json!({})),
                budget: McpBudget {
                    max_bytes: 10,
                    max_ms: 20,
                },
            },
            PlatformRequest::Stop { task: "t1".into() },
            PlatformRequest::Extension {
                ns: "x.y.v1".into(),
                minor: 3,
                body: vec![1, 2, 3],
            },
        ];
        for r in requests {
            assert_eq!(decode::<PlatformRequest>(&encode(&r).unwrap()).unwrap(), r);
        }
        let task = ToolTask {
            id: "a".into(),
            label: "A".into(),
            kind: "k".into(),
            stoppable: true,
        };
        let calls = vec![
            HostCall::ReadFile {
                id: 1,
                offset: 2,
                len: 3,
            },
            HostCall::Report {
                task: "a".into(),
                progress: Some(0.25),
                note: "n".into(),
            },
            HostCall::TaskStarted {
                tool: "t".into(),
                task,
            },
            HostCall::TaskFinished { task: "a".into() },
            HostCall::Output {
                name: "o.bin".into(),
                bytes: vec![9; 4],
                last: true,
            },
            HostCall::Elevated {
                tool: "t".into(),
                op: "o".into(),
                args: Json(json!({"k": "v"})),
                timeout_ms: 5,
            },
            HostCall::Log {
                lines: vec!["l".into()],
                dropped: 2,
            },
        ];
        for c in calls {
            assert_eq!(decode::<HostCall>(&encode(&c).unwrap()).unwrap(), c);
        }
        let fail = Fail {
            reason: Some(FailureReason::Denied),
            message: "no".into(),
        };
        assert_eq!(decode::<Fail>(&encode(&fail).unwrap()).unwrap(), fail);
        assert_eq!(
            (Fail::new("m").reason, Fail::new("m").to_string()),
            (None, "m".to_string())
        );
        let saved = SavedOutput {
            name: "a".into(),
            bytes: 5,
        };
        assert_eq!(
            decode::<SavedOutput>(&encode(&saved).unwrap()).unwrap(),
            saved
        );
    }

    #[test]
    fn a_description_roundtrips_as_json_and_the_protocol_is_two_point_zero() {
        assert_eq!(PLATFORM_PROTOCOL.to_string(), "2.0");
        let d = Described {
            id: "p".into(),
            name: "P".into(),
            version: "1.0.0".into(),
            protocol: PLATFORM_PROTOCOL,
            memory_limit: Some(1 << 20),
            tools: vec![
                crate::tools::ToolInfo::new("t", "T")
                    .op(OpSpec::new("o", "O").args(Schema::object().field("n", Schema::int(1, 2)))),
            ],
            settings: None,
            views: vec![ViewInfo {
                id: "v".into(),
                title: "V".into(),
                writes: true,
            }],
            mcp_tools: vec![McpToolInfo {
                name: "m".into(),
                description: "d".into(),
                args: Schema::object(),
            }],
            extensions: vec![ExtensionInfo {
                ns: "x.v1".into(),
                minor: 1,
            }],
        };
        let back: Described = serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn output_names_are_plain_and_lines_are_cut_at_a_character() {
        for ok in ["a.bin", "capture-1_final.etl", "A"] {
            check_output_name("file name", ok).unwrap();
        }
        for bad in [
            "",
            ".hidden",
            "trailing.",
            "a/b",
            "a b",
            "..",
            "CON",
            "com1.txt",
            "lpt9",
            "NUL.log",
            &"a".repeat(MAX_OUTPUT_NAME + 1),
        ] {
            let e = check_output_name("file name", bad).unwrap_err();
            assert!(e.starts_with("invalid output file name"), "{bad}: {e}");
        }
        assert_eq!(cut_line("abc", 5), "abc");
        assert_eq!(cut_line("abcdef", 3), "abc");
        assert_eq!(cut_line("aé", 2), "a");
    }
}
