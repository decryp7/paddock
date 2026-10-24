//! The sample plugin the tests start: one tool, one settings section, one view, one MCP tool and one contract namespace.
//! The host crate's tests run it as a real process (`paddock-plugin-fixture`); the fake host runs it in-process. It also
//! shows what a plugin looks like.

use paddock_ipc::{RemoteError, codes};
use paddock_plugin_api::host_file::{HostFile, HostFileRef};
use paddock_plugin_api::output::OutputWriter;
use paddock_plugin_api::prelude::*;
use paddock_plugin_api::protocol::{McpBudget, McpToolInfo, ViewInfo};
use paddock_plugin_api::tools::ToolTask;
use paddock_settings::{ApplyError, Configurable, FieldKind, SectionOwner, SettingField, SettingsSection, ValidValues};
use paddock_view::{ActionRef, Node, NodeKind, SCHEMA, ViewAction, ViewDoc, ViewPatch};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

/// The fixture's id, as its manifest names it.
pub const ID: &str = "net.example.fixture";
/// The fixture's tool.
pub const TOOL: &str = "fixture.tool";
/// The fixture's settings section.
pub const SETTINGS: &str = "fixture.sample";
/// The fixture's view.
pub const VIEW: &str = "fixture.view";
/// The fixture's action on its view.
pub const ACTION: &str = "refresh";
/// The fixture's MCP tool.
pub const MCP_TOOL: &str = "fixture_echo";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// What the app asks over the fixture's contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FixtureRequest {
    /// Adds two numbers.
    Add(u32, u32),
    /// Answers the same text.
    Echo(String),
    /// Panics inside the handler.
    Panic,
    /// Answers a typed failure.
    Fail,
    /// Never answers and ignores the cancel (a hang).
    Hang,
    /// Ends the process at once (a crash).
    Exit(i32),
    /// Answers this many bytes (an oversized frame when it exceeds the limit).
    Big(usize),
    /// Answers the bytes of the host file.
    Read(HostFileRef),
    /// Added in minor 1: answers its text twice.
    Twice(String),
}

/// What the fixture answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FixtureReply {
    /// A sum.
    Sum(u32),
    /// A text.
    Text(String),
    /// Bytes.
    Bytes(Vec<u8>),
}

/// The fixture's contract: `paddock.fixture.v1`, minor 1.
pub struct FixtureContract;

impl Contract for FixtureContract {
    const NS: &'static str = "paddock.fixture.v1";
    const MINOR: u16 = 1;
    type Request = FixtureRequest;
    type Reply = FixtureReply;
}

/// The fixture's settings as it applied them.
#[derive(Default)]
struct Applied(Mutex<(String, bool)>);

struct Sample(Arc<Applied>);

impl Configurable for Sample {
    fn section(&self) -> SettingsSection {
        SettingsSection::new(SETTINGS, "Fixture", SectionOwner::App)
            .field(SettingField::new("greeting", "Greeting", FieldKind::Text { max_len: 40, pattern: None }, "hello"))
            .field(SettingField::new("shout", "Shout", FieldKind::Bool, false))
    }

    fn apply(&self, values: &ValidValues) -> Result<(), ApplyError> {
        let greeting = values.text("greeting").unwrap_or("hello").to_string();
        if greeting == "refuse" {
            return Err("the fixture refuses this greeting".into());
        }
        *lock(&self.0.0) = (greeting, values.bool("shout").unwrap_or(false));
        Ok(())
    }
}

struct FixtureTool {
    applied: Arc<Applied>,
}

impl Tool for FixtureTool {
    fn id(&self) -> &str {
        TOOL
    }

    fn describe(&self) -> ToolInfo {
        ToolInfo::new(TOOL, "Fixture tool")
            .describe("Does small things so tests can see the platform work")
            .op(OpSpec::new("echo", "Echo").args(Schema::object().field("text", Schema::text(1000))))
            .op(OpSpec::new("greet", "Greet"))
            .op(OpSpec::new("fail", "Fail"))
            .op(OpSpec::new("deny", "Deny"))
            .op(OpSpec::new("panic", "Panic"))
            .op(OpSpec::new("crash", "Crash"))
            .op(OpSpec::new("hang", "Hang"))
            .op(OpSpec::new("big", "Big").args(Schema::object().field("n", Schema::int(0, 1 << 30))))
            .op(OpSpec::new("read", "Read a host file").args(Schema::object().field("id", Schema::int(0, i64::MAX)).field("len", Schema::int(0, 1 << 40))))
            .op(OpSpec::new("task", "Run a task"))
            .op(OpSpec::new("save", "Save output").args(Schema::object().field("name", Schema::text(100)).field("bytes", Schema::int(0, 1 << 24))))
            .op(OpSpec::new("lift", "Run elevated").args(Schema::object().field("op", Schema::text(64))))
            .op(OpSpec::new("open", "Open a path").args(Schema::object().field(
                "path",
                Schema::path(paddock_plugin_api::tools::PathKind::File, paddock_plugin_api::tools::PathAccess::Read),
            )))
            .op(OpSpec::new("root", "Elevated only").elevated())
    }

    fn invoke(&self, op: &str, args: Value, ctx: &ToolCtx) -> Result<Value, ToolError> {
        match op {
            "echo" => Ok(json!({ "text": args["text"] })),
            "greet" => {
                let (greeting, shout) = lock(&self.applied.0).clone();
                let greeting = if greeting.is_empty() { "hello".to_string() } else { greeting };
                Ok(json!(if shout { greeting.to_uppercase() } else { greeting }))
            }
            "fail" => Err(ToolError::failed("the fixture failed on purpose")),
            "deny" => Err(ToolError::Denied("the fixture lacks a right".into())),
            "panic" => panic!("the fixture panicked on purpose"),
            "crash" => std::process::exit(86),
            "hang" => loop {
                std::thread::park();
            },
            "big" => Ok(json!("x".repeat(usize::try_from(args["n"].as_u64().unwrap_or(0)).unwrap_or(0)))),
            "read" => {
                let id = args["id"].as_u64().unwrap_or(0);
                let len = args["len"].as_u64().unwrap_or(0);
                let file = HostFile::new(ctx.host().clone(), HostFileRef { id, len });
                let bytes = file.read_all(len, &|| ctx.is_cancelled())?;
                Ok(json!(String::from_utf8_lossy(&bytes)))
            }
            "task" => {
                ctx.task_started(ToolTask { id: "job-1".into(), label: "A job".into(), kind: "fixture".into(), stoppable: true });
                ctx.report("job-1", Some(0.5), "halfway");
                Ok(json!("started"))
            }
            "save" => {
                let name = args["name"].as_str().unwrap_or("out.bin");
                let bytes = usize::try_from(args["bytes"].as_u64().unwrap_or(0)).unwrap_or(0);
                let mut out = OutputWriter::create(ctx.host().clone(), name)?;
                out.write_all(&vec![7u8; bytes])?;
                let saved = out.finish()?;
                Ok(json!({ "name": saved.name, "bytes": saved.bytes }))
            }
            "lift" => ctx.elevated(args["op"].as_str().unwrap_or(""), json!({}), std::time::Duration::from_secs(5)),
            "open" => Ok(json!(std::fs::read_to_string(PathBuf::from(args["path"].as_str().unwrap_or(""))).unwrap_or_default())),
            "root" => Ok(json!("root")),
            other => Err(ToolError::failed(format!("unknown operation {other}"))),
        }
    }

    fn stop(&self, task: &str, ctx: &ToolCtx) -> Result<(), ToolError> {
        if task == "job-1" {
            ctx.task_finished(task);
            Ok(())
        } else {
            Err(ToolError::Missing(format!("no task {task}")))
        }
    }
}

struct FixtureView;

impl View for FixtureView {
    fn info(&self) -> ViewInfo {
        ViewInfo { id: VIEW.into(), title: "Fixture view".into(), writes: false }
    }

    fn render(&self, ctx: &ViewCtx) -> Result<ViewDoc, paddock_plugin_api::ToolError> {
        let title = ctx.args["title"].as_str().unwrap_or("Fixture").to_string();
        if ctx.args["broken"].as_bool().unwrap_or(false) {
            // A document over the limits: the plugin's own serve loop refuses to send it.
            return Ok(ViewDoc { schema: SCHEMA, title, root: Node::text("x".repeat(paddock_view::MAX_STRING + 1)), etag: 1 });
        }
        let action = ActionRef { id: ACTION.into(), label: "Refresh".into(), confirm: None, args: paddock_ipc::Json(json!({})), writes: false };
        let root = Node::column(vec![Node::text("Hello from the fixture"), Node::new(NodeKind::Empty {
            icon: paddock_view::Icon::Info,
            title: "Nothing here".into(),
            hint: "Refresh to look again".into(),
            action: Some(action),
        })]);
        Ok(ViewDoc { schema: SCHEMA, title, root, etag: 1 })
    }

    fn action(&self, ctx: &ViewCtx, action: &ViewAction) -> Result<ViewPatch, paddock_plugin_api::ToolError> {
        let _ = ctx;
        if action.action != ACTION {
            return Err(paddock_plugin_api::ToolError::failed(format!("no action {}", action.action)));
        }
        Ok(ViewPatch::Doc(ViewDoc { schema: SCHEMA, title: "Fixture".into(), root: Node::text("Refreshed"), etag: 2 }))
    }
}

struct FixtureMcp;

impl McpTool for FixtureMcp {
    fn info(&self) -> McpToolInfo {
        McpToolInfo {
            name: MCP_TOOL.into(),
            description: "Echoes its text".into(),
            args: Schema::object().field("text", Schema::text(100000)),
        }
    }

    fn call(&self, args: Value, _budget: McpBudget, _ctx: &ToolCtx) -> Result<Value, ToolError> {
        Ok(json!({ "text": args["text"] }))
    }
}

fn handle(ctx: &ExtCtx, request: FixtureRequest) -> Result<FixtureReply, RemoteError> {
    match request {
        FixtureRequest::Add(a, b) => Ok(FixtureReply::Sum(a + b)),
        FixtureRequest::Echo(t) => Ok(FixtureReply::Text(t)),
        FixtureRequest::Twice(t) => Ok(FixtureReply::Text(format!("{t}{t}"))),
        FixtureRequest::Panic => panic!("the fixture's contract panicked on purpose"),
        FixtureRequest::Fail => Err(RemoteError::new("fixture.failed", "the fixture's contract failed on purpose")),
        FixtureRequest::Hang => loop {
            std::thread::park();
        },
        FixtureRequest::Exit(code) => std::process::exit(code),
        FixtureRequest::Big(n) => Ok(FixtureReply::Bytes(vec![1; n])),
        FixtureRequest::Read(file) => {
            let bytes = HostFile::new(ctx.host().clone(), file)
                .read_all(file.len, &|| ctx.is_cancelled())
                .map_err(|e| RemoteError::new(codes::INTERNAL, e.to_string()))?;
            Ok(FixtureReply::Bytes(bytes))
        }
    }
}

/// The sample plugin.
pub struct FixturePlugin {
    applied: Arc<Applied>,
}

impl FixturePlugin {
    /// A fresh fixture.
    pub fn new() -> FixturePlugin {
        FixturePlugin { applied: Arc::default() }
    }
}

impl Default for FixturePlugin {
    fn default() -> Self {
        FixturePlugin::new()
    }
}

impl Plugin for FixturePlugin {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "Fixture"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }

    fn register(&self, c: &mut Contributions) -> Result<(), RegisterError> {
        c.add_tool(FixtureTool { applied: self.applied.clone() });
        c.set_settings(Arc::new(Sample(self.applied.clone())));
        c.add_view(FixtureView);
        c.add_mcp_tool(FixtureMcp);
        c.add_extension::<FixtureContract>(handle);
        Ok(())
    }
}

/// The fixture program (`paddock-plugin-fixture`) as `cargo build` left it, next to the running test: built first when it is
/// not there (a `cargo test -p` of another crate does not build this crate's binaries).
///
/// # Errors
///
/// When it cannot be found or built.
pub fn locate() -> Result<PathBuf, String> {
    locate_bin("paddock-plugin-fixture")
}

/// The frozen protocol-1 program (`paddock-plugin-fixture-v1`); see [`locate`].
///
/// # Errors
///
/// When it cannot be found or built.
pub fn locate_v1() -> Result<PathBuf, String> {
    locate_bin("paddock-plugin-fixture-v1")
}

fn locate_bin(name: &str) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut dir = exe.parent().map(PathBuf::from).unwrap_or_default();
    if dir.file_name().is_some_and(|n| n == "deps") {
        dir.pop();
    }
    find_or_build(&dir, name, || {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        std::process::Command::new(cargo)
            .args(["build", "--quiet", "-p", "paddock-plugin-test", "--bin", name])
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// `<dir>/<name>` (with `.exe` on Windows) when it exists, else what `build` makes of it.
fn find_or_build(dir: &std::path::Path, name: &str, build: impl FnOnce() -> bool) -> Result<PathBuf, String> {
    let path = dir.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() });
    if path.is_file() || (build() && path.is_file()) {
        Ok(path)
    } else {
        Err(format!("{} is not there and could not be built", path.display()))
    }
}

/// The `manifest.toml` of the fixture for platform `protocol` (`"2.0"`), `binary` as its program's name and `permissions` as
/// extra `[[permissions]]` blocks (the fixture asks for `mcpExpose` itself).
pub fn manifest_toml(protocol: &str, binary: &str, permissions: &str) -> String {
    let target = paddock_plugin_package::manifest::host_target();
    format!(
        r#"format = 1

[plugin]
id = "{ID}"
name = "Fixture"
version = "0.1.0"
publisher = "Example"
description = "The sample plugin of the platform's tests."
protocol = "{protocol}"
binary = "{binary}"
targets = ["{target}"]

[[contributions]]
kind = "tool"
id = "{TOOL}"
actions = [{{ id = "echo" }}, {{ id = "greet" }}, {{ id = "fail" }}, {{ id = "deny" }}, {{ id = "panic" }}, {{ id = "crash" }}, {{ id = "hang" }}, {{ id = "big" }}, {{ id = "read" }}, {{ id = "task" }}, {{ id = "save" }}, {{ id = "lift" }}, {{ id = "open" }}, {{ id = "root" }}]

[[contributions]]
kind = "settings"
id = "{SETTINGS}"

[[contributions]]
kind = "view"
id = "{VIEW}"
actions = [{{ id = "{ACTION}" }}]

[[contributions]]
kind = "mcpTool"
id = "{MCP_TOOL}"

[[contributions]]
kind = "extension"
ns = "{ns}/{minor}"

[[permissions]]
permission = "mcpExpose"
reason = "Offers its echo tool to assistants."
{permissions}
"#,
        ns = FixtureContract::NS,
        minor = FixtureContract::MINOR,
    )
}

/// The fixture's manifest, parsed.
///
/// # Panics
///
/// When `permissions` makes it invalid.
pub fn manifest(protocol: &str, binary: &str, permissions: &str) -> paddock_plugin_package::Manifest {
    paddock_plugin_package::Manifest::parse(&manifest_toml(protocol, binary, permissions)).expect("the fixture manifest is valid")
}

/// Packs the fixture's `manifest` and `program` into `<dir>/fixture.paddockplugin`, signed with `sign`.
///
/// # Errors
///
/// When the package cannot be written.
pub fn build_package(
    dir: &std::path::Path,
    manifest: paddock_plugin_package::Manifest,
    program: &std::path::Path,
    sign: Option<&paddock_plugin_package::SecretKey>,
) -> Result<PathBuf, paddock_plugin_package::PackageError> {
    let out = dir.join("fixture.paddockplugin");
    let target = paddock_plugin_package::manifest::host_target().to_string();
    paddock_plugin_package::archive::build(paddock_plugin_package::archive::Build { manifest, programs: vec![(target, program)], sign }, &out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_that_is_there_is_found_and_a_missing_one_is_built_once() {
        let dir = tempfile::tempdir().unwrap();
        let name = "prog";
        let file = dir.path().join(if cfg!(windows) { "prog.exe" } else { "prog" });
        let made = std::cell::Cell::new(0);
        // Not there, and the build makes nothing.
        let err = find_or_build(dir.path(), name, || {
            made.set(made.get() + 1);
            false
        })
        .unwrap_err();
        assert!(err.contains("could not be built") && made.get() == 1);
        // Not there, and the build makes it.
        let found = find_or_build(dir.path(), name, || std::fs::write(&file, b"x").is_ok()).unwrap();
        assert_eq!(found, file);
        // There: nothing is built.
        assert_eq!(find_or_build(dir.path(), name, || panic!("must not build")).unwrap(), file);
    }

    #[test]
    fn the_fixture_manifest_is_valid_and_lists_what_the_fixture_contributes() {
        let m = manifest("2.0", "paddock-plugin-fixture", "");
        assert_eq!(m.id.as_str(), ID);
        assert_eq!(m.contribution_keys().len(), 5);
        assert!(manifest_toml("2.0", "x", "").contains(FixtureContract::NS));
    }
}
