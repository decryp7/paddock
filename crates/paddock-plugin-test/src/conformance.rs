//! The checks every plugin runs. [`assert_conforms`] starts the plugin in a [`FakeHost`] and runs all of them; [`run`] returns
//! the findings instead of panicking.
//!
//! 1. **describe**: it describes itself with the platform protocol, valid ids, schemas and settings;
//! 2. **unknown requests**: an unknown tool, view, MCP tool, contract or task is refused, never answered;
//! 3. **newer contract minor**: a request for a newer minor of a contract it speaks is refused with text that says it must be
//!    updated;
//! 4. **malformed input**: arguments and bodies that do not fit are refused and the plugin stays up;
//! 5. **views**: every view renders a document within the limits (or a typed failure);
//! 6. **limits**: an answer over the MCP budget is refused, settings with a wrong type are refused, the defaults are accepted.
//!
//! Operations are only ever called with arguments that fail validation, so the checks never run a plugin's real work.

use crate::fake::{CallFailure, FakeHost};
use paddock_ipc::Json;
use paddock_ipc::codes;
use paddock_plugin_api::Plugin;
use paddock_plugin_api::protocol::{Described, McpBudget, PLATFORM_PROTOCOL, PlatformRequest};
use serde_json::{Value, json};
use std::sync::Arc;

/// A check a plugin failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The check.
    pub check: &'static str,
    /// What is wrong.
    pub problem: String,
}

struct Report(Vec<Finding>);

impl Report {
    fn add(&mut self, check: &'static str, problem: impl Into<String>) {
        self.0.push(Finding {
            check,
            problem: problem.into(),
        });
    }

    fn require(&mut self, check: &'static str, ok: bool, problem: impl FnOnce() -> String) {
        if !ok {
            self.add(check, problem());
        }
    }
}

fn is_remote(r: &Result<impl Sized, CallFailure>, code: &str) -> bool {
    matches!(r, Err(CallFailure::Remote(e)) if e.code == code)
}

/// Runs every check against the plugin behind `host`.
pub fn run(host: &FakeHost) -> Vec<Finding> {
    let mut report = Report(Vec::new());
    let described = match host.describe() {
        Ok(d) => d,
        Err(e) => {
            report.add("describe", format!("it did not describe itself ({e})"));
            return report.0;
        }
    };
    describe(&mut report, &described);
    unknown_requests(&mut report, host);
    newer_minor(&mut report, host, &described);
    malformed(&mut report, host, &described);
    views(&mut report, host, &described);
    limits(&mut report, host, &described);
    report.require(
        "alive",
        !host.is_closed() && host.describe().is_ok(),
        || "the plugin did not survive the checks".into(),
    );
    report.0
}

fn describe(report: &mut Report, d: &Described) {
    report.require("describe", d.protocol == PLATFORM_PROTOCOL, || {
        format!(
            "it was built for protocol {}, this platform speaks {PLATFORM_PROTOCOL}",
            d.protocol
        )
    });
    report.require(
        "describe",
        !d.id.is_empty() && !d.name.trim().is_empty() && !d.version.is_empty(),
        || "id, name and version are required".into(),
    );
    for t in &d.tools {
        if let Err(e) = t.check() {
            report.add("describe", e);
        }
    }
    if let Some(Err(e)) = d.settings.as_ref().map(|s| s.check()) {
        report.add("describe", format!("settings: {e}"));
    }
    for e in &d.extensions {
        report.require("describe", !e.ns.is_empty(), || {
            "a contract has no namespace".into()
        });
    }
}

fn unknown_requests(report: &mut Report, host: &FakeHost) {
    let c = "unknown requests";
    report.require(
        c,
        is_remote(&host.invoke("no.such.tool", "x", json!({})), codes::INVALID),
        || "an unknown tool was not refused".into(),
    );
    report.require(
        c,
        is_remote(&host.render_view("no.such.view", json!({})), codes::INVALID),
        || "an unknown view was not refused".into(),
    );
    let budget = McpBudget {
        max_bytes: 1 << 20,
        max_ms: 1000,
    };
    report.require(
        c,
        is_remote(
            &host.mcp_tool("no.such.tool", json!({}), budget),
            codes::INVALID,
        ),
        || "an unknown MCP tool was not refused".into(),
    );
    let body = paddock_ipc::wire::encode(&()).unwrap_or_default();
    let ext = host.raw(&PlatformRequest::Extension {
        ns: "no.such.contract".into(),
        minor: 0,
        body,
    });
    report.require(
        c,
        matches!(&ext, Err(paddock_ipc::IpcError::Remote(e)) if e.code == codes::UNIMPLEMENTED),
        || "an unknown contract was not refused".into(),
    );
    report.require(
        c,
        matches!(host.stop_task("no-such-task"), Err(CallFailure::Fail(_))),
        || "stopping an unknown task was not a typed failure".into(),
    );
}

fn newer_minor(report: &mut Report, host: &FakeHost, d: &Described) {
    for e in &d.extensions {
        let asked = host.raw(&PlatformRequest::Extension {
            ns: e.ns.clone(),
            minor: e.minor.saturating_add(1),
            body: Vec::new(),
        });
        let ok = matches!(&asked, Err(paddock_ipc::IpcError::Remote(r)) if r.code == codes::VERSION && r.message.contains("update it"));
        report.require("newer contract minor", ok, || {
            format!(
                "a request for a newer minor of \"{}\" was not refused with 'update it'",
                e.ns
            )
        });
    }
}

fn malformed(report: &mut Report, host: &FakeHost, d: &Described) {
    let c = "malformed input";
    for t in &d.tools {
        for op in &t.ops {
            for bad in [
                Value::Null,
                json!([1, 2, 3]),
                json!("text"),
                json!({ "\u{0}unknown": 1 }),
            ] {
                // Arguments that cannot be valid for any schema with an object root; an `Any` schema takes them all, so
                // only an answer that is neither a result nor a refusal counts.
                let r = host.invoke(&t.id, &op.name, bad);
                report.require(c, !matches!(&r, Err(CallFailure::Ipc(_))), || {
                    format!(
                        "{} {}: malformed arguments broke the connection",
                        t.id, op.name
                    )
                });
            }
        }
    }
    for e in &d.extensions {
        let garbled = host.raw(&PlatformRequest::Extension {
            ns: e.ns.clone(),
            minor: 0,
            body: vec![0xff; 16],
        });
        report.require(
            c,
            matches!(&garbled, Err(paddock_ipc::IpcError::Remote(r)) if r.code == codes::INVALID),
            || format!("a garbled body for \"{}\" was not refused as invalid", e.ns),
        );
    }
    report.require(c, host.raw(&PlatformRequest::Describe).is_ok(), || {
        "the plugin stopped answering after malformed input".into()
    });
}

fn views(report: &mut Report, host: &FakeHost, d: &Described) {
    for v in &d.views {
        match host.render_view(&v.id, json!({})) {
            Ok(doc) => {
                if let Err(e) = paddock_view::validate(&doc) {
                    report.add(
                        "views",
                        format!("view {} returned an invalid document ({e})", v.id),
                    );
                }
            }
            Err(CallFailure::Fail(_)) => {}
            Err(e) => report.add("views", format!("view {} did not render ({e})", v.id)),
        }
    }
}

fn limits(report: &mut Report, host: &FakeHost, d: &Described) {
    let c = "limits";
    for t in &d.mcp_tools {
        // A budget of no bytes: any answer is over it, so the platform must turn it into a typed failure.
        let r = host.mcp_tool(
            &t.name,
            json!({}),
            McpBudget {
                max_bytes: 0,
                max_ms: 1000,
            },
        );
        report.require(c, r.is_err(), || {
            format!("MCP tool {} answered over a budget of 0 bytes", t.name)
        });
    }
    if let Some(section) = &d.settings {
        report.require(
            c,
            host.apply_settings(Value::Object(
                section
                    .fields
                    .iter()
                    .map(|f| (f.key.clone(), f.default.clone()))
                    .collect(),
            ))
            .is_ok(),
            || "the plugin refused its own default settings".into(),
        );
        if let Some(field) = section.fields.first() {
            let bad = json!({ field.key.clone(): [1, 2, 3] });
            let r = host.raw(&PlatformRequest::ApplySettings { json: Json(bad) });
            report.require(
                c,
                matches!(&r, Err(paddock_ipc::IpcError::Remote(e)) if e.code == codes::INVALID),
                || {
                    format!(
                        "settings of the wrong type for \"{}\" were not refused",
                        field.key
                    )
                },
            );
        }
    }
}

/// Starts `plugin` in a [`FakeHost`], runs the checks and panics with every finding. The one line of a plugin's conformance test.
///
/// # Panics
///
/// When a check fails.
pub fn assert_conforms(plugin: Arc<dyn Plugin>) {
    let host = FakeHost::start(plugin).expect("the plugin connects to the fake host");
    let findings = run(&host);
    assert!(
        findings.is_empty(),
        "the plugin fails {} conformance check(s):\n{}",
        findings.len(),
        findings
            .iter()
            .map(|f| format!("  [{}] {}", f.check, f.problem))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
