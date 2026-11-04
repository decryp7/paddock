//! Preflight: what must hold before the broker starts or reverts a VM (ADR 0054 §1.5), as pure functions over facts the
//! adapters read. A failed preflight is a "could not run" result with its reasons, never a FAIL of the product under test.
//!
//! The VM's `.vmx` (or Tart config) parses; every disk it names exists; leftover `.lck` folders are reported with age and
//! owner and a run does not start over them; no other process runs the VM.

use super::vmx::Vmx;
use serde_json::Value;

/// A lock folder VMware left beside a `.vmx`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockDir {
    /// Its file name.
    pub name: String,
    /// Seconds since it was last changed.
    pub age_secs: u64,
    /// The names inside it (VMware writes one `M<id>.lck` per owning process).
    pub owner: Vec<String>,
}

/// A process line from `ps`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcLine {
    /// Its pid.
    pub pid: u32,
    /// Its command line.
    pub command: String,
}

/// What the broker read about a Fusion guest.
#[derive(Debug, Clone, Default)]
pub struct FusionFacts {
    /// The text of the `.vmx`, if it could be read.
    pub vmx_text: Option<String>,
    /// File names beside the `.vmx`.
    pub dir_files: Vec<String>,
    /// Lock folders beside it.
    pub locks: Vec<LockDir>,
    /// The `.vmx` paths `vmrun list` shows as running.
    pub running: Vec<String>,
    /// This guest's `.vmx` path.
    pub vmx_path: String,
    /// The guest's base name (the `.vmx` without its extension), for redo-log names.
    pub base: String,
    /// The `vmrun` binary exists.
    pub vmrun_found: bool,
    /// The guest is encrypted and the keychain has no password for it.
    pub missing_password: bool,
}

/// What the broker read about a Tart guest.
#[derive(Debug, Clone, Default)]
pub struct TartFacts {
    /// `tart` is installed.
    pub tart_found: bool,
    /// `TART_HOME` (or the default) exists.
    pub home_exists: bool,
    /// The guest as `tart list` shows it, if listed.
    pub listed: Option<TartVm>,
}

/// One row of `tart list --format json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TartVm {
    /// Its name.
    pub name: String,
    /// Whether it runs.
    pub running: bool,
}

/// What a preflight is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Start, revert or clone onto it: the guest must not run, and nothing may be left over from a run.
    Change,
    /// Stop or snapshot: the guest may run.
    Inspect,
}

/// The result: reasons it cannot run, and things worth logging.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preflight {
    /// Each reason a run must not start, with the next step.
    pub problems: Vec<String>,
    /// Facts for the log.
    pub notes: Vec<String>,
}

impl Preflight {
    /// No problem found.
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

/// Parses `vmrun list`: the `.vmx` paths after the "Total running VMs" line.
pub fn parse_vmrun_list(out: &str) -> Vec<String> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("Total running VMs"))
        .map(str::to_string)
        .collect()
}

/// Parses `tart list --format json`.
pub fn parse_tart_list(json: &str) -> Result<Vec<TartVm>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("tart list did not give JSON: {e}"))?;
    let rows = v.as_array().ok_or("tart list did not give a list")?;
    rows.iter()
        .map(|r| {
            let name = r.get("Name").and_then(Value::as_str).ok_or("a tart list row has no Name")?;
            let running =
                r.get("Running").and_then(Value::as_bool).unwrap_or(false) || r.get("State").and_then(Value::as_str) == Some("running");
            Ok(TartVm {
                name: name.to_string(),
                running,
            })
        })
        .collect()
}

/// Parses `ps -axo pid=,command=`.
pub fn parse_ps(out: &str) -> Vec<ProcLine> {
    out.lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let (pid, command) = l.split_once(char::is_whitespace)?;
            Some(ProcLine {
                pid: pid.parse().ok()?,
                command: command.trim().to_string(),
            })
        })
        .collect()
}

/// The processes that run the Fusion guest at `vmx_path`.
pub fn fusion_processes(vmx_path: &str, ps: &[ProcLine]) -> Vec<ProcLine> {
    ps.iter()
        .filter(|p| p.command.contains("vmware-vmx") && p.command.contains(vmx_path))
        .cloned()
        .collect()
}

/// The processes that run the Tart guest `name` (`tart run ... <name>`).
pub fn tart_processes(name: &str, ps: &[ProcLine]) -> Vec<ProcLine> {
    ps.iter()
        .filter(|p| {
            let mut words = p.command.split_whitespace();
            words.next().is_some_and(|w| w.ends_with("/tart") || w == "tart")
                && p.command.contains(" run ")
                && p.command.split_whitespace().last() == Some(name)
        })
        .cloned()
        .collect()
}

/// Preflight for a Fusion guest.
pub fn fusion(f: &FusionFacts, intent: Intent) -> Preflight {
    let mut p = Preflight::default();
    if !f.vmrun_found {
        p.problems
            .push("vmrun was not found; install VMware Fusion or set FUSION_APP".into());
    }
    if f.missing_password {
        p.problems.push(
            "the guest is encrypted and the login keychain has no item paddock-win11-test-vm/vmrun (docs/testing/windows-test-vm.md, TPM)"
                .into(),
        );
    }
    let running = f.running.contains(&f.vmx_path);
    match &f.vmx_text {
        None => p.problems.push(format!(
            "the .vmx at {} cannot be read; prepare the VM (scripts/vm-win/prepare.sh)",
            f.vmx_path
        )),
        Some(text) => {
            let c = Vmx::parse(text).check(&f.dir_files);
            p.notes.push(format!(
                "vmx: {} entries, disk {}",
                c.entries,
                c.disk.as_deref().unwrap_or("(none named)")
            ));
            if c.entries == 0 {
                p.problems
                    .push("the .vmx has no entries; a person must repair it (a backup is never restored)".into());
            }
            if !c.malformed.is_empty() {
                p.problems.push(format!(
                    "the .vmx has lines that do not parse (line {}); a person must repair it",
                    join_numbers(&c.malformed)
                ));
            }
            if !c.duplicates.is_empty() {
                p.problems.push(format!(
                    "the .vmx repeats keys: {}; a person must repair it",
                    c.duplicates.join(", ")
                ));
            }
            match (&c.disk, c.disk_exists) {
                (None, _) => p.problems.push("the .vmx names no boot disk (nvme0:0.fileName)".into()),
                (Some(d), false) => p.problems.push(format!(
                    "the boot disk {d} does not exist beside the .vmx; revert from a snapshot or repair the .vmx"
                )),
                _ => {}
            }
        }
    }
    if intent == Intent::Change {
        if running {
            p.problems
                .push("the guest is already running (vmrun list); another job may hold it".into());
        } else if !f.locks.is_empty() {
            let list: Vec<String> = f
                .locks
                .iter()
                .map(|l| {
                    format!(
                        "{} (age {} s, owner {})",
                        l.name,
                        l.age_secs,
                        if l.owner.is_empty() { "unknown".into() } else { l.owner.join("+") }
                    )
                })
                .collect();
            p.problems.push(format!(
                "leftover lock folders with no running guest: {}; VMware would show a stale-lock dialog on the desktop. Check no vmware-vmx process uses them, then remove them by hand",
                list.join(", ")
            ));
        }
    }
    p.notes.push(format!("running: {}", if running { "yes" } else { "no" }));
    p
}

/// Preflight for a Tart guest `name` (`must_exist`: false when the guest is about to be cloned).
pub fn tart(f: &TartFacts, intent: Intent, must_exist: bool) -> Preflight {
    let mut p = Preflight::default();
    if !f.tart_found {
        p.problems.push("tart was not found; brew install cirruslabs/cli/tart".into());
    }
    if !f.home_exists {
        p.problems
            .push("TART_HOME does not exist; set it to the folder that holds the images".into());
    }
    match (&f.listed, must_exist) {
        (None, true) => p
            .problems
            .push("tart list does not show the guest; clone it from its base first".into()),
        (Some(_), false) => p
            .problems
            .push("a guest with this name already exists; delete it or pick another name".into()),
        (Some(v), true) if v.running && intent == Intent::Change => p
            .problems
            .push("the guest is already running (tart list); another job may hold it".into()),
        _ => {}
    }
    p
}

fn join_numbers(n: &[usize]) -> String {
    n.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const VMX: &str = "/v/win.vmx";
    fn good() -> FusionFacts {
        FusionFacts {
            vmx_text: Some("displayName = \"w\"\nnvme0:0.fileName = \"w-000001.vmdk\"\n".into()),
            dir_files: vec!["w-000001.vmdk".into()],
            vmx_path: VMX.into(),
            base: "w".into(),
            vmrun_found: true,
            ..FusionFacts::default()
        }
    }

    #[test]
    fn a_good_guest_passes() {
        let p = fusion(&good(), Intent::Change);
        assert!(p.ok(), "{:?}", p.problems);
    }

    #[test]
    fn a_running_guest_blocks_a_change_but_not_an_inspection() {
        let mut f = good();
        f.running = vec![VMX.into()];
        assert!(fusion(&f, Intent::Change).problems[0].contains("already running"));
        assert!(fusion(&f, Intent::Inspect).ok());
    }

    #[test]
    fn leftover_locks_with_no_running_guest_block_and_name_age_and_owner() {
        let mut f = good();
        f.locks = vec![LockDir {
            name: "w.vmx.lck".into(),
            age_secs: 7200,
            owner: vec!["M1.lck".into()],
        }];
        let p = fusion(&f, Intent::Change);
        assert!(p.problems[0].contains("w.vmx.lck (age 7200 s, owner M1.lck)"), "{:?}", p.problems);
        f.running = vec![VMX.into()];
        assert!(
            !fusion(&f, Intent::Change).problems.iter().any(|m| m.contains("leftover")),
            "locks of a running guest are normal"
        );
    }

    #[test]
    fn a_broken_vmx_is_a_problem_for_a_person_not_a_restore() {
        let mut f = good();
        f.vmx_text = Some("a = \"1\"\na = \"2\"\njunk\nnvme0:0.fileName = \"gone.vmdk\"\n".into());
        let all = fusion(&f, Intent::Change).problems.join("\n");
        assert!(
            all.contains("repeats keys: a") && all.contains("do not parse (line 3)") && all.contains("gone.vmdk does not exist"),
            "{all}"
        );
        assert!(!all.to_lowercase().contains("restore the backup"));
    }

    #[test]
    fn an_unreadable_vmx_and_missing_tools_are_reported() {
        let f = FusionFacts {
            vmx_path: VMX.into(),
            ..FusionFacts::default()
        };
        let all = fusion(&f, Intent::Change).problems.join("\n");
        assert!(all.contains("vmrun was not found") && all.contains("cannot be read"));
    }

    #[test]
    fn vmrun_list_parses_paths() {
        assert_eq!(
            parse_vmrun_list("Total running VMs: 2\n/a/b.vmx\n/c d/e.vmx\n"),
            vec!["/a/b.vmx", "/c d/e.vmx"]
        );
        assert!(parse_vmrun_list("Total running VMs: 0\n").is_empty());
    }

    #[test]
    fn tart_list_parses_running_state() {
        let v =
            parse_tart_list(r#"[{"Name":"a","Running":false,"State":"stopped"},{"Name":"b","Running":true,"State":"running"}]"#).unwrap();
        assert_eq!(
            v,
            vec![
                TartVm {
                    name: "a".into(),
                    running: false
                },
                TartVm {
                    name: "b".into(),
                    running: true
                }
            ]
        );
        assert!(parse_tart_list("not json").is_err());
        assert!(parse_tart_list(r#"[{"x":1}]"#).is_err());
    }

    #[test]
    fn tart_preflight_covers_missing_running_and_existing() {
        let ok = TartFacts {
            tart_found: true,
            home_exists: true,
            listed: Some(TartVm {
                name: "g".into(),
                running: false,
            }),
        };
        assert!(tart(&ok, Intent::Change, true).ok());
        assert!(!tart(&ok, Intent::Change, false).ok(), "cloning onto an existing name");
        let running = TartFacts {
            listed: Some(TartVm {
                name: "g".into(),
                running: true,
            }),
            ..ok.clone()
        };
        assert!(!tart(&running, Intent::Change, true).ok());
        assert!(tart(&running, Intent::Inspect, true).ok());
        let missing = TartFacts { listed: None, ..ok };
        assert!(!tart(&missing, Intent::Change, true).ok());
        assert!(tart(&missing, Intent::Change, false).ok());
    }

    #[test]
    fn processes_are_found_by_vmx_path_and_tart_name() {
        let ps = parse_ps(
            "  100 /Applications/VMware Fusion.app/Contents/Library/vmware-vmx -s x /v/win.vmx\n  200 /opt/homebrew/bin/tart run --no-graphics g1\n  300 /opt/homebrew/bin/tart run --no-graphics g10\n 400 vim notes\n",
        );
        assert_eq!(fusion_processes(VMX, &ps).iter().map(|p| p.pid).collect::<Vec<_>>(), vec![100]);
        assert_eq!(tart_processes("g1", &ps).iter().map(|p| p.pid).collect::<Vec<_>>(), vec![200]);
        assert!(tart_processes("g", &ps).is_empty());
    }
}
