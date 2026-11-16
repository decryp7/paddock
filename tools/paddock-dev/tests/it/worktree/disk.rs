//! `dev disk` end to end against the throw-away repositories of [`Fixture`] (ER-14): the owner record `worktree add` writes,
//! the states of real worktrees, what `clean` removes and keeps, and `adopt`.
use super::{Fixture, Run};
use std::path::{Path, PathBuf};

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// `dev disk <args>` over the fixture's folder as the only root, a private index, no live window (so only real signals
/// make a folder live), and a fake free-space program.
fn disk(fx: &Fixture, args: &[&str]) -> Run {
    let index = fx.root.join("index.sqlite");
    let mut all: Vec<&str> = vec!["disk"];
    all.extend_from_slice(args);
    all.extend(["--root", s(&fx.root), "--index", s(&index), "--live-hours", "0", "--jobs", "2"]);
    super::run_of(fx.cmd(&fx.primary, &all).output().unwrap())
}

fn state_of(report: &Run, path: &Path) -> String {
    for d in report.json["disks"].as_array().unwrap() {
        for f in d["folders"].as_array().unwrap() {
            if f["path"] == s(path) {
                return f["state"].as_str().unwrap().to_string();
            }
        }
    }
    panic!("{path:?} is not in the report: {}", report.json);
}

/// A worktree with one commit, released (unlocked, unowned).
fn released(fx: &Fixture, name: &str) -> PathBuf {
    let p = fx.add(name);
    fx.commit_in(&p, &format!("{name}.txt"));
    assert_eq!(fx.dev(&["worktree", "release", s(&p)]).code, 0);
    p
}

#[test]
fn worktree_add_writes_an_owner_record_that_git_does_not_show_as_untracked() {
    let fx = Fixture::new();
    let p = fx.add("rec");
    let text = std::fs::read_to_string(p.join(".paddock-owner.toml")).unwrap();
    assert!(
        text.contains("kind = \"worktree\"") && text.contains("owner = \"implementer:rec\""),
        "{text}"
    );
    assert!(text.contains("branch = \"rec\""), "{text}");
    assert_eq!(
        fx.git(&p, &["status", "--porcelain"]),
        "",
        "the record must not make the worktree dirty"
    );
}

#[test]
fn claim_touches_last_use_but_not_created() {
    let fx = Fixture::new();
    let p = fx.add("touched");
    let rec = p.join(".paddock-owner.toml");
    let old = std::fs::read_to_string(&rec).unwrap().replace(|c: char| c.is_ascii_digit(), "0");
    // Age both times, then claim: last_used moves, created does not.
    let aged = std::fs::read_to_string(&rec)
        .unwrap()
        .lines()
        .map(|l| {
            if l.starts_with("last_used") || l.starts_with("created") {
                format!("{} = \"2020-01-01T00:00:00Z\"", l.split(' ').next().unwrap())
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let _ = old;
    std::fs::write(&rec, aged).unwrap();
    assert_eq!(fx.claim(&p, "implementer:touched").code, 0);
    let after = std::fs::read_to_string(&rec).unwrap();
    assert!(after.contains("created = \"2020-01-01T00:00:00Z\""), "{after}");
    assert!(!after.contains("last_used = \"2020-01-01"), "{after}");
}

#[test]
fn report_gives_each_worktree_its_state_and_clean_removes_only_merged_and_stale_pushed() {
    let fx = Fixture::new();
    // merged: its commit is in origin/main.
    let merged = released(&fx, "merged-one");
    fx.land("merged-one");
    // stale-pushed: clean, pushed to a branch of origin, not merged.
    let pushed = released(&fx, "pushed-one");
    fx.git(&pushed, &["push", "-q", "origin", "pushed-one"]);
    // unsaved: an unpushed commit; another with a dirty file.
    let unpushed = released(&fx, "unpushed-one");
    // Pushed first, so that only the dirty file can make it unsaved (the dirty check must stand on its own).
    let dirty = released(&fx, "dirty-one");
    fx.git(&dirty, &["push", "-q", "origin", "dirty-one"]);
    std::fs::write(dirty.join("scratch.txt"), "wip\n").unwrap();
    // live: still locked.
    let locked = fx.add("locked-one");

    let r = disk(&fx, &["report"]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert_eq!(state_of(&r, &merged), "merged");
    assert_eq!(state_of(&r, &pushed), "stale-pushed");
    assert_eq!(state_of(&r, &unpushed), "unsaved");
    assert_eq!(state_of(&r, &dirty), "unsaved");
    assert_eq!(state_of(&r, &locked), "live");
    assert_eq!(state_of(&r, &fx.primary), "orphan", "the primary has no record");

    // Dry run (the default): nothing goes.
    let dry = disk(&fx, &["clean"]);
    assert_eq!(dry.code, 0, "{} {}", dry.json, dry.err);
    let would: Vec<String> = dry.json["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["path"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(would.len(), 2, "{would:?}");
    for p in [&merged, &pushed, &unpushed, &dirty, &locked, &fx.primary] {
        assert!(p.is_dir(), "{p:?} must survive a dry run");
    }

    // --apply: the two go (through git, which also forgets them), everything else stays.
    let go = disk(&fx, &["clean", "--apply"]);
    assert_eq!(go.code, 0, "{} {}", go.json, go.err);
    assert!(!merged.exists() && !pushed.exists());
    for p in [&unpushed, &dirty, &locked, &fx.primary] {
        assert!(p.is_dir(), "{p:?} must be kept");
    }
    assert!(!fx.porcelain().contains("merged-one") && fx.porcelain().contains("dirty-one"));
    assert!(
        go.err.contains("removed") && go.err.contains("merged-one"),
        "every path is logged: {}",
        go.err
    );
}

#[test]
fn a_worktree_an_agent_still_owns_is_live_even_when_unlocked() {
    let fx = Fixture::new();
    let p = fx.add("owned");
    fx.commit_in(&p, "owned.txt");
    fx.land("owned");
    assert_eq!(fx.claim(&p, "implementer:other").code, 0);
    // Unlocked behind the tool's back (raw git): the owner map still names the agent.
    fx.git(&fx.primary, &["worktree", "unlock", s(&p)]);
    let r = disk(&fx, &["report"]);
    assert_eq!(state_of(&r, &p), "live");
    let c = disk(&fx, &["clean", "--apply"]);
    assert_eq!(c.json["removed"].as_array().unwrap().len(), 0, "{} {}", c.json, c.err);
    assert!(p.is_dir());
}

#[test]
fn adopt_records_existing_worktrees_and_build_folders_named_after_them_and_leaves_the_rest_orphans() {
    let fx = Fixture::new();
    // A worktree made with raw git (no record), a build folder named after it, and a folder nobody can name.
    let raw = fx.wt.join("raw-one");
    fx.git(&fx.primary, &["worktree", "add", "-q", "-b", "raw-one", s(&raw), "origin/main"]);
    let build = fx.root.join("raw-one-target");
    std::fs::create_dir_all(&build).unwrap();
    std::fs::write(build.join("blob"), vec![0u8; 2000]).unwrap();
    let mystery = fx.root.join("mystery");
    std::fs::create_dir_all(&mystery).unwrap();

    let dry = disk(&fx, &["adopt", "--dry-run"]);
    assert_eq!(dry.code, 0, "{} {}", dry.json, dry.err);
    assert!(!raw.join(".paddock-owner.toml").exists(), "a dry run writes nothing");

    let r = disk(&fx, &["adopt"]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    let wt_rec = std::fs::read_to_string(raw.join(".paddock-owner.toml")).unwrap();
    assert!(
        wt_rec.contains("kind = \"worktree\"") && wt_rec.contains("branch = \"raw-one\""),
        "{wt_rec}"
    );
    assert_eq!(fx.git(&raw, &["status", "--porcelain"]), "", "the adopted record is ignored by git");
    let b_rec = std::fs::read_to_string(build.join(".paddock-owner.toml")).unwrap();
    assert!(
        b_rec.contains("kind = \"build\"") && b_rec.contains(&format!("link = \"{}\"", raw.display())),
        "{b_rec}"
    );
    assert!(!mystery.join(".paddock-owner.toml").exists());

    let rep = disk(&fx, &["report"]);
    assert_eq!(state_of(&rep, &mystery), "orphan");
    assert_ne!(state_of(&rep, &build), "orphan");
    // Adopting twice writes nothing new.
    assert_eq!(disk(&fx, &["adopt"]).json["written"].as_array().unwrap().len(), 0);
}

#[test]
fn a_corrupt_index_does_not_stop_the_report_it_is_set_aside_and_rebuilt_from_the_records() {
    let fx = Fixture::new();
    let p = released(&fx, "survivor");
    fx.land("survivor");
    std::fs::write(
        fx.root.join("index.sqlite"),
        b"definitely not a database, just some text to fill the header bytes",
    )
    .unwrap();
    let r = disk(&fx, &["report"]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert_eq!(state_of(&r, &p), "merged");
    assert!(
        r.json["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n.as_str().unwrap().contains("unusable")),
        "{}",
        r.json
    );
    assert!(
        std::fs::read_dir(&fx.root)
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains("index.sqlite.corrupt-"))
    );
}

/// A folder under the fixture's root with a build record (no link) holding a clone of the origin.
fn build_with_clone(fx: &Fixture, name: &str) -> PathBuf {
    let dir = fx.root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let r = fx.dev(&["disk", "record", s(&dir), "--kind", "build", "--owner", "implementer:probe"]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    // The record is old: only real facts may keep it.
    let rec = dir.join(".paddock-owner.toml");
    let aged = std::fs::read_to_string(&rec)
        .unwrap()
        .lines()
        .map(|l| {
            if l.starts_with("last_used") || l.starts_with("created") {
                format!("{} = \"2020-01-01T00:00:00Z\"", l.split(' ').next().unwrap())
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&rec, aged).unwrap();
    fx.git(&fx.root, &["clone", "-q", "origin.git", &format!("{name}/inner")]);
    age(&dir);
    dir
}

/// The folder's own time counts as use: make it old.
fn age(dir: &Path) {
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
    std::fs::File::open(dir).unwrap().set_modified(old).unwrap();
}

#[test]
fn a_build_folder_with_no_link_that_holds_a_repository_with_an_unpushed_commit_is_kept() {
    // The reviewer's probe: no link, an old record, a clone inside with a commit that is on no remote.
    let fx = Fixture::new();
    let dir = build_with_clone(&fx, "probe-build");
    fx.commit_in(&dir.join("inner"), "unsaved.txt");
    age(&dir);
    let go = disk(&fx, &["clean", "--apply"]);
    assert_eq!(go.code, 0, "{} {}", go.json, go.err);
    assert!(dir.join("inner/unsaved.txt").exists(), "unpushed work must survive: {}", go.err);
    assert!(
        go.err.contains("no remote") || go.json.to_string().contains("no remote"),
        "{} {}",
        go.json,
        go.err
    );
}

#[test]
fn nested_repositories_with_a_dirty_file_a_stash_or_no_remote_are_kept_and_a_clean_pushed_one_goes() {
    let fx = Fixture::new();
    let clean = build_with_clone(&fx, "nested-clean");
    let dirty = build_with_clone(&fx, "nested-dirty");
    std::fs::write(dirty.join("inner/wip.txt"), "wip\n").unwrap();
    let stash = build_with_clone(&fx, "nested-stash");
    std::fs::write(stash.join("inner/README.md"), "changed\n").unwrap();
    fx.git(&stash.join("inner"), &["stash", "push", "-q"]);
    let local = fx.root.join("nested-local");
    std::fs::create_dir_all(&local).unwrap();
    assert_eq!(
        fx.dev(&["disk", "record", s(&local), "--kind", "scratch", "--owner", "implementer:probe"])
            .code,
        0
    );
    fx.git(&local, &["init", "-q"]);
    for d in [&clean, &dirty, &stash, &local] {
        let rec = d.join(".paddock-owner.toml");
        let t = std::fs::read_to_string(&rec).unwrap();
        let aged: Vec<String> = t
            .lines()
            .map(|l| {
                if l.starts_with("last_used") || l.starts_with("created") {
                    format!("{} = \"2020-01-01T00:00:00Z\"", l.split(' ').next().unwrap())
                } else {
                    l.to_string()
                }
            })
            .collect();
        std::fs::write(&rec, aged.join("\n")).unwrap();
        age(d);
    }
    let go = disk(&fx, &["clean", "--apply"]);
    assert_eq!(go.code, 0, "{} {}", go.json, go.err);
    assert!(!clean.exists(), "{}", go.err);
    for kept in [&dirty, &stash, &local] {
        assert!(kept.is_dir(), "{kept:?} must be kept: {}", go.err);
    }
}

#[test]
fn a_build_folder_linked_to_a_dirty_worktree_is_kept() {
    let fx = Fixture::new();
    let wt = released(&fx, "linked-wt");
    fx.git(&wt, &["push", "-q", "origin", "linked-wt"]);
    std::fs::write(wt.join("wip.txt"), "wip\n").unwrap();
    let build = fx.root.join("linked-wt-target");
    std::fs::create_dir_all(&build).unwrap();
    let r = fx.dev(&[
        "disk",
        "record",
        s(&build),
        "--kind",
        "build",
        "--owner",
        "implementer:t",
        "--link",
        s(&wt),
    ]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    let rec = build.join(".paddock-owner.toml");
    let t = std::fs::read_to_string(&rec).unwrap();
    let aged: Vec<String> = t
        .lines()
        .map(|l| {
            if l.starts_with("last_used") || l.starts_with("created") {
                format!("{} = \"2020-01-01T00:00:00Z\"", l.split(' ').next().unwrap())
            } else {
                l.to_string()
            }
        })
        .collect();
    std::fs::write(&rec, aged.join("\n")).unwrap();
    age(&build);
    let rep = disk(&fx, &["report"]);
    assert_eq!(
        state_of(&rep, &build),
        "unsaved",
        "a build folder follows the state of the worktree it serves"
    );
    let go = disk(&fx, &["clean", "--apply"]);
    assert!(build.is_dir() && wt.is_dir(), "{} {}", go.json, go.err);
}

#[test]
fn safe_remove_refuses_a_locked_or_claimed_worktree_even_with_an_old_record() {
    let fx = Fixture::new();
    let wt = fx.add("proof-locked");
    let roots = vec![fx.root.clone()];
    let e = paddock_dev::safe_remove::check_with(&wt, &roots, Ok(vec![])).unwrap_err();
    assert!(e.contains("locked"), "{e}");
    assert_eq!(fx.claim(&wt, "implementer:x").code, 0);
    fx.git(&fx.primary, &["worktree", "unlock", s(&wt)]);
    let e = paddock_dev::safe_remove::check_with(&wt, &roots, Ok(vec![])).unwrap_err();
    assert!(e.contains("claimed"), "{e}");
    assert!(wt.is_dir());
}

#[test]
fn every_git_call_of_a_scan_runs_without_optional_locks() {
    let fx = Fixture::new();
    let _wt = fx.add("locks-wt");
    let log = fx.root.join("git-env.log");
    let path_env = fx.fake_git("locks", &format!("echo \"${{GIT_OPTIONAL_LOCKS:-unset}}\" >> '{}'", log.display()));
    let index = fx.root.join("index.sqlite");
    let out = fx
        .cmd(
            &fx.primary,
            &["disk", "scan", "--no-sizes", "--root", s(&fx.root), "--index", s(&index)],
        )
        .env("PATH", path_env)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let seen = std::fs::read_to_string(&log).unwrap();
    assert!(seen.lines().count() > 3, "the scan asked git something: {seen}");
    assert!(
        seen.lines().all(|l| l == "0"),
        "every git call must set GIT_OPTIONAL_LOCKS=0: {seen}"
    );
}

#[test]
fn a_missing_root_is_not_measured_in_the_report_not_an_empty_disk() {
    let fx = Fixture::new();
    let index = fx.root.join("index.sqlite");
    let gone = fx.root.join("not-mounted");
    let r = super::run_of(
        fx.cmd(
            &fx.primary,
            &["disk", "report", "--root", s(&fx.root), "--root", s(&gone), "--index", s(&index)],
        )
        .output()
        .unwrap(),
    );
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    let disk = r.json["disks"].as_array().unwrap().iter().find(|d| d["root"] == s(&gone)).unwrap();
    assert_eq!(disk["measured"], false);
    assert!(disk["free_bytes"].is_null());
}
