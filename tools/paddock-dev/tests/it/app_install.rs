//! `dev app bootstrap` and `verify-installed` against the scripts they replaced (`tools/bootstrap.sh`, `tools/verify-installed.sh` and
//! their tests in `tools/test`, and the apps' own bootstrap and verify scripts): every program the command probes is a shim on a PATH
//! of its own, so nothing on the machine is touched. The checklist lines, the exit codes and the programs called must be what the
//! scripts' tests expected. Unix only: the shims are shell scripts.
// windows-twin: none: waived until 2026-12-31; the shim programs are shell scripts and a Windows twin needs a compiled shim (the unit tests of src/app_install.rs run on every OS)
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_testkit::TestCommand;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn shim(bin: &Path, name: &str, body: &str) {
    let p = bin.join(name);
    std::fs::write(
        &p,
        format!("#!/bin/sh\necho \"{name} $*\" >> '{}/calls.log'\n{body}\n", bin.display()),
    )
    .unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn calls(bin: &Path) -> Vec<String> {
    std::fs::read_to_string(bin.join("calls.log"))
        .map(|t| t.lines().map(str::to_string).collect())
        .unwrap_or_default()
}

/// A repository with `.nvmrc` and one app whose `install` section is `install`.
fn repo(d: &Path, install: &str) -> PathBuf {
    let root = d.join("repo");
    std::fs::create_dir_all(root.join("apps/demo")).unwrap();
    std::fs::write(root.join(".nvmrc"), "24\n").unwrap();
    std::fs::write(
        root.join("apps/demo/paddock.ci.json"),
        format!("{{\"cargo\":\".\",\"install\":{install}}}"),
    )
    .unwrap();
    root
}

fn dev(root: &Path, bin: &Path, home: &Path, args: &[&str]) -> (i32, String) {
    let mut cmd = TestCommand::new(env!("CARGO_BIN_EXE_paddock-dev"));
    cmd.env("PATH", bin)
        .env("HOME", home)
        .args(args)
        .args(["--root", root.to_str().unwrap()]);
    let o = cmd.output().unwrap();
    (
        o.status.code().unwrap(),
        format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
    )
}

fn mac_machine(bin: &Path, macos: &str) {
    shim(bin, "sw_vers", &format!("echo {macos}"));
    shim(bin, "xcode-select", "echo /Library/Developer/CommandLineTools");
    shim(bin, "rustc", "echo 'rustc 1.100.0 (abc 2026-01-01)'");
    shim(bin, "cargo", "echo 'cargo 1.100.0'");
    shim(bin, "node", "echo v24.1.0");
}

#[test]
fn bootstrap_check_passes_on_a_machine_that_has_everything_and_builds_nothing() {
    let d = tempfile::tempdir().unwrap();
    let (bin, home) = (d.path().join("bin"), d.path().join("home"));
    std::fs::create_dir_all(&bin).unwrap();
    let root = repo(d.path(), r#"{"name":"Demo"}"#);
    mac_machine(&bin, "14.2");
    let (code, text) = dev(&root, &bin, &home, &["app", "bootstrap", "demo", "--check", "--os", "macos"]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("Demo macos bootstrap") && text.contains("ok    macOS 14.2 (13 or newer)") && text.contains("ok    Node v24.1.0"),
        "{text}"
    );
    assert!(
        !calls(&bin).iter().any(|c| c.starts_with("npm") || c.starts_with("npx")),
        "--check builds nothing"
    );
}

#[test]
fn a_missing_tool_is_named_with_what_to_do_and_the_node_major_must_match_nvmrc() {
    let d = tempfile::tempdir().unwrap();
    let (bin, home) = (d.path().join("bin"), d.path().join("home"));
    std::fs::create_dir_all(&bin).unwrap();
    let root = repo(d.path(), r#"{"name":"Demo"}"#);
    mac_machine(&bin, "12.6");
    shim(&bin, "node", "echo v20.1.0");
    std::fs::remove_file(bin.join("cargo")).unwrap();
    let (code, text) = dev(&root, &bin, &home, &["app", "bootstrap", "demo", "--check", "--os", "macos"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("missing  macOS 13 or newer\n      found 12.6;"), "{text}");
    assert!(text.contains("missing  Rust\n      install rustup"), "{text}");
    assert!(
        text.contains("missing  Node 24\n      found Node v20.1.0; install Node 24"),
        "{text}"
    );
    let (ok, _) = dev(
        &root,
        &bin,
        &home,
        &["app", "bootstrap", "demo", "--check", "--os", "macos", "--min-macos", "12"],
    );
    assert_eq!(ok, 1, "Rust and Node are still missing");
}

#[test]
fn linux_checks_the_architecture_and_the_development_packages() {
    let d = tempfile::tempdir().unwrap();
    let (bin, home) = (d.path().join("bin"), d.path().join("home"));
    std::fs::create_dir_all(&bin).unwrap();
    let root = repo(d.path(), r#"{"name":"Demo"}"#);
    mac_machine(&bin, "14.2");
    shim(&bin, "pkg-config", r#"[ "$2" != "librsvg-2.0" ]"#);
    let (code, text) = dev(
        &root,
        &bin,
        &home,
        &["app", "bootstrap", "demo", "--check", "--os", "linux", "--arch", "x86_64"],
    );
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("ok    webkit2gtk-4.1 (development files)")
            && text.contains("missing  librsvg-2.0\n      sudo apt install libwebkit2gtk-4.1-dev"),
        "{text}"
    );
    let (_, odd) = dev(
        &root,
        &bin,
        &home,
        &["app", "bootstrap", "demo", "--check", "--os", "linux", "--arch", "riscv64"],
    );
    assert!(odd.contains("builds for x86_64 and aarch64 only (found riscv64)"), "{odd}");
}

#[test]
fn an_app_without_an_install_section_and_an_unsupported_os_are_one_error_each() {
    let d = tempfile::tempdir().unwrap();
    let (bin, home) = (d.path().join("bin"), d.path().join("home"));
    std::fs::create_dir_all(&bin).unwrap();
    let root = repo(d.path(), r#"{"name":"Demo"}"#);
    std::fs::write(root.join("apps/demo/paddock.ci.json"), "{}").unwrap();
    let (code, text) = dev(&root, &bin, &home, &["app", "bootstrap", "demo", "--check", "--os", "macos"]);
    assert_eq!(code, 1);
    assert!(
        text.contains("::error::") && text.contains("no `install` section") && text.contains("Next step:"),
        "{text}"
    );
    let root = repo(d.path(), r#"{"name":"Demo"}"#);
    let (code, text) = dev(&root, &bin, &home, &["app", "bootstrap", "demo", "--check", "--os", "windows"]);
    assert_eq!(code, 1);
    assert!(text.contains("windows is not supported by bootstrap"), "{text}");
}

const INSTALL: &str = r#"{"name":"Demo","service":"demo-service","label":"net.test.demo.service","socket":"demo.sock"}"#;

/// A fake installed Mac: the bundle with the service binary and the plist, a data folder with a log, shims for launchctl and plutil.
fn installed_mac(d: &Path, launchctl: &str, with_service: bool) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let (bin, home) = (d.join("bin"), d.join("home"));
    let bundle = home.join("Apps/Demo.app");
    std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
    std::fs::create_dir_all(bundle.join("Contents/Library/LaunchAgents")).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bundle.join("Contents/Library/LaunchAgents/net.test.demo.service.plist"), "<plist/>").unwrap();
    if with_service {
        let p = bundle.join("Contents/MacOS/demo-service");
        std::fs::write(&p, "#!/bin/sh\necho 'status: ok'\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let data = home.join("Library/Application Support/Demo");
    std::fs::create_dir_all(data.join("logs")).unwrap();
    std::fs::write(data.join("logs/service.log"), "one\ntwo\nthree\n").unwrap();
    shim(&bin, "launchctl", launchctl);
    shim(&bin, "plutil", "");
    shim(&bin, "id", "echo 501");
    (bin, home, bundle, data)
}

#[test]
fn macos_a_healthy_install_passes_and_shows_the_status_and_the_log_tail() {
    let d = tempfile::tempdir().unwrap();
    let root = repo(d.path(), INSTALL);
    let (bin, home, bundle, data) = installed_mac(d.path(), r#"printf 'gui/501/x = {\n\tstate = running\n\tpid = 4242\n}\n'"#, true);
    std::fs::write(data.join("demo.sock"), "").unwrap();
    let (code, text) = dev(
        &root,
        &bin,
        &home,
        &[
            "app",
            "verify-installed",
            "demo",
            "--os",
            "macos",
            "--bundle",
            bundle.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains("ok    app bundle") && text.contains("ok    launchd runs the agent (pid 4242)") && text.contains("status: ok"),
        "{text}"
    );
    assert!(
        text.contains("Service log (last lines of")
            && text.contains("    one\n    two\n    three")
            && text.contains("Everything looks right."),
        "{text}"
    );
    assert!(
        calls(&bin)
            .iter()
            .any(|c| c.starts_with("launchctl print gui/501/net.test.demo.service"))
    );
}

#[test]
fn macos_a_stale_or_unloaded_agent_a_missing_binary_and_a_missing_socket_are_one_problem_each() {
    let d = tempfile::tempdir().unwrap();
    let root = repo(d.path(), INSTALL);
    let (bin, home, bundle, _) = installed_mac(d.path(), r#"printf '\tstate = spawn scheduled\n'"#, false);
    let (code, text) = dev(
        &root,
        &bin,
        &home,
        &[
            "app",
            "verify-installed",
            "demo",
            "--os",
            "macos",
            "--bundle",
            bundle.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 1, "{text}");
    assert!(
        text.contains("fail  launch agent\n        launchd knows it but its state is 'spawn scheduled'"),
        "{text}"
    );
    assert!(
        text.contains("service binary") && text.contains("demo-service is missing; rebuild"),
        "{text}"
    );
    assert!(
        text.contains("fail  service socket") && text.contains("3 problem(s) found."),
        "{text}"
    );
    shim(&bin, "launchctl", "echo 'Could not find service' >&2; exit 113");
    let (_, text) = dev(
        &root,
        &bin,
        &home,
        &[
            "app",
            "verify-installed",
            "demo",
            "--os",
            "macos",
            "--bundle",
            bundle.to_str().unwrap(),
        ],
    );
    assert!(text.contains("not loaded: open Demo > Settings > Service > Turn on"), "{text}");
}

#[test]
fn a_runner_log_row_says_whether_the_server_accepted_the_runner() {
    let d = tempfile::tempdir().unwrap();
    let root = repo(
        d.path(),
        r#"{"name":"Demo","runners":{"dir":"runners","log":"logs/runner.log","accepted":"declare successfully"}}"#,
    );
    let (bin, home, bundle, data) = installed_mac(d.path(), "exit 0", false);
    for (n, text) in [("a", "x\ndeclare successfully\n"), ("b", "waiting\n")] {
        std::fs::create_dir_all(data.join(format!("runners/{n}/logs"))).unwrap();
        std::fs::write(data.join(format!("runners/{n}/logs/runner.log")), text).unwrap();
    }
    let (_, text) = dev(
        &root,
        &bin,
        &home,
        &[
            "app",
            "verify-installed",
            "demo",
            "--os",
            "macos",
            "--bundle",
            bundle.to_str().unwrap(),
        ],
    );
    assert!(
        text.contains("ok    a was accepted by its server") && text.contains("..    b has not been accepted by its server yet"),
        "{text}"
    );
}
