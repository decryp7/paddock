//! list and clean: only unlocked, merged, clean, unused worktrees go; never `rm`, never a forced removal.
use super::Fixture;
use serde_json::Value;
use std::path::Path;
use std::process::{Child, Command, Stdio};

fn p(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn row<'a>(list: &'a Value, path: &Path) -> &'a Value {
    list["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["path"] == p(path))
        .unwrap_or_else(|| panic!("{path:?} not listed in {list}"))
}

/// Makes the worktree `name` with one commit, unlocks it and (when `merged`) lands the commit on origin/main.
fn finished(fx: &Fixture, name: &str, merged: bool) -> std::path::PathBuf {
    let path = fx.add(name);
    fx.commit_in(&path, &format!("{name}.txt"));
    if merged {
        fx.land(name);
    }
    assert_eq!(fx.dev(&["worktree", "release", p(&path)]).code, 0);
    path
}

/// A process whose current folder is `dir`, ended when dropped.
struct Inside(Child);
impl Inside {
    // Starts the tool under test, git or a helper by program name (a throw-away fixture), never a process by pid (C53).
    #[allow(clippy::disallowed_methods)]
    fn at(dir: &Path) -> Inside {
        Inside(
            Command::new("sleep")
                .arg("120")
                .current_dir(dir)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
}
impl Drop for Inside {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn list_reports_every_worktree_with_owner_lock_merged_and_dirty() {
    let fx = Fixture::new();
    let locked = fx.add("locked");
    assert_eq!(fx.claim(&locked, "agent-A").code, 0);
    fx.commit_in(&locked, "a.txt");
    let merged = finished(&fx, "done", true);
    let unmerged = finished(&fx, "open", false);
    let empty = fx.add("fresh");
    assert_eq!(fx.dev(&["worktree", "release", p(&empty)]).code, 0);
    std::fs::write(unmerged.join("dirty.txt"), "x").unwrap();
    let r = fx.dev(&["worktree", "list"]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert_eq!(r.json["base"], "origin/main");
    let list = &r.json;
    assert_eq!(list["worktrees"].as_array().unwrap().len(), 5);
    let primary = row(list, &fx.primary);
    assert_eq!(
        (primary["primary"].clone(), primary["branch"].clone(), primary["locked"].clone()),
        (true.into(), "main".into(), false.into())
    );
    let l = row(list, &locked);
    assert_eq!(l["locked"], true);
    assert_eq!(l["agent_id"], "agent-A");
    assert_eq!(l["owner_note"], "implementer:locked");
    assert_eq!(l["merged"], "no");
    assert_eq!(l["stale_lock"], false);
    assert!(l["lock_age_secs"].as_u64().unwrap() < 600);
    let m = row(list, &merged);
    assert_eq!(
        (m["locked"].clone(), m["merged"].clone(), m["dirty"].clone(), m["agent_id"].clone()),
        (false.into(), "yes".into(), false.into(), Value::Null)
    );
    let u = row(list, &unmerged);
    assert_eq!((u["merged"].clone(), u["dirty"].clone()), ("no".into(), true.into()));
    assert_eq!(row(list, &empty)["merged"], "empty");
}

#[test]
fn list_shows_a_detached_review_copy_and_a_missing_folder() {
    let fx = Fixture::new();
    let copy = fx.path("copy");
    let r = fx.dev(&[
        "worktree",
        "add",
        "copy",
        "--base",
        "origin/main",
        "--detach",
        "--owner",
        "reviewer:t",
        "--agent",
        "agent-R",
    ]);
    assert_eq!(r.code, 0, "{}", r.err);
    let gone = fx.add("gone");
    std::fs::remove_dir_all(&gone).unwrap();
    let list = fx.dev(&["worktree", "list"]).json;
    let c = row(&list, &copy);
    assert_eq!(
        (c["detached"].clone(), c["branch"].clone(), c["merged"].clone()),
        (true.into(), Value::Null, "yes".into())
    );
    let g = row(&list, &gone);
    assert_eq!((g["prunable"].clone(), g["dirty"].clone()), (true.into(), Value::Null));
}

#[test]
fn list_flags_a_lock_older_than_seven_days_without_removing_it() {
    let fx = Fixture::new();
    let path = fx.path("old");
    let note = "owner=implementer:old; branch=old; base=abc; created=2026-01-01T00:00:00Z; by=coordinator";
    fx.git(
        &fx.primary,
        &["worktree", "add", "--lock", "--reason", note, "-b", "old", p(&path), "origin/main"],
    );
    let list = fx.dev(&["worktree", "list"]).json;
    let o = row(&list, &path);
    assert_eq!(o["stale_lock"], true);
    assert!(o["lock_age_secs"].as_u64().unwrap() > 7 * 24 * 3600);
    assert_eq!(fx.dev(&["worktree", "clean"]).code, 0);
    assert!(path.exists(), "an old lock is reported, never removed");
}

#[test]
fn clean_removes_only_unlocked_merged_clean_unused_worktrees_and_their_branch() {
    let fx = Fixture::new();
    let gone = finished(&fx, "gone", true);
    let locked = fx.add("locked");
    fx.commit_in(&locked, "l.txt");
    fx.land("locked");
    let unmerged = finished(&fx, "unmerged", false);
    let dirty = finished(&fx, "dirty", true);
    std::fs::write(dirty.join("wip.txt"), "x").unwrap();
    let fresh = fx.add("fresh");
    assert_eq!(fx.dev(&["worktree", "release", p(&fresh)]).code, 0);
    let busy = finished(&fx, "busy", true);
    let _proc = Inside::at(&busy);
    let r = fx.dev(&["worktree", "clean"]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    let removed: Vec<&str> = r.json["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["path"].as_str().unwrap())
        .collect();
    assert_eq!(removed, vec![p(&gone)], "{}", r.json);
    assert!(!gone.exists());
    assert!(
        !fx.git_ok(&fx.primary, &["rev-parse", "--verify", "-q", "refs/heads/gone"]),
        "its merged branch goes with it"
    );
    for (kept, why) in [
        (&locked, "locked"),
        (&unmerged, "not merged"),
        (&dirty, "uncommitted"),
        (&fresh, "no commits yet"),
        (&busy, "current folder"),
    ] {
        assert!(kept.exists(), "{kept:?} must stay");
        let entry = r.json["kept"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["path"] == p(kept))
            .unwrap_or_else(|| panic!("{kept:?} not in kept: {}", r.json));
        assert!(entry["reason"].as_str().unwrap().contains(why), "{kept:?}: {entry}");
    }
    assert!(dirty.join("wip.txt").exists(), "uncommitted work survives");
    assert!(fx.lock_reason(&locked).is_some());
}

#[test]
fn clean_names_the_owner_note_of_a_locked_worktree_it_keeps() {
    let fx = Fixture::new();
    let w = fx.add("w1");
    assert_eq!(fx.claim(&w, "agent-A").code, 0);
    fx.commit_in(&w, "a.txt");
    fx.land("w1");
    let r = fx.dev(&["worktree", "clean"]);
    let reason = r.json["kept"].as_array().unwrap().iter().find(|v| v["path"] == p(&w)).unwrap()["reason"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(reason.contains("implementer:w1") && reason.contains("agent-A"), "{reason}");
    assert!(r.json["removed"].as_array().unwrap().is_empty());
}

#[test]
fn clean_dry_run_changes_nothing() {
    let fx = Fixture::new();
    let gone = finished(&fx, "gone", true);
    let r = fx.dev(&["worktree", "clean", "--dry-run"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.json["dry_run"], true);
    assert_eq!(r.json["removed"][0]["path"], p(&gone));
    assert!(gone.exists());
    assert!(fx.git_ok(&fx.primary, &["rev-parse", "--verify", "-q", "refs/heads/gone"]));
}

#[test]
fn clean_keeps_a_worktree_whose_folder_is_missing_and_never_touches_the_primary() {
    let fx = Fixture::new();
    let gone = finished(&fx, "gone", true);
    std::fs::remove_dir_all(&gone).unwrap();
    let r = fx.dev(&["worktree", "clean"]);
    assert_eq!(r.code, 0, "{}", r.err);
    let reason = r.json["kept"].as_array().unwrap().iter().find(|v| v["path"] == p(&gone)).unwrap()["reason"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(reason.contains("prune"), "{reason}");
    assert!(fx.primary.join("README.md").exists());
}

#[test]
fn clean_removes_a_released_detached_copy_at_the_base() {
    let fx = Fixture::new();
    let copy = fx.path("copy");
    assert_eq!(
        fx.dev(&[
            "worktree",
            "add",
            "copy",
            "--base",
            "origin/main",
            "--detach",
            "--owner",
            "reviewer:t"
        ])
        .code,
        0
    );
    assert_eq!(fx.dev(&["worktree", "release", p(&copy)]).code, 0);
    let r = fx.dev(&["worktree", "clean"]);
    assert_eq!(r.json["removed"][0]["path"], p(&copy), "{}", r.json);
    assert!(!copy.exists());
}

#[test]
fn clean_with_an_unknown_base_keeps_everything() {
    let fx = Fixture::new();
    let gone = finished(&fx, "gone", true);
    let r = fx.dev(&["worktree", "clean", "--base", "origin/nope"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(gone.exists());
    assert!(r.json["removed"].as_array().unwrap().is_empty());
}

#[test]
fn clean_deletes_a_branch_only_at_the_head_it_looked_at() {
    // The branch moves between the snapshot and the delete (the seam runs `update-ref` first): git refuses, the commit is safe.
    let fx = Fixture::new();
    let gone = finished(&fx, "gone", true);
    let moved_to = fx.git(
        &fx.primary,
        &["commit-tree", "-p", "origin/main", "-m", "later", "origin/main^{tree}"],
    );
    let path_env = fx.fake_git(
        "move",
        &format!(r#"case "$*" in *"update-ref -d"*) "$REAL" update-ref refs/heads/gone {moved_to};; esac"#),
    );
    let r = run_with_path(&fx, &["worktree", "clean"], &path_env);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert!(!gone.exists(), "the worktree itself was removed");
    assert_eq!(
        fx.git(&fx.primary, &["rev-parse", "refs/heads/gone"]),
        moved_to,
        "the moved branch survives"
    );
    let kept = r.json["removed"][0]["branch"].as_str();
    assert_eq!(kept, Some("gone"));
}

fn run_with_path(fx: &Fixture, args: &[&str], path: &str) -> super::Run {
    let out = fx.cmd(&fx.primary, args).env("PATH", path).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    super::Run {
        code: out.status.code().unwrap(),
        json: serde_json::from_str(text.trim()).unwrap_or(Value::Null),
        err: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}
