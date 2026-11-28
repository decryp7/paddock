//! Real processes under a deadline: nothing here can hang a test run, and nothing is left behind.
#![allow(dead_code)]

use paddock_proc::command;
use paddock_testkit::{TestChild, TestCommand};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

pub const BIN: &str = env!("CARGO_BIN_EXE_throttle");
/// Every wait in the tests is bounded by this.
pub const DEADLINE: Duration = Duration::from_secs(40);

/// A child that is killed (and reaped) when dropped, so a failing test leaves no process behind.
pub struct Guard {
    pub child: TestChild,
    pub label: String,
}

impl Guard {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Waits for the exit within `limit`; None on timeout.
    pub fn wait(&mut self, limit: Duration) -> Option<std::process::ExitStatus> {
        let end = Instant::now() + limit;
        loop {
            if let Ok(Some(s)) = self.child.try_wait() {
                return Some(s);
            }
            if Instant::now() >= end {
                return None;
            }
            // conformance: hang-guard — a poll of the child's exit, bounded by the 40 s limit the caller passes
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn finish(&mut self) -> std::process::ExitStatus {
        self.wait(DEADLINE)
            .unwrap_or_else(|| panic!("{} did not finish within {DEADLINE:?}", self.label))
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

/// Polls `cond` every 20 ms until it holds or the deadline passes.
pub fn wait_until(what: &str, mut cond: impl FnMut() -> bool) {
    let end = Instant::now() + DEADLINE;
    while Instant::now() < end {
        if cond() {
            return;
        }
        // conformance: hang-guard — a poll of a condition, bounded by the 40 s DEADLINE
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {what}");
}

/// A scratch world: the lock folder, a flag file, an output file, a home folder.
pub struct World {
    pub dir: tempfile::TempDir,
}

/// A test that ends, by a panic too, releases every holder of its world and shows that none is left: nothing started with this world's
/// folder in its command line may outlive it (ER-14, ER-51). A survivor is ended by its identity and fails the test.
impl Drop for World {
    fn drop(&mut self) {
        self.release();
        let dir = self.dir.path().to_path_buf();
        let end = Instant::now() + Duration::from_secs(10);
        let mut left = leaked_in(&dir);
        while !left.is_empty() && Instant::now() < end {
            // conformance: hang-guard — a poll of the holders ending after the release, bounded by 10 s
            std::thread::sleep(Duration::from_millis(50));
            left = leaked_in(&dir);
        }
        if left.is_empty() {
            return;
        }
        for id in &left {
            paddock_proc::end(*id);
        }
        let msg = format!("test left {} process(es) running that mention {}", left.len(), dir.display());
        if std::thread::panicking() {
            eprintln!("{msg}");
        } else {
            panic!("{msg}");
        }
    }
}

impl World {
    pub fn new() -> World {
        World {
            dir: tempfile::tempdir().expect("tempdir"),
        }
    }
    pub fn root(&self) -> PathBuf {
        let p = self.dir.path().join("locks");
        let _ = std::fs::create_dir_all(&p);
        p
    }
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
    pub fn lock_dir(&self) -> PathBuf {
        self.root().join("throttle-slot1.info")
    }
    pub fn out(&self) -> String {
        std::fs::read_to_string(self.path("out")).unwrap_or_default()
    }
    pub fn out_words(&self) -> Vec<String> {
        self.out().split_whitespace().map(String::from).collect()
    }
    pub fn flag(&self) -> PathBuf {
        self.path("flag")
    }
    pub fn release(&self) {
        let _ = std::fs::write(self.flag(), "");
    }
    pub fn count(&self, prefix: &str) -> usize {
        std::fs::read_dir(self.root())
            .map(|r| {
                r.filter_map(Result::ok)
                    .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
                    .count()
            })
            .unwrap_or(0)
    }
    pub fn owner_line(&self, n: usize) -> String {
        std::fs::read_to_string(self.lock_dir())
            .unwrap_or_default()
            .lines()
            .nth(n)
            .unwrap_or("")
            .to_string()
    }
    pub fn locked(&self) -> bool {
        std::fs::metadata(self.lock_dir()).is_ok_and(|m| m.len() > 0)
    }
    pub fn read(&self, p: impl AsRef<Path>) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }
}

/// The shell loop every test holder runs, written once: it waits for the file named by `$<var>` and ends on its own when the flag's
/// folder is gone (the test's temp folder was deleted, so the flag can never appear) or after `${HOLD_MAX:-3000}` polls of 0.1 s
/// (at least 5 minutes; the slowest throttle test takes under 30 s, 40 s is the longest wait, ER-12). A holder that nobody released
/// (a test that panicked) ended up polling for ever with parent pid 1. Afterwards `[ -e <flag> ]` tells released from given up.
#[macro_export]
macro_rules! wait_flag_loop {
    ($flag:literal, $dir:literal) => {
        concat!(
            r#"hold_n=0; while [ ! -e ""#,
            $flag,
            r#"" ] && [ -d ""#,
            $dir,
            r#"" ] && [ "$hold_n" -lt "${HOLD_MAX:-3000}" ]; do sleep 0.1; hold_n=$((hold_n+1)); done"#
        )
    };
}

/// The command a holder runs: waits for the flag file (see [`wait_flag_loop`]), then appends its name to the output file; a holder that
/// gave up writes nothing.
pub const HOLD: &str = concat!(crate::wait_flag_loop!("$0", "${0%/*}"), r#"; [ -e "$0" ] && echo "$2" >> "$1""#);

/// The processes of other tests or of the machine never match: the marker is this world's own folder name, unique to the run.
/// Identities (pid plus start time), never bare pids (rule 53).
pub fn leaked_in(dir: &Path) -> Vec<paddock_proc::ProcessId> {
    paddock_proc::processes_mentioning(&dir.to_string_lossy())
}

/// Base environment of a test run of throttle or the shell plock: fast polling, a private lock folder, a private home. `TestCommand`
/// starts from an empty environment, so no `THROTTLE_*`, `CI` or `CARGO_TARGET_DIR` of the machine reaches the child (no deny-list).
pub fn base_env(c: &mut TestCommand, w: &World) {
    c.env("PLOCK_DIR", w.root())
        .env("PLOCK_POLL", "0.2")
        .env("PLOCK_WAIT_LOG", "1")
        .env("PLOCK_UNREADABLE_LIMIT", "3")
        // One at a time: the tests of the queue need it whatever the load of the machine they run on (slots.rs covers the rest).
        .env("THROTTLE_SLOTS", "1")
        // The world's home and every per-user folder derived from it, set together: testkit's command sandbox names its own (XDG_*,
        // APPDATA, ...), and throttle reads XDG_STATE_HOME and XDG_CONFIG_HOME before HOME on Linux, so a HOME alone left the
        // sandbox's folders in force there (run 9143, places tests on the Linux runner).
        .env("HOME", w.path("home"))
        .env("USERPROFILE", w.path("home"))
        .env("XDG_STATE_HOME", w.path("home/.local/state"))
        .env("XDG_CONFIG_HOME", w.path("home/.config"))
        .env("XDG_DATA_HOME", w.path("home/.local/share"))
        .env("XDG_CACHE_HOME", w.path("home/.cache"))
        .env("APPDATA", w.path("home/AppData/Roaming"))
        .env("LOCALAPPDATA", w.path("home/AppData/Local"))
        .env("CARGO_HOME", w.path("home/.cargo"))
        // The run logs go to the world's own dev-targets folder, never the machine's (ER-14).
        .env("PADDOCK_DEV_TARGETS", w.path("dev-targets"))
        .stdin(Stdio::null());
}

/// Starts `throttle <args>` in the world.
pub fn throttle(w: &World, args: &[&str]) -> Guard {
    heavy_env(w, args, &[])
}

pub fn heavy_env(w: &World, args: &[&str], env: &[(&str, &str)]) -> Guard {
    heavy_in(w, None, args, env)
}

/// Like `heavy_env`, in the folder `cwd`.
pub fn heavy_in(w: &World, cwd: Option<&Path>, args: &[&str], env: &[(&str, &str)]) -> Guard {
    let mut c = TestCommand::new(BIN);
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    base_env(&mut c, w);
    c.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        if *v == UNSET {
            c.env_remove(k);
        } else {
            c.env(k, v);
        }
    }
    Guard {
        child: c.spawn().expect("spawn throttle"),
        label: format!("throttle {args:?}"),
    }
}

/// As a value in the `env` of [`heavy_in`]: the variable is removed from throttle's environment instead of set.
pub const UNSET: &str = "\u{0}unset";

/// Runs `throttle <args>` to the end and returns (exit code, stdout, stderr).
pub fn heavy_out(w: &World, args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
    heavy_out_in(w, None, args, env)
}

/// Like `heavy_out`, in the folder `cwd`.
pub fn heavy_out_in(w: &World, cwd: Option<&Path>, args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
    let mut g = heavy_in(w, cwd, args, env);
    let st = g.finish();
    let mut o = String::new();
    let mut e = String::new();
    use std::io::Read;
    if let Some(mut s) = g.child.stdout.take() {
        let _ = s.read_to_string(&mut o);
    }
    if let Some(mut s) = g.child.stderr.take() {
        let _ = s.read_to_string(&mut e);
    }
    (st.code().unwrap_or(-1), o, e)
}

/// `throttle run --no-gate NAME -- sh -c HOLD FLAG OUT NAME`
pub fn hold_args<'a>(name: &'a str, flag: &'a str, out: &'a str) -> Vec<&'a str> {
    vec!["run", "--no-gate", name, "--", "sh", "-c", HOLD, flag, out, name]
}

/// `throttle run --no-gate NAME -- sh -c 'echo NAME >> OUT' OUT`
pub fn echo_args<'a>(name: &'a str, out: &'a str) -> Vec<&'a str> {
    vec!["run", "--no-gate", name, "--", "sh", "-c", r#"echo "$1" >> "$0""#, out, name]
}

/// A process that has already ended: its (now free) pid.
pub fn dead_pid() -> u32 {
    let mut c = command("sh").args(["-c", "exit 0"]).spawn().expect("spawn sh");
    let pid = c.id();
    let _ = c.wait();
    pid
}

/// A long-running process (a live pid); killed when the guard drops.
pub fn sleeper() -> Guard {
    Guard {
        child: TestCommand::new("sleep").arg("60").spawn().expect("spawn sleep"),
        label: "sleep 60".into(),
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
