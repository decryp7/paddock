//! `dev ci guardian-as-user`: runs the orphan guardian's acting tests as a non-root user (the port of
//! `conformance/ci/guardian-as-user.sh`; ADR 0076 amendment 2026-11-09; docs/plans/ci-in-rust.md, WP-5). The Linux CI containers run as
//! root, and the guardian is off for root, so the crates job alone never runs the guardian's Linux code (`exe_of` through `/proc`,
//! `owner_holds_pipe`) in acting mode: as root those tests assert the root behaviour instead.
//!
//! Run from `crates/`: builds the `paddock-proc` and `paddock-chromium` unit tests (all features), then runs the guardian tests and
//! `paddock-chromium`'s owner-killed test as `PADDOCK_TEST_UID` (default 65534, nobody) through `setpriv`, with a private `TMPDIR` and
//! `HOME`. As root it drops to that user; run by a user it runs the tests directly. Each failure prints one `::error::` line with the
//! command, its exit code and the next step (ER-27); the job summary says what ran.
//!
//! Everything that starts a program, asks who runs it or changes an owner sits behind [`Machine`], so the root path (which a test cannot
//! take: it would need `sudo`) is tested with a fake that records the command lines. Both programs run with a limit (ER-32): the build
//! `PADDOCK_GUARDIAN_BUILD_TIMEOUT` seconds (default 1800), each test run `PADDOCK_GUARDIAN_TEST_TIMEOUT` (default 900); neither limit
//! is measured against a run yet (the shell had none), they only end a hang.

use super::Step;
use crate::ci_tool::{Output, Ran, ToolCall};
use clap::Args as ClapArgs;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// `dev ci guardian-as-user` (no arguments: `PADDOCK_TEST_UID` and the job's summary file come from the environment).
#[derive(ClapArgs, Debug)]
pub struct GuardianAsUserArgs {}

/// The short name the CLI uses.
pub type Args = GuardianAsUserArgs;

const NAME: &str = "guardian-as-user";

/// What the step needs from the machine.
pub trait Machine {
    /// The effective user id of this process.
    fn euid(&self) -> Result<u32, String>;
    /// Whether `program` is on `PATH`.
    fn has_program(&self, program: &str) -> bool;
    /// Runs a program (its `run:` and `ran:` lines go to `log`).
    fn run(&self, call: &ToolCall, log: &mut dyn Write) -> Result<Ran, String>;
    /// Gives a folder to a user and group id.
    fn chown(&self, path: &Path, uid: u32) -> Result<(), String>;
}

/// The machine this process runs on.
pub struct Real<'a> {
    /// The job's environment.
    pub env: &'a HashMap<String, String>,
}

impl Machine for Real<'_> {
    fn euid(&self) -> Result<u32, String> {
        let ran = ToolCall::new("id")
            .args(["-u"])
            .timeout(Duration::from_secs(20))
            .run(self.env, &mut std::io::sink())?;
        ran.stdout
            .trim()
            .parse()
            .map_err(|_| format!("`id -u` said '{}'", ran.stdout.trim()))
    }

    fn has_program(&self, program: &str) -> bool {
        super::on_path(self.env, program)
    }

    fn run(&self, call: &ToolCall, log: &mut dyn Write) -> Result<Ran, String> {
        call.run(self.env, log)
    }

    fn chown(&self, path: &Path, uid: u32) -> Result<(), String> {
        #[cfg(unix)]
        {
            std::os::unix::fs::chown(path, Some(uid), Some(uid)).map_err(|e| e.to_string())
        }
        #[cfg(not(unix))]
        {
            let _ = (path, uid);
            Err("changing an owner is not done on this OS".to_string())
        }
    }
}

/// The test binary of a unit-test target in cargo's `--no-run` output: the last `Executable unittests src/lib.rs (<path>/<crate>-<hash>)`.
pub fn exe_of(build_output: &str, krate: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(
        r"(?m)Executable unittests src/lib\.rs \((.*/{}-[0-9a-f]*)\)\s*$",
        regex::escape(krate)
    ))
    .ok()?;
    re.captures_iter(build_output).last().map(|c| c[1].to_string())
}

fn seconds(env: &HashMap<String, String>, key: &str, default: u64) -> Duration {
    Duration::from_secs(
        env.get(key)
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|n| *n > 0)
            .unwrap_or(default),
    )
}

/// A private folder (mode 0700) under the temp folder that is removed at the end.
struct Private(PathBuf);

impl Private {
    fn make(base: &Path) -> std::io::Result<Private> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let dir = base.join(format!("guardian-as-user.{}.{nanos}", std::process::id()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
        }
        #[cfg(not(unix))]
        std::fs::create_dir_all(&dir)?;
        Ok(Private(dir))
    }
}

impl Drop for Private {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The command over the real machine.
pub fn run(env: &HashMap<String, String>, out: &mut dyn Write) -> i32 {
    run_with(&Real { env }, env, out)
}

/// The command over a machine (tests).
pub fn run_with(machine: &dyn Machine, env: &HashMap<String, String>, out: &mut dyn Write) -> i32 {
    let mut step = Step {
        env,
        out,
        windows: false,
        name: NAME,
    };
    let mut uid: u32 = match step.var("PADDOCK_TEST_UID").map(str::parse) {
        None => 65534,
        Some(Ok(u)) => u,
        Some(Err(_)) => {
            return step.fail(
                format!(
                    "PADDOCK_TEST_UID is '{}', not a user id.",
                    step.var("PADDOCK_TEST_UID").unwrap_or("")
                ),
                "set it to a number, or leave it unset (65534, nobody).",
            );
        }
    };
    let summary = PathBuf::from(step.var("GITHUB_STEP_SUMMARY").unwrap_or("/dev/null"));
    let euid = match machine.euid() {
        Ok(u) => u,
        Err(e) => {
            return step.fail(
                format!("cannot tell who runs this step: {e}."),
                "check that `id` is installed in the image.",
            );
        }
    };
    let as_user: Vec<String> = if euid == 0 {
        if !machine.has_program("setpriv") {
            return step.fail(
                format!("setpriv (util-linux) is missing, so the tests cannot run as uid {uid}."),
                "install util-linux in the image.",
            );
        }
        [
            "setpriv".to_string(),
            format!("--reuid={uid}"),
            format!("--regid={uid}"),
            "--clear-groups".into(),
            "--no-new-privs".into(),
            "--".into(),
        ]
        .to_vec()
    } else {
        uid = euid;
        Vec::new()
    };

    step.say("Building the paddock-proc and paddock-chromium unit tests");
    let Some(temp) = super::temp_folder(env) else {
        return step.fail(
            "no temp folder (TMPDIR, RUNNER_TEMP, TEMP or TMP) is set.",
            "run this step in a runner job.",
        );
    };
    let private = match Private::make(&temp) {
        Ok(p) => p,
        Err(e) => {
            return step.fail(
                format!("cannot make a private folder under {}: {e}.", temp.display()),
                "make the temp folder writable.",
            );
        }
    };
    let build_log = private.0.join("build.log");
    // The build log lives in the private folder of the user's tests too, so the private folder is made first and handed over after.
    let mut log = Vec::new();
    let build = ToolCall::new("cargo")
        .args(["test", "--no-run", "--all-features", "-p", "paddock-proc", "-p", "paddock-chromium"])
        .pass(crate::pre_push::exec::PASS.iter().copied())
        .timeout(seconds(env, "PADDOCK_GUARDIAN_BUILD_TIMEOUT", 1800))
        .output(Output::File(build_log.clone()));
    let ran = machine.run(&build, &mut log);
    flush(&mut step, &mut log);
    let ran = match ran {
        Ok(r) => r,
        Err(e) => {
            return step.fail(
                format!("cargo could not be started: {e}"),
                "check that cargo is on PATH in the image.",
            );
        }
    };
    if !ran.success {
        for l in crate::ci_tool::tail(&ran.stderr, crate::ci_tool::TAIL_LINES).lines() {
            step.say(l);
        }
        return step.fail(
            format!("`cargo test --no-run` failed ({}; output above).", ran.how),
            "fix the build first.",
        );
    }
    let (Some(proc_bin), Some(chromium_bin)) = (exe_of(&ran.stderr, "paddock_proc"), exe_of(&ran.stderr, "paddock_chromium")) else {
        return step.fail(
            format!(
                "the unit test binaries were not found in cargo's output; see {}.",
                build_log.display()
            ),
            "check that both packages have unit tests in src/lib.rs.",
        );
    };

    if euid == 0
        && let Err(e) = machine.chown(&private.0, uid)
    {
        return step.fail(
            format!("cannot give {} to uid {uid}: {e}.", private.0.display()),
            "check the image's users.",
        );
    }
    let guardian_log = private.0.join("guardian.log");
    let mut text = String::from("### Orphan guardian as a user\n\n| Tests | As | Result | Time |\n|---|---|---|---|\n");
    let mut results = Vec::new();
    for (bin, filter) in [
        (&proc_bin, "guardian::"),
        (&chromium_bin, "launch::tests::a_browser_whose_owner_is_killed"),
    ] {
        let mut argv = as_user.clone();
        argv.extend([
            "env".to_string(),
            format!("TMPDIR={}", private.0.display()),
            format!("HOME={}", private.0.display()),
            format!("PADDOCK_PROC_GUARDIAN_LOG={}", guardian_log.display()),
            bin.clone(),
            filter.to_string(),
        ]);
        step.say(format!("Running as uid {uid}: {}", argv.join(" ")));
        let started = Instant::now();
        let call = ToolCall::new(argv[0].clone())
            .args(argv[1..].iter().cloned())
            .pass(crate::pre_push::exec::PASS.iter().copied())
            .timeout(seconds(env, "PADDOCK_GUARDIAN_TEST_TIMEOUT", 900))
            .output(Output::Stream);
        let mut log = Vec::new();
        let ran = machine.run(&call, &mut log);
        flush(&mut step, &mut log);
        let took = started.elapsed().as_secs();
        let failure = match ran {
            Ok(r) if r.success => None,
            Ok(r) => Some(r.how),
            Err(e) => Some(e),
        };
        if let Some(how) = failure {
            match std::fs::read_to_string(&guardian_log) {
                Ok(t) => {
                    step.say("The guardian's log:");
                    step.say(t.trim_end());
                }
                Err(_) => step.say(format!("The guardian wrote no log ({})", guardian_log.display())),
            }
            results.push(format!("| `{filter}` | uid {uid} | failed ({how}) | {took} s |\n"));
            text.push_str(&results.join(""));
            write_summary(&summary, &text);
            return step.fail(
                format!("`{filter}` failed as uid {uid} ({how}, {took} s): {}.", argv.join(" ")),
                "run the same command in the CI image to see it.",
            );
        }
        results.push(format!("| `{filter}` | uid {uid} | passed | {took} s |\n"));
    }
    text.push_str(&results.join(""));
    write_summary(&summary, &text);
    step.say(format!("The guardian's acting tests passed as uid {uid}"));
    0
}

fn flush(step: &mut Step<'_>, log: &mut Vec<u8>) {
    for l in String::from_utf8_lossy(log).lines() {
        step.say(l);
    }
    log.clear();
}

/// Appends to the job's summary file (`/dev/null` without one); a summary that cannot be written never fails the step.
fn write_summary(path: &Path, text: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).create(true).open(path) {
        let _ = f.write_all(text.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const BUILD: &str = "   Compiling paddock-proc v0.1.0\n  Executable unittests src/lib.rs (target/debug/deps/paddock_proc-0123abcd)\n  Executable tests/it/main.rs (target/debug/deps/it-ff)\n  Executable unittests src/lib.rs (target/debug/deps/paddock_chromium-9f8e7d)\n";

    /// A scripted machine: who runs it, what the programs say, what was started.
    struct Fake {
        euid: u32,
        setpriv: bool,
        build_ok: bool,
        fail_filter: Option<&'static str>,
        started: Mutex<Vec<String>>,
        chowned: Mutex<Vec<(PathBuf, u32)>>,
        build_text: &'static str,
    }

    impl Fake {
        fn new(euid: u32) -> Fake {
            Fake {
                euid,
                setpriv: true,
                build_ok: true,
                fail_filter: None,
                started: Mutex::new(Vec::new()),
                chowned: Mutex::new(Vec::new()),
                build_text: BUILD,
            }
        }
    }

    impl Machine for Fake {
        fn euid(&self) -> Result<u32, String> {
            Ok(self.euid)
        }
        fn has_program(&self, program: &str) -> bool {
            program != "setpriv" || self.setpriv
        }
        fn run(&self, call: &ToolCall, _: &mut dyn Write) -> Result<Ran, String> {
            self.started.lock().unwrap().push(call.display());
            let cargo = call.display().starts_with("cargo ");
            let failed = self.fail_filter.is_some_and(|f| call.display().ends_with(f)) || (cargo && !self.build_ok);
            Ok(Ran {
                success: !failed,
                how: if failed { "exit 101".into() } else { "exit 0".into() },
                stdout: String::new(),
                stderr: if cargo { self.build_text.to_string() } else { String::new() },
            })
        }
        fn chown(&self, path: &Path, uid: u32) -> Result<(), String> {
            self.chowned.lock().unwrap().push((path.to_path_buf(), uid));
            Ok(())
        }
    }

    fn env(dir: &Path) -> HashMap<String, String> {
        [
            ("TMPDIR".to_string(), dir.display().to_string()),
            ("GITHUB_STEP_SUMMARY".to_string(), dir.join("summary").display().to_string()),
        ]
        .into()
    }

    fn go(f: &Fake, extra: &[(&str, &str)]) -> (i32, String, String) {
        let dir = super::super::canonical_tempdir();
        let mut e = env(dir.path());
        e.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        let mut out = Vec::new();
        let code = run_with(f, &e, &mut out);
        (
            code,
            String::from_utf8(out).unwrap(),
            std::fs::read_to_string(dir.path().join("summary")).unwrap_or_default(),
        )
    }

    #[test]
    fn the_test_binaries_are_the_last_unit_test_executables_of_each_crate() {
        assert_eq!(
            exe_of(BUILD, "paddock_proc"),
            Some("target/debug/deps/paddock_proc-0123abcd".into())
        );
        assert_eq!(
            exe_of(BUILD, "paddock_chromium"),
            Some("target/debug/deps/paddock_chromium-9f8e7d".into())
        );
        assert_eq!(exe_of(BUILD, "paddock_missing"), None);
        let twice = format!("{BUILD}  Executable unittests src/lib.rs (target/debug/deps/paddock_proc-beef)\n");
        assert_eq!(exe_of(&twice, "paddock_proc"), Some("target/debug/deps/paddock_proc-beef".into()));
    }

    #[test]
    fn as_root_the_tests_run_as_the_configured_user_through_setpriv_with_a_private_folder_given_to_it() {
        let f = Fake::new(0);
        let (code, out, summary) = go(&f, &[("PADDOCK_TEST_UID", "4242")]);
        assert_eq!(code, 0, "{out}");
        let started = f.started.lock().unwrap().clone();
        assert!(
            started[0].starts_with("cargo test --no-run --all-features -p paddock-proc -p paddock-chromium"),
            "{started:?}"
        );
        for (i, filter) in [(1, "guardian::"), (2, "launch::tests::a_browser_whose_owner_is_killed")] {
            assert!(
                started[i].starts_with("setpriv --reuid=4242 --regid=4242 --clear-groups --no-new-privs -- env TMPDIR="),
                "{}",
                started[i]
            );
            assert!(
                started[i].contains("PADDOCK_PROC_GUARDIAN_LOG=") && started[i].ends_with(filter),
                "{}",
                started[i]
            );
        }
        let chowned = f.chowned.lock().unwrap().clone();
        assert_eq!(chowned.len(), 1);
        assert_eq!(chowned[0].1, 4242);
        assert!(out.contains("Running as uid 4242: setpriv "), "{out}");
        assert!(out.ends_with("The guardian's acting tests passed as uid 4242\n"), "{out}");
        assert!(summary.contains("| `guardian::` | uid 4242 | passed |"), "{summary}");
    }

    #[test]
    fn as_a_user_the_tests_run_directly_as_that_user_and_the_default_uid_is_nobody() {
        let f = Fake::new(501);
        let (code, out, _) = go(&f, &[("PADDOCK_TEST_UID", "4242")]);
        assert_eq!(code, 0, "{out}");
        let started = f.started.lock().unwrap().clone();
        assert!(started[1].starts_with("env TMPDIR="), "no setpriv: {}", started[1]);
        assert!(out.contains("Running as uid 501: env "), "{out}");
        assert!(f.chowned.lock().unwrap().is_empty());
        let (_, out, _) = go(&Fake::new(0), &[]);
        assert!(out.contains("Running as uid 65534: setpriv --reuid=65534"), "{out}");
    }

    #[test]
    fn every_failure_is_one_error_line_with_the_command_and_the_next_step() {
        let mut no_setpriv = Fake::new(0);
        no_setpriv.setpriv = false;
        let (code, out, _) = go(&no_setpriv, &[]);
        assert_eq!(code, 1);
        assert_eq!(
            out,
            "::error::guardian-as-user: setpriv (util-linux) is missing, so the tests cannot run as uid 65534. Next step: install util-linux in the image.\n"
        );
        let mut broken = Fake::new(501);
        broken.build_ok = false;
        let (code, out, _) = go(&broken, &[]);
        assert_eq!(code, 1);
        assert_eq!(out.lines().filter(|l| l.starts_with("::error::")).count(), 1, "{out}");
        assert!(
            out.contains("`cargo test --no-run` failed (exit 101; output above). Next step: fix the build first."),
            "{out}"
        );
        let mut lost = Fake::new(501);
        lost.build_text = "Compiling only\n";
        let (code, out, _) = go(&lost, &[]);
        assert_eq!(code, 1);
        assert!(
            out.contains("the unit test binaries were not found in cargo's output; see "),
            "{out}"
        );
        let mut red = Fake::new(501);
        red.fail_filter = Some("guardian::");
        let (code, out, summary) = go(&red, &[]);
        assert_eq!(code, 1);
        assert!(out.contains("The guardian wrote no log ("), "{out}");
        assert!(
            out.contains("::error::guardian-as-user: `guardian::` failed as uid 501 (exit 101, "),
            "{out}"
        );
        assert!(out.contains("run the same command in the CI image to see it."), "{out}");
        assert!(summary.contains("| `guardian::` | uid 501 | failed (exit 101) |"), "{summary}");
        let (code, out, _) = go(&Fake::new(501), &[("PADDOCK_TEST_UID", "nobody")]);
        assert_eq!(code, 1);
        assert!(out.contains("PADDOCK_TEST_UID is 'nobody', not a user id."), "{out}");
    }
}
