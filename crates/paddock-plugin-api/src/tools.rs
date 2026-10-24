//! Tools: typed request/response operations a plugin contributes and the app dispatches generically.
//!
//! A [`Tool`] describes itself once ([`ToolInfo`]): its operations, each with an argument [`Schema`], a time budget and
//! whether it needs administrator rights. The host validates the arguments against the schema before the tool sees
//! them, including **path arguments** ([`Schema::Path`]), which must be paths the user chose (the plugin's grants) and
//! arrive canonicalised, and runs the operation under the plugin's guard.
//!
//! Long-running work is reported through [`ToolCtx`]'s task methods so the app can list it and stop it later
//! ([`Tool::stop`]).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Budget of an operation that declares none.
pub const DEFAULT_OP_BUDGET: Duration = Duration::from_secs(30);
/// Largest argument document accepted (serialized JSON).
pub const MAX_ARGS_BYTES: usize = 1 << 20;
/// Longest tool id, operation and task id.
pub const MAX_ID_LEN: usize = 64;
/// Longest path argument, in characters.
pub const MAX_PATH_LEN: usize = 4096;

/// An operating system a tool can run on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    /// Microsoft Windows.
    Windows,
    /// Apple macOS.
    Macos,
    /// Linux and other Unixes.
    Linux,
}

impl Platform {
    /// The platform this binary was built for (other Unixes count as Linux).
    pub const fn current() -> Platform {
        #[cfg(windows)]
        return Platform::Windows;
        #[cfg(target_os = "macos")]
        return Platform::Macos;
        #[cfg(not(any(windows, target_os = "macos")))]
        return Platform::Linux;
    }

    /// The platform's display name.
    pub fn label(self) -> &'static str {
        match self {
            Platform::Windows => "Windows",
            Platform::Macos => "macOS",
            Platform::Linux => "Linux",
        }
    }
}

/// What a path argument names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PathKind {
    /// A file.
    File,
    /// A folder.
    Dir,
}

/// What a tool does with a path argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PathAccess {
    /// The tool reads the path.
    Read,
    /// The tool writes to the path.
    Write,
}

/// The shape of an operation's arguments. Checked by the host before the tool is called; the tool still
/// parses them into its own types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Schema {
    /// Any JSON value; the tool validates it itself (e.g. a nested config).
    Any,
    /// A boolean.
    Bool,
    /// A whole number in `min..=max`.
    Int {
        /// Smallest accepted value.
        min: i64,
        /// Largest accepted value.
        max: i64,
    },
    /// Text of at most `max_len` characters.
    Text {
        /// Longest accepted text, in characters.
        #[serde(rename = "maxLen")]
        max_len: usize,
    },
    /// One of these strings.
    Enum {
        /// The accepted strings.
        values: Vec<String>,
    },
    /// A list of items that all match `items`.
    List {
        /// The schema every item must match.
        items: Box<Schema>,
        /// Most items accepted.
        #[serde(rename = "maxItems")]
        max_items: usize,
    },
    /// An object with the named members.
    Object {
        /// The accepted members.
        fields: Vec<Field>,
        /// Whether names not in `fields` are accepted.
        additional: bool,
    },
    /// An absolute path the user chose: the host checks it against the calling window's path grants and
    /// passes it on canonicalised. A path the user did not choose is refused before the tool runs.
    Path {
        /// Whether the path is a file or a folder.
        kind: PathKind,
        /// Whether the tool reads or writes it.
        access: PathAccess,
    },
}

/// One named member of an [`Schema::Object`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    /// The member's name.
    pub name: String,
    /// The schema its value must match.
    pub schema: Schema,
    /// Optional fields may be missing or `null`.
    pub required: bool,
}

impl Schema {
    /// Text of at most `max_len` characters.
    pub fn text(max_len: usize) -> Schema {
        Schema::Text { max_len }
    }

    /// A whole number in `min..=max`.
    pub fn int(min: i64, max: i64) -> Schema {
        Schema::Int { min, max }
    }

    /// One of `values`.
    pub fn one_of<I: IntoIterator<Item = S>, S: Into<String>>(values: I) -> Schema {
        Schema::Enum {
            values: values.into_iter().map(Into::into).collect(),
        }
    }

    /// A list of at most `max_items` items that match `items`.
    pub fn list(items: Schema, max_items: usize) -> Schema {
        Schema::List {
            items: Box::new(items),
            max_items,
        }
    }

    /// A path the user chose, read by the tool.
    pub fn path(kind: PathKind, access: PathAccess) -> Schema {
        Schema::Path { kind, access }
    }

    /// An object without fields that refuses unknown names (add fields with [`Schema::field`] and
    /// [`Schema::optional`]).
    pub fn object() -> Schema {
        Schema::Object {
            fields: Vec::new(),
            additional: false,
        }
    }

    fn with_field(mut self, name: &str, schema: Schema, required: bool) -> Schema {
        if let Schema::Object { fields, .. } = &mut self {
            fields.push(Field {
                name: name.into(),
                schema,
                required,
            });
        }
        self
    }

    /// Adds a required field (objects only; ignored otherwise).
    pub fn field(self, name: &str, schema: Schema) -> Schema {
        self.with_field(name, schema, true)
    }

    /// Adds an optional field (objects only; ignored otherwise).
    pub fn optional(self, name: &str, schema: Schema) -> Schema {
        self.with_field(name, schema, false)
    }

    /// Accepts names that are not declared (objects only).
    pub fn open(mut self) -> Schema {
        if let Schema::Object { additional, .. } = &mut self {
            *additional = true;
        }
        self
    }

    /// Checks `value`; the error names the offending location (`args.template.name`).
    pub fn validate(&self, value: &Value) -> std::result::Result<(), SchemaError> {
        self.check(value, "args")
    }

    fn check(&self, value: &Value, at: &str) -> std::result::Result<(), SchemaError> {
        let fail = |message: String| {
            Err(SchemaError {
                at: at.into(),
                message,
            })
        };
        match (self, value) {
            (Schema::Any, _) | (Schema::Bool, Value::Bool(_)) => Ok(()),
            (Schema::Int { min, max }, Value::Number(n)) => match n.as_i64() {
                Some(i) if (*min..=*max).contains(&i) => Ok(()),
                Some(i) => fail(format!("must be between {min} and {max} (got {i})")),
                None => fail(format!("must be a whole number between {min} and {max}")),
            },
            (Schema::Text { max_len }, Value::String(s)) => match s.chars().count() {
                n if n > *max_len => {
                    fail(format!("must be at most {max_len} characters (got {n})"))
                }
                _ => Ok(()),
            },
            (Schema::Path { .. }, Value::String(s)) => {
                if s.chars().count() > MAX_PATH_LEN || s.contains('\0') {
                    fail(format!(
                        "must be a path of at most {MAX_PATH_LEN} characters"
                    ))
                } else if !Path::new(s).is_absolute() {
                    fail("must be an absolute path".into())
                } else {
                    Ok(())
                }
            }
            (Schema::Enum { values }, Value::String(s)) if values.contains(s) => Ok(()),
            (Schema::Enum { values }, Value::String(s)) => {
                fail(format!("\"{s}\" is not one of: {}", values.join(", ")))
            }
            (Schema::List { items, max_items }, Value::Array(a)) => {
                if a.len() > *max_items {
                    return fail(format!(
                        "must have at most {max_items} items (got {})",
                        a.len()
                    ));
                }
                a.iter()
                    .enumerate()
                    .try_for_each(|(i, v)| items.check(v, &format!("{at}[{i}]")))
            }
            (Schema::Object { fields, additional }, Value::Object(map)) => {
                for f in fields {
                    match map.get(&f.name) {
                        None | Some(Value::Null) if f.required => {
                            return Err(SchemaError {
                                at: format!("{at}.{}", f.name),
                                message: "is required".into(),
                            });
                        }
                        None | Some(Value::Null) => {}
                        Some(v) => f.schema.check(v, &format!("{at}.{}", f.name))?,
                    }
                }
                match map
                    .keys()
                    .find(|k| !*additional && !fields.iter().any(|f| &f.name == *k))
                {
                    Some(k) => Err(SchemaError {
                        at: format!("{at}.{k}"),
                        message: "is not a known argument".into(),
                    }),
                    None => Ok(()),
                }
            }
            (expected, got) => fail(format!("must be {}, not {}", expected.kind(), kind_of(got))),
        }
    }

    /// Checks every [`Schema::Path`] argument in `value` (which already passed [`Schema::validate`]) with
    /// `check` and replaces it by what `check` returns (the canonical path).
    pub fn check_paths(
        &self,
        value: &mut Value,
        check: &dyn Fn(&str, PathKind, PathAccess) -> std::result::Result<PathBuf, String>,
    ) -> std::result::Result<(), SchemaError> {
        self.paths_at(value, "args", check)
    }

    fn paths_at(
        &self,
        value: &mut Value,
        at: &str,
        check: &dyn Fn(&str, PathKind, PathAccess) -> std::result::Result<PathBuf, String>,
    ) -> std::result::Result<(), SchemaError> {
        match (self, value) {
            (Schema::Path { kind, access }, Value::String(s)) => {
                let p = check(s, *kind, *access).map_err(|message| SchemaError {
                    at: at.into(),
                    message,
                })?;
                *s = p.to_string_lossy().into_owned();
                Ok(())
            }
            (Schema::List { items, .. }, Value::Array(a)) => a
                .iter_mut()
                .enumerate()
                .try_for_each(|(i, v)| items.paths_at(v, &format!("{at}[{i}]"), check)),
            (Schema::Object { fields, .. }, Value::Object(map)) => {
                for f in fields {
                    if let Some(v) = map.get_mut(&f.name)
                        && !v.is_null()
                    {
                        f.schema.paths_at(v, &format!("{at}.{}", f.name), check)?;
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Whether the schema has a path argument anywhere.
    pub fn has_paths(&self) -> bool {
        match self {
            Schema::Path { .. } => true,
            Schema::List { items, .. } => items.has_paths(),
            Schema::Object { fields, .. } => fields.iter().any(|f| f.schema.has_paths()),
            _ => false,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Schema::Any => "any value",
            Schema::Bool => "true or false",
            Schema::Int { .. } => "a whole number",
            Schema::Text { .. } | Schema::Enum { .. } => "text",
            Schema::Path { .. } => "a path",
            Schema::List { .. } => "a list",
            Schema::Object { .. } => "an object",
        }
    }
}

fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "true/false",
        Value::Number(_) => "a number",
        Value::String(_) => "text",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

/// Where and why arguments do not match an operation's schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError {
    /// `args`, `args.template.name`, `args.counters[2]`, ...
    pub at: String,
    /// What is wrong with the value.
    pub message: String,
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.at, self.message)
    }
}

/// One operation of a tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpSpec {
    /// Operation id, as passed to `invoke`.
    pub name: String,
    /// Name shown to the user.
    pub title: String,
    /// The schema its arguments must match.
    pub args: Schema,
    /// Time the operation may take before it is abandoned and its plugin marked hung; `None` means unlimited.
    #[serde(rename = "budgetMs", with = "millis")]
    pub budget: Option<Duration>,
    /// It needs administrator rights: the host runs it in the plugin's elevated process (ADR 0017 §7),
    /// started through the administrator prompt on first use.
    #[serde(default)]
    pub elevated: bool,
    /// A read-only status operation the UI polls: the shell runs it through the quiet `tool_poll`
    /// (not logged per call) and refuses `tool_poll` for any other operation.
    #[serde(default)]
    pub poll: bool,
}

mod millis {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(d: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_some(&(d.as_millis() as u64)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
        Ok(Option::<u64>::deserialize(d)?.map(Duration::from_millis))
    }
}

impl OpSpec {
    /// An operation taking no arguments, with [`DEFAULT_OP_BUDGET`].
    pub fn new(name: &str, title: &str) -> OpSpec {
        OpSpec {
            name: name.into(),
            title: title.into(),
            args: Schema::object(),
            budget: Some(DEFAULT_OP_BUDGET),
            elevated: false,
            poll: false,
        }
    }

    /// Marks a read-only status operation that may be polled (see [`OpSpec::poll`]).
    pub fn polling(mut self) -> OpSpec {
        self.poll = true;
        self
    }

    /// Sets the argument schema.
    pub fn args(mut self, schema: Schema) -> OpSpec {
        self.args = schema;
        self
    }

    /// Sets the time the operation may take before it is abandoned; `None` means unlimited.
    pub fn budget(mut self, budget: Option<Duration>) -> OpSpec {
        self.budget = budget;
        self
    }

    /// Runs in the plugin's elevated process.
    pub fn elevated(mut self) -> OpSpec {
        self.elevated = true;
        self
    }
}

/// What a tool is and what it can do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    /// Stable id, unique across plugins (`"perfmon.sets"`).
    pub id: String,
    /// Name shown to the user.
    pub title: String,
    /// What the tool does, shown under its title.
    pub description: String,
    /// Platforms it works on; empty means every platform.
    pub platforms: Vec<Platform>,
    /// Usually needs administrator rights (the UI offers an elevated restart).
    pub needs_elevation: bool,
    /// The operations it offers.
    pub ops: Vec<OpSpec>,
}

impl ToolInfo {
    /// Creates a tool with no operations, available on every platform.
    pub fn new(id: &str, title: &str) -> ToolInfo {
        ToolInfo {
            id: id.into(),
            title: title.into(),
            description: String::new(),
            platforms: Vec::new(),
            needs_elevation: false,
            ops: Vec::new(),
        }
    }

    /// Sets the description.
    pub fn describe(mut self, text: &str) -> ToolInfo {
        self.description = text.into();
        self
    }

    /// Restricts the tool to `platforms`.
    pub fn only_on(mut self, platforms: &[Platform]) -> ToolInfo {
        self.platforms = platforms.to_vec();
        self
    }

    /// Marks the tool as usually needing administrator rights.
    pub fn needs_elevation(mut self) -> ToolInfo {
        self.needs_elevation = true;
        self
    }

    /// Adds an operation.
    pub fn op(mut self, op: OpSpec) -> ToolInfo {
        self.ops.push(op);
        self
    }

    /// Ids, operation names and schemas are well formed; the error says what to fix.
    pub fn check(&self) -> Result<(), String> {
        check_id("tool id", &self.id)?;
        if self.ops.is_empty() {
            return Err(format!("tool \"{}\" has no operations", self.id));
        }
        for (i, op) in self.ops.iter().enumerate() {
            check_id(&format!("tool \"{}\": operation name", self.id), &op.name)?;
            if self.ops[..i].iter().any(|o| o.name == op.name) {
                return Err(format!(
                    "tool \"{}\": operation \"{}\" is declared twice",
                    self.id, op.name
                ));
            }
        }
        Ok(())
    }

    /// Why the tool cannot run on `platform`, if it cannot.
    pub fn unsupported_on(&self, platform: Platform) -> Option<String> {
        if self.platforms.is_empty() || self.platforms.contains(&platform) {
            return None;
        }
        let on: Vec<&str> = self.platforms.iter().map(|p| p.label()).collect();
        Some(format!(
            "{} is only available on {} (this is {})",
            self.title,
            on.join(" and "),
            platform.label()
        ))
    }
}

/// Checks an id: 1–[`MAX_ID_LEN`] characters of a-z, 0-9, '.', '-', '_' or ':'.
pub fn check_id(what: &str, id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{what} \"{id}\" must be 1-{MAX_ID_LEN} characters of a-z, 0-9, '.', '-', '_' or ':'"
        ))
    }
}

/// Background work a tool started (shown in the tray and status bar).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTask {
    /// Stable id, unique per task within the plugin (`"monitor"`, `"capture:main"`).
    pub id: String,
    /// Display name of the task.
    pub label: String,
    /// A hint for the app's icon (`"monitor"`, `"capture"`, ...).
    pub kind: String,
    /// Whether [`Tool::stop`] can stop it (the tray then offers Stop).
    pub stoppable: bool,
}

/// Why a tool did not do what was asked, as a value the app can show: the user lacked a right, something was missing,
/// the request was cancelled, or it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    /// A right was missing (the app offers to grant it or to elevate).
    Denied(String),
    /// Something the operation needs is not there (a tool, a file, a program).
    Missing(String),
    /// The call was cancelled; nothing was changed.
    Cancelled,
    /// It failed, with a message the user can act on.
    Failed(String),
}

impl ToolError {
    /// [`ToolError::Failed`] with `message`.
    pub fn failed(message: impl Into<String>) -> ToolError {
        ToolError::Failed(message.into())
    }

    /// The reason the app can branch on, if there is one.
    pub fn reason(&self) -> Option<FailureReason> {
        match self {
            ToolError::Denied(_) => Some(FailureReason::Denied),
            ToolError::Missing(_) => Some(FailureReason::Missing),
            ToolError::Cancelled | ToolError::Failed(_) => None,
        }
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolError::Denied(m) | ToolError::Missing(m) | ToolError::Failed(m) => f.write_str(m),
            ToolError::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for ToolError {}

impl From<std::io::Error> for ToolError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::PermissionDenied => ToolError::Denied(e.to_string()),
            std::io::ErrorKind::NotFound => ToolError::Missing(e.to_string()),
            _ => ToolError::Failed(e.to_string()),
        }
    }
}

impl From<serde_json::Error> for ToolError {
    fn from(e: serde_json::Error) -> Self {
        ToolError::Failed(e.to_string())
    }
}

/// Why a call that the plugin answered still did not succeed, for the app: the right it lacked, or what was missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureReason {
    /// A right was missing.
    Denied,
    /// Something was missing.
    Missing,
}

/// The services of the app a plugin calls back while it works (the plugin side of [`HostCall`]). In a plugin process
/// this is the IPC connection ([`crate::serve::PeerHost`]); plugin unit tests use `paddock-plugin-test`'s fake.
pub trait Host: Send + Sync {
    /// Up to `len` bytes (at most [`crate::protocol::READ_CHUNK`]) of the host file `id` at `offset`.
    fn read_file(&self, id: u64, offset: u64, len: u32) -> Result<Vec<u8>, ToolError>;
    /// Tells the app how far a task is (`None`: unknown) and what it is doing now.
    fn report(&self, task: &str, progress: Option<f32>, note: &str);
    /// Announces a background task.
    fn task_started(&self, tool: &str, task: &ToolTask);
    /// Announces that a task ended.
    fn task_finished(&self, task: &str);
    /// One piece of the output file `name` (see [`crate::output`]); `last` ends it and returns where it was saved.
    fn output(
        &self,
        name: &str,
        bytes: &[u8],
        last: bool,
    ) -> Result<Option<crate::protocol::SavedOutput>, ToolError>;
    /// Lines of the plugin's own log, which the app writes to its log (never file contents by default).
    fn log(&self, lines: &[String]);
    /// Runs the named elevated operation of `tool` through the app.
    fn elevated(
        &self,
        tool: &str,
        op: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value, ToolError>;
}

/// A [`Host`] that offers nothing: reads fail, reports are dropped.
pub struct NoHost;

impl Host for NoHost {
    fn read_file(&self, _: u64, _: u64, _: u32) -> Result<Vec<u8>, ToolError> {
        Err(ToolError::Missing("this call has no host files".into()))
    }
    fn report(&self, _: &str, _: Option<f32>, _: &str) {}
    fn task_started(&self, _: &str, _: &ToolTask) {}
    fn task_finished(&self, _: &str) {}
    fn output(
        &self,
        _: &str,
        _: &[u8],
        _: bool,
    ) -> Result<Option<crate::protocol::SavedOutput>, ToolError> {
        Err(ToolError::Missing("this call cannot save output".into()))
    }
    fn log(&self, _: &[String]) {}
    fn elevated(&self, _: &str, _: &str, _: Value, _: Duration) -> Result<Value, ToolError> {
        Err(ToolError::Missing(
            "this call cannot run elevated operations".into(),
        ))
    }
}

/// What a tool operation sees: its tool's id, the plugin's data folder and the app's services.
#[derive(Clone)]
pub struct ToolCtx {
    tool: String,
    data_dir: Option<PathBuf>,
    host: Arc<dyn Host>,
    cancelled: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl ToolCtx {
    /// A context for `tool` with the given services.
    pub fn new(tool: &str, data_dir: Option<PathBuf>, host: Arc<dyn Host>) -> ToolCtx {
        ToolCtx {
            tool: tool.into(),
            data_dir,
            host,
            cancelled: Arc::new(|| false),
        }
    }

    /// Lets the context say when the app cancelled the call.
    pub fn with_cancel(mut self, cancelled: Arc<dyn Fn() -> bool + Send + Sync>) -> ToolCtx {
        self.cancelled = cancelled;
        self
    }

    /// Whether the app cancelled the call; long work stops early (and returns [`ToolError::Cancelled`]) when it is.
    pub fn is_cancelled(&self) -> bool {
        (self.cancelled)()
    }

    /// The tool's id.
    pub fn tool(&self) -> &str {
        &self.tool
    }

    /// The plugin's private data folder, when the app gave it one.
    pub fn data_dir(&self) -> Option<&Path> {
        self.data_dir.as_deref()
    }

    /// The app's services.
    pub fn host(&self) -> &Arc<dyn Host> {
        &self.host
    }

    /// Announces a background task of this tool.
    pub fn task_started(&self, task: ToolTask) {
        self.host.task_started(&self.tool, &task);
    }

    /// Reports a task's progress (`None`: unknown) and what it is doing.
    pub fn report(&self, task: &str, progress: Option<f32>, note: &str) {
        self.host.report(task, progress, note);
    }

    /// Announces that a task ended.
    pub fn task_finished(&self, task: &str) {
        self.host.task_finished(task);
    }

    /// Sends a line to the app's log for this plugin.
    pub fn log(&self, line: &str) {
        self.host.log(&[line.to_string()]);
    }

    /// Runs one of this tool's elevated operations through the app.
    pub fn elevated(&self, op: &str, args: Value, timeout: Duration) -> Result<Value, ToolError> {
        self.host.elevated(&self.tool, op, args, timeout)
    }
}

/// A tool a plugin contributes.
pub trait Tool: Send + Sync {
    /// The id, the same as [`ToolInfo::id`] of [`Tool::describe`].
    fn id(&self) -> &str;
    /// What the tool is and does; taken once, when it is registered.
    fn describe(&self) -> ToolInfo;
    /// Runs operation `op` with arguments the host validated against its schema.
    fn invoke(&self, op: &str, args: Value, ctx: &ToolCtx) -> Result<Value, ToolError>;
    /// Stops the background task `task` this tool started (the default: there is none).
    fn stop(&self, task: &str, ctx: &ToolCtx) -> Result<(), ToolError> {
        let _ = ctx;
        Err(ToolError::Missing(format!(
            "tool \"{}\" has no background task \"{task}\" to stop",
            self.id()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    fn abs() -> String {
        std::env::temp_dir()
            .join("set.xml")
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn path_arguments_are_validated_and_checked_by_the_host() {
        let s = Schema::object()
            .field("xmlPath", Schema::path(PathKind::File, PathAccess::Write))
            .optional(
                "inputs",
                Schema::list(Schema::path(PathKind::File, PathAccess::Read), 4),
            );
        assert!(s.has_paths() && !Schema::object().field("x", Schema::Bool).has_paths());
        let mut args = json!({"xmlPath": abs(), "inputs": [abs()], "ignored": null});
        s.validate(&json!({"xmlPath": abs(), "inputs": [abs()]}))
            .unwrap();
        // Relative paths and NULs are refused by the schema itself.
        assert_eq!(
            s.validate(&json!({"xmlPath": "rel/x"}))
                .unwrap_err()
                .message,
            "must be an absolute path"
        );
        assert!(
            s.validate(&json!({"xmlPath": format!("{}\0", abs())}))
                .is_err()
        );
        // The host's policy decides, and its canonical answer replaces the argument.
        let seen = Mutex::new(Vec::new());
        s.check_paths(&mut args, &|p, k, a| {
            seen.lock().unwrap().push((p.to_string(), k, a));
            Ok(PathBuf::from(format!("{p}.canonical")))
        })
        .unwrap();
        assert_eq!(args["xmlPath"], format!("{}.canonical", abs()));
        assert_eq!(args["inputs"][0], format!("{}.canonical", abs()));
        assert_eq!(seen.lock().unwrap()[0].2, PathAccess::Write);
        let err = s
            .check_paths(&mut json!({"xmlPath": abs()}), &|p, _, _| {
                Err(format!("\"{p}\" was not chosen"))
            })
            .unwrap_err();
        assert_eq!(err.at, "args.xmlPath");
        assert!(err.message.contains("not chosen") && err.to_string().starts_with("args.xmlPath "));
        // A path where the schema has none is left alone, as is a null optional.
        let mut other = json!({"inputs": null});
        s.check_paths(&mut other, &|_, _, _| Err("never".into()))
            .unwrap();
    }

    #[test]
    fn paths_are_found_in_lists_and_left_alone_elsewhere() {
        let list = Schema::list(Schema::path(PathKind::File, PathAccess::Read), 3);
        assert!(list.has_paths() && !Schema::list(Schema::Bool, 3).has_paths());
        let mixed = Schema::object()
            .field("n", Schema::int(0, 5))
            .field("p", Schema::path(PathKind::Dir, PathAccess::Read));
        let mut args = json!({"n": 3, "p": abs()});
        mixed
            .check_paths(&mut args, &|p, _, _| Ok(PathBuf::from(format!("{p}!"))))
            .unwrap();
        assert_eq!(
            (args["n"].clone(), args["p"].clone()),
            (json!(3), json!(format!("{}!", abs())))
        );
        let mut items = json!([abs(), abs()]);
        list.check_paths(&mut items, &|p, _, _| Ok(PathBuf::from(p.to_uppercase())))
            .unwrap();
        assert_eq!(items[1], json!(abs().to_uppercase()));
        assert_eq!(Schema::Any.kind(), "any value");
    }

    #[test]
    fn schemas_validate_every_kind_of_value() {
        let s = Schema::object()
            .field("n", Schema::int(1, 5))
            .field("t", Schema::text(3))
            .field("e", Schema::one_of(["a", "b"]))
            .field("b", Schema::Bool)
            .optional("l", Schema::list(Schema::int(0, 9), 2))
            .optional("any", Schema::Any);
        let ok = json!({"n": 3, "t": "abc", "e": "a", "b": true, "l": [1, 2], "any": {"x": 1}});
        s.validate(&ok).unwrap();
        let bad = |v: Value| s.validate(&v).unwrap_err().to_string();
        assert!(
            bad(json!({"n": 9, "t": "a", "e": "a", "b": true})).contains("between 1 and 5 (got 9)")
        );
        assert!(bad(json!({"n": 1.5, "t": "a", "e": "a", "b": true})).contains("whole number"));
        assert!(
            bad(json!({"n": 1, "t": "abcd", "e": "a", "b": true})).contains("at most 3 characters")
        );
        assert!(
            bad(json!({"n": 1, "t": "a", "e": "z", "b": true})).contains("is not one of: a, b")
        );
        assert!(
            bad(json!({"n": 1, "t": "a", "e": "a", "b": "yes"}))
                .contains("must be true or false, not text")
        );
        assert!(
            bad(json!({"n": 1, "t": "a", "e": "a", "b": true, "l": [1, 2, 3]}))
                .contains("at most 2 items")
        );
        assert!(
            bad(json!({"n": 1, "t": "a", "e": "a", "b": true, "l": [1, 10]})).contains("args.l[1]")
        );
        assert!(bad(json!({"t": "a", "e": "a", "b": true})).contains("args.n is required"));
        assert!(
            bad(json!({"n": 1, "t": "a", "e": "a", "b": true, "zzz": 1}))
                .contains("is not a known argument")
        );
        assert!(bad(json!(["x"])).contains("must be an object, not a list"));
        assert!(bad(json!({"n": null, "t": "a", "e": "a", "b": true})).contains("is required"));
        // Open objects take more; every kind has a name in the message.
        Schema::object().open().validate(&json!({"x": 1})).unwrap();
        for (schema, value, kind) in [
            (Schema::Any, json!(null), ""),
            (Schema::text(1), json!(1), "text, not a number"),
            (
                Schema::path(PathKind::Dir, PathAccess::Read),
                json!(true),
                "a path, not true/false",
            ),
            (
                Schema::list(Schema::Any, 1),
                json!({}),
                "a list, not an object",
            ),
            (Schema::int(0, 1), json!(null), "a whole number, not null"),
            (Schema::object(), json!([]), "an object, not a list"),
            (Schema::one_of(["a"]), json!("a"), ""),
        ] {
            let r = schema.validate(&value);
            if kind.is_empty() {
                r.unwrap();
            } else {
                assert!(r.unwrap_err().message.contains(kind), "{kind}");
            }
        }
        assert!(
            Schema::path(PathKind::File, PathAccess::Read)
                .validate(&json!("x".repeat(MAX_PATH_LEN + 1)))
                .is_err()
        );
        assert!(Schema::int(0, 1).validate(&json!(u64::MAX)).is_err());
    }

    #[test]
    fn descriptions_roundtrip_as_json_and_are_checked() {
        let info = ToolInfo::new("perfmon.templates", "PerfMon")
            .describe("d")
            .only_on(&[Platform::Windows])
            .needs_elevation()
            .op(OpSpec::new("generate", "Generate")
                .args(Schema::object().field("n", Schema::int(1, 5)).open())
                .budget(None)
                .elevated())
            .op(OpSpec::new("poll", "Poll")
                .polling()
                .budget(Some(Duration::from_millis(1500))));
        info.check().unwrap();
        let back: ToolInfo = serde_json::from_value(serde_json::to_value(&info).unwrap()).unwrap();
        assert_eq!(back, info);
        assert!(back.ops[0].elevated && back.ops[0].budget.is_none() && back.ops[1].poll);
        assert_eq!(back.ops[1].budget, Some(Duration::from_millis(1500)));
        assert!(
            info.unsupported_on(Platform::Linux)
                .unwrap()
                .contains("only available on Windows")
        );
        assert!(
            info.unsupported_on(Platform::Macos)
                .unwrap()
                .contains("(this is macOS)")
        );
        assert_eq!(info.unsupported_on(Platform::Windows), None);
        assert_eq!(
            ToolInfo::new("x", "X").unsupported_on(Platform::Linux),
            None
        );
        assert!(ToolInfo::new("x", "X").check().is_err());
        let twice = ToolInfo::new("x", "X")
            .op(OpSpec::new("a", "A"))
            .op(OpSpec::new("a", "A again"));
        assert!(twice.check().unwrap_err().contains("declared twice"));
        assert!(
            ToolInfo::new("x", "X")
                .op(OpSpec::new("bad op", "B"))
                .check()
                .is_err()
        );
        assert!(
            check_id("id", "bad id").is_err()
                && check_id("id", "").is_err()
                && check_id("id", &"a".repeat(MAX_ID_LEN + 1)).is_err()
        );
        assert!(check_id("id", "a.b-c_d:e").is_ok());
        assert_eq!(OpSpec::new("a", "A").budget, Some(DEFAULT_OP_BUDGET));
        assert!(!Platform::current().label().is_empty());
        assert_eq!(Platform::Linux.label(), "Linux");
    }

    #[test]
    fn contexts_scope_tasks_to_their_tool() {
        let rec = Arc::new(crate::testing::Memory::default());
        let ctx = ToolCtx::new("t", Some(PathBuf::from("data")), rec.clone());
        assert_eq!((ctx.tool(), ctx.data_dir()), ("t", Some(Path::new("data"))));
        ctx.task_started(ToolTask {
            id: "a".into(),
            label: "A".into(),
            kind: "k".into(),
            stoppable: true,
        });
        ctx.report("a", Some(0.5), "half");
        ctx.task_finished("a");
        assert_eq!(
            ctx.elevated("op", json!({"x": 1}), Duration::from_secs(1))
                .unwrap(),
            json!({"x": 1})
        );
        assert_eq!(ctx.host().read_file(1, 2, 3).unwrap(), Vec::<u8>::new());
        assert_eq!(
            ctx.host()
                .output("o.bin", b"abc", true)
                .unwrap()
                .unwrap()
                .name,
            "o.bin"
        );
        ctx.log("hi");
        assert_eq!(
            rec.log(),
            [
                "started t a",
                "report a Some(0.5) half",
                "finished a",
                "elevated t op",
                "read 1 2 3",
                "output o.bin 3 true",
                "log hi"
            ]
        );
        assert!(ToolCtx::new("t", None, rec).data_dir().is_none());
        assert!(!ctx.is_cancelled());
        assert!(ctx.clone().with_cancel(Arc::new(|| true)).is_cancelled());
    }

    #[test]
    fn no_host_offers_nothing() {
        let h = NoHost;
        assert!(h.read_file(1, 0, 1).is_err() && h.output("a", b"", true).is_err());
        assert!(
            h.elevated("t", "o", json!(null), Duration::from_secs(1))
                .is_err()
        );
        h.report("a", None, "");
        h.task_started(
            "t",
            &ToolTask {
                id: "a".into(),
                label: "A".into(),
                kind: "k".into(),
                stoppable: false,
            },
        );
        h.task_finished("a");
        h.log(&["line".to_string()]);
    }

    struct Plain;
    impl Tool for Plain {
        fn id(&self) -> &str {
            "plain"
        }
        fn describe(&self) -> ToolInfo {
            ToolInfo::new("plain", "Plain").op(OpSpec::new("x", "X"))
        }
        fn invoke(&self, _: &str, _: Value, _: &ToolCtx) -> Result<Value, ToolError> {
            Ok(Value::Null)
        }
    }

    #[test]
    fn errors_keep_their_reason_and_a_tool_has_no_task_by_default() {
        let ctx = ToolCtx::new("plain", None, Arc::new(NoHost));
        let e = Plain.stop("t1", &ctx).unwrap_err();
        assert_eq!(e.reason(), Some(FailureReason::Missing));
        assert!(e.to_string().contains("no background task \"t1\""));
        assert_eq!(
            ToolError::Denied("d".into()).reason(),
            Some(FailureReason::Denied)
        );
        assert_eq!(ToolError::failed("f").reason(), None);
        assert_eq!(
            (
                ToolError::Cancelled.reason(),
                ToolError::Cancelled.to_string()
            ),
            (None, "cancelled".to_string())
        );
        assert!(matches!(
            ToolError::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            ToolError::Denied(_)
        ));
        assert!(matches!(
            ToolError::from(std::io::Error::from(std::io::ErrorKind::NotFound)),
            ToolError::Missing(_)
        ));
        assert!(matches!(
            ToolError::from(std::io::Error::other("x")),
            ToolError::Failed(_)
        ));
        let json_err = serde_json::from_str::<Value>("{").unwrap_err();
        assert!(matches!(ToolError::from(json_err), ToolError::Failed(_)));
        assert_eq!(Plain.invoke("x", Value::Null, &ctx).unwrap(), Value::Null);
        assert_eq!((Plain.id(), Plain.describe().ops.len()), ("plain", 1));
    }
}
