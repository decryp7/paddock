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
//! let cargo = StandIn::new(&scratch.work(), "cargo").stdout("added 3 packages").exit(0).records_args().install()?;
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
//! More, for the fixtures that were shell scripts: output filled from the call ([`Actions::stdout_with`] and the other `_with`
//! methods name `{args}`, `{arg:N}`, `{flag:PREFIX}`, `{cwd}`, `{dir}`, `{name}`, `{pid}`, `{run:program|arg|arg}`, `{env:NAME}` / `{env:NAME|fallback}`, `{stdin}` and
//! `{stdin_line}`; `{{` and `}}` are braces), [`Actions::writes_atomically`], [`Actions::waits_for_file`], named pipes
//! ([`Actions::makes_fifo`], [`Actions::reads_file`]), [`Actions::ignores_term`] (Unix), [`Actions::installs`] (a second stand-in
//! written at run time, as a fake `cargo` that "builds" a tool does), [`Actions::exits_with_env`] and [`Actions::execs`] (run another
//! program with the call's arguments, as a wrapper script does). [`StandIn::when`] answers a call by several conditions at once
//! ([`Case`]: arguments, a file that exists, the working folder), and [`StandIn::answers`] replaces the default answer.
//!
//! Realistic defaults are opt-in: [`StandIn::npm_like`] and [`StandIn::git_like`] answer the common calls with the shape the real
//! program prints; override any call with [`StandIn::when_args`] (rules added later are matched first).
//!
//! Where the binary comes from (once per build folder, not per test): the variable `PADDOCK_STAND_IN_BIN` when it is set (the Windows test VM, which has no compiler: `docs/testing/windows-test-vm.md`; the binary must be built from the same sources, which is checked), else the kit compiles the
//! binary's one source file with `rustc` (std only, about a second) into the build folder the test program is in, under a name
//! that holds a hash of the source, so an edited stand-in is rebuilt and an unchanged one is found by every later test process.
//! The stand-in finds its script beside itself, so install it where the code under test will look for the program; copying or moving
//! the installed program alone leaves the script behind. It is installed as a hard link (a copy when the folders are on different volumes), never a symlink: the stand-in finds its
//! script from its own path. The copy is the fallback only: a process that forks while another thread of the same process has the
//! copy open for writing can make that thread's first run of it fail with "Text file busy" (ETXTBSY) on Linux, so tests in one process
//! that install across volumes take turns (nextest runs each test in its own process, where this cannot happen).

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
    echoes: bool,
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

    /// Prints on stdout what was read on stdin (the stand-in then reads stdin, without recording it).
    #[must_use]
    pub fn echoes_stdin(mut self) -> Actions {
        self.echoes = true;
        self.step("echo_stdin", Value::Null)
    }

    /// Prints `template` and a newline on stdout, with the call's values filled in (see the module text for the tokens; a `{stdin}` token makes the stand-in read stdin).
    #[must_use]
    pub fn stdout_with(mut self, template: &str) -> Actions {
        self.echoes |= template.contains("{stdin");
        self.step("stdout_t", Value::Str(template.to_string()))
    }

    /// Prints `template` and a newline on stderr, with the call's values filled in.
    #[must_use]
    pub fn stderr_with(mut self, template: &str) -> Actions {
        self.echoes |= template.contains("{stdin");
        self.step("stderr_t", Value::Str(template.to_string()))
    }

    /// Appends `content_template` to the file `path_template`, both with the call's values filled in (a call log: `{args}`, `{cwd}`).
    #[must_use]
    pub fn appends_with(self, path_template: &str, content_template: &str) -> Actions {
        self.file_with(path_template, content_template, true, false)
    }

    /// Writes `content_template` to the file `path_template` (replacing it), both with the call's values filled in.
    #[must_use]
    pub fn writes_with(self, path_template: &str, content_template: &str) -> Actions {
        self.file_with(path_template, content_template, false, false)
    }

    /// Writes `content_template` to `path_template` so that a reader polling for the file never sees half of it (written beside
    /// it, then renamed onto it); both are filled with the call's values (`{pid}` is the stand-in's own pid).
    #[must_use]
    pub fn writes_atomically(self, path_template: &str, content_template: &str) -> Actions {
        self.file_with(path_template, content_template, false, true)
    }

    fn file_with(mut self, path: &str, content: &str, append: bool, atomic: bool) -> Actions {
        self.echoes |= content.contains("{stdin");
        let m = vec![
            ("path".to_string(), Value::Str(path.to_string())),
            ("content".to_string(), Value::Str(content.to_string())),
            ("append".to_string(), Value::Bool(append)),
            ("atomic".to_string(), Value::Bool(atomic)),
            ("template".to_string(), Value::Bool(true)),
        ];
        self.step("write", Value::Obj(m))
    }

    /// Waits until the file exists (polling every 10 ms, at most two minutes, then the stand-in is broken: exit 70).
    #[must_use]
    pub fn waits_for_file(self, path_template: &str) -> Actions {
        self.step("wait_file", Value::Str(path_template.to_string()))
    }

    /// Reads the file to its end; for a named pipe that blocks until a writer has opened and closed it.
    #[must_use]
    pub fn reads_file(self, path_template: &str) -> Actions {
        self.step("read_file", Value::Str(path_template.to_string()))
    }

    /// Makes a named pipe (Unix only; on Windows the stand-in is broken: exit 70). Opening it for writing, as [`Actions::writes`]
    /// does, blocks until a reader opens it.
    #[must_use]
    pub fn makes_fifo(self, path_template: &str) -> Actions {
        self.step("mkfifo", Value::Str(path_template.to_string()))
    }

    /// Makes a folder and its parents.
    #[must_use]
    pub fn makes_dir(self, path_template: &str) -> Actions {
        self.step("mkdir", Value::Str(path_template.to_string()))
    }

    /// Stops itself (SIGSTOP, Unix only), as `kill -STOP $$` does; it goes on after SIGCONT.
    #[must_use]
    pub fn stops_itself(self) -> Actions {
        self.step("stop_self", Value::Null)
    }

    /// Ignores SIGTERM from here on (Unix only), as `trap '' TERM` does.
    #[must_use]
    pub fn ignores_term(self) -> Actions {
        self.step("ignore_term", Value::Null)
    }

    /// Installs `program` at run time at `path_template` (a copy of the stand-in and its script; the folder must exist), as a fake
    /// `cargo` does when it "builds" a tool. The strings of `program` are filled with the call's values now; write `{{args}}` for a
    /// token the installed program should fill when it runs. `program`'s own folder and recording are not used.
    #[must_use]
    pub fn installs(self, path_template: &str, program: &StandIn) -> Actions {
        let m = vec![
            ("path".to_string(), Value::Str(path_template.to_string())),
            ("script".to_string(), program.script(Path::new(""))),
        ];
        self.step("install", Value::Obj(m))
    }

    /// Ends with the exit code that the environment variable holds, or `default` when it is unset or empty.
    #[must_use]
    pub fn exits_with_env(self, name: &str, default: i32) -> Actions {
        let m = vec![
            ("name".to_string(), Value::Str(name.to_string())),
            ("default".to_string(), Value::Num(i64::from(default))),
        ];
        self.step("exit_env", Value::Obj(m))
    }

    /// Runs another program (see [`Exec`]) in this stand-in's place and ends with its exit code; steps after it do not run.
    #[must_use]
    pub fn execs(self, exec: &Exec) -> Actions {
        self.step("exec", exec.to_value())
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

/// Another program that a stand-in runs with the call's arguments (`exec "$@"` in a wrapper script).
#[derive(Debug, Clone, Default)]
pub struct Exec {
    program: Option<String>,
    args: Vec<String>,
    from: &'static str,
    separator: Option<String>,
    env: Vec<(String, String)>,
}

impl Exec {
    /// Runs the first argument of the call, with the rest (`exec "$@"`).
    pub fn call_arguments() -> Exec {
        Exec {
            from: "all",
            ..Exec::default()
        }
    }

    /// Runs `program` (a template) with the call's arguments after it (`exec prog "$@"`).
    pub fn program(program: &str) -> Exec {
        Exec {
            program: Some(program.to_string()),
            from: "all",
            ..Exec::default()
        }
    }

    /// Passes only the call's arguments after the first `separator` (the earlier ones are dropped).
    #[must_use]
    pub fn after(mut self, separator: &str) -> Exec {
        self.from = "after";
        self.separator = Some(separator.to_string());
        self
    }

    /// Passes none of the call's arguments.
    #[must_use]
    pub fn without_call_arguments(mut self) -> Exec {
        self.from = "none";
        self
    }

    /// Arguments (templates) that come before the call's.
    #[must_use]
    pub fn args<I: IntoIterator<Item = S>, S: AsRef<str>>(mut self, args: I) -> Exec {
        self.args = args.into_iter().map(|a| a.as_ref().to_string()).collect();
        self
    }

    /// Sets an environment variable (a template) for the program.
    #[must_use]
    pub fn env(mut self, name: &str, template: &str) -> Exec {
        self.env.push((name.to_string(), template.to_string()));
        self
    }

    fn to_value(&self) -> Value {
        let mut m = vec![
            ("from".to_string(), Value::Str(self.from.to_string())),
            ("args".to_string(), strs(&self.args)),
            (
                "env".to_string(),
                Value::Obj(
                    self.env
                        .iter()
                        .map(|(k, v)| (k.clone(), Value::Str(v.clone())))
                        .collect(),
                ),
            ),
        ];
        if let Some(p) = &self.program {
            m.push(("program".to_string(), Value::Str(p.clone())));
        }
        if let Some(s) = &self.separator {
            m.push(("separator".to_string(), Value::Str(s.clone())));
        }
        Value::Obj(m)
    }
}

/// A call that satisfies every condition added to it (see [`StandIn::when`]).
#[derive(Debug, Clone, Default)]
pub struct Case {
    conditions: Vec<Value>,
}

impl Case {
    /// A case with no condition yet (it matches every call until one is added).
    pub fn new() -> Case {
        Case::default()
    }

    fn with(mut self, kind: &str, v: Value) -> Case {
        self.conditions
            .push(Value::Obj(vec![(kind.to_string(), v)]));
        self
    }

    /// The first arguments are these.
    #[must_use]
    pub fn args_start_with(self, prefix: &[&str]) -> Case {
        let v = Value::Arr(
            prefix
                .iter()
                .map(|a| Value::Str((*a).to_string()))
                .collect(),
        );
        self.with("args", v)
    }

    /// Some argument is exactly this.
    #[must_use]
    pub fn has_arg(self, arg: &str) -> Case {
        self.with("arg", Value::Str(arg.to_string()))
    }

    /// No argument is any of these.
    #[must_use]
    pub fn has_none_of(self, args: &[&str]) -> Case {
        let v = Value::Arr(args.iter().map(|a| Value::Str((*a).to_string())).collect());
        self.with("no_arg_of", v)
    }

    /// The path exists when the call is made.
    #[must_use]
    pub fn file_exists(self, path: &Path) -> Case {
        self.with("file", Value::Str(text(path)))
    }

    /// The path does not exist when the call is made.
    #[must_use]
    pub fn file_missing(self, path: &Path) -> Case {
        self.with("no_file", Value::Str(text(path)))
    }

    /// The arguments, joined by single spaces with a space on each side, hold `text` (the shell's `case " $* " in *text*)`).
    #[must_use]
    pub fn args_text_contains(self, text: &str) -> Case {
        self.with("args_text", Value::Str(text.to_string()))
    }

    /// The file exists and holds `text` (a relative path is read from the call's working folder).
    #[must_use]
    pub fn file_contains(self, path: &Path, text: &str) -> Case {
        self.with(
            "file_has",
            Value::Arr(vec![
                Value::Str(self::text(path)),
                Value::Str(text.to_string()),
            ]),
        )
    }

    /// The file does not hold `text`, or does not exist.
    #[must_use]
    pub fn file_lacks(self, path: &Path, text: &str) -> Case {
        self.with(
            "file_lacks",
            Value::Arr(vec![
                Value::Str(self::text(path)),
                Value::Str(text.to_string()),
            ]),
        )
    }

    /// The working folder ends with this text (compared with `/` as the separator).
    #[must_use]
    pub fn cwd_ends_with(self, tail: &str) -> Case {
        self.with("cwd_ends", Value::Str(tail.to_string()))
    }
}

/// Which calls a case answers.
#[derive(Debug, Clone)]
enum Match {
    /// The first arguments are these.
    Prefix(Vec<String>),
    /// Any argument holds this text.
    Contains(String),
    /// Every condition holds.
    All(Vec<Value>),
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

    /// Replaces the default answer (the one no case answers) with `actions`, which may use every step of [`Actions`].
    #[must_use]
    pub fn answers(mut self, actions: Actions) -> StandIn {
        self.actions = actions;
        self
    }

    /// Answers a call that satisfies every condition of `case` with `actions`, as [`StandIn::when_args`] (later cases win).
    #[must_use]
    pub fn when(mut self, case: &Case, actions: Actions) -> StandIn {
        self.cases
            .insert(0, (Match::All(case.conditions.clone()), actions));
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

    /// Whether any answer echoes stdin, which needs it read.
    fn reads_stdin_for_echo(&self) -> bool {
        self.actions.echoes || self.cases.iter().any(|(_, a)| a.echoes)
    }

    /// Every exit code the stand-in can end with.
    fn exit_codes(&self) -> impl Iterator<Item = i32> + '_ {
        std::iter::once(self.actions.exit).chain(self.cases.iter().map(|(_, a)| a.exit))
    }

    fn script(&self, record: &Path) -> Value {
        let cases = self
            .cases
            .iter()
            .map(|(m, a)| {
                let key = match m {
                    Match::Prefix(p) => ("args".to_string(), strs(p)),
                    Match::Contains(t) => ("contains".to_string(), Value::Str(t.clone())),
                    Match::All(c) => ("all".to_string(), Value::Arr(c.clone())),
                };
                Value::Obj(vec![key, ("actions".to_string(), a.to_value())])
            })
            .collect();
        let mut m = vec![
            ("actions".to_string(), self.actions.to_value()),
            ("cases".to_string(), Value::Arr(cases)),
            (
                "read_stdin".to_string(),
                Value::Bool(self.read_stdin || self.reads_stdin_for_echo()),
            ),
            ("record_env".to_string(), strs(&self.record_env)),
        ];
        if self.record {
            m.push(("record".to_string(), Value::Str(text(record))));
        }
        Value::Obj(m)
    }

    /// The stand-in as bytes, for a test that packs it into an archive or serves it (a "tool" a code under test downloads or unpacks and
    /// then runs): the program and its script, to be written side by side. Nothing is installed and nothing is recorded (a recording
    /// stand-in has no folder of its own here: it is refused).
    ///
    /// # Errors
    ///
    /// When the name is not a bare program name, the stand-in is recording, or the stand-in binary cannot be found, built or read.
    pub fn files(&self) -> io::Result<Files> {
        self.check_name()?;
        if self.record {
            return Err(invalid(
                "a recording stand-in cannot be packed: its record has no folder".to_string(),
            ));
        }
        let stub = stub_binary().map_err(io::Error::other)?;
        Ok(Files {
            program_name: format!("{}{}", self.name, std::env::consts::EXE_SUFFIX),
            program: std::fs::read(&stub)?,
            script_name: format!("{}.stand-in.json", self.name),
            script: self.script(Path::new("")).to_json().into_bytes(),
        })
    }

    fn check_name(&self) -> io::Result<()> {
        let bad_name = self.name.is_empty()
            || self.name.contains(['/', '\\'])
            || self.name.to_ascii_lowercase().ends_with(".exe");
        if bad_name {
            return Err(invalid(format!(
                "`{}` is not a bare program name (no folder, no `.exe`)",
                self.name
            )));
        }
        Ok(())
    }

    /// Installs the stand-in under its name in the folder and writes its script. A stand-in of the same name already there is
    /// replaced and its record starts empty.
    ///
    /// # Errors
    ///
    /// When the name is not a bare program name, the folder is missing, or the stand-in binary cannot be found, built or linked.
    pub fn install(&self) -> io::Result<Installed> {
        self.check_name()?;
        // A Unix exit status is one byte: 256 would read as 0, and -1 as 255, so a test asking for either never sees what it wrote.
        if let Some(code) = self
            .exit_codes()
            .find(|c| cfg!(unix) && !(0..=255).contains(c))
        {
            return Err(invalid(format!(
                "exit code {code} does not exist on Unix: use 0 to 255"
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

/// A stand-in as the two files an archive or a download holds: put both beside each other under `program_name` and `script_name`
/// and the program runs as installed (see [`StandIn::files`]).
#[derive(Debug, Clone)]
pub struct Files {
    /// The file name of the program (`<name>` plus `.exe` on Windows).
    pub program_name: String,
    /// The bytes of the program.
    pub program: Vec<u8>,
    /// The file name of its script (`<name>.stand-in.json`), which must sit in the same folder.
    pub script_name: String,
    /// The bytes of the script.
    pub script: Vec<u8>,
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

/// The hash of the sources this kit was built from, in the name of the compiled binary and checked on a prebuilt one.
fn source_hash() -> String {
    stand_in_json::source_hash(STUB_SOURCE, JSON_SOURCE)
}

fn locate_stub() -> Result<PathBuf, String> {
    locate_from(
        crate::ambient::ambient()
            .var(STAND_IN_ENV)
            .map(std::ffi::OsStr::to_os_string),
    )
}

/// [`locate_stub`] with the variable's value given (a test names a binary without touching the environment).
fn locate_from(named: Option<std::ffi::OsString>) -> Result<PathBuf, String> {
    if let Some(p) = named {
        let p = PathBuf::from(p);
        return if p.is_file() {
            check_prebuilt(&p).map(|()| p)
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

/// A prebuilt binary must come from these sources: it prints their hash when asked, so a binary left from another commit is refused
/// with the fix, not run with a script format it does not know.
fn check_prebuilt(bin: &Path) -> Result<(), String> {
    let out = TestCommand::new(bin)
        .env("PADDOCK_STAND_IN_PRINT_HASH", "1")
        .output()
        .map_err(|e| format!("cannot run {STAND_IN_ENV} ({}): {e}", bin.display()))?;
    let said = String::from_utf8_lossy(&out.stdout);
    if said.trim() == source_hash() {
        Ok(())
    } else {
        Err(format!(
            "{STAND_IN_ENV} ({}) is not built from these sources (it says `{}`, the kit's is `{}`): rebuild it from this commit",
            bin.display(),
            said.trim(),
            source_hash()
        ))
    }
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
    let result = TestCommand::new(rustc_for(crate::ambient::ambient()))
        .env("RUSTUP_AUTO_INSTALL", "0")
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
    publish_stub(&out, built)
}

/// Moves the compiled stand-in to its hash-named place. On Windows a rename onto a file another test process already started fails; the
/// file is then the same program (its name holds the hash of the sources), so a file that is there is success.
fn publish_stub(out: &Path, built: &Path) -> Result<(), String> {
    match std::fs::rename(out, built) {
        Ok(()) => Ok(()),
        Err(_) if built.is_file() => Ok(()),
        Err(e) => Err(format!(
            "cannot move {} to {}: {e}",
            out.display(),
            built.display()
        )),
    }
}

/// [`rustc_program`] over what the kit captured (never the live `CARGO` or `RUSTC`: another test may have changed them, ADR 0100).
fn rustc_for(machine: &crate::Ambient) -> std::ffi::OsString {
    rustc_program(
        machine.cargo.clone().map(PathBuf::into_os_string),
        machine.var("RUSTC").map(std::ffi::OsStr::to_os_string),
    )
}

/// The compiler for the stand-in, from the values of `CARGO` and `RUSTC` (parameters, so a test changes no environment): the real `rustc` beside the `cargo` that runs the tests (a rustup proxy could install a toolchain
/// or pick another one), else `$RUSTC`, else `rustc` on the `PATH`.
fn rustc_program(
    cargo: Option<std::ffi::OsString>,
    rustc: Option<std::ffi::OsString>,
) -> std::ffi::OsString {
    let beside = cargo
        .map(PathBuf::from)
        .and_then(|c| {
            c.parent()
                .map(|d| d.join(format!("rustc{}", std::env::consts::EXE_SUFFIX)))
        })
        .filter(|p| p.is_file());
    beside
        .map(PathBuf::into_os_string)
        .or(rustc)
        .unwrap_or_else(|| "rustc".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Scratch;

    /// The compiler comes from the captured values: a live `RUSTC` another test set is not read (it fails when the lookup is live).
    #[test]
    fn the_compiler_comes_from_the_captured_values_not_the_live_environment() {
        let mut env = crate::env_lock();
        env.set("RUSTC", "/live/rustc");
        let mut machine = crate::ambient().clone();
        machine.cargo = None;
        machine.vars.retain(|(k, _)| k != "RUSTC");
        machine
            .vars
            .push(("RUSTC".into(), "/captured/rustc".into()));
        assert_eq!(
            rustc_for(&machine),
            std::ffi::OsString::from("/captured/rustc")
        );
        machine.vars.retain(|(k, _)| k != "RUSTC");
        assert_eq!(
            rustc_for(&machine),
            std::ffi::OsString::from("rustc"),
            "a live RUSTC must not be the fallback"
        );
    }

    #[test]
    fn a_rename_that_fails_onto_a_file_that_is_there_is_success_and_onto_nothing_is_an_error() {
        let s = Scratch::new();
        let built = s.work().join("paddock-stand-in-x");
        std::fs::write(&built, "already there").unwrap();
        // The source is gone, as when the rename lost the race for the target on Windows: the file that is there is the same program.
        assert_eq!(publish_stub(&s.work().join("missing"), &built), Ok(()));
        let err = publish_stub(&s.work().join("missing"), &s.work().join("nothing")).unwrap_err();
        assert!(err.contains("cannot move"), "{err}");
    }

    #[test]
    fn the_compiler_is_the_one_beside_cargo_then_rustc_then_the_path() {
        let s = Scratch::new();
        let rustc = s
            .work()
            .join(format!("rustc{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&rustc, "").unwrap();
        let cargo = Some(s.work().join("cargo").into_os_string());
        let named = std::ffi::OsString::from("/the/other/rustc");
        let other = Some(named.clone());
        assert_eq!(rustc_program(cargo, other.clone()), rustc.into_os_string());
        let gone = Some(s.work().join("no-such-dir").join("cargo").into_os_string());
        assert_eq!(
            rustc_program(gone.clone(), other.clone()),
            other.clone().unwrap()
        );
        assert_eq!(rustc_program(None, other), named);
        assert_eq!(rustc_program(gone, None), std::ffi::OsString::from("rustc"));
    }

    #[test]
    fn a_stale_or_missing_binary_named_by_the_variable_is_refused_before_any_test_runs_it() {
        let ok = stub_binary().unwrap();
        assert_eq!(locate_from(Some(ok.clone().into_os_string())), Ok(ok));
        let missing = locate_from(Some("/no/such/stand-in".into())).unwrap_err();
        assert!(missing.contains("which is not a file"), "{missing}");
        #[cfg(unix)]
        assert!(
            locate_from(Some("/bin/echo".into()))
                .unwrap_err()
                .contains("is not built from these sources")
        );
    }

    #[test]
    fn a_prebuilt_binary_of_these_sources_is_accepted_and_another_program_is_refused() {
        check_prebuilt(&stub_binary().unwrap()).unwrap();
        #[cfg(unix)]
        {
            let err = check_prebuilt(Path::new("/bin/echo")).unwrap_err();
            assert!(
                err.contains("is not built from these sources")
                    && err.contains("rebuild it from this commit"),
                "{err}"
            );
        }
        let missing = check_prebuilt(Path::new("/no/such/stand-in")).unwrap_err();
        assert!(missing.contains("cannot run"), "{missing}");
    }
}
