//! The VMware Fusion adapter: `vmrun` command lines (built here, run through a [`Runner`]), headless only (`nogui`).
//!
//! This and [`super::tart`] are the only places that call a hypervisor tool (check C97).

use super::{Hypervisor, Runner};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A bound for `vmrun start`/`revert` (a cold start with a large guest can take minutes).
const LONG: Duration = Duration::from_secs(600);
/// A bound for the quick `vmrun` calls.
const SHORT: Duration = Duration::from_secs(60);

/// One Fusion guest.
pub struct Fusion<'a> {
    /// How commands are run.
    pub runner: &'a dyn Runner,
    /// The `vmrun` binary.
    pub vmrun: PathBuf,
    /// The guest's `.vmx`.
    pub vmx: PathBuf,
    /// The guest's password, when it is encrypted (passed as `-vp`, masked in logs).
    pub password: Option<String>,
}

/// `<app>/Contents/Library/vmrun` for a Fusion app folder.
pub fn vmrun_path(app: &Path) -> PathBuf {
    app.join("Contents/Library/vmrun")
}

impl Fusion<'_> {
    /// The arguments for `vmrun` subcommand `sub` (first word) with `rest` after the `.vmx`.
    pub fn args(&self, sub: &str, rest: &[&str]) -> Vec<String> {
        let mut a = vec!["-T".to_string(), "fusion".to_string()];
        if let Some(pw) = &self.password {
            a.push("-vp".into());
            a.push(pw.clone());
        }
        a.push(sub.into());
        a.push(self.vmx.display().to_string());
        a.extend(rest.iter().map(|s| (*s).to_string()));
        a
    }

    fn call(&self, sub: &str, rest: &[&str], timeout: Duration) -> Result<String, String> {
        let args = self.args(sub, rest);
        let out = self.runner.run(&self.vmrun.display().to_string(), &args, &[], timeout)?;
        if out.timed_out {
            return Err(format!("vmrun {sub} did not finish in {} s", timeout.as_secs()));
        }
        if out.code != 0 {
            let tail = out.stdout.trim().lines().chain(out.stderr.trim().lines()).last().unwrap_or("");
            return Err(format!("vmrun {sub} exited {}: {tail}", out.code));
        }
        Ok(out.stdout)
    }
}

impl Hypervisor for Fusion<'_> {
    fn is_running(&self) -> Result<bool, String> {
        let out = self.runner.run(
            &self.vmrun.display().to_string(),
            &["-T".into(), "fusion".into(), "list".into()],
            &[],
            SHORT,
        )?;
        if out.code != 0 || out.timed_out {
            return Err(format!("vmrun list failed (exit {})", out.code));
        }
        let want = self.vmx.display().to_string();
        Ok(super::preflight::parse_vmrun_list(&out.stdout).contains(&want))
    }

    fn start(&self, _extra: &[String]) -> Result<String, String> {
        self.call("start", &["nogui"], LONG).map(|_| "vmrun start nogui".into())
    }

    fn stop_soft(&self) -> Result<(), String> {
        self.call("stop", &["soft"], SHORT).map(|_| ())
    }

    fn stop_hard(&self) -> Result<(), String> {
        self.call("stop", &["hard"], SHORT).map(|_| ())
    }

    fn revert(&self, snapshot: &str) -> Result<(), String> {
        if !self.snapshots()?.iter().any(|s| s == snapshot) {
            return Err(format!("no snapshot named {snapshot}"));
        }
        self.call("revertToSnapshot", &[snapshot], LONG).map(|_| ())
    }

    fn snapshot(&self, name: &str) -> Result<(), String> {
        self.call("snapshot", &[name], LONG).map(|_| ())
    }

    fn snapshots(&self) -> Result<Vec<String>, String> {
        let out = self.call("listSnapshots", &[], SHORT)?;
        Ok(out
            .lines()
            .skip(1)
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::test_support::FakeRunner;

    fn fusion<'a>(r: &'a FakeRunner, password: Option<&str>) -> Fusion<'a> {
        Fusion {
            runner: r,
            vmrun: "/F/vmrun".into(),
            vmx: "/v/w.vmx".into(),
            password: password.map(str::to_string),
        }
    }

    #[test]
    fn start_is_headless_and_the_password_goes_before_the_subcommand() {
        let r = FakeRunner::new(vec![(0, "")]);
        fusion(&r, Some("pw")).start(&[]).unwrap();
        assert_eq!(r.calls(), vec!["/F/vmrun -T fusion -vp pw start /v/w.vmx nogui"]);
    }

    #[test]
    fn revert_checks_the_snapshot_exists_first() {
        let r = FakeRunner::new(vec![(0, "Total snapshots: 2\nclean\ngui-session-tools\n"), (0, "")]);
        fusion(&r, None).revert("gui-session-tools").unwrap();
        assert_eq!(r.calls()[1], "/F/vmrun -T fusion revertToSnapshot /v/w.vmx gui-session-tools");
        let r = FakeRunner::new(vec![(0, "Total snapshots: 1\nclean\n")]);
        assert_eq!(fusion(&r, None).revert("nope").unwrap_err(), "no snapshot named nope");
        assert_eq!(r.calls().len(), 1, "no revert was attempted");
    }

    #[test]
    fn running_is_read_from_vmrun_list_by_exact_path() {
        let r = FakeRunner::new(vec![
            (0, "Total running VMs: 1\n/v/w.vmx\n"),
            (0, "Total running VMs: 1\n/v/w.vmx.other\n"),
        ]);
        let f = fusion(&r, None);
        assert!(f.is_running().unwrap());
        assert!(!f.is_running().unwrap());
    }

    #[test]
    fn a_failing_vmrun_names_its_exit_and_last_line() {
        let r = FakeRunner::new(vec![(1, "Error: The file is locked")]);
        assert_eq!(
            fusion(&r, None).stop_hard().unwrap_err(),
            "vmrun stop exited 1: Error: The file is locked"
        );
    }
}
