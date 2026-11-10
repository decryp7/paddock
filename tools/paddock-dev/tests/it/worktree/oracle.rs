//! The engineering-rules `scripts/worktree.sh` is the oracle for add, list and clean: the same fixture repository goes
//! through both tools and their decisions are compared. The script lives in another repository, so the test is ignored
//! unless `WORKTREE_SH` names it:
//!
//! `WORKTREE_SH=<engineering-rules>/scripts/worktree.sh cargo test --test it worktree::oracle -- --ignored`
use super::Fixture;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

fn script() -> PathBuf {
    PathBuf::from(std::env::var("WORKTREE_SH").expect("WORKTREE_SH must name engineering-rules/scripts/worktree.sh"))
}

fn sh(fx: &Fixture, args: &[&str]) -> String {
    let out = Command::new("bash")
        .arg(script())
        .args(args)
        .env("WT_REPO", &fx.primary)
        .env("WT_ROOT", fx.root.join("sh-wt"))
        .env("WT_BUILD_ROOT", fx.root.join("sh-build"))
        .env("GIT_CONFIG_GLOBAL", fx.root.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "worktree.sh {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Branches made by worktree.sh: merged (committed, fast-forwarded into main), unmerged, empty, and merged but dirty.
fn build(fx: &Fixture) -> BTreeMap<&'static str, PathBuf> {
    let mut out = BTreeMap::new();
    for name in ["merged", "unmerged", "empty", "dirtymerged"] {
        sh(fx, &["add", name, "--base", "main"]);
        out.insert(name, fx.root.join("sh-wt").join(name).canonicalize().unwrap());
    }
    for name in ["merged", "unmerged", "dirtymerged"] {
        fx.commit_in(&out[name], &format!("{name}.txt"));
    }
    for name in ["merged", "dirtymerged"] {
        fx.git(&fx.primary, &["merge", "-q", "--no-edit", name]);
    }
    std::fs::write(out["dirtymerged"].join("wip.txt"), "x").unwrap();
    out
}

/// worktree.sh's list as (branch, merged, state) by path.
fn sh_list(fx: &Fixture) -> BTreeMap<String, (String, String, String)> {
    sh(fx, &["list", "--base", "main"])
        .lines()
        .skip(1)
        .map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (
                f[0].to_string(),
                (f[1].to_string(), f[f.len() - 2].to_string(), f[f.len() - 1].to_string()),
            )
        })
        .collect()
}

#[test]
#[ignore = "needs WORKTREE_SH (engineering-rules/scripts/worktree.sh)"]
fn list_agrees_with_worktree_sh_on_merged_and_dirty() {
    let fx = Fixture::new();
    let made = build(&fx);
    let theirs = sh_list(&fx);
    assert_eq!(theirs.len(), 4, "{theirs:?}");
    let ours = fx.dev(&["worktree", "list", "--base", "main"]).json;
    for (name, path) in &made {
        let key = path.to_str().unwrap();
        let (branch, merged, state) = &theirs[key];
        let w = ours["worktrees"].as_array().unwrap().iter().find(|w| w["path"] == key).unwrap();
        assert_eq!(w["branch"], branch.as_str(), "{name}");
        assert_eq!(w["merged"], merged.as_str(), "{name}: ours {} theirs {merged}", w["merged"]);
        assert_eq!(w["dirty"], state == "dirty", "{name}");
    }
}

#[test]
#[ignore = "needs WORKTREE_SH (engineering-rules/scripts/worktree.sh)"]
fn clean_removes_the_same_worktrees_as_worktree_sh() {
    let fx = Fixture::new();
    let made = build(&fx);
    let theirs: Vec<String> = sh(&fx, &["clean", "--base", "main", "--dry-run"])
        .lines()
        .filter_map(|l| l.strip_prefix("would remove "))
        .map(|l| l.split(',').next().unwrap().to_string())
        .collect();
    assert_eq!(
        theirs,
        vec![made["merged"].to_str().unwrap().to_string()],
        "the oracle's own decision"
    );
    let ours = fx.dev(&["worktree", "clean", "--base", "main", "--dry-run"]).json;
    let removed: Vec<String> = ours["removed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["path"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(removed, theirs);
    let kept: Vec<String> = ours["kept"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["path"].as_str().unwrap().to_string())
        .collect();
    for name in ["unmerged", "empty", "dirtymerged"] {
        assert!(kept.contains(&made[name].to_str().unwrap().to_string()), "{name} kept: {kept:?}");
    }
    let real = fx.dev(&["worktree", "clean", "--base", "main"]);
    assert_eq!(real.code, 0, "{} {}", real.json, real.err);
    assert!(!made["merged"].exists());
    assert!(["unmerged", "empty", "dirtymerged"].iter().all(|n| made[n].exists()));
}

#[test]
#[ignore = "needs WORKTREE_SH (engineering-rules/scripts/worktree.sh)"]
fn add_starts_from_the_same_commit_on_a_new_branch_but_locks_it() {
    let fx = Fixture::new();
    sh(&fx, &["add", "theirs", "--base", "main"]);
    let theirs = fx.root.join("sh-wt/theirs").canonicalize().unwrap();
    let path = fx.path("ours");
    let r = fx.dev(&[
        "worktree",
        "add",
        path.to_str().unwrap(),
        "-b",
        "ours",
        "main",
        "--owner",
        "implementer:x",
    ]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(fx.git(&theirs, &["rev-parse", "HEAD"]), fx.git(&path, &["rev-parse", "HEAD"]));
    assert_eq!(fx.git(&path, &["rev-parse", "--abbrev-ref", "HEAD"]), "ours");
    assert!(fx.lock_reason(&path).is_some(), "ours is locked");
    assert!(fx.lock_reason(&theirs).is_none(), "worktree.sh's is not (WP-4 adds --owner there)");
    let again = Command::new(env!("CARGO_BIN_EXE_paddock-dev"))
        .args(["worktree", "add", path.to_str().unwrap(), "-b", "x", "main", "--owner", "a:b"])
        .current_dir(&fx.primary)
        .output()
        .unwrap();
    assert!(!again.status.success(), "both refuse an existing path");
}
