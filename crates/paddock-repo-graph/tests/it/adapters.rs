//! The git and cargo adapters against a throwaway repository and this crate's own workspace.
#![cfg(all(feature = "git-history", feature = "cargo-metadata"))]
use paddock_repo_graph::{CargoCli, CargoMetadata, GitHistory, GraphError, History};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const T: Duration = Duration::from_secs(60);

fn git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env(
            "GIT_CONFIG_SYSTEM",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

#[test]
fn git_history_lists_diffs_shows_and_logs() {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-q", "-b", "main"]);
    std::fs::create_dir(d.path().join("sub")).unwrap();
    std::fs::write(d.path().join("sub/a.txt"), "one\n").unwrap();
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-q", "-m", "feat: first"]);
    std::fs::write(d.path().join("sub/a.txt"), "two\n").unwrap();
    std::fs::write(d.path().join("b.txt"), "b\n").unwrap();
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-q", "-m", "fix: second"]);
    let h = GitHistory::new(d.path(), T);
    assert_eq!(h.tracked_files().unwrap(), ["b.txt", "sub/a.txt"]);
    assert_eq!(
        h.changed_files("HEAD~1", "HEAD").unwrap(),
        ["b.txt", "sub/a.txt"]
    );
    assert_eq!(
        h.file_at("HEAD~1", "sub/a.txt").unwrap().as_deref(),
        Some("one\n")
    );
    assert_eq!(h.file_at("HEAD~1", "b.txt").unwrap(), None);
    let log = h.log("HEAD", 5).unwrap();
    assert_eq!(
        log.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(),
        ["fix: second", "feat: first"]
    );
    assert_eq!(h.log("HEAD", 1).unwrap().len(), 1);
    assert!(matches!(
        h.changed_files("nope", "HEAD"),
        Err(GraphError::History(_))
    ));
    assert!(matches!(h.log("nope", 1), Err(GraphError::History(_))));
    let outside = tempfile::tempdir().unwrap();
    assert!(
        GitHistory::new(outside.path().join("missing"), T)
            .tracked_files()
            .is_err()
    );
    let none = GitHistory::new(d.path(), T).with_program("paddock-no-such-git");
    for r in [
        none.tracked_files().map(drop),
        none.file_at("HEAD", "b.txt").map(drop),
        none.log("HEAD", 1).map(drop),
    ] {
        assert!(r.is_err_and(|e| e.to_string().contains("not available")));
    }
}

#[test]
fn cargo_cli_reads_a_workspace_and_reports_failures() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("Cargo.toml"),
        "[workspace]\nmembers=[\"m\"]\nresolver=\"3\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(d.path().join("m/src")).unwrap();
    std::fs::write(
        d.path().join("m/Cargo.toml"),
        "[package]\nname=\"m\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    std::fs::write(d.path().join("m/src/lib.rs"), "").unwrap();
    let cli = CargoCli::new(d.path(), T);
    let json = cli.metadata("Cargo.toml").unwrap();
    assert!(json.contains("\"name\":\"m\""));
    std::fs::write(d.path().join("m/Cargo.toml"), "[package\n").unwrap();
    let err = cli.metadata("Cargo.toml").unwrap_err();
    assert!(matches!(err, GraphError::Metadata { .. }));
    let err = CargoCli::new(d.path(), T)
        .with_program("paddock-no-such-cargo")
        .metadata("Cargo.toml")
        .unwrap_err();
    assert!(err.to_string().contains("not available"));
    let err = cli.metadata("nope/Cargo.toml").unwrap_err();
    assert!(matches!(err, GraphError::Metadata { .. }));
}
