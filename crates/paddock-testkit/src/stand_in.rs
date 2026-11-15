//! [`StandIn`]: one stand-in program for every test that has to plant a program the code under test starts.
//!
//! A fixture that writes `#!/bin/sh` into a file and marks it executable cannot run on Windows, and a hand-written script rarely
//! prints what the real program prints (an npm stand-in that printed nothing hid a `--json` bug). `StandIn` installs the testkit's
//! one stand-in binary (`paddock-stand-in`, std only) under the program's name (`cargo`, `git`, `npm`, ... with `.exe` on
//! Windows), writes the actions it performs next to it, and reads back how it was called:
//!
//! ```no_run
//! # use paddock_testkit::{Scratch, StandIn};
//! # fn demo(scratch: &Scratch) -> std::io::Result<()> {
//! let cargo = StandIn::new(scratch.work(), "cargo").stdout("added 3 packages").exit(0).records_args().install()?;
//! // put cargo.dir() first on the PATH of the code under test, run it, then:
//! assert_eq!(cargo.calls()?, vec![vec!["build".to_string(), "--locked".to_string()]]);
//! # Ok(()) }
//! ```
//!
//! What a stand-in can do, in this order: print stdout and stderr lines, write or append files, sleep (at most two minutes),
//! start a named grandchild (for tree-kill tests, optionally writing its pid to a file), echo what it read on stdin, then exit
//! with a code. [`StandIn::when_args`] answers a call whose first arguments match differently (the first match wins), so one
//! program answers `--version` and `build` its own way. Calls are recorded when [`StandIn::records_args`] is set: arguments
//! (with spaces, quotes and non-ASCII as they were given; a non-UTF-8 argument is lossy), the environment variables asked for
//! with [`StandIn::records_env`] and, with [`StandIn::reads_stdin`], what was written to stdin (at most 1 MiB).
//!
//! Realistic defaults are opt-in: [`StandIn::npm_like`] and [`StandIn::git_like`] answer the common calls with the shape the real
//! program prints; override any call with [`StandIn::when_args`] (rules added later are matched first).
//!
//! Where the binary comes from (once per build folder, not per test): the variable `PADDOCK_STAND_IN_BIN` when it is set (CI jobs
//! and the Windows test VM, which has no compiler, point it at a binary cross-built with the testkit), else the kit compiles the
//! binary's one source file with `rustc` (std only, about a second) into the build folder the test program is in, under a name
//! that holds a hash of the source, so an edited stand-in is rebuilt and an unchanged one is found by every later test process.
//! The stand-in finds its script beside itself, so install it where the code under test will look for the program; copying or moving
//! the installed program alone leaves the script behind. It is installed as a hard link (a copy when the folders are on different volumes), never a symlink: the stand-in finds its
//! script from its own path.

use crate::cargo_layout::profile_dir_for;
use crate::process::TestCommand;
use crate::stand_in_json::{self, Value};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

/// The program name of the stand-in binary.
pub const STAND_IN_BIN: &str = "paddock-stand-in";
/// The environment variable that names the stand-in binary to install.
pub const STAND_IN_ENV: &str = "PADDOCK_STAND_IN_BIN";

/// A grandchild the stand-in starts (and does not wait for).
#[derive(Debug, Clone, Default)]
pub struct Spawn {
    program: String,
    args: Vec<String>,
    pid_file: Option<PathBuf>,
    inherit_output: bool,
}

impl Spawn {
    /// Starts `program`: a bare name that is also an installed stand-in beside this one is that stand-in.
    pub fn new(program: &str) -> Spawn {
        Spawn {
            program: program.to_string(),
            ..Spawn::default()
        }
    }

    /// Its arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Spawn
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.args = args.into_iter().map(|a| a.as_ref().to_string()).collect();
        self
    }

    /// Writes the grandchild's pid to `file`.
    #[must_use]
    pub fn pid_file(mut self, file: &Path) -> Spawn {
        self.pid_file = Some(file.to_path_buf());
        self
    }

    /// Gives the grandchild the stand-in's stdout and stderr (a reader of the stand-in's output then waits for it too).
    #[must_use]
    pub fn inherit_output(mut self) -> Spawn {
        self.inherit_output = true;
        self
    }

    fn to_value(&self) -> Value {
        let mut m = vec![
            ("program".to_string(), Value::Str(self.program.clone())),
            ("args".to_string(), strs(&self.args)),
            (
                "inherit_output".to_string(),
                Value::Bool(self.inherit_output),
            ),
        ];
        if let Some(p) = &self.pid_file {
            m.push(("pid_file".to_string(), Value::Str(text(p))));
        }
        Value::Obj(m)
    }
}

/// What one call does. [`StandIn`] has the same methods for its default answer.
#[derive(Debug, Clone, Default)]
pub struct Actions {
    steps: Vec<Value>,
    exit: i32,
}

impl Actions {
    /// No steps, exit code 0.
    pub fn new() -> Actions {
        Actions::default()
    }

    fn step(mut self, kind: &str, v: Value) -> Actions {
        self.steps.push(Value::Obj(vec![(kind.to_string(), v)]));
        self
    }

    /// Prints `line` and a newline on stdout.
    #[must_use]
    pub fn stdout(self, line: &str) -> Actions {
        self.step("stdout", Value::Str(line.to_string()))
    }

    /// Prints `line` and a newline on stderr.
    #[must_use]
    pub fn stderr(self, line: &str) -> Actions {
        self.step("stderr", Value::Str(line.to_string()))
    }

    /// Writes `content` to `path` (replacing it).
    #[must_use]
    pub fn writes(self, path: &Path, content: &str) -> Actions {
        self.file(path, content, false)
    }

    /// Appends `content` to `path`.
    #[must_use]
    pub fn appends(self, path: &Path, content: &str) -> Actions {
        self.file(path, content, true)
    }

    fn file(self, path: &Path, content: &str, append: bool) -> Actions {
        let m = vec![
            ("path".to_string(), Value::Str(text(path))),
            ("content".to_string(), Value::Str(content.to_string())),
            ("append".to_string(), Value::Bool(append)),
        ];
        self.step("write", Value::Obj(m))
    }

    /// Sleeps for `d`, at most two minutes (the stand-in cuts a longer one). A test that wants a hung program bounds its own wait.
    #[must_use]
    pub fn sleeps(self, d: Duration) -> Actions {
        let ms = i64::try_from(d.as_millis()).unwrap_or(i64::MAX);
        self.step("sleep_ms", Value::Num(ms))
    }

    /// Starts a grandchild and goes on.
    #[must_use]
    pub fn spawns(self, child: &Spawn) -> Actions {
        self.step("spawn", child.to_value())
    }

    /// Prints on stdout what was read on stdin (needs [`StandIn::reads_stdin`]).
    #[must_use]
    pub fn echoes_stdin(self) -> Actions {
        self.step("echo_stdin", Value::Null)
    }

    /// The exit code (default 0).
    #[must_use]
    pub fn exit(mut self, code: i32) -> Actions {
        self.exit = code;
        self
    }

    fn to_value(&self) -> Value {
        Value::Obj(vec![
            ("steps".to_string(), Value::Arr(self.steps.clone())),
            ("exit".to_string(), Value::Num(i64::from(self.exit))),
        ])
    }
}

/// Which calls a case answers.
#[derive(Debug, Clone)]
enum Match {
    /// The first arguments are these.
    Prefix(Vec<String>),
    /// Any argument holds this text.
    Contains(String),
}

/// A stand-in program to install.
#[derive(Debug, Clone)]
pub struct StandIn {
    dir: PathBuf,
    name: String,
    actions: Actions,
    /// Later entries are asked first, so a test overrides a default.
    cases: Vec<(Match, Actions)>,
    record: bool,
    record_env: Vec<String>,
    read_stdin: bool,
}

impl StandIn {
    /// A stand-in named `name` (no extension, no folder) to be installed in the existing folder `dir`. Does nothing, exits 0.
    pub fn new(dir: &Path, name: &str) -> StandIn {
        StandIn {
            dir: dir.to_path_buf(),
            name: name.to_string(),
            actions: Actions::new(),
            cases: Vec::new(),
            record: false,
            record_env: Vec::new(),
            read_stdin: false,
        }
    }

    /// Prints `line` on stdout for any call no [`StandIn::when_args`] case answers.
    #[must_use]
    pub fn stdout(mut self, line: &str) -> StandIn {
        self.actions = self.actions.stdout(line);
        self
    }

    /// Prints `line` on stderr, as [`StandIn::stdout`].
    #[must_use]
    pub fn stderr(mut self, line: &str) -> StandIn {
        self.actions = self.actions.stderr(line);
        self
    }

    /// Writes a file, as [`Actions::writes`].
    #[must_use]
    pub fn writes(mut self, path: &Path, content: &str) -> StandIn {
        self.actions = self.actions.writes(path, content);
        self
    }

    /// Appends to a file, as [`Actions::appends`].
    #[must_use]
    pub fn appends(mut self, path: &Path, content: &str) -> StandIn {
        self.actions = self.actions.appends(path, content);
        self
    }

    /// Sleeps, as [`Actions::sleeps`].
    #[must_use]
    pub fn sleeps(mut self, d: Duration) -> StandIn {
        self.actions = self.actions.sleeps(d);
        self
    }

    /// Starts a grandchild, as [`Actions::spawns`].
    #[must_use]
    pub fn spawns(mut self, child: &Spawn) -> StandIn {
        self.actions = self.actions.spawns(child);
        self
    }

    /// Echoes stdin, as [`Actions::echoes_stdin`]; also [`StandIn::reads_stdin`].
    #[must_use]
    pub fn echoes_stdin(mut self) -> StandIn {
        self.read_stdin = true;
        self.actions = self.actions.echoes_stdin();
        self
    }

    /// The exit code of the default answer.
    #[must_use]
    pub fn exit(mut self, code: i32) -> StandIn {
        self.actions = self.actions.exit(code);
        self
    }

    /// Answers a call whose arguments start with `prefix` with `actions` instead of the default (the program name is not part of
    /// the arguments). Rules added later win over earlier ones, so a test overrides a default such as [`StandIn::npm_like`]'s.
    #[must_use]
    pub fn when_args(mut self, prefix: &[&str], actions: Actions) -> StandIn {
        self.cases.insert(
            0,
            (
                Match::Prefix(prefix.iter().map(|a| (*a).to_string()).collect()),
                actions,
            ),
        );
        self
    }

    /// Answers a call in which any argument contains `text` with `actions` (a drive name such as `usb-`), as [`StandIn::when_args`].
    #[must_use]
    pub fn when_arg_contains(mut self, text: &str, actions: Actions) -> StandIn {
        self.cases
            .insert(0, (Match::Contains(text.to_string()), actions));
        self
    }

    /// Records every call's arguments (read them back with [`Installed::calls`]).
    #[must_use]
    pub fn records_args(mut self) -> StandIn {
        self.record = true;
        self
    }

    /// Records these environment variables with every call (implies [`StandIn::records_args`]); a variable that is not set is `None`.
    #[must_use]
    pub fn records_env(mut self, names: &[&str]) -> StandIn {
        self.record = true;
        self.record_env
            .extend(names.iter().map(|n| (*n).to_string()));
        self
    }

    /// Reads stdin to its end (at most 1 MiB) and records it (implies [`StandIn::records_args`]).
    #[must_use]
    pub fn reads_stdin(mut self) -> StandIn {
        self.record = true;
        self.read_stdin = true;
        self
    }

    /// An `npm` that answers what npm prints for `--version`, `install` and `ci` ("added N packages, and audited M packages in 1s"),
    /// `ls --json` (an empty tree) and `run <script>` (the `> name@version script` banner); any other call fails as npm does for an
    /// unknown command. Recorded.
    pub fn npm_like(dir: &Path) -> StandIn {
        let added = Actions::new()
            .stdout("")
            .stdout("added 3 packages, and audited 4 packages in 1s")
            .stdout("")
            .stdout("found 0 vulnerabilities");
        StandIn::new(dir, "npm")
            .records_args()
            .stderr("npm error code EUSAGE")
            .exit(1)
            .when_args(&["--version"], Actions::new().stdout("10.9.2"))
            .when_args(&["install"], added.clone())
            .when_args(&["ci"], added)
            .when_args(
                &["ls", "--json"],
                Actions::new().stdout("{\n  \"name\": \"fixture\",\n  \"version\": \"1.0.0\"\n}"),
            )
            .when_args(
                &["run"],
                Actions::new()
                    .stdout("")
                    .stdout("> fixture@1.0.0 script")
                    .stdout(""),
            )
    }

    /// A `git` that answers `--version`, `status --porcelain` (clean) and `rev-parse HEAD` (a fixed 40-hex id) like git; any other
    /// call exits 1 with git's "is not a git command" line. Recorded.
    pub fn git_like(dir: &Path) -> StandIn {
        StandIn::new(dir, "git")
            .records_args()
            .stderr("git: 'unknown' is not a git command. See 'git --help'.")
            .exit(1)
            .when_args(&["--version"], Actions::new().stdout("git version 2.47.1"))
            .when_args(&["status", "--porcelain"], Actions::new())
            .when_args(
                &["rev-parse", "HEAD"],
                Actions::new().stdout("0123456789abcdef0123456789abcdef01234567"),
            )
    }

    fn script(&self, record: &Path) -> Value {
        let cases = self
            .cases
            .iter()
            .map(|(m, a)| {
                let key = match m {
                    Match::Prefix(p) => ("args".to_string(), strs(p)),
                    Match::Contains(t) => ("contains".to_string(), Value::Str(t.clone())),
                };
                Value::Obj(vec![key, ("actions".to_string(), a.to_value())])
            })
            .collect();
        let mut m = vec![
            ("actions".to_string(), self.actions.to_value()),
            ("cases".to_string(), Value::Arr(cases)),
            ("read_stdin".to_string(), Value::Bool(self.read_stdin)),
            ("record_env".to_string(), strs(&self.record_env)),
        ];
        if self.record {
            m.push(("record".to_string(), Value::Str(text(record))));
        }
        Value::Obj(m)
    }

    /// Installs the stand-in under its name in the folder and writes its script. A stand-in of the same name already there is
    /// replaced and its record starts empty.
    ///
    /// # Errors
    ///
    /// When the name is not a bare program name, the folder is missing, or the stand-in binary cannot be found, built or linked.
    pub fn install(&self) -> io::Result<Installed> {
        let bad_name = self.name.is_empty()
            || self.name.contains(['/', '\\'])
            || self.name.to_ascii_lowercase().ends_with(".exe");
        if bad_name {
            return Err(invalid(format!(
                "`{}` is not a bare program name (no folder, no `.exe`)",
                self.name
            )));
        }
        let stub = stub_binary().map_err(io::Error::other)?;
        let program = self
            .dir
            .join(format!("{}{}", self.name, std::env::consts::EXE_SUFFIX));
        let record = self.dir.join(format!("{}.stand-in.calls", self.name));
        let _ = std::fs::remove_file(&program);
        let _ = std::fs::remove_file(&record);
        if std::fs::hard_link(&stub, &program).is_err() {
            std::fs::copy(&stub, &program).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!(
                        "cannot install {} as {}: {e}",
                        stub.display(),
                        program.display()
                    ),
                )
            })?;
        }
        std::fs::write(
            self.dir.join(format!("{}.stand-in.json", self.name)),
            self.script(&record).to_json(),
        )?;
        Ok(Installed {
            dir: self.dir.clone(),
            program,
            record,
            recording: self.record,
        })
    }
}

/// An installed stand-in.
#[derive(Debug, Clone)]
pub struct Installed {
    dir: PathBuf,
    program: PathBuf,
    record: PathBuf,
    recording: bool,
}

/// One recorded call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The arguments, after the program name.
    pub args: Vec<String>,
    /// The environment variables asked for with [`StandIn::records_env`], in that order; `None` when not set.
    pub env: Vec<(String, Option<String>)>,
    /// What was read on stdin, when [`StandIn::reads_stdin`] was set.
    pub stdin: Option<String>,
}

impl Installed {
    /// The folder it was installed in: put it on the `PATH` of the code under test.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The full path of the installed program, for code that is given a program path.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Every call so far, oldest first.
    ///
    /// # Errors
    ///
    /// When recording was not asked for, or the record is unreadable (the message says which).
    pub fn invocations(&self) -> io::Result<Vec<Invocation>> {
        if !self.recording {
            return Err(invalid(
                "this stand-in does not record: ask for it with `.records_args()`".to_string(),
            ));
        }
        let text = match std::fs::read_to_string(&self.record) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        text.lines()
            .map(parse_invocation)
            .collect::<Result<_, _>>()
            .map_err(|e| invalid(format!("{}: {e}", self.record.display())))
    }

    /// The arguments of every call so far, oldest first.
    ///
    /// # Errors
    ///
    /// As [`Installed::invocations`].
    pub fn calls(&self) -> io::Result<Vec<Vec<String>>> {
        Ok(self.invocations()?.into_iter().map(|i| i.args).collect())
    }
}

fn parse_invocation(line: &str) -> Result<Invocation, String> {
    let v = stand_in_json::parse(line)?;
    let strings = |v: Option<&Value>| -> Vec<String> {
        v.and_then(Value::as_arr)
            .unwrap_or(&[])
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    };
    Ok(Invocation {
        args: strings(v.get("args")),
        env: v
            .get("env")
            .and_then(Value::as_obj)
            .unwrap_or(&[])
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().map(str::to_string)))
            .collect(),
        stdin: v.get("stdin").and_then(Value::as_str).map(str::to_string),
    })
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn strs(v: &[String]) -> Value {
    Value::Arr(v.iter().cloned().map(Value::Str).collect())
}

fn text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

static STUB: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// The stand-in binary, found or built once per test process.
fn stub_binary() -> Result<PathBuf, String> {
    STUB.get_or_init(locate_stub).clone()
}

const STUB_SOURCE: &str = include_str!("bin/paddock-stand-in.rs");
const JSON_SOURCE: &str = include_str!("stand_in_json.rs");

/// FNV-1a over both sources: a name that changes when either does.
fn source_hash() -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in STUB_SOURCE.bytes().chain([0]).chain(JSON_SOURCE.bytes()) {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn locate_stub() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os(STAND_IN_ENV) {
        let p = PathBuf::from(p);
        return if p.is_file() {
            Ok(p)
        } else {
            Err(format!(
                "{STAND_IN_ENV} names {}, which is not a file",
                p.display()
            ))
        };
    }
    let exe = std::env::current_exe()
        .map_err(|e| format!("the test program's own path is unknown: {e}"))?;
    let profile = profile_dir_for(&exe)?;
    let built = profile.join(format!(
        "{STAND_IN_BIN}-{}{}",
        source_hash(),
        std::env::consts::EXE_SUFFIX
    ));
    if built.is_file() {
        return Ok(built);
    }
    compile_stub(&profile, &built)?;
    Ok(built)
}

/// Compiles the stand-in with `rustc` into a private folder under `profile`, then moves it to `built` (a rename, so a test process
/// that starts at the same time finds either nothing or the whole file, and two that both compile both end with a whole file).
fn compile_stub(profile: &Path, built: &Path) -> Result<(), String> {
    let hint = format!("set {STAND_IN_ENV} to a built paddock-stand-in binary");
    let work = tempfile::Builder::new()
        .prefix("stand-in-src-")
        .tempdir_in(profile)
        .map_err(|e| {
            format!(
                "cannot make a build folder in {}: {e} ({hint})",
                profile.display()
            )
        })?;
    // The same relative layout as in the crate, because the binary's source includes the JSON module by a relative `#[path]`.
    let bin_dir = work.path().join("bin");
    std::fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("cannot write the stand-in source: {e}"))?;
    let main = bin_dir.join("paddock-stand-in.rs");
    std::fs::write(&main, STUB_SOURCE)
        .and_then(|()| std::fs::write(work.path().join("stand_in_json.rs"), JSON_SOURCE))
        .map_err(|e| format!("cannot write the stand-in source: {e}"))?;
    let out = work
        .path()
        .join(format!("{STAND_IN_BIN}{}", std::env::consts::EXE_SUFFIX));
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let result = TestCommand::new(rustc)
        .args([
            "--edition",
            "2021",
            "--crate-name",
            "paddock_stand_in",
            "-C",
            "debuginfo=0",
            "-o",
        ])
        .arg(&out)
        .arg(&main)
        .current_dir(work.path())
        .output()
        .map_err(|e| format!("cannot run rustc to build {STAND_IN_BIN}: {e} ({hint})"))?;
    if !result.status.success() {
        return Err(format!(
            "rustc failed on {STAND_IN_BIN} ({}); {hint}.\n{}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    std::fs::rename(&out, built)
        .map_err(|e| format!("cannot move {} to {}: {e}", out.display(), built.display()))
}
