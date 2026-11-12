//! `dev ci build-jobs` against `conformance/ci/build-jobs.sh`, the script it replaces (docs/plans/ci-in-rust.md, WP-1): both run
//! on the same inputs, as the runner runs the script (`bash -e -o pipefail`), and must agree on everything that reaches the
//! job: the exit code, what is appended to `$GITHUB_ENV`, standard output and standard error. Two spellings differ on purpose and
//! are normalised before the comparison: the one `::notice::` that names the file to fix, and a failure's `Next step:` that the
//! Rust command adds to the shell's cause (ER-27). Cases: every runner class, the runner's own variables, a split job, memory as
//! the ceiling, wrong values, leading zeros, no core count, no GITHUB_ENV, and the probes (their order, and that no token of the
//! job reaches them). Unix only: the Windows runner runs the real thing on every run.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

#[derive(Debug, PartialEq, Eq)]
struct Result {
    code: i32,
    env_file: String,
    stdout: String,
    stderr: String,
}

struct Scratch {
    dir: tempfile::TempDir,
}

impl Scratch {
    fn new() -> Scratch {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("bin")).unwrap();
        Scratch { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// A program on the front of PATH that appends the name of its call and what it saw of the job's token to `calls`.
    fn shim(&self, name: &str, body: &str) {
        let p = self.path("bin").join(name);
        let text = format!(
            "#!/bin/sh\necho \"{name} $* token=${{PADDOCK_DEV_TOKEN:-}}\" >> '{}'\n{body}\n",
            self.path("calls").display()
        );
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn base_env(&self, vars: &[(&str, &str)], with_env_file: bool) -> Vec<(String, String)> {
        let mut env: Vec<(String, String)> = vec![
            ("PATH".into(), format!("{}:/usr/bin:/bin", self.path("bin").display())),
            ("HOME".into(), self.dir.path().display().to_string()),
            // A secret of the job that no probe has any use for: the Rust command never passes it on.
            ("PADDOCK_DEV_TOKEN".into(), "tok-secret".into()),
        ];
        if with_env_file {
            env.push(("GITHUB_ENV".into(), self.path("github-env").display().to_string()));
        }
        for (k, v) in vars {
            env.retain(|(key, _)| key != k);
            env.push((k.to_string(), v.to_string()));
        }
        env
    }

    fn collect(&self, r: std::process::Output) -> Result {
        Result {
            code: r.status.code().unwrap_or(-1),
            env_file: std::fs::read_to_string(self.path("github-env")).unwrap_or_default(),
            stdout: String::from_utf8_lossy(&r.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&r.stderr).into_owned(),
        }
    }

    fn reset(&self) {
        let _ = std::fs::remove_file(self.path("github-env"));
        let _ = std::fs::remove_file(self.path("calls"));
    }

    #[allow(clippy::disallowed_methods)]
    fn shell(&self, split: Option<&str>, vars: &[(&str, &str)], with_env_file: bool) -> Result {
        self.reset();
        let mut cmd = Command::new("bash");
        cmd.args(["-e", "-o", "pipefail"]).arg(repo().join("conformance/ci/build-jobs.sh"));
        cmd.args(split);
        let out = cmd.env_clear().envs(self.base_env(vars, with_env_file)).output().unwrap();
        self.collect(out)
    }

    #[allow(clippy::disallowed_methods)]
    fn rust(&self, split: Option<&str>, vars: &[(&str, &str)], with_env_file: bool) -> Result {
        self.reset();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_paddock-dev"));
        cmd.args(["ci", "build-jobs"]).args(split).current_dir(repo());
        let out = cmd.env_clear().envs(self.base_env(vars, with_env_file)).output().unwrap();
        self.collect(out)
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.path("calls")).unwrap_or_default()
    }
}

/// The shell's output as the Rust command spells it: the notice that names the file to fix, and a failure's next step.
fn normalised(sh: &Result, rs: &Result) -> (Result, Result) {
    let fix = |t: &str| t.replace("or fix build-jobs.sh if it differs", "or fix the class table in ci_build_jobs.rs if it differs");
    let sh_out = fix(&sh.stdout);
    // Every `::error::` of the shell is one line; the Rust line starts with it and goes on with ` Next step: ...`.
    let rs_out = rs
        .stdout
        .lines()
        .map(|l| match (l.starts_with("::error::"), l.find(" Next step: ")) {
            (true, Some(at)) => l[..at].to_string(),
            _ => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let sh_out = sh_out.trim_end_matches('\n').to_string();
    let rs_out = rs_out.trim_end_matches('\n').to_string();
    let copy = |r: &Result, stdout: String| Result {
        code: r.code,
        env_file: r.env_file.clone(),
        stdout,
        stderr: r.stderr.clone(),
    };
    (copy(sh, sh_out), copy(rs, rs_out))
}

/// The two implementations agree on everything that reaches the job.
fn same(s: &Scratch, split: Option<&str>, vars: &[(&str, &str)], with_env_file: bool) -> Result {
    let sh = s.shell(split, vars, with_env_file);
    let rs = s.rust(split, vars, with_env_file);
    let (a, b) = normalised(&sh, &rs);
    assert_eq!(a, b, "split {split:?}, vars {vars:?}\nshell: {sh:?}\nrust: {rs:?}");
    if rs.code != 0 {
        assert!(rs.stdout.contains(" Next step: "), "a failure names its next step: {}", rs.stdout);
    }
    rs
}

fn host(os: &'static str, cores: &'static str, ram: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![("RUNNER_OS", os), ("PADDOCK_CI_CORES", cores), ("PADDOCK_CI_RAM_MB", ram)]
}

#[test]
fn every_runner_class_gets_the_shells_share() {
    let s = Scratch::new();
    for (os, cores, ram) in [
        ("macOS", "14", "65536"),
        ("macOS", "8", "16384"),
        ("Windows", "16", "32768"),
        ("Windows", "8", "16384"),
        ("Linux", "16", "131072"),
        ("Linux", "2", "4096"),
        ("", "4", "8192"),
        ("FreeBSD", "6", "8192"),
    ] {
        for split in [None, Some("1"), Some("2"), Some("4"), Some("40")] {
            same(&s, split, &host(os, cores, ram), true);
            same(&s, split, &host(os, cores, ram), false);
        }
    }
}

#[test]
fn the_class_values_are_the_documented_ones() {
    let s = Scratch::new();
    let mac = same(&s, None, &host("macOS", "14", "65536"), true);
    assert_eq!(mac.env_file, "CARGO_BUILD_JOBS=4\nNEXTEST_TEST_THREADS=8\n");
    let win = same(&s, None, &host("Windows", "16", "32768"), true);
    assert_eq!(win.env_file, "CARGO_BUILD_JOBS=5\nNEXTEST_TEST_THREADS=8\n");
    assert!(win.stdout.contains("::notice::build-jobs assumes the Windows runner's capacity is 3"), "{}", win.stdout);
    let linux = same(&s, None, &host("Linux", "16", "131072"), true);
    assert_eq!(linux.env_file, "", "Linux keeps cargo's default");
    let split = same(&s, Some("4"), &host("Linux", "16", "131072"), true);
    assert_eq!(split.env_file, "CARGO_BUILD_JOBS=4\n");
}

#[test]
fn the_runners_own_variables_win_over_the_class() {
    let s = Scratch::new();
    for extra in [
        vec![("PADDOCK_CI_BUILD_JOBS", "6")],
        vec![("PADDOCK_CI_BUILD_JOBS", "1")],
        vec![("PADDOCK_CI_BUILD_JOBS", "6"), ("PADDOCK_CI_TEST_THREADS", "12")],
        vec![("PADDOCK_CI_TEST_THREADS", "3")],
        vec![("PADDOCK_CI_JOBS_PER_HOST", "2"), ("PADDOCK_CI_RESERVED_CORES", "1")],
        vec![("PADDOCK_CI_JOBS_PER_HOST", "4"), ("PADDOCK_CI_RESERVED_RAM_MB", "0")],
        vec![("PADDOCK_CI_JOBS_PER_HOST", "2"), ("PADDOCK_CI_RUSTC_MB", "8192")],
        vec![("PADDOCK_CI_JOBS_PER_HOST", "-")],
    ] {
        for os in ["macOS", "Windows", "Linux"] {
            let mut vars = host(os, "16", "32768");
            vars.extend(extra.iter().copied());
            for split in [None, Some("3")] {
                same(&s, split, &vars, true);
            }
        }
    }
}

#[test]
fn memory_can_be_the_ceiling() {
    let s = Scratch::new();
    let mut vars = host("macOS", "14", "4096");
    vars.push(("PADDOCK_CI_RESERVED_RAM_MB", "1024"));
    let r = same(&s, None, &vars, true);
    assert!(r.stderr.contains("MB per rustc"), "{}", r.stderr);
    let unknown = same(&s, None, &[("RUNNER_OS", "macOS"), ("PADDOCK_CI_CORES", "14"), ("PADDOCK_CI_RAM_MB", "x")], true);
    assert!(unknown.stderr.contains("RAM unknown: no memory ceiling"), "{}", unknown.stderr);
}

#[test]
fn wrong_values_fail_with_the_shells_cause_and_a_next_step() {
    let s = Scratch::new();
    for (name, value) in [
        ("PADDOCK_CI_BUILD_JOBS", "0"),
        ("PADDOCK_CI_BUILD_JOBS", "08"),
        ("PADDOCK_CI_BUILD_JOBS", "x"),
        ("PADDOCK_CI_TEST_THREADS", "010"),
        ("PADDOCK_CI_JOBS_PER_HOST", "0"),
        ("PADDOCK_CI_JOBS_PER_HOST", "two"),
        ("PADDOCK_CI_RESERVED_CORES", "01"),
        ("PADDOCK_CI_RESERVED_CORES", "-1"),
        ("PADDOCK_CI_RESERVED_RAM_MB", "1G"),
        ("PADDOCK_CI_RUSTC_MB", "0"),
    ] {
        let mut vars = host("macOS", "14", "65536");
        vars.push((name, value));
        let r = same(&s, None, &vars, true);
        assert_eq!(r.code, 1, "{name}={value}");
    }
    for split in ["0", "x", "01", "-2"] {
        let r = same(&s, Some(split), &host("macOS", "14", "65536"), true);
        assert_eq!(r.code, 1, "split {split}");
    }
    let zero_reserved = same(&s, None, &host("macOS", "14", "65536"), true);
    assert_eq!(zero_reserved.code, 0);
}

#[test]
fn a_host_with_no_cores_to_count_or_no_core_left_still_builds() {
    let s = Scratch::new();
    let r = same(&s, None, &[("RUNNER_OS", "macOS"), ("PADDOCK_CI_CORES", "abc"), ("PADDOCK_CI_RAM_MB", "8192")], true);
    assert_eq!((r.code, r.env_file.as_str()), (0, ""), "unknown cores leave cargo's default");
    assert!(r.stdout.contains("::warning::could not count this host's cores"), "{}", r.stdout);
    let all_kept = same(&s, None, &[("RUNNER_OS", "macOS"), ("PADDOCK_CI_CORES", "2"), ("PADDOCK_CI_RAM_MB", "65536")], true);
    assert!(all_kept.stdout.contains("keeps all of this host's 2 cores"), "{}", all_kept.stdout);
    assert!(all_kept.env_file.starts_with("CARGO_BUILD_JOBS=1\n"), "{}", all_kept.env_file);
}

#[test]
fn without_a_github_env_the_assignments_are_printed() {
    let s = Scratch::new();
    let r = same(&s, None, &host("macOS", "14", "65536"), false);
    assert_eq!(r.env_file, "");
    assert!(r.stdout.starts_with("CARGO_BUILD_JOBS=4\nNEXTEST_TEST_THREADS=8"), "{}", r.stdout);
}

/// The probes are programs: `nproc` first, then `sysctl -n hw.ncpu`, then `NUMBER_OF_PROCESSORS`, then `getconf`; the RAM from
/// `sysctl -n hw.memsize` when no file answers. Shims stand in for them so the answer is not this machine's.
#[test]
fn the_probes_are_tried_in_the_shells_order() {
    let s = Scratch::new();
    s.shim("nproc", "exit 1");
    s.shim("sysctl", "case \"$2\" in hw.ncpu) echo 10 ;; hw.memsize) echo 34359738368 ;; esac");
    s.shim("getconf", "echo 6");
    let vars = [("RUNNER_OS", "macOS")];
    let r = same(&s, None, &vars, true);
    assert!(r.stderr.contains("this host: 10 cores"), "{}", r.stderr);
    // nproc answering wins over everything.
    s.shim("nproc", "echo 12");
    let r = same(&s, None, &vars, true);
    assert!(r.stderr.contains("this host: 12 cores"), "{}", r.stderr);
    // sysctl failing, the variable then getconf.
    s.shim("nproc", "exit 1");
    s.shim("sysctl", "exit 1");
    let r = same(&s, None, &[("RUNNER_OS", "macOS"), ("NUMBER_OF_PROCESSORS", "7")], true);
    assert!(r.stderr.contains("this host: 7 cores"), "{}", r.stderr);
    let r = same(&s, None, &vars, true);
    assert!(r.stderr.contains("this host: 6 cores"), "{}", r.stderr);
}

/// ER-33: a parity probe counts only once it has been shown to see a difference.
#[test]
fn the_comparison_sees_a_difference() {
    let s = Scratch::new();
    let sh = s.shell(None, &host("macOS", "14", "65536"), true);
    let rs = s.rust(None, &host("macOS", "13", "65536"), true);
    let (a, b) = normalised(&sh, &rs);
    assert_ne!(a, b, "a different core count must show");
}

/// A token of the job never reaches a probe (the wrapper contract: every child starts from the base environment).
#[test]
fn a_token_of_the_job_never_reaches_a_probe() {
    let s = Scratch::new();
    s.shim("nproc", "echo 4");
    s.shim("sysctl", "echo 8192");
    let vars = [("RUNNER_OS", "macOS")];
    s.reset();
    let rs = s.rust(None, &vars, true);
    assert_eq!(rs.code, 0, "{}", rs.stderr);
    let calls = s.calls();
    assert!(calls.contains("nproc"), "the probe ran: {calls:?}");
    assert!(!calls.contains("tok-secret"), "the probe saw the token: {calls:?}");
}
