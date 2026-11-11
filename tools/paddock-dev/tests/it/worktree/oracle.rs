//! The engineering-rules `scripts/worktree.sh` is the oracle for add, list and clean: the same fixture repository goes
//! through both tools and their decisions are compared. The tests run against a pinned copy of the script
//! (`tests/fixtures/worktree-oracle/worktree-sh.txt`, its source commit in the first line); `WORKTREE_SH` names another
//! script instead (for example the current one in engineering-rules). A test that did not run must not pass (ER-33), so
//! there is no skip.
use super::Fixture;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

/// The oracle script: `WORKTREE_SH`, else the pinned copy.
fn script() -> PathBuf {
    std::env::var("WORKTREE_SH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/worktree-oracle/worktree-sh.txt"))
}

#[test]
fn the_pinned_oracle_names_its_source_commit() {
    let text =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/worktree-oracle/worktree-sh.txt")).unwrap();
    let first = text.lines().next().unwrap();
    let sha = first
        .split("at commit ")
        .nth(1)
        .and_then(|r| r.split_whitespace().next())
        .unwrap_or("");
    assert!(sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()), "{first}");
    assert_eq!(text.lines().nth(1), Some("#!/bin/bash"), "the script follows its header line");
}

// Starts the tool under test, git or a helper by program name (a throw-away fixture), never a process by pid (C53).
#[allow(clippy::disallowed_methods)]
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
fn clean_removes_the_same_worktrees_as_worktree_sh() {
    let fx = Fixture::new();
    let made = build(&fx);
    // worktree.sh trusts "merged into local main"; the tool also wants the commits on a remote (it never removes work no remote
    // has), so the fixture pushes main first, as a landing does.
    fx.git(&fx.primary, &["push", "-q", "origin", "main"]);
    fx.git(&fx.primary, &["fetch", "-q", "origin"]);
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
fn add_starts_from_the_same_commit_on_a_new_branch_but_locks_it() {
    let fx = Fixture::new();
    sh(&fx, &["add", "theirs", "--base", "main"]);
    let theirs = fx.root.join("sh-wt/theirs").canonicalize().unwrap();
    let path = fx.path("ours");
    let r = fx.dev(&["worktree", "add", "ours", "--base", "main", "--owner", "implementer:x"]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(fx.git(&theirs, &["rev-parse", "HEAD"]), fx.git(&path, &["rev-parse", "HEAD"]));
    assert_eq!(fx.git(&path, &["rev-parse", "--abbrev-ref", "HEAD"]), "ours");
    assert!(fx.lock_reason(&path).is_some(), "ours is locked");
    assert!(fx.lock_reason(&theirs).is_none(), "worktree.sh's is not (WP-4 adds --owner there)");
    let again = fx.dev(&["worktree", "add", "ours", "--base", "main", "-b", "x", "--owner", "a:b"]);
    assert_eq!(again.code, 4, "both refuse an existing path: {}", again.err);
}
