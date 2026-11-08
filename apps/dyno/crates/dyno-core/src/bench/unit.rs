//! Linux: the benchmark child as a named transient unit of the user manager (ADR 0030 section 2, D5). Pure: the commands are
//! built here, the agent runs them. A unit started with `systemd-run --user` lives in its own cgroup at nice 0, outside the
//! agent's lowered cgroup; killing the `systemd-run` client does not stop it, so Cancel, a safety stop, the agent's shutdown
//! and a crash cleanup all go through `systemctl --user stop`. A leftover unit is stopped on the agent's next start only when
//! both its name and its InvocationID match what the agent recorded (rule 53: never by a name alone, a reused name could be
//! someone else's unit).

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Every benchmark unit's name starts with this.
pub const UNIT_PREFIX: &str = "dyno-bench-";
/// What a run records when there is no user manager and the child runs inside the agent's cgroup.
pub const LIMITED_NOTE: &str = "This computer has no systemd user session, so the test ran at a lower priority with a 256 MiB memory cap and its numbers are less certain. Sign in to a normal desktop session, or turn on systemd user services, and run again for full-confidence numbers.";
/// The longest run id used in a unit name.
const MAX_ID: usize = 64;

/// A program and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitCommand {
    /// The program.
    pub program: &'static str,
    /// Its arguments.
    pub args: Vec<String>,
}

impl UnitCommand {
    /// The command as a log line (nothing in it is secret).
    pub fn display(&self) -> String {
        std::iter::once(self.program.to_string())
            .chain(self.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The unit name for a run: `dyno-bench-<run id>` with every character a unit name does not allow replaced by `-`.
pub fn unit_name(run_id: &str) -> String {
    let id: String = run_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(MAX_ID)
        .collect();
    format!("{UNIT_PREFIX}{id}")
}

fn service(unit: &str) -> String {
    format!("{unit}.service")
}

/// Starts `runner` with `runner_args` as the transient unit `unit`, waiting for it and piping its stdout and stderr back
/// (nice 0 and no memory limit: nothing is set).
pub fn run_command(unit: &str, runner: &Path, runner_args: &[String]) -> UnitCommand {
    let mut args = vec![
        "--user".to_string(),
        format!("--unit={unit}"),
        "--collect".into(),
        "--wait".into(),
        "--pipe".into(),
        "--service-type=exec".into(),
        "--".into(),
        runner.to_string_lossy().into_owned(),
    ];
    args.extend(runner_args.iter().cloned());
    UnitCommand {
        program: "systemd-run",
        args,
    }
}

/// Stops the unit and everything in it.
pub fn stop_command(unit: &str) -> UnitCommand {
    UnitCommand {
        program: "systemctl",
        args: vec!["--user".into(), "stop".into(), service(unit)],
    }
}

/// Asks for the unit's InvocationID (the id of this start of the unit; a reused name has another).
pub fn invocation_command(unit: &str) -> UnitCommand {
    UnitCommand {
        program: "systemctl",
        args: vec![
            "--user".into(),
            "show".into(),
            "-p".into(),
            "InvocationID".into(),
            "--value".into(),
            service(unit),
        ],
    }
}

/// Lists the benchmark units that exist, loaded or not.
pub fn list_command() -> UnitCommand {
    UnitCommand {
        program: "systemctl",
        args: vec![
            "--user".into(),
            "list-units".into(),
            "--all".into(),
            "--plain".into(),
            "--no-legend".into(),
            "--no-pager".into(),
            format!("{UNIT_PREFIX}*.service"),
        ],
    }
}

/// Asks whether the user manager runs.
pub fn manager_state_command() -> UnitCommand {
    UnitCommand {
        program: "systemctl",
        args: vec!["--user".into(), "is-system-running".into()],
    }
}

/// Whether `is-system-running`'s answer means a usable user manager (`running`, or `degraded`: running with a failed unit).
pub fn manager_usable(output: &str) -> bool {
    matches!(output.trim(), "running" | "degraded")
}

/// The InvocationID in `systemctl show -p InvocationID --value`'s output: 32 hexadecimal digits, or `None` (no unit, or empty).
pub fn parse_invocation_id(output: &str) -> Option<String> {
    let id = output.trim();
    (id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit())).then(|| id.to_ascii_lowercase())
}

/// The benchmark unit names in `list-units` output (the first column of each line); anything else is ignored.
pub fn parse_unit_list(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter_map(|u| u.strip_suffix(".service"))
        .filter(|u| u.starts_with(UNIT_PREFIX) && u.len() > UNIT_PREFIX.len())
        .map(str::to_string)
        .collect()
}

/// What the agent wrote down before it began to measure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitRecord {
    /// The unit's name.
    pub unit: String,
    /// Its InvocationID when the agent started it.
    pub invocation_id: String,
}

/// A unit that exists now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveUnit {
    /// The unit's name.
    pub unit: String,
    /// Its InvocationID now (`None` when it could not be read).
    pub invocation_id: Option<String>,
}

/// What the agent's start does about leftovers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cleanup {
    /// Units to stop: the name and the InvocationID both match a record.
    pub stop: Vec<String>,
    /// Live units left alone (a matching name with a foreign or unreadable id, or no record at all).
    pub left_alone: Vec<String>,
}

/// Decides the cleanup: stop a live unit only if a record has its name and its InvocationID. The records are forgotten
/// afterwards whatever happens (the caller deletes the file).
pub fn cleanup(records: &[UnitRecord], live: &[LiveUnit]) -> Cleanup {
    let mut out = Cleanup::default();
    for l in live {
        let mine = l
            .invocation_id
            .as_deref()
            .is_some_and(|id| records.iter().any(|r| r.unit == l.unit && r.invocation_id == id));
        if mine {
            out.stop.push(l.unit.clone());
        } else {
            out.left_alone.push(l.unit.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn the_unit_name_is_the_prefix_and_a_safe_run_id() {
        assert_eq!(unit_name("1762190000000-1"), "dyno-bench-1762190000000-1");
        assert_eq!(unit_name("a b/c.d"), "dyno-bench-a-b-c-d");
        assert_eq!(unit_name(&"x".repeat(500)).len(), UNIT_PREFIX.len() + 64);
        assert_eq!(unit_name(""), "dyno-bench-");
    }

    #[test]
    fn the_run_command_is_the_documented_one() {
        let c = run_command(
            "dyno-bench-7",
            Path::new("/opt/dyno/dyno-bench"),
            &["cpu".into(), "--reps".into(), "5".into()],
        );
        assert_eq!(c.program, "systemd-run");
        assert_eq!(
            c.display(),
            "systemd-run --user --unit=dyno-bench-7 --collect --wait --pipe --service-type=exec -- /opt/dyno/dyno-bench cpu --reps 5"
        );
        // Nice 0 and no memory limit: no property is set.
        assert!(!c.args.iter().any(|a| a.contains("Nice") || a.contains("Memory") || a == "-p"));
    }

    #[test]
    fn stop_show_and_list_commands() {
        assert_eq!(stop_command("dyno-bench-7").display(), "systemctl --user stop dyno-bench-7.service");
        assert_eq!(
            invocation_command("dyno-bench-7").display(),
            "systemctl --user show -p InvocationID --value dyno-bench-7.service"
        );
        assert!(list_command().display().ends_with("dyno-bench-*.service"));
        assert_eq!(manager_state_command().display(), "systemctl --user is-system-running");
    }

    #[test]
    fn outputs_are_parsed_strictly() {
        assert!(manager_usable("running\n") && manager_usable("degraded") && !manager_usable("offline") && !manager_usable(""));
        assert_eq!(parse_invocation_id(&format!("{ID}\n")), Some(ID.to_string()));
        assert_eq!(parse_invocation_id("0123"), None);
        assert_eq!(parse_invocation_id(&"g".repeat(32)), None);
        assert_eq!(parse_invocation_id(""), None);
        let list = "dyno-bench-7.service loaded active running Dyno\nother.service loaded active running X\n\ndyno-bench-.service x\ndyno-bench-9.service not-found inactive dead\n";
        assert_eq!(parse_unit_list(list), vec!["dyno-bench-7", "dyno-bench-9"]);
    }

    #[test]
    fn restart_cleanup_stops_only_a_unit_whose_name_and_invocation_id_match() {
        let other = "ffffffffffffffffffffffffffffffff";
        let records = vec![UnitRecord {
            unit: "dyno-bench-7".into(),
            invocation_id: ID.into(),
        }];
        let live = vec![
            LiveUnit {
                unit: "dyno-bench-7".into(),
                invocation_id: Some(ID.into()),
            },
            // A reused name with another invocation: someone else's.
            LiveUnit {
                unit: "dyno-bench-7".into(),
                invocation_id: Some(other.into()),
            },
            // Unreadable id: not proven ours.
            LiveUnit {
                unit: "dyno-bench-7".into(),
                invocation_id: None,
            },
            // No record.
            LiveUnit {
                unit: "dyno-bench-8".into(),
                invocation_id: Some(ID.into()),
            },
        ];
        let c = cleanup(&records, &live);
        assert_eq!(c.stop, vec!["dyno-bench-7"]);
        assert_eq!(c.left_alone.len(), 3);
        assert_eq!(cleanup(&[], &live).stop, Vec::<String>::new());
        assert_eq!(cleanup(&records, &[]), Cleanup::default());
    }

    #[test]
    fn records_round_trip() {
        let r = UnitRecord {
            unit: "dyno-bench-1".into(),
            invocation_id: ID.into(),
        };
        let json = serde_json::to_string(&vec![r.clone()]).unwrap();
        assert_eq!(serde_json::from_str::<Vec<UnitRecord>>(&json).unwrap(), vec![r]);
    }
}
