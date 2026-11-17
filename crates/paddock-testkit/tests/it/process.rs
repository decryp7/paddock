//! `TestCommand`: a child starts from an empty environment plus the listed one (ADR 0100).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_testkit::process::allowed_names;
use paddock_testkit::{Ambient, TestCommand, env_lock};
use std::collections::BTreeMap;
use std::path::Path;

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

/// This process's machine values with another `PATH` and the link folders under `links` (the test's own folder): a stand-in
/// toolchain never changes the process environment, and its link folders go with the test.
fn machine_with(path: impl Into<std::ffi::OsString>, links: &Path) -> Ambient {
    Ambient {
        path: Some(path.into()),
        link_root: Some(links.to_path_buf()),
        ..paddock_testkit::ambient().clone()
    }
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
    // What the kit itself reads is the captured Ambient: one that holds the machine's own HOME and RUSTC_WRAPPER (a build
    // variable, passed only to `cargo_build`) gives the child neither.
    let mut m = paddock_testkit::ambient().clone();
    m.home = Some("/machine/home".into());
    m.vars.retain(|(k, _)| k != "RUSTC_WRAPPER");
    m.vars.push(("RUSTC_WRAPPER".into(), "machine".into()));
    let program = if cfg!(windows) { "cmd" } else { "env" };
    let seen = child_env(&mut TestCommand::on(program, &m));
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
    assert!(!seen.contains_key("RUSTC_WRAPPER"), "{seen:?}");
    assert!(
        seen.get("HOME").is_none_or(|h| h != "/machine/home"),
        "{seen:?}"
    );
}

/// ER-47: a variable that restricts a child (a download ban, an install ban, an offline switch, a failure marker) is never stripped
/// by the clearing: the child that lost it would do what the caller forbade. The machine here is an `Ambient` that carries them.
#[test]
fn restricting_variables_the_machine_has_reach_the_child() {
    let mut m = paddock_testkit::ambient().clone();
    m.vars
        .retain(|(k, _)| !paddock_env::RESTRICTING.contains(&k.to_str().unwrap_or_default()));
    for k in paddock_env::RESTRICTING {
        m.vars.push(((*k).into(), format!("{k}-restricts").into()));
    }
    let program = if cfg!(windows) { "cmd" } else { "env" };
    let seen = child_env(&mut TestCommand::on(program, &m));
    for k in paddock_env::RESTRICTING {
        assert_eq!(
            seen.get(*k).map(String::as_str),
            Some(format!("{k}-restricts").as_str()),
            "{k} did not reach the child: {seen:?}"
        );
    }
}

/// Positive control (ER-33): the same machine without the variables gives a child without them, so the test above sees the
/// pass-through and not a value the child always has. `RUSTUP_AUTO_INSTALL` is the one the kit sets itself.
#[test]
fn a_machine_without_restricting_variables_gives_the_child_none_but_the_install_ban() {
    let mut m = paddock_testkit::ambient().clone();
    m.vars
        .retain(|(k, _)| !paddock_env::RESTRICTING.contains(&k.to_str().unwrap_or_default()));
    let program = if cfg!(windows) { "cmd" } else { "env" };
    let seen = child_env(&mut TestCommand::on(program, &m));
    for k in paddock_env::RESTRICTING
        .iter()
        .filter(|k| **k != "RUSTUP_AUTO_INSTALL")
    {
        assert!(
            !seen.contains_key(*k),
            "{k} appeared from nowhere: {seen:?}"
        );
    }
    assert_eq!(
        seen.get("RUSTUP_AUTO_INSTALL").map(String::as_str),
        Some("0"),
        "{seen:?}"
    );
}

/// A child's rustup never installs a toolchain into the user's `~/.rustup` unless the caller allowed it (ER-45).
#[test]
fn the_install_ban_is_set_when_the_caller_has_none_and_the_callers_value_stands() {
    let program = if cfg!(windows) { "cmd" } else { "env" };
    let mut none = paddock_testkit::ambient().clone();
    none.vars.retain(|(k, _)| k != "RUSTUP_AUTO_INSTALL");
    let seen = child_env(&mut TestCommand::on(program, &none));
    assert_eq!(
        seen.get("RUSTUP_AUTO_INSTALL").map(String::as_str),
        Some("0"),
        "{seen:?}"
    );
    let mut allowed = none.clone();
    allowed
        .vars
        .push(("RUSTUP_AUTO_INSTALL".into(), "1".into()));
    let seen = child_env(&mut TestCommand::on(program, &allowed));
    assert_eq!(
        seen.get("RUSTUP_AUTO_INSTALL").map(String::as_str),
        Some("1"),
        "{seen:?}"
    );
}

/// `env_clear` goes back to the listed set, and the restricting variables are in it.
#[test]
fn env_clear_keeps_the_restricting_variables() {
    let mut m = paddock_testkit::ambient().clone();
    m.vars.retain(|(k, _)| k != "PADDOCK_DEV_DOWNLOAD");
    m.vars.push(("PADDOCK_DEV_DOWNLOAD".into(), "0".into()));
    let program = if cfg!(windows) { "cmd" } else { "env" };
    let mut c = TestCommand::on(program, &m);
    c.env("ADDED", "1").env_clear();
    let seen = child_env(&mut c);
    assert_eq!(
        seen.get("PADDOCK_DEV_DOWNLOAD").map(String::as_str),
        Some("0"),
        "{seen:?}"
    );
    assert!(!seen.contains_key("ADDED"), "{seen:?}");
}

#[test]
fn home_and_temp_are_the_commands_private_folders_never_the_machines() {
    let mut env = env_lock();
    env.set("XDG_CACHE_HOME", "/machine/cache");
    // The machine's home as the kit captured it: the child never gets it.
    let mut m = paddock_testkit::ambient().clone();
    m.home = Some("/machine/home".into());
    let mut c = TestCommand::on(if cfg!(windows) { "cmd" } else { "env" }, &m);
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
    let d = paddock_testkit::tempdir();
    let c = TestCommand::new_in("env", &d.path().join("mine"));
    assert_eq!(c.private_dir(), d.path().join("mine"));
    drop(c);
    assert!(d.path().join("mine/home").is_dir());
}

#[test]
fn named_additions_and_a_path_first_folder_reach_the_child() {
    let d = paddock_testkit::tempdir();
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
    let d = paddock_testkit::tempdir();
    let m = machine_with("/machine/only/bin:/usr/bin:/bin", d.path());
    let seen = child_env(&mut TestCommand::on("env", &m));
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
    let d = paddock_testkit::tempdir();
    let (tools, other) = (d.path().join("tools"), d.path().join("other"));
    for (dir, name) in [(&tools, "node"), (&other, "not-a-listed-tool")] {
        std::fs::create_dir(dir).unwrap();
        paddock_testkit::StandIn::new(dir, name).install().unwrap();
    }
    let m = machine_with(
        format!("{}:{}:/usr/bin:/bin", tools.display(), other.display()),
        &d.path().join("links"),
    );
    let seen = child_env(&mut TestCommand::on("/usr/bin/env", &m));
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
    // The run's build variables, as the captured machine values would hold them.
    let mut m = paddock_testkit::ambient().clone();
    m.vars
        .retain(|(k, _)| k != "RUSTFLAGS" && k != "RUSTC_WRAPPER");
    m.vars
        .push(("RUSTFLAGS".into(), "-Cinstrument-coverage".into()));
    m.vars.push(("RUSTC_WRAPPER".into(), "sccache".into()));
    let program = if cfg!(windows) { "cmd" } else { "env" };
    let plain = TestCommand::on(program, &m).environment();
    assert!(
        !plain
            .iter()
            .any(|(k, _)| k == "RUSTFLAGS" || k == "RUSTC_WRAPPER" || k == "CARGO_TARGET_DIR"),
        "{plain:?}"
    );
    let mut c = TestCommand::on(program, &m);
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
    let other = paddock_testkit::tempdir();
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
    let dir = paddock_testkit::tempdir();
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
    assert_eq!(ceiling, Some(paddock_testkit::ambient().temp_dir.clone()));
    let repo = paddock_testkit::tempdir();
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
    let d = paddock_testkit::tempdir();
    let home_bin = d.path().join("cargo-home-bin");
    std::fs::create_dir(&home_bin).unwrap();
    for name in ["rustc", "rustup", "heavy"] {
        paddock_testkit::StandIn::new(&home_bin, name)
            .install()
            .unwrap();
    }
    let m = machine_with(
        format!("{}:/usr/bin:/bin", home_bin.display()),
        &d.path().join("links"),
    );
    let mut cmd = TestCommand::on("/bin/sh", &m);
    cmd.args([
        "-c",
        "if command -v heavy; then exit 3; fi; found=$(command -v rustc) && test -x \"$found\" && echo \"$found\"",
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
    let d = paddock_testkit::tempdir();
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
    let m = machine_with(git.join("mingw64\\bin"), &d.path().join("links"));
    let cmd = TestCommand::on("cmd", &m);
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
    // The `git` a child (the program under test) finds is the real one, never Git's `bin\git.exe` or `cmd\git.exe`, which
    // only start it again: one more process per call (ci.yml run 8161: merge_check about 5.7 times slower; measured on the
    // test VM, 62 against 44 ms per call alone and 167 against 101 ms with 8 at once, called in turn, docs/benchmarks.md).
    let first_git = report
        .child
        .iter()
        .map(|d| d.join("git.exe"))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().replace('/', "\\").to_lowercase());
    let real = git
        .join("mingw64\\bin\\git.exe")
        .to_string_lossy()
        .to_lowercase();
    assert_eq!(first_git.as_deref(), Some(real.as_str()), "{report}");
}

/// `new("cargo")` starts the cargo that runs this test (`CARGO`), not rustup's proxy, which puts all of `$CARGO_HOME/bin` in
/// front of its child's PATH (review of 0916c8060, P2-1: a build script found `cargo-hakari` that way). A build script of a
/// child cargo must not reach a tool planted in the `CARGO_HOME` it was given.
#[cfg(unix)]
#[test]
fn a_child_cargos_build_script_does_not_reach_the_tools_of_cargo_home() {
    let d = paddock_testkit::tempdir();
    let crate_dir = d.path().join("probe");
    std::fs::create_dir_all(crate_dir.join("src")).unwrap();
    std::fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\nbuild = \"build.rs\"\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(crate_dir.join("src/lib.rs"), "").unwrap();
    std::fs::write(
        crate_dir.join("build.rs"),
        "fn main() {\n    let path = std::env::var_os(\"PATH\").unwrap_or_default();\n    if let Some(d) = std::env::split_paths(&path).find(|d| d.join(\"cargo-hakari\").exists()) {\n        panic!(\"cargo-hakari is reachable in {}\", d.display());\n    }\n}\n",
    )
    .unwrap();
    // The CARGO_HOME the child is given, with an installed tool in its bin (the real one's registry is not needed: no dependencies).
    let cargo_home = d.path().join("cargo-home");
    std::fs::create_dir_all(cargo_home.join("bin")).unwrap();
    paddock_testkit::StandIn::new(&cargo_home.join("bin"), "cargo-hakari")
        .install()
        .unwrap();
    let mut cmd = TestCommand::new("cargo");
    cmd.args(["build", "--offline", "--quiet"])
        .current_dir(&crate_dir)
        .env("CARGO_HOME", &cargo_home)
        .env("CARGO_TARGET_DIR", d.path().join("target"));
    let report = cmd.path_report();
    let program = cmd.get_program().to_owned();
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "started {}: {}\n{report}",
        program.to_string_lossy(),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Every command of a test process starts its `PATH` with one shared toolchain link folder under its link root (ci.yml run 8152:
/// made per command, the Windows tests that start many children ran about 6 times slower), never inside a command's or a
/// `new_in` folder; without a link root (a test program outside a target folder) each command links into its own folder.
#[test]
fn many_commands_share_one_toolchain_link_folder_under_their_link_root() {
    let d = paddock_testkit::tempdir();
    let tc = d.path().join("toolchain");
    std::fs::create_dir(&tc).unwrap();
    std::fs::write(
        tc.join(if cfg!(windows) { "rustc.exe" } else { "rustc" }),
        "",
    )
    .unwrap();
    // Only the stand-in toolchain, and the link folders in the test's own folder: the commands are made and looked at, never
    // started.
    let links = d.path().join("links");
    let m = machine_with(&tc, &links);
    let given = d.path().join("given");
    let mut cmds: Vec<TestCommand> = (0..5).map(|_| TestCommand::on("git", &m)).collect();
    cmds.push(TestCommand::on_in("git", &given, &m));
    let firsts: Vec<std::path::PathBuf> = cmds
        .iter()
        .map(|c| {
            let report = c.path_report();
            report
                .child
                .first()
                .cloned()
                .unwrap_or_else(|| panic!("no PATH: {report}"))
        })
        .collect();
    assert!(
        firsts.iter().all(|f| f == &firsts[0]),
        "the commands do not share one folder: {firsts:?}"
    );
    assert!(
        firsts[0].starts_with(&links),
        "{} is not under the link root {}",
        firsts[0].display(),
        links.display()
    );
    assert!(
        cmds.iter().all(|c| !firsts[0].starts_with(c.private_dir()))
            && !firsts[0].starts_with(&given)
    );
    // Without a link root (a test program outside a target folder) each command links into its own folder.
    let alone = Ambient {
        link_root: None,
        ..m.clone()
    };
    let c = TestCommand::on("git", &alone);
    let first = c.path_report().child.first().cloned().unwrap();
    assert!(
        first.starts_with(c.private_dir()),
        "{} is outside the command's own folder",
        first.display()
    );
}

/// A link folder removed while a process still uses it (another process's sweep, a cleaned temp folder) is made again on the next
/// command, never silently dropped from the PATH (review of df9af3cda, P2-1: 199 of 200 commands lost their first PATH folder).
#[test]
fn a_link_folder_removed_under_a_process_is_made_again_on_its_next_command() {
    let d = paddock_testkit::tempdir();
    let tc = d.path().join("toolchain");
    std::fs::create_dir(&tc).unwrap();
    std::fs::write(
        tc.join(if cfg!(windows) { "rustc.exe" } else { "rustc" }),
        "",
    )
    .unwrap();
    let m = machine_with(&tc, &d.path().join("links"));
    let held = TestCommand::on("git", &m);
    let first = held.path_report().toolchain;
    let folder = first
        .first()
        .and_then(|l| l.parent())
        .expect("a link folder")
        .to_path_buf();
    std::fs::remove_dir_all(&folder).unwrap();
    let next = TestCommand::on("git", &m);
    let again = next.path_report();
    assert!(
        !again.toolchain.is_empty(),
        "the next command has no toolchain links: {again}"
    );
    assert!(again.toolchain.iter().all(|l| l.exists()), "{again}");
}

/// The sweep removes, once they are an hour old, the staging folders (`.`) a crashed process left and the link folders whose
/// programs are all gone (a test's stand-in toolchain); a link folder whose programs are there, and anything younger, stays
/// (review of df9af3cda and of a9a7e6484).
#[test]
fn the_sweep_removes_only_hour_old_staging_folders_and_folders_of_gone_programs() {
    use paddock_testkit::process::sweep_toolchain_folders;
    use std::time::{Duration, SystemTime};
    let root = paddock_testkit::tempdir();
    let present = root.path().join("present-program");
    std::fs::write(&present, "").unwrap();
    let live = root.path().join("0000000000000001");
    let gone = root.path().join("0000000000000002");
    let staging = root.path().join(".0000000000000003-1-1");
    for f in [&live, &gone, &staging] {
        std::fs::create_dir(f).unwrap();
    }
    let record = |programs: &[&Path]| {
        let mut t = String::from("# paddock-test-toolchain links v2\n");
        for p in programs {
            t.push_str(&format!("rustc\t{}\n", p.display()));
        }
        t
    };
    std::fs::write(live.join("links.txt"), record(&[&present])).unwrap();
    std::fs::write(
        gone.join("links.txt"),
        record(&[&root.path().join("no-such-program")]),
    )
    .unwrap();
    sweep_toolchain_folders(root.path(), SystemTime::now());
    assert!(
        live.is_dir() && gone.is_dir() && staging.is_dir(),
        "a young folder was removed"
    );
    sweep_toolchain_folders(
        root.path(),
        SystemTime::now() + Duration::from_secs(2 * 3600),
    );
    assert!(!staging.exists(), "an old staging folder stayed");
    assert!(
        !gone.exists(),
        "an old folder whose programs are all gone stayed"
    );
    assert!(
        live.is_dir(),
        "a folder whose programs are there was removed"
    );
}

/// A link folder whose record has no format header, or an older one, is made again (review of a9a7e6484): its links may be of
/// an older shape, so it is never reused.
#[test]
fn a_link_folder_without_the_current_format_header_is_made_again() {
    let d = paddock_testkit::tempdir();
    let tc = d.path().join("toolchain");
    std::fs::create_dir(&tc).unwrap();
    std::fs::write(
        tc.join(if cfg!(windows) { "rustc.exe" } else { "rustc" }),
        "",
    )
    .unwrap();
    let m = machine_with(&tc, &d.path().join("links"));
    let first = TestCommand::on("git", &m);
    let folder = first.path_report().child.first().cloned().unwrap();
    let record = folder.join("links.txt");
    for old in ["", "# paddock-test-toolchain links v1\n"] {
        let body: String = std::fs::read_to_string(&record)
            .unwrap()
            .lines()
            .skip(1)
            .map(|l| format!("{l}\n"))
            .collect();
        std::fs::write(&record, format!("{old}{body}")).unwrap();
        let next = TestCommand::on("git", &m);
        let text =
            std::fs::read_to_string(next.path_report().child.first().unwrap().join("links.txt"))
                .unwrap();
        assert_eq!(
            text.lines().next(),
            Some("# paddock-test-toolchain links v2"),
            "a folder with {old:?} was reused: {text}"
        );
    }
}

/// A toolchain program found as `rustc.EXE` (Windows' `PATHEXT` spelling) is linked under its own name, `rustc.exe`, and a bare
/// `rustc` starts through that link (ci.yml run 8154: the link was named after the found spelling, `rustc.EXE`).
#[test]
fn a_toolchain_program_found_in_another_spelling_is_linked_and_started_under_its_own_name() {
    use paddock_env::Os;
    let d = paddock_testkit::tempdir();
    let tc = d.path().join("toolchain");
    std::fs::create_dir(&tc).unwrap();
    std::fs::write(
        tc.join(if cfg!(windows) { "rustc.EXE" } else { "rustc" }),
        "",
    )
    .unwrap();
    let m = machine_with(&tc, &d.path().join("links"));
    let cmd = TestCommand::on("rustc", &m);
    let report = cmd.path_report();
    let own = format!("rustc{}", std::env::consts::EXE_SUFFIX);
    let link = report
        .toolchain
        .iter()
        .find(|l| {
            l.file_name().is_some_and(|n| {
                paddock_env::winpath::same(Path::new(n), Path::new(&own), Os::current())
            })
        })
        .unwrap_or_else(|| panic!("no rustc link: {report}"));
    assert_eq!(
        link.file_name().unwrap().to_string_lossy(),
        own,
        "the link keeps the spelling it was found in: {report}"
    );
    let program = Path::new(cmd.get_program());
    assert!(
        paddock_env::winpath::same(program, link, Os::current()),
        "a bare rustc starts {}, not the link: {report}",
        program.display()
    );
}

/// `path_report()` names the program each toolchain link stands for, as recorded when the link was made (on Windows a hard link
/// or a copy does not name it).
#[test]
fn the_path_report_names_each_links_program() {
    use paddock_env::Os;
    let d = paddock_testkit::tempdir();
    let tc = d.path().join("toolchain");
    std::fs::create_dir(&tc).unwrap();
    let rustc = tc.join(if cfg!(windows) { "rustc.exe" } else { "rustc" });
    std::fs::write(&rustc, "").unwrap();
    let m = machine_with(&tc, &d.path().join("links"));
    let cmd = TestCommand::on("git", &m);
    let report = cmd.path_report();
    let own = format!("rustc{}", std::env::consts::EXE_SUFFIX);
    let (link, target) = report
        .targets
        .iter()
        .find(|(l, _)| l.file_name().is_some_and(|n| n == own.as_str()))
        .unwrap_or_else(|| panic!("no recorded rustc link: {report}"));
    assert!(report.toolchain.contains(link), "{report}");
    assert!(
        paddock_env::winpath::same(target, &rustc, Os::current()),
        "{} is not {}: {report}",
        target.display(),
        rustc.display()
    );
    assert!(
        report
            .to_string()
            .contains(&format!("{} -> {}", link.display(), target.display())),
        "{report}"
    );
}

/// A toolchain's own `bin` (it holds `rustc_driver-*.dll`) goes on a child's PATH as it is, and a bare `rustc` starts from it:
/// its programs load their DLLs from their own folder, so a link or copy elsewhere fails with STATUS_DLL_NOT_FOUND (the full
/// paddock-dev suite on the test VM, whose toolchain has no rustup: `rustc -vV` exited 0xC0000135 in 13 tests).
#[cfg(windows)]
#[test]
fn a_toolchains_own_bin_goes_on_the_path_as_it_is_and_rustc_starts_from_it() {
    let d = paddock_testkit::tempdir();
    let bin = d.path().join("toolchain\\bin");
    std::fs::create_dir_all(&bin).unwrap();
    for f in [
        "rustc.exe",
        "cargo.exe",
        "rustc_driver-0123456789abcdef.dll",
    ] {
        std::fs::write(bin.join(f), "").unwrap();
    }
    let mut m = machine_with(&bin, &d.path().join("links"));
    m.cargo = Some(bin.join("cargo.exe"));
    let cmd = TestCommand::on("rustc", &m);
    let report = cmd.path_report();
    let program = Path::new(cmd.get_program());
    assert!(
        paddock_env::winpath::same(program, &bin.join("rustc.exe"), paddock_env::Os::Windows),
        "a bare rustc starts {}: {report}",
        program.display()
    );
    assert!(
        report
            .child
            .iter()
            .any(|p| paddock_env::winpath::same(p, &bin, paddock_env::Os::Windows)),
        "{report}"
    );
    assert!(
        !report.toolchain.iter().any(|l| l
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("rustc.exe"))),
        "rustc.exe was linked away from its DLLs: {report}"
    );
}
