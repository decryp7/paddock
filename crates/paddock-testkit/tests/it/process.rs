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

/// A listed tool (`node`) brings its folder from the machine's PATH; a folder that holds no listed tool stays out.
#[cfg(unix)]
#[test]
fn a_listed_tool_brings_its_machine_folder_and_an_unlisted_one_does_not() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let (tools, other) = (d.path().join("tools"), d.path().join("other"));
    for (dir, name) in [(&tools, "node"), (&other, "not-a-listed-tool")] {
        std::fs::create_dir(dir).unwrap();
        let f = dir.join(name);
        std::fs::write(&f, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut env = env_lock();
    env.set(
        "PATH",
        format!("{}:{}:/usr/bin:/bin", tools.display(), other.display()),
    );
    let seen = child_env(&mut TestCommand::new("/usr/bin/env"));
    let dirs: Vec<std::path::PathBuf> = std::env::split_paths(&seen["PATH"]).collect();
    assert!(dirs.contains(&tools), "{dirs:?}");
    assert!(!dirs.contains(&other), "{dirs:?}");
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

#[test]
fn env_clear_goes_back_to_the_listed_environment_and_never_erases_it() {
    let mut c = env_program();
    c.env("PADDOCK_TEST_NAMED", "yes").env_clear();
    let seen = child_env(&mut c);
    assert!(
        !seen.contains_key("PADDOCK_TEST_NAMED"),
        "an addition survived env_clear: {seen:?}"
    );
    assert!(
        seen.contains_key("PATH"),
        "env_clear erased the curated PATH: {seen:?}"
    );
    assert!(
        seen.contains_key("HOME"),
        "env_clear erased the private home: {seen:?}"
    );
}

#[test]
fn a_build_child_gets_the_runs_target_folder_and_build_variables_but_a_plain_one_does_not() {
    let mut env = env_lock();
    env.set("RUSTFLAGS", "-Cinstrument-coverage");
    env.set("RUSTC_WRAPPER", "sccache");
    let plain = env_program().environment();
    assert!(
        !plain
            .iter()
            .any(|(k, _)| k == "RUSTFLAGS" || k == "RUSTC_WRAPPER" || k == "CARGO_TARGET_DIR"),
        "{plain:?}"
    );
    let mut c = env_program();
    let built = c.cargo_build().environment();
    let get = |k: &str| {
        built
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.to_string_lossy().into_owned())
    };
    assert_eq!(get("RUSTFLAGS").as_deref(), Some("-Cinstrument-coverage"));
    assert_eq!(get("RUSTC_WRAPPER").as_deref(), Some("sccache"));
    assert!(
        get("CARGO_TARGET_DIR").is_some_and(|t| std::path::Path::new(&t).is_dir()),
        "{built:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_spawned_child_keeps_its_private_home_after_the_builder_is_dropped() {
    use std::io::Write;
    // The command is a temporary: it is gone before the child reads its line, which then writes to HOME.
    let mut child = TestCommand::new("sh")
        .args(["-c", "read x; echo ok > \"$HOME/out\" && echo \"$HOME\""])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"go\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "the child could not write to its home"
    );
}
/// The kill itself is proved by identity in paddock-testkit-start (`reap::tests`), which has `paddock_proc`.
#[cfg(unix)]
#[test]
fn a_dropped_test_childs_private_folder_goes_with_it() {
    let child = TestCommand::new("sh")
        .args(["-c", "read x"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let private = child.private_dir().to_path_buf();
    assert!(private.is_dir());
    drop(child);
    assert!(!private.exists(), "the private folder outlived the child");
}

#[test]
fn a_builder_childs_git_has_no_background_maintenance_and_a_test_identity() {
    let git = |args: &[&str]| {
        let out = TestCommand::new("git").args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    assert_eq!(git(&["config", "--global", "maintenance.auto"]), "false");
    assert_eq!(git(&["config", "--global", "gc.auto"]), "0");
    assert_eq!(
        git(&["config", "--global", "user.email"]),
        "test@example.invalid"
    );
}

/// git finds the private config through `HOME`, never through `GIT_CONFIG_GLOBAL` (fix/linux-container-gate, 88c06e902): with that
/// variable set, a test that gives its child another home with its own `.gitconfig` (the kept-clone fixture turns maintenance off that
/// way) had that file ignored.
#[test]
fn a_childs_git_reads_the_gitconfig_of_the_home_it_is_given() {
    let other = tempfile::tempdir().unwrap();
    std::fs::write(
        other.path().join(".gitconfig"),
        "[maintenance]\n\tauto = 7\n",
    )
    .unwrap();
    let out = TestCommand::new("git")
        .args(["config", "--global", "maintenance.auto"])
        .env("HOME", other.path())
        .env("USERPROFILE", other.path())
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "7",
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !TestCommand::new("git")
            .environment()
            .iter()
            .any(|(k, _)| k == "GIT_CONFIG_GLOBAL"),
        "GIT_CONFIG_GLOBAL is set, so git ignores the home's .gitconfig"
    );
}

/// `GIT_CONFIG_NOSYSTEM` hides the system config, where the CI image and the local Linux container name `safe.directory`; the
/// private config names it, so a child's git still works in a checkout another user owns (a mounted worktree in the container).
#[test]
fn a_childs_git_trusts_any_folder_since_the_system_config_is_not_read() {
    let out = TestCommand::new("git")
        .args(["config", "--global", "--get-all", "safe.directory"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "*",
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `new_in` leaves a `.gitconfig` the test already wrote into the home it names (review of a8fca0ebd, P2 4: it was overwritten, and
/// two children sharing a home rewrote it under each other).
#[test]
fn new_in_keeps_the_gitconfig_a_test_wrote_into_its_home() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("home")).unwrap();
    std::fs::write(
        dir.path().join("home/.gitconfig"),
        "[user]\n\tname = Kept\n",
    )
    .unwrap();
    let out = TestCommand::new_in("git", dir.path())
        .args(["config", "--global", "user.name"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "Kept");
}

/// `safe.directory=*` trusts every folder, so git must not look for a repository above the temp root (CVE-2022-24765: a `.git`
/// another user planted in a shared `/tmp`): `GIT_CEILING_DIRECTORIES` names it. A repository below it is still found.
#[test]
fn a_childs_git_stops_at_the_temp_root_and_still_finds_a_repository_below_it() {
    let ceiling = TestCommand::new("git")
        .environment()
        .into_iter()
        .find(|(k, _)| k == "GIT_CEILING_DIRECTORIES")
        .map(|(_, v)| std::path::PathBuf::from(v));
    assert_eq!(ceiling, Some(std::env::temp_dir()));
    let repo = tempfile::tempdir().unwrap();
    let init = TestCommand::new("git")
        .args(["init", "-q"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    std::fs::create_dir(repo.path().join("sub")).unwrap();
    let top = TestCommand::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(repo.path().join("sub"))
        .output()
        .unwrap();
    assert!(
        top.status.success(),
        "{}",
        String::from_utf8_lossy(&top.stderr)
    );
}

/// The shell tools `./dev` and the gate scripts call are found by a child's shell on every OS (ci.yml run 8138: on Windows
/// `grep`, `tr` and `touch` were "command not found" because Git for Windows' `usr/bin` was not on the curated PATH).
#[test]
fn a_childs_shell_finds_the_tools_the_scripts_call() {
    let mut cmd = TestCommand::new("sh");
    cmd.args([
        "-c",
        "command -v grep && command -v tr && command -v touch && command -v sed && command -v git",
    ]);
    // The failure names what decided the PATH (ER-27): the child's, the machine's, and where git, sh and bash were found.
    let report = cmd.path_report();
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{}{}\n{report}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The toolchain reaches a child, its siblings do not (review of 7edb8cad6, P1-1): `cargo install` puts tools such as `heavy`
/// beside the rustup proxies in `$CARGO_HOME/bin`, and that folder on the child's PATH reopened ADR 0100's first incident. The
/// child gets links to the toolchain's programs in a private folder instead.
#[cfg(unix)]
#[test]
fn an_installed_tool_beside_the_toolchain_is_not_reachable_and_the_toolchain_is() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let home_bin = d.path().join("cargo-home-bin");
    std::fs::create_dir(&home_bin).unwrap();
    for name in ["rustc", "rustup", "heavy"] {
        let f = home_bin.join(name);
        std::fs::write(&f, "#!/bin/sh\necho $0\n").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut env = env_lock();
    env.set("PATH", format!("{}:/usr/bin:/bin", home_bin.display()));
    let mut cmd = TestCommand::new("/bin/sh");
    cmd.args([
        "-c",
        "if command -v heavy; then exit 3; fi; command -v rustc && rustc",
    ]);
    let report = cmd.path_report();
    let out = cmd.output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout).into_owned();
    assert_ne!(
        out.status.code(),
        Some(3),
        "heavy is reachable: {said}\n{report}"
    );
    assert!(
        out.status.success(),
        "rustc is not reachable: {said}{}\n{report}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !said.contains(home_bin.to_str().unwrap()),
        "the child ran rustc from the machine folder, not a link: {said}"
    );
    assert!(
        said.trim_end().ends_with("/rustc"),
        "rustup dispatches on the name it was started as: {said}"
    );
}

/// Git for Windows found only through `mingw64\bin` (what Git's bash puts first on PATH): its `usr\bin`, where the shell tools are,
/// still reaches the child, and a folder that is not Git's never does (review of 7edb8cad6, P2-4; ci.yml run 8138).
#[cfg(windows)]
#[test]
fn git_for_windows_found_through_mingw64_brings_its_usr_bin() {
    let d = tempfile::tempdir().unwrap();
    let git = d.path().join("Git");
    for sub in ["mingw64\\bin", "cmd", "usr\\bin", "bin"] {
        std::fs::create_dir_all(git.join(sub)).unwrap();
    }
    for f in [
        "mingw64\\bin\\git.exe",
        "cmd\\git.exe",
        "bin\\git.exe",
        "usr\\bin\\bash.exe",
    ] {
        std::fs::write(git.join(f), "").unwrap();
    }
    let mut env = env_lock();
    env.set("PATH", git.join("mingw64\\bin"));
    let cmd = TestCommand::new("cmd");
    let report = cmd.path_report();
    // Compared as Windows compares folders: without case, either separator.
    let dirs: Vec<String> = report
        .child
        .iter()
        .map(|p| p.to_string_lossy().replace('/', "\\").to_lowercase())
        .collect();
    let usr_bin = git.join("usr\\bin").to_string_lossy().to_lowercase();
    assert!(dirs.iter().any(|d| d == &usr_bin), "{report}");
    let system32 = dirs.iter().filter(|d| d.ends_with("system32")).count();
    assert_eq!(system32, 1, "System32 joined more than once: {report}");
}
