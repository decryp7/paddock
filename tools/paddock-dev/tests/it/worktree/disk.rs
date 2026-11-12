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
    let dirty = released(&fx, "dirty-one");
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
