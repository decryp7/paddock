//! `dev ci build-env` against `conformance/ci/build-env.mjs`, the script it replaces (docs/plans/ci-in-rust.md, WP-3): both run on
//! the same inputs and must append the same lines to `$GITHUB_ENV` and print the same. The date is the day the test runs (both read
//! the clock), so the comparison runs twice around a midnight at worst. The probe sees a difference: the commit lower-casing and the
//! `unknown` case are compared too.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    paddock_env::winpath::real(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap()
}

fn run(program: &mut Command, sha: Option<&str>, env_file: &Path) -> (i32, String, String) {
    program.env_clear();
    for k in ["PATH", "HOME", "SystemRoot", "TMPDIR", "TEMP"] {
        if let Ok(v) = std::env::var(k) {
            program.env(k, v);
        }
    }
    program.env("GITHUB_ENV", env_file).current_dir(repo());
    if let Some(s) = sha {
        program.env("GITHUB_SHA", s);
    }
    let out = program
        .output()
        .expect("node and the built tool must be runnable (ER-33: a probe that cannot run is a failure)");
    (
        out.status.code().unwrap_or(-1),
        std::fs::read_to_string(env_file).unwrap_or_default(),
        String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"),
    )
}

#[test]
fn the_rust_command_and_the_script_append_and_print_the_same() {
    let dir = tempfile::tempdir().unwrap();
    for (n, sha) in [
        Some("ABCDEF0123456789abcdef0123456789abcdef01"),
        Some("abcdef0"),
        Some("not a sha"),
        None,
    ]
    .into_iter()
    .enumerate()
    {
        let (a, b) = (dir.path().join(format!("rust{n}")), dir.path().join(format!("node{n}")));
        std::fs::write(&a, "A=1\n").unwrap();
        std::fs::write(&b, "A=1\n").unwrap();
        let rust = run(
            paddock_proc::command(env!("CARGO_BIN_EXE_paddock-dev")).args(["ci", "build-env"]),
            sha,
            &a,
        );
        let node = run(paddock_proc::command("node").arg("conformance/ci/build-env.mjs"), sha, &b);
        assert_eq!(rust, node, "GITHUB_SHA={sha:?}");
        assert!(rust.1.starts_with("A=1\nPADDOCK_BUILD_COMMIT="), "{}", rust.1);
    }
}

#[test]
fn without_a_github_env_file_it_only_prints() {
    let out = paddock_proc::command(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(["ci", "build-env"])
        .env_remove("GITHUB_ENV")
        .env("GITHUB_SHA", "0123456789abcdef")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    assert!(text.starts_with("PADDOCK_BUILD_COMMIT=0123456789ab\nPADDOCK_BUILD_DATE="), "{text}");
}
