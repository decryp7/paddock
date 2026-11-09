//! Real processes under a deadline: nothing here can hang a test run, and nothing is left behind.
#![allow(dead_code)]

use paddock_proc::command;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

pub const BIN: &str = env!("CARGO_BIN_EXE_heavy");
/// Every wait in the tests is bounded by this.
pub const DEADLINE: Duration = Duration::from_secs(40);

/// A child that is killed (and reaped) when dropped, so a failing test leaves no process behind.
pub struct Guard {
    pub child: Child,
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
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {what}");
}

/// A scratch world: the lock folder, a flag file, an output file, a home folder.
pub struct World {
    pub dir: tempfile::TempDir,
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
        self.root().join("pitcrew-heavy-v2.lock")
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
        std::fs::read_to_string(self.lock_dir().join("owner"))
            .unwrap_or_default()
            .lines()
            .nth(n)
            .unwrap_or("")
            .to_string()
    }
    pub fn locked(&self) -> bool {
        std::fs::metadata(self.lock_dir().join("owner")).is_ok_and(|m| m.len() > 0)
    }
    pub fn read(&self, p: impl AsRef<Path>) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }
}

/// The command a holder runs: waits for the flag file, then appends its name to the output file.
pub const HOLD: &str = r#"while [ ! -e "$0" ]; do sleep 0.1; done; echo "$2" >> "$1""#;

/// Base environment of a test run of heavy or the shell plock: fast polling, a private lock folder, a private home.
pub fn base_env(c: &mut std::process::Command, w: &World) {
    c.env("PLOCK_DIR", w.root())
        .env("PLOCK_POLL", "0.2")
        .env("PLOCK_WAIT_LOG", "1")
        .env("PLOCK_UNREADABLE_LIMIT", "3")
        .env("HOME", w.path("home"))
        .env_remove("HEAVY_DIR")
        .env_remove("CI")
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("HEAVY_SLOTS")
        .env_remove("HEAVY_LOG_FILE")
        .env_remove("DEVKIT_RUN_ID")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env_remove("HEAVY_LOCK_DIR")
        .env_remove("HEAVY_DRAIN_MS")
        .env("CARGO_HOME", w.path("home/.cargo"))
        .stdin(Stdio::null());
}

/// Starts `heavy <args>` in the world.
pub fn heavy(w: &World, args: &[&str]) -> Guard {
    heavy_env(w, args, &[])
}

pub fn heavy_env(w: &World, args: &[&str], env: &[(&str, &str)]) -> Guard {
    heavy_in(w, None, args, env)
}

/// Like `heavy_env`, in the folder `cwd`.
pub fn heavy_in(w: &World, cwd: Option<&Path>, args: &[&str], env: &[(&str, &str)]) -> Guard {
    let mut c = command(BIN);
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    base_env(&mut c, w);
    c.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (k, v) in env {
        c.env(k, v);
    }
    Guard {
        child: c.spawn().expect("spawn heavy"),
        label: format!("heavy {args:?}"),
    }
}

/// Runs `heavy <args>` to the end and returns (exit code, stdout, stderr).
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

/// `heavy run --no-gate NAME -- sh -c HOLD FLAG OUT NAME`
pub fn hold_args<'a>(name: &'a str, flag: &'a str, out: &'a str) -> Vec<&'a str> {
    vec!["run", "--no-gate", name, "--", "sh", "-c", HOLD, flag, out, name]
}

/// `heavy run --no-gate NAME -- sh -c 'echo NAME >> OUT' OUT`
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
        child: command("sleep").arg("60").spawn().expect("spawn sleep"),
        label: "sleep 60".into(),
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
