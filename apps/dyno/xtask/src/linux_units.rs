//! `linux-units`: the benchmark unit tests on a Linux machine that has a systemd user manager (the OrbStack test machine
//! `paddock-gate-arm` by default, never the developer's own session). The tests are `#[ignore]`d in a plain `cargo test` and fail,
//! never pass silently, without a manager, so this is the only place they run (the `tool:` gate in docs/gates.toml names it).

use std::path::Path;
use std::time::Duration;

const DEFAULT_MACHINE: &str = "paddock-gate-arm";
/// The test run itself: a build and seven unit tests; bounded so a hung machine cannot hold the terminal.
const RUN_LIMIT: Duration = Duration::from_secs(3600);

/// The machine name: letters, digits, `-`, `_`, `.`.
pub fn machine_ok(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

/// `s` as one shell word: single quotes, each embedded quote closed, escaped and reopened.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The script the machine's login shell runs: it starts the user manager if it can, checks it is up, then runs the tests.
pub fn guest_script(machine: &str, root: &str, target: &str) -> String {
    format!(
        r#"set -e
uid=$(id -u)
sudo -n loginctl enable-linger $USER >/dev/null 2>&1 || true
sudo -n systemctl start user@$uid.service >/dev/null 2>&1 || true
export XDG_RUNTIME_DIR=/run/user/$uid DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$uid/bus CARGO_TARGET_DIR={target}
state=$(systemctl --user is-system-running || true)
echo "user manager: $state"
case "$state" in running|degraded) ;; *) echo "::error::no systemd user manager on {machine} (is-system-running said $state); start it: sudo loginctl enable-linger $USER"; exit 1;; esac
cd {root}
nice -n 10 cargo test -p dyno-agent --test bench_units -- --ignored --test-threads=1
"#,
        target = sh_quote(target),
        root = sh_quote(root),
        machine = machine.replace('"', ""),
    )
}

/// The command that runs this task again under the test machines lock (`scripts/vm-lock-run.sh`, the interim lock of
/// docs/dev-setup.md): starting or driving a test machine without it can collide with another agent's run (ER-14).
pub fn locked_command(repo: &Path, exe: &Path, machine: &str) -> Vec<String> {
    vec![
        "bash".to_string(),
        repo.join("scripts/vm-lock-run.sh").to_string_lossy().into_owned(),
        "dyno-linux-units".to_string(),
        "--".to_string(),
        exe.to_string_lossy().into_owned(),
        "linux-units".to_string(),
        machine.to_string(),
    ]
}

/// The command that asks `scripts/vm-lock-run.sh --check` (vm-lock.sh's `vm_lock_inherited`) whether this process's ancestor holds the lock.
pub fn held_check_command(repo: &Path) -> Vec<String> {
    vec![
        "bash".to_string(),
        repo.join("scripts/vm-lock-run.sh").to_string_lossy().into_owned(),
        "--check".to_string(),
    ]
}

/// `cargo xtask linux-units [machine]`.
pub fn run_task(args: &[String]) -> Result<(), String> {
    let machine = args.first().map_or(DEFAULT_MACHINE, String::as_str);
    if !machine_ok(machine) || args.len() > 1 {
        return Err("usage: cargo xtask linux-units [machine]  (a machine name: letters, digits, - _ .)".to_string());
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    // Is the lock held by an ancestor? Asked of vm-lock.sh itself (`vm_lock_inherited`: the pid, its start time, the owner folder and
    // its who file all have to agree), never guessed from a variable that anyone can set.
    let check = held_check_command(&repo);
    let held = crate::run(
        "bash",
        &check[1..].iter().map(String::as_str).collect::<Vec<_>>(),
        &[],
        Duration::from_secs(60),
    )
    .is_ok_and(|r| r.success);
    if !held {
        let exe = std::env::current_exe().map_err(|e| format!("cannot find this program: {e}"))?;
        let cmd = locked_command(&repo, &exe, machine);
        let args: Vec<&str> = cmd[1..].iter().map(String::as_str).collect();
        // The re-run's output comes as it happens: the build and the tests take minutes.
        return if crate::run_streamed("bash", &args, RUN_LIMIT + Duration::from_secs(3600))? {
            Ok(())
        } else {
            Err("::error::linux-units did not pass, or the test machines lock is busy (the output above names the holder). Next step: read it; a busy lock frees when its holder ends.".to_string())
        };
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("the xtask folder has no parent")?;
    let root = root.to_string_lossy();
    let target = std::env::var("LINUX_UNITS_TARGET").unwrap_or_else(|_| "/home/$USER/cache/dyno-bench-target".to_string());
    // `$USER` in the default target is the guest's: it must reach the guest shell unquoted.
    let script = guest_script(machine, &root, &target).replace(&sh_quote(&target), &format!("\"{target}\""));
    eprintln!("linux-units: machine {machine}, checkout {root}");
    let _ = crate::run("orbctl", &["start", machine], &[], Duration::from_secs(120));
    let passed = crate::run_streamed("orb", &["-m", machine, "bash", "-lc", &script], RUN_LIMIT)?;
    if passed {
        eprintln!("linux-units: passed on {machine}");
        Ok(())
    } else {
        Err(format!(
            "::error::the benchmark unit tests failed on {machine}: read the output above; the Linux runner must not ship units until they pass (docs/linux.md)"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_names_are_plain_words() {
        assert!(machine_ok("paddock-gate-arm") && machine_ok("a.b_c"));
        assert!(!machine_ok("") && !machine_ok("a b") && !machine_ok("a;rm") && !machine_ok("$(x)"));
    }

    #[test]
    fn the_task_runs_again_under_the_test_machines_lock() {
        let c = locked_command(Path::new("/r"), Path::new("/t/xtask"), "m1");
        assert_eq!(
            c.join(" "),
            "bash /r/scripts/vm-lock-run.sh dyno-linux-units -- /t/xtask linux-units m1"
        );
    }

    #[test]
    fn the_held_lock_is_asked_of_the_lock_script_not_guessed_from_a_variable() {
        assert_eq!(
            held_check_command(Path::new("/r")).join(" "),
            "bash /r/scripts/vm-lock-run.sh --check"
        );
    }

    #[test]
    fn a_quote_in_a_path_cannot_end_the_word() {
        assert_eq!(sh_quote("/a b/it's"), r"'/a b/it'\''s'");
    }

    #[test]
    fn the_guest_script_runs_the_ignored_unit_tests_alone_after_the_manager_check() {
        let s = guest_script("m1", "/work/dyno", "/t");
        assert!(s.contains("cd '/work/dyno'\nnice -n 10 cargo test -p dyno-agent --test bench_units -- --ignored --test-threads=1\n"));
        assert!(s.contains("CARGO_TARGET_DIR='/t'") && s.contains("is-system-running"));
        assert!(s.contains("no systemd user manager on m1"));
        assert!(s.find("is-system-running").unwrap() < s.find("cargo test").unwrap());
    }
}
