//! The CI kit's `conformance/ci/job-isolation.sh` against this real binary (ADR 0025): acquire, a kept folder, release and the
//! disk sweep, with a fake HOME (the CI home is its `.paddock-ci`) and a fake runner job folder, all under a scratch folder
//! (rule 7: never the machine's CI home). Unix only: the script runs under Git bash on Windows, where the CI host jobs
//! themselves exercise it on every run.
// windows-twin: none: it runs the shell script, which the Windows host jobs exercise under Git bash on every run
#![cfg(unix)]
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]

use paddock_slots::{FsLocks, SlotId, SlotLocks};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/ci/job-isolation.sh")
}

struct Job {
    home: PathBuf,
    ci: PathBuf,
    temp: PathBuf,
    env_file: PathBuf,
}

impl Job {
    fn new(root: &Path) -> Job {
        let home = root.join("home");
        let temp = root.join("work/0123456789abcdef/tmp");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&temp).unwrap();
        let env_file = root.join("work/0123456789abcdef/github-env");
        std::fs::write(&env_file, "").unwrap();
        Job {
            ci: home.join(".paddock-ci"),
            home,
            temp,
            env_file,
        }
    }

    fn exported(&self) -> HashMap<String, String> {
        std::fs::read_to_string(&self.env_file)
            .unwrap()
            .lines()
            .filter_map(|l| l.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
            .collect()
    }

    /// Runs the script as a host job's step: the slots of a CI home of its own, the real paddock-dev, nothing inherited.
    fn run(&self, args: &[&str], later: bool) -> Output {
        let mut cmd = Command::new("bash");
        cmd.arg(script())
            .args(args)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", &self.home)
            .env("RUNNER_OS", "macOS")
            .env("RUNNER_TEMP", &self.temp)
            .env("GITHUB_ENV", &self.env_file)
            .env("PADDOCK_DEV", env!("CARGO_BIN_EXE_paddock-dev"))
            .env("PADDOCK_CI_SLOTS", "3")
            .env("PADDOCK_CI_KEEP_SLOTS", "1")
            .env("PADDOCK_CI_MIN_FREE_GB", "0");
        if later {
            cmd.envs(self.exported());
        }
        cmd.output().unwrap()
    }
}

fn free(ci: &Path, n: u32) -> bool {
    FsLocks::new(ci).try_lock(SlotId::new(n).unwrap()).unwrap().is_some()
}

#[test]
fn job_isolation_sh_acquires_uses_releases_and_sweeps_with_the_real_binary() {
    let d = tempfile::tempdir().unwrap();
    // The CI home is resolved (ci-home.sh), so the scratch root must be too (macOS: /var is /private/var).
    let job = Job::new(&d.path().canonicalize().unwrap());
    // An idle slot above the kept one, left by an earlier job: due, so the acquire's sweep removes it under its lock.
    std::fs::create_dir_all(job.ci.join("target-old-s3")).unwrap();

    let acq = job.run(&["acquire"], false);
    assert!(acq.status.success(), "{}", String::from_utf8_lossy(&acq.stderr));
    assert!(acq.stdout.is_empty(), "messages go to stderr");
    let env = job.exported();
    assert_eq!(env.get("PADDOCK_CI_SLOT").map(String::as_str), Some("s1"));
    assert!(env.get("PADDOCK_CI_SLOT_TOKEN").is_some_and(|t| !t.is_empty()));
    assert!(!free(&job.ci, 1), "the holder keeps s1 after acquire returned");
    assert!(!job.ci.join("target-old-s3").exists(), "the sweep removed the idle slot's folder");

    let path = job.run(&["path", "target", "x-app"], true);
    assert!(path.status.success());
    assert_eq!(
        String::from_utf8_lossy(&path.stdout).trim(),
        job.ci.join("target-x-app-s1").to_str().unwrap()
    );

    // A folder of s2 (above the one kept slot) and the job's own s1 folder: release frees s1, the sweep takes s2's.
    std::fs::create_dir_all(job.ci.join("target-y-s2")).unwrap();
    let rel = job.run(&["release"], true);
    assert!(rel.status.success(), "{}", String::from_utf8_lossy(&rel.stderr));
    assert!(free(&job.ci, 1), "released");
    assert!(!job.ci.join("target-y-s2").exists());
    assert!(job.ci.join("target-x-app-s1").exists(), "the kept slot's folder stays");
    let status = job.run(&["status"], false);
    assert!(String::from_utf8_lossy(&status.stdout).contains("s1 free"));
}
