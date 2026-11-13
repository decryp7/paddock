//! `TestCommand`: a child starts from an empty environment plus the listed one (ADR 0100).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_testkit::process::allowed_names;
use paddock_testkit::{TestCommand, env_lock};
use std::collections::BTreeMap;

/// The environment of a real child: `env` on Unix, `set` on Windows (both print `NAME=value` per line).
fn child_env(c: &mut TestCommand) -> BTreeMap<String, String> {
    #[cfg(unix)]
    let out = c.output().unwrap();
    #[cfg(windows)]
    let out = c.args(["/C", "set"]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn env_program() -> TestCommand {
    #[cfg(unix)]
    return TestCommand::new("env");
    #[cfg(windows)]
    return TestCommand::new("cmd");
}

#[test]
fn a_child_sees_only_listed_names_whatever_the_machine_has() {
    let mut env = env_lock();
    for k in [
        "ENGINEERING_RULES",
        "HEAVY_LOCK_DIR",
        "HEAVY_SLOTS",
        "PADDOCK_HEAVY",
        "PADDOCK_DEV_HOME",
        "CI",
        "GITHUB_TOKEN",
        "RUSTC_WRAPPER",
        "PADDOCK_NEW_LOOKUP_FROM_TOMORROW",
    ] {
        env.set(k, "machine");
    }
    let seen = child_env(&mut env_program());
    let allowed = allowed_names();
    // A shell that `env` or `cmd` starts may add its own (`PWD`, `_`, `SHLVL`, `PROMPT`...): not a lookup of the machine.
    let strays: Vec<&String> = seen
        .keys()
        .filter(|k| {
            !allowed.contains(&k.as_str())
                && !["PWD", "OLDPWD", "SHLVL", "_", "PROMPT", "=C:"].contains(&k.as_str())
        })
        .collect();
    assert!(
        strays.is_empty(),
        "variables nobody listed reached the child: {strays:?}"
    );
    assert!(
        seen.contains_key("PATH") && seen.contains_key("HOME") || cfg!(windows),
        "{seen:?}"
    );
}

#[test]
fn home_and_temp_are_the_commands_private_folders_never_the_machines() {
    let mut env = env_lock();
    env.set("HOME", "/machine/home");
    env.set("XDG_CACHE_HOME", "/machine/cache");
    let mut c = env_program();
    let (private, home) = (c.private_dir().to_path_buf(), c.home());
    let seen = child_env(&mut c);
    let under = |k: &str| std::path::Path::new(&seen[k]).starts_with(&private);
    assert!(
        under("HOME") && under("TMPDIR") && under("XDG_CACHE_HOME"),
        "{seen:?}"
    );
    assert_eq!(std::path::Path::new(&seen["HOME"]), home);
    // The slot tool keeps its slots under $HOME/.cache: a child's lands in the private folder, never ~/.cache/heavy-slots.
    assert!(std::path::Path::new(&seen["XDG_CACHE_HOME"]).starts_with(&home));
}

#[test]
fn the_private_folder_goes_with_the_command_and_a_given_one_stays() {
    let c = TestCommand::new("env");
    let own = c.private_dir().to_path_buf();
    assert!(own.is_dir());
    drop(c);
    assert!(!own.exists(), "the private folder was not removed");
    let d = tempfile::tempdir().unwrap();
    let c = TestCommand::new_in("env", &d.path().join("mine"));
    assert_eq!(c.private_dir(), d.path().join("mine"));
    drop(c);
    assert!(d.path().join("mine/home").is_dir());
}

#[test]
fn named_additions_and_a_path_first_folder_reach_the_child() {
    let d = tempfile::tempdir().unwrap();
    let mut c = env_program();
    c.path_first(d.path()).env("PADDOCK_TEST_NAMED", "yes");
    let seen = child_env(&mut c);
    assert_eq!(seen["PADDOCK_TEST_NAMED"], "yes");
    let first = std::env::split_paths(&seen["PATH"]).next().unwrap();
    assert_eq!(first, d.path());
}

#[cfg(unix)]
#[test]
fn the_curated_path_has_the_toolchain_and_the_system_but_not_the_machines_path() {
    let mut env = env_lock();
    env.set("PATH", "/machine/only/bin:/usr/bin:/bin");
    let seen = child_env(&mut TestCommand::new("env"));
    let dirs: Vec<std::path::PathBuf> = std::env::split_paths(&seen["PATH"]).collect();
    assert!(
        !dirs.iter().any(|d| d.ends_with("machine/only/bin")),
        "{dirs:?}"
    );
    assert!(
        dirs.ends_with(&["/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(std::path::PathBuf::from)),
        "{dirs:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_bare_program_name_is_resolved_once_from_the_test_process() {
    // `sh` is found although the child's PATH is curated, and the child runs.
    let out = TestCommand::new("sh")
        .args(["-c", "echo ok"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ok");
}
