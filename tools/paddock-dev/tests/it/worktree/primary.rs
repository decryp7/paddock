//! `check primary`: the probe and its positive control (ADR 0083 §3, ER-33).
use super::{Fixture, Run};
use std::path::Path;

fn p(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn ids(r: &Run, severity: &str) -> Vec<String> {
    r.json["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["severity"] == severity)
        .map(|f| f["id"].as_str().unwrap().to_string())
        .collect()
}

fn probe(fx: &Fixture, extra: &[&str]) -> Run {
    let mut args = vec!["check", "primary"];
    args.extend(extra);
    fx.dev(&args)
}

#[test]
fn a_healthy_primary_passes_and_says_liveness_is_not_measured() {
    let fx = Fixture::new();
    let r = probe(&fx, &[]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert_eq!(r.json["healthy"], true);
    assert_eq!(r.json["liveness"], "not measured");
    assert!(r.json["findings"].as_array().unwrap().is_empty());
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(["check", "primary"])
        .current_dir(&fx.primary)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("liveness: not measured"), "{text}");
    assert!(text.contains("ok"), "{text}");
}

#[test]
fn the_positive_control_sees_a_stray_commit_a_tracked_change_an_unclaimed_lock_and_an_orphan_entry() {
    let fx = Fixture::new();
    // 1: a stray commit on main (the incident that started ADR 0083).
    std::fs::write(fx.primary.join("stray.txt"), "x").unwrap();
    fx.git(&fx.primary, &["add", "stray.txt"]);
    fx.git(&fx.primary, &["commit", "-q", "-m", "stray"]);
    // 2: a tracked change.
    std::fs::write(fx.primary.join("README.md"), "edited\n").unwrap();
    // 3: a locked worktree no agent claimed.
    let unclaimed = fx.add("unclaimed");
    // 4: an owner-map entry whose worktree is not locked (here: gone).
    let orphan = fx.path("orphan");
    let a = fx.dev(&[
        "worktree",
        "add",
        p(&orphan),
        "-b",
        "orphan",
        "origin/main",
        "--owner",
        "a:b",
        "--agent",
        "agent-O",
    ]);
    assert_eq!(a.code, 0, "{}", a.err);
    fx.git(&fx.primary, &["worktree", "unlock", p(&orphan)]);
    fx.git(&fx.primary, &["worktree", "remove", p(&orphan)]);
    let r = probe(&fx, &[]);
    assert_eq!(r.code, 5, "{} {}", r.json, r.err);
    assert_eq!(r.json["healthy"], false);
    let errors = ids(&r, "error");
    for want in ["local-commits", "tracked", "owner-missing", "owner-orphan"] {
        assert!(errors.iter().any(|e| e == want), "{want} missing from {errors:?}: {}", r.json);
    }
    let text = std::process::Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(["check", "primary"])
        .current_dir(&fx.primary)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(text.contains("::error::local-commits") && text.contains("Next:"), "{text}");
    assert!(text.contains(p(&unclaimed)) && text.contains(p(&orphan)), "{text}");
}

#[test]
fn a_primary_on_another_branch_or_detached_is_reported() {
    let fx = Fixture::new();
    fx.git(&fx.primary, &["switch", "-q", "-c", "feature"]);
    let r = probe(&fx, &[]);
    assert_eq!(r.code, 5);
    assert!(ids(&r, "error").contains(&"branch".to_string()), "{}", r.json);
    fx.git(&fx.primary, &["switch", "-q", "--detach", "main"]);
    let r = probe(&fx, &[]);
    assert_eq!(r.code, 5);
    assert!(ids(&r, "error").contains(&"branch".to_string()), "{}", r.json);
}

#[test]
fn local_main_behind_origin_is_fine_and_ahead_is_not() {
    let fx = Fixture::new();
    let w = fx.add("w1");
    assert_eq!(fx.claim(&w, "agent-A").code, 0);
    fx.commit_in(&w, "f.txt");
    fx.land("w1");
    assert_eq!(
        probe(&fx, &[]).code,
        0,
        "behind origin/main is the normal state between fast-forwards"
    );
    fx.git(&fx.primary, &["merge", "-q", "--ff-only", "origin/main"]);
    assert_eq!(probe(&fx, &[]).code, 0);
}

#[test]
fn a_missing_origin_is_not_measured_and_does_not_pass() {
    let fx = Fixture::new();
    fx.git(&fx.primary, &["remote", "remove", "origin"]);
    let r = probe(&fx, &[]);
    assert_eq!(r.code, 5, "{}", r.json);
    assert_eq!(ids(&r, "not measured"), vec!["local-commits".to_string()], "{}", r.json);
    assert_eq!(r.json["healthy"], false);
}

#[test]
fn a_half_done_merge_and_a_sparse_primary_are_reported() {
    let fx = Fixture::new();
    fx.git(&fx.primary, &["checkout", "-q", "-b", "other"]);
    std::fs::write(fx.primary.join("README.md"), "other\n").unwrap();
    fx.git(&fx.primary, &["commit", "-q", "-am", "other"]);
    fx.git(&fx.primary, &["checkout", "-q", "main"]);
    std::fs::write(fx.primary.join("README.md"), "mainline\n").unwrap();
    fx.git(&fx.primary, &["commit", "-q", "-am", "main change"]);
    fx.git(&fx.primary, &["push", "-q", "origin", "main"]);
    fx.git(&fx.primary, &["fetch", "-q", "origin"]);
    assert!(!fx.git_ok(&fx.primary, &["merge", "other"]), "the merge must conflict");
    let r = probe(&fx, &[]);
    assert!(ids(&r, "error").contains(&"in-progress".to_string()), "{}", r.json);
    fx.git(&fx.primary, &["merge", "--abort"]);
    assert_eq!(probe(&fx, &[]).code, 0);
    fx.git(&fx.primary, &["sparse-checkout", "set", "--cone", "nothing"]);
    let r = probe(&fx, &[]);
    assert!(ids(&r, "error").contains(&"sparse".to_string()), "{}", r.json);
}

#[test]
fn a_sparse_agent_worktree_does_not_make_the_primary_sparse() {
    let fx = Fixture::new();
    let path = fx.path("sp");
    let r = fx.dev(&[
        "worktree",
        "add",
        p(&path),
        "-b",
        "sp",
        "origin/main",
        "--owner",
        "a:b",
        "--sparse",
        "nothing",
        "--agent",
        "agent-A",
    ]);
    assert_eq!(r.code, 0, "{}", r.err);
    let r = probe(&fx, &[]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
}

#[test]
fn untracked_entries_are_listed_unless_allowed_and_never_fail_the_probe() {
    let fx = Fixture::new();
    for d in [".serena", "node_modules", ".claude/densepack-vault"] {
        std::fs::create_dir_all(fx.primary.join(d)).unwrap();
        std::fs::write(fx.primary.join(d).join("x"), "x").unwrap();
    }
    std::fs::write(fx.primary.join("stray.whl"), "x").unwrap();
    std::fs::create_dir_all(fx.primary.join("extra")).unwrap();
    std::fs::write(fx.primary.join("extra/y"), "y").unwrap();
    let r = probe(&fx, &[]);
    assert_eq!(r.code, 0, "{}", r.json);
    assert_eq!(ids(&r, "warning"), vec!["untracked".to_string()]);
    let msg = r.json["findings"][0]["message"].as_str().unwrap().to_string();
    assert!(msg.contains("stray.whl") && msg.contains("extra/"), "{msg}");
    assert!(
        !msg.contains(".serena") && !msg.contains("node_modules") && !msg.contains("densepack"),
        "{msg}"
    );
    let r = probe(&fx, &["--allow-untracked", "stray.whl", "--allow-untracked", "extra/"]);
    assert!(r.json["findings"].as_array().unwrap().is_empty(), "{}", r.json);
}

#[test]
fn a_locked_worktree_without_a_note_is_an_error() {
    let fx = Fixture::new();
    let path = fx.path("bare");
    fx.git(&fx.primary, &["worktree", "add", "-b", "bare", p(&path), "origin/main"]);
    fx.git(&fx.primary, &["worktree", "lock", p(&path)]);
    assert_eq!(fx.claim(&path, "agent-A").code, 0);
    let r = probe(&fx, &[]);
    assert_eq!(ids(&r, "error"), vec!["owner-note-missing".to_string()], "{}", r.json);
    // git's own placeholder for `add --lock` without a reason counts as no note too.
    let other = fx.path("placeholder");
    fx.git(
        &fx.primary,
        &["worktree", "add", "--lock", "-b", "placeholder", p(&other), "origin/main"],
    );
    assert_eq!(fx.claim(&other, "agent-B").code, 0);
    assert_eq!(ids(&probe(&fx, &[]), "error").len(), 2);
}

#[test]
fn liveness_is_measured_only_against_a_list_and_a_dead_agent_is_a_warning() {
    let fx = Fixture::new();
    let a = fx.add("w1");
    let b = fx.add("w2");
    assert_eq!(fx.claim(&a, "agent-A").code, 0);
    assert_eq!(fx.claim(&b, "agent-B").code, 0);
    let without = probe(&fx, &[]);
    assert_eq!((without.code, without.json["liveness"].clone()), (0, "not measured".into()));
    assert!(ids(&without, "warning").is_empty(), "no lock is called stale without a list");
    let live = fx.root.join("live.txt");
    std::fs::write(&live, "# live agents\nagent-A\n\n").unwrap();
    let with = probe(&fx, &["--live", p(&live)]);
    assert_eq!(with.code, 0, "{}", with.json);
    assert_eq!(with.json["liveness"], "measured");
    assert_eq!(ids(&with, "warning"), vec!["agent-not-live".to_string()]);
    let f = &with.json["findings"][0];
    assert!(
        f["message"].as_str().unwrap().contains("agent-B") && f["message"].as_str().unwrap().contains(p(&b)),
        "{f}"
    );
    let missing = probe(&fx, &["--live", p(&fx.root.join("nope.txt"))]);
    assert_eq!(missing.code, 1, "{}", missing.err);
}

#[test]
fn an_old_lock_is_a_warning_with_the_next_step() {
    let fx = Fixture::new();
    let path = fx.path("old");
    let note = "owner=implementer:old; branch=old; base=abc; created=2026-01-01T00:00:00Z; by=coordinator";
    fx.git(
        &fx.primary,
        &["worktree", "add", "--lock", "--reason", note, "-b", "old", p(&path), "origin/main"],
    );
    assert_eq!(fx.claim(&path, "agent-A").code, 0);
    let r = probe(&fx, &[]);
    assert_eq!(r.code, 0, "{}", r.json);
    assert_eq!(ids(&r, "warning"), vec!["lock-old".to_string()]);
    assert!(r.json["findings"][0]["next"].as_str().unwrap().contains("release"));
}

#[test]
fn the_probe_checks_the_primary_even_when_run_from_a_worktree() {
    let fx = Fixture::new();
    let w = fx.add("w1");
    assert_eq!(fx.claim(&w, "agent-A").code, 0);
    std::fs::write(fx.primary.join("README.md"), "edited\n").unwrap();
    let r = fx.dev_in(&w, &["check", "primary"]);
    assert_eq!(r.code, 5, "{}", r.json);
    assert_eq!(r.json["primary"], p(&fx.primary));
    assert!(ids(&r, "error").contains(&"tracked".to_string()));
}

#[test]
fn a_corrupt_owner_map_is_not_measured_not_a_pass() {
    let fx = Fixture::new();
    std::fs::create_dir_all(fx.primary.join(".git/paddock")).unwrap();
    std::fs::write(fx.primary.join(".git/paddock/owners.json"), "garbage").unwrap();
    let r = probe(&fx, &[]);
    assert_eq!(r.code, 5, "{}", r.json);
    assert_eq!(ids(&r, "not measured"), vec!["ownership".to_string()]);
}
