//! The Tart adapter: `tart` command lines (built here, run through a [`Runner`]), headless only (`--no-graphics`).
//!
//! Tart has no snapshots, so "revert" means delete the clone and clone it again from its base image.

use super::{Hypervisor, Runner};
use std::path::PathBuf;
use std::time::Duration;

const LONG: Duration = Duration::from_secs(1800);
const SHORT: Duration = Duration::from_secs(120);
/// Arguments of `tart run` the broker passes on; anything else (`--graphics`, `--vnc`, a window) could put a window on the desktop.
const ALLOWED_RUN_ARGS: &[&str] = &["--vnc-experimental", "--no-audio", "--no-clipboard", "--suspendable"];

/// One Tart guest.
pub struct Tart<'a> {
    /// How commands are run.
    pub runner: &'a dyn Runner,
    /// The `tart` binary.
    pub tart: PathBuf,
    /// The guest's name.
    pub name: String,
    /// `TART_HOME`.
    pub home: PathBuf,
    /// Where the detached `tart run` writes its output.
    pub log: PathBuf,
}

/// Whether a caller-supplied `tart run` argument is allowed: a listed flag, `--rosetta=<tag>` or `--dir=<spec>`.
pub fn allowed_run_arg(a: &str) -> bool {
    ALLOWED_RUN_ARGS.contains(&a)
        || a.strip_prefix("--rosetta=").is_some_and(|t| !t.is_empty())
        || a.strip_prefix("--dir=").is_some_and(|t| !t.is_empty())
}

impl Tart<'_> {
    fn env(&self) -> Vec<(String, String)> {
        vec![("TART_HOME".into(), self.home.display().to_string())]
    }

    fn call(&self, args: &[&str], timeout: Duration) -> Result<String, String> {
        let a: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        let out = self.runner.run(&self.tart.display().to_string(), &a, &self.env(), timeout)?;
        if out.timed_out {
            return Err(format!("tart {} did not finish in {} s", args[0], timeout.as_secs()));
        }
        if out.code != 0 {
            return Err(format!(
                "tart {} exited {}: {}",
                args[0],
                out.code,
                out.stderr.trim().lines().last().unwrap_or("")
            ));
        }
        Ok(out.stdout)
    }

    /// `tart list --format json`.
    pub fn list_json(&self) -> Result<String, String> {
        self.call(&["list", "--format", "json"], SHORT)
    }

    /// Clones `base` as this guest.
    pub fn clone_from(&self, base: &str) -> Result<(), String> {
        self.call(&["clone", base, &self.name], LONG).map(|_| ())
    }

    /// Deletes this guest (stopping it first if it runs).
    pub fn delete(&self) -> Result<(), String> {
        if self.is_running()? {
            self.stop_hard()?;
        }
        self.call(&["delete", &self.name], SHORT).map(|_| ())
    }
}

impl Hypervisor for Tart<'_> {
    fn is_running(&self) -> Result<bool, String> {
        let rows = super::preflight::parse_tart_list(&self.list_json()?)?;
        Ok(rows.iter().any(|r| r.name == self.name && r.running))
    }

    fn start(&self, extra: &[String]) -> Result<String, String> {
        if let Some(bad) = extra.iter().find(|a| !allowed_run_arg(a)) {
            return Err(format!(
                "tart run argument {bad} is not allowed (only {} , --rosetta=<tag>, --dir=<spec>)",
                ALLOWED_RUN_ARGS.join(", ")
            ));
        }
        let mut args = vec!["run".to_string(), "--no-graphics".to_string()];
        args.extend(extra.iter().cloned());
        args.push(self.name.clone());
        let pid = self
            .runner
            .spawn_detached(&self.tart.display().to_string(), &args, &self.env(), &self.log)?;
        Ok(format!("tart run --no-graphics (pid {pid}, output in {})", self.log.display()))
    }

    fn stop_soft(&self) -> Result<(), String> {
        self.call(&["stop", &self.name, "--timeout", "60"], SHORT * 2).map(|_| ())
    }

    fn stop_hard(&self) -> Result<(), String> {
        self.call(&["stop", &self.name, "--timeout", "0"], SHORT).map(|_| ())
    }

    fn revert(&self, base: &str) -> Result<(), String> {
        self.delete()?;
        self.clone_from(base)
    }

    fn snapshot(&self, _name: &str) -> Result<(), String> {
        Err("Tart has no snapshots; clone the guest under another name instead".into())
    }

    fn snapshots(&self) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::test_support::FakeRunner;

    fn tart<'a>(r: &'a FakeRunner) -> Tart<'a> {
        Tart {
            runner: r,
            tart: "/t/tart".into(),
            name: "g1".into(),
            home: "/h".into(),
            log: "/l/g1.log".into(),
        }
    }

    #[test]
    fn start_is_detached_headless_and_only_allowed_arguments_pass() {
        let r = FakeRunner::new(vec![]);
        tart(&r).start(&["--vnc-experimental".into(), "--rosetta=rosetta".into()]).unwrap();
        assert_eq!(
            r.calls(),
            vec!["detached /t/tart run --no-graphics --vnc-experimental --rosetta=rosetta g1 TART_HOME=/h"]
        );
        for bad in ["--graphics", "--vnc", "--no-graphics=false", "--rosetta=", "g2"] {
            assert!(tart(&r).start(&[bad.into()]).is_err(), "{bad}");
        }
    }

    #[test]
    fn revert_deletes_and_clones_again_and_stops_a_running_guest_first() {
        let r = FakeRunner::new(vec![(0, r#"[{"Name":"g1","Running":true}]"#), (0, ""), (0, ""), (0, "")]);
        tart(&r).revert("base").unwrap();
        assert_eq!(
            r.calls(),
            vec![
                "/t/tart list --format json TART_HOME=/h",
                "/t/tart stop g1 --timeout 0 TART_HOME=/h",
                "/t/tart delete g1 TART_HOME=/h",
                "/t/tart clone base g1 TART_HOME=/h"
            ]
        );
    }

    #[test]
    fn running_needs_the_exact_name_to_be_running() {
        let r = FakeRunner::new(vec![(0, r#"[{"Name":"g1","Running":false},{"Name":"g10","Running":true}]"#)]);
        assert!(!tart(&r).is_running().unwrap());
    }

    #[test]
    fn snapshots_are_not_supported() {
        let r = FakeRunner::new(vec![]);
        assert!(tart(&r).snapshot("x").is_err());
    }
}
