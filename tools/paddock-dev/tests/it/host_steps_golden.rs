//! The Windows twin of `host_steps.rs` (and a golden of the Rust output on every OS): the host steps of `dev ci` as real processes
//! with the exact lines and files the shell gave for the same inputs. The parity cases in `host_steps.rs` prove the equality on
//! Unix, where the scripts run; this file keeps the answers as literals so the Windows and Linux legs check them too (ER-9,
//! docs/plans/ci-in-rust.md section 7). The Windows spellings that cannot run elsewhere (junction, drive paths) are checked by the
//! unit tests of `ci_host` with an injected link maker and by the Windows jobs themselves.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

struct Run {
    code: i32,
    stdout: String,
    github_env: String,
}

/// Starts the tool under test with a cleared environment (what the runner gives a step, no more) inside `dir`.
#[allow(clippy::disallowed_methods)]
fn dev(dir: &Path, args: &[&str], vars: &[(&str, String)]) -> Run {
    let env_file: PathBuf = dir.join("github-env");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_paddock-dev"));
    cmd.arg("ci").args(args).current_dir(dir).env_clear();
    for key in [
        "PATH",
        "SystemRoot",
        "SYSTEMROOT",
        "TEMP",
        "TMP",
        "NUMBER_OF_PROCESSORS",
        "COMSPEC",
        "PATHEXT",
    ] {
        if let Ok(v) = std::env::var(key) {
            cmd.env(key, v);
        }
    }
    cmd.env("GITHUB_ENV", &env_file)
        .env("HOME", dir.join("home"))
        .env("USERPROFILE", dir.join("home"));
    cmd.env(
        "PADDOCK_CI_TABLES",
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/ci"),
    );
    cmd.envs(vars.iter().map(|(k, v)| (*k, v)));
    let out = cmd.output().unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"),
        github_env: std::fs::read_to_string(&env_file).unwrap_or_default().replace("\r\n", "\n"),
    }
}

fn scratch() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let root = paddock_env::winpath::real(d.path()).unwrap();
    // `canonicalize` gives `\\?\C:\...` on Windows; the tool prints plain paths.
    let root = PathBuf::from(root.display().to_string().trim_start_matches(r"\\?\"));
    std::fs::create_dir_all(root.join("home")).unwrap();
    (d, root)
}

fn forward(p: &Path) -> String {
    paddock_testkit::path::text(p)
}

#[test]
fn a_linux_container_leaves_the_workspace_and_a_missing_mount_is_a_notice() {
    let (_d, root) = scratch();
    let linux = [("RUNNER_OS", "Linux".to_string()), ("GITHUB_WORKSPACE", forward(&root))];
    let r = dev(&root, &["stable-workspace", "--name", "x"], &linux);
    assert_eq!((r.code, r.stdout.as_str()), (0, ""));
    let mut vars = linux.to_vec();
    vars.push(("PADDOCK_CI_MOUNT", forward(&root.join("no-mount"))));
    let r = dev(&root, &["node-caches", "--angular-dir", "ui", "--name", "e2e-1"], &vars);
    assert_eq!(r.code, 0);
    assert_eq!(
        r.stdout,
        "::notice::no /sccache mount in this container; the npm and Angular caches live and die with the job.\n"
    );
    assert_eq!(r.github_env, "", "nothing is exported without the mount");
}

#[test]
fn build_folder_in_a_container_builds_in_target_and_a_config_without_the_include_is_refused() {
    let (_d, root) = scratch();
    let ws = root.join("ws");
    std::fs::create_dir_all(ws.join(".cargo")).unwrap();
    std::fs::write(
        ws.join(".cargo/config.toml"),
        "include = [{ path = \"ci.toml\", optional = true }]\n",
    )
    .unwrap();
    std::fs::write(ws.join(".cargo/ci.toml"), "# left by an earlier job\n").unwrap();
    let wsarg = forward(&ws);
    let r = dev(&root, &["build-folder", &wsarg, "spotter-rust"], &[("RUNNER_OS", "Linux".into())]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert!(!ws.join(".cargo/ci.toml").exists(), "an old ci.toml is removed in a container");
    let canon = paddock_env::winpath::real_for_tools(&ws).unwrap();
    assert_eq!(r.github_env, format!("PADDOCK_BUILD_FOLDER={canon}/target\n"));
    assert_eq!(r.stdout, format!("build folder: {canon}/target\n"));
    std::fs::write(ws.join(".cargo/config.toml"), "[alias]\n").unwrap();
    let r = dev(&root, &["build-folder", &wsarg, "spotter-rust"], &[("RUNNER_OS", "Linux".into())]);
    assert_eq!(r.code, 1);
    assert!(
        r.stdout.starts_with("::error::build-folder: ")
            && r.stdout.contains("must include the job's build folder")
            && r.stdout.contains(" Next step: "),
        "{}",
        r.stdout
    );
    let r = dev(&root, &["build-folder", &wsarg, "a b"], &[("RUNNER_OS", "Linux".into())]);
    assert_eq!(r.code, 1);
    assert!(r.stdout.contains("may only hold letters, digits"), "{}", r.stdout);
}

#[test]
fn prune_folder_caps_a_folder_incremental_caches_first_and_refuses_a_bad_cap() {
    let (_d, root) = scratch();
    let dir = root.join("ci").join("build");
    for (rel, kb) in [("debug/incremental", 400usize), ("debug/deps", 50)] {
        std::fs::create_dir_all(dir.join(rel)).unwrap();
        let data: Vec<u8> = (0..kb * 1024).map(|i| (i.wrapping_mul(2_654_435_761usize) >> 7) as u8).collect();
        std::fs::write(dir.join(rel).join("blob"), data).unwrap();
    }
    let a = forward(&dir);
    let r = dev(&root, &["prune-folder", &a, "300"], &[("PADDOCK_PRUNE_UNIT", "kb".into())]);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("removing incremental caches"), "{}", r.stdout);
    assert!(!dir.join("debug/incremental").exists() && dir.join("debug/deps/blob").exists());
    assert_eq!(dev(&root, &["prune-folder", &a, "many"], &[]).code, 2);
}

#[test]
fn host_cleanup_refuses_bad_arguments_with_exit_2_and_sweeps_only_its_prefix_of_the_jobs_temp() {
    let (_d, root) = scratch();
    let tmp = root.join("jobtemp");
    for d in ["pitwall-abc", "other-job"] {
        std::fs::create_dir_all(tmp.join(d)).unwrap();
    }
    let vars = [
        ("RUNNER_OS", "macOS".to_string()),
        ("PADDOCK_JOB_TEMP", forward(&tmp)),
        ("RUNNER_TEMP", forward(&tmp)),
    ];
    assert_eq!(dev(&root, &["host-cleanup"], &vars).code, 2);
    assert_eq!(dev(&root, &["host-cleanup", "--id", "Bad Id"], &vars).code, 2);
    assert_eq!(dev(&root, &["host-cleanup", "--id", "x", "--temp-prefix", "../"], &vars).code, 2);
    let r = dev(&root, &["host-cleanup", "--id", "pitwall"], &vars);
    assert_eq!(r.code, 0, "{}", r.stdout);
    assert!(!tmp.join("pitwall-abc").exists() && tmp.join("other-job").exists());
}

#[test]
fn sccache_stats_without_sccache_prints_nothing_and_host_tools_without_a_pin_on_the_path_fails_with_one_error() {
    let (_d, root) = scratch();
    // An empty PATH folder: no sccache, no cargo-nextest.
    let empty = root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let r = dev(&root, &["sccache-stats"], &[("PATH", forward(&empty))]);
    assert_eq!((r.code, r.stdout.as_str()), (0, ""));
    let r = dev(&root, &["host-tools"], &[("RUNNER_OS", "Linux".into()), ("PATH", forward(&empty))]);
    assert_eq!(r.code, 1);
    let errors: Vec<&str> = r.stdout.lines().filter(|l| l.starts_with("::error::")).collect();
    assert_eq!(errors.len(), 1, "{}", r.stdout);
    assert!(
        errors[0].starts_with("::error::cargo-nextest is missing on this runner, the pin is ") && errors[0].contains(" Next step: "),
        "{}",
        errors[0]
    );
}
