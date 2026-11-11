//! add, claim, reassign, release and drop: ADR 0083 §1 and R8's worktree cases.
use super::Fixture;

fn p(path: &std::path::Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn add_makes_a_locked_worktree_with_an_owner_note_on_a_new_branch() {
    let fx = Fixture::new();
    let path = fx.path("w1");
    let r = fx.dev(&[
        "worktree",
        "add",
        "w1",
        "-b",
        "feat/w1",
        "--base",
        "origin/main",
        "--owner",
        "implementer:RT-3",
    ]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert_eq!(r.json["path"], p(&path));
    assert_eq!(r.json["branch"], "feat/w1");
    assert_eq!(r.json["locked"], true);
    assert_eq!(fx.git(&path, &["rev-parse", "--abbrev-ref", "HEAD"]), "feat/w1");
    assert_eq!(
        fx.git(&path, &["rev-parse", "HEAD"]),
        fx.git(&fx.primary, &["rev-parse", "origin/main"])
    );
    let reason = fx.lock_reason(&path).expect("locked");
    for part in ["owner=implementer:RT-3", "branch=feat/w1", "created=", "by=coordinator", "base="] {
        assert!(reason.contains(part), "{part} in {reason}");
    }
    // The coordinator made it: nobody has claimed it yet.
    assert_eq!(fx.owner_of(&path), None);
}

#[test]
fn add_by_an_agent_claims_the_new_worktree() {
    let fx = Fixture::new();
    let path = fx.path("w1");
    let r = fx.dev(&[
        "worktree",
        "add",
        "w1",
        "-b",
        "w1",
        "--base",
        "origin/main",
        "--owner",
        "reviewer:x",
        "--agent",
        "agent-A",
        "--agent-type",
        "reviewer",
    ]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert_eq!(fx.owner_of(&path).as_deref(), Some("agent-A"));
    assert_eq!(fx.owners()["owners"][p(&path)]["agent_type"], "reviewer");
    assert!(fx.lock_reason(&path).unwrap().contains("by=agent:agent-A"));
}

#[test]
fn add_detach_makes_a_review_copy_with_no_branch() {
    let fx = Fixture::new();
    let path = fx.path("copy");
    let r = fx.dev(&[
        "worktree",
        "add",
        "copy",
        "--base",
        "origin/main",
        "--detach",
        "--owner",
        "reviewer:RT-3",
        "--agent",
        "agent-R",
    ]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert_eq!(r.json["branch"], serde_json::Value::Null);
    assert!(!fx.git_ok(&path, &["symbolic-ref", "-q", "HEAD"]), "HEAD must be detached");
    assert_eq!(
        fx.git(&path, &["rev-parse", "HEAD"]),
        fx.git(&fx.primary, &["rev-parse", "origin/main"])
    );
    assert!(fx.lock_reason(&path).unwrap().contains("branch=detached"));
}

#[test]
fn add_names_the_branch_after_the_folder_by_default_and_refuses_branch_with_detach() {
    let fx = Fixture::new();
    let path = fx.path("w");
    let neither = fx.dev(&["worktree", "add", "w", "--base", "origin/main", "--owner", "a:b"]);
    assert_eq!(neither.code, 0, "{}", neither.err);
    assert_eq!(fx.git(&path, &["rev-parse", "--abbrev-ref", "HEAD"]), "w");
    let both = fx.dev(&[
        "worktree",
        "add",
        "x",
        "-b",
        "x",
        "--base",
        "origin/main",
        "--detach",
        "--owner",
        "a:b",
    ]);
    assert_eq!(both.code, 2, "{}", both.err);
    assert!(!fx.path("x").exists());
}

#[test]
fn add_refuses_a_bad_owner_and_an_unknown_base_and_makes_nothing() {
    let fx = Fixture::new();
    let path = fx.path("w");
    for owner in ["implementer", "a b:c", "a:b;c"] {
        let r = fx.dev(&["worktree", "add", "w", "-b", "w", "--base", "origin/main", "--owner", owner]);
        assert_eq!(r.code, 2, "{owner}: {}", r.err);
    }
    let r = fx.dev(&["worktree", "add", "w", "-b", "w", "origin/nope", "--owner", "a:b"]);
    assert_eq!(r.code, 2, "{}", r.err);
    assert!(r.err.contains("origin/nope"), "{}", r.err);
    assert!(!path.exists());
    assert!(!fx.git_ok(&fx.primary, &["rev-parse", "--verify", "-q", "refs/heads/w"]));
}

#[test]
fn add_refuses_an_existing_path_and_never_rebinds_it() {
    // R8: agent-B's `add` of a name that exists, while the owner map says agent-A, changes nothing.
    let fx = Fixture::new();
    let path = fx.add("w1");
    assert_eq!(fx.claim(&path, "agent-A").code, 0);
    let before = fx.owners();
    let r = fx.dev(&[
        "worktree",
        "add",
        "w1",
        "-b",
        "other",
        "--base",
        "origin/main",
        "--owner",
        "x:y",
        "--agent",
        "agent-B",
    ]);
    assert_eq!(r.code, 4, "{} {}", r.json, r.err);
    assert!(r.err.contains("already"), "{}", r.err);
    assert_eq!(fx.owners(), before);
    assert_eq!(fx.owner_of(&path).as_deref(), Some("agent-A"));
    assert!(!fx.git_ok(&fx.primary, &["rev-parse", "--verify", "-q", "refs/heads/other"]));
}

#[test]
fn add_refuses_an_existing_plain_folder_and_a_dangling_link() {
    let fx = Fixture::new();
    let folder = fx.path("folder");
    std::fs::create_dir_all(&folder).unwrap();
    let r = fx.dev(&["worktree", "add", "folder", "-b", "f", "--base", "origin/main", "--owner", "a:b"]);
    assert_eq!(r.code, 4, "{}", r.err);
    #[cfg(unix)]
    {
        let link = fx.path("link");
        std::os::unix::fs::symlink(fx.root.join("missing"), &link).unwrap();
        let r = fx.dev(&["worktree", "add", "link", "-b", "l", "--base", "origin/main", "--owner", "a:b"]);
        assert_eq!(r.code, 4, "{}", r.err);
    }
}

#[test]
fn add_refuses_a_path_the_owner_map_already_has_even_when_the_folder_is_gone() {
    let fx = Fixture::new();
    let path = fx.path("w1");
    let r = fx.dev(&[
        "worktree",
        "add",
        "w1",
        "-b",
        "w1",
        "--base",
        "origin/main",
        "--owner",
        "a:b",
        "--agent",
        "agent-A",
    ]);
    assert_eq!(r.code, 0, "{}", r.err);
    // Someone removed the worktree behind the tool's back: the entry stays.
    assert!(fx.git_ok(&fx.primary, &["worktree", "unlock", p(&path)]));
    assert!(fx.git_ok(&fx.primary, &["worktree", "remove", p(&path)]));
    let r = fx.dev(&[
        "worktree",
        "add",
        "w1",
        "-b",
        "w2",
        "--base",
        "origin/main",
        "--owner",
        "a:b",
        "--agent",
        "agent-B",
    ]);
    assert_eq!(r.code, 4, "{}", r.err);
    assert!(r.err.contains("owner map"), "{}", r.err);
    assert_eq!(fx.owner_of(&path).as_deref(), Some("agent-A"));
    // The coordinator frees the entry, then the path can be used again.
    assert_eq!(fx.dev(&["worktree", "reassign", p(&path)]).code, 0);
    let r = fx.dev(&[
        "worktree",
        "add",
        "w1",
        "-b",
        "w2",
        "--base",
        "origin/main",
        "--owner",
        "a:b",
        "--agent",
        "agent-B",
    ]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(fx.owner_of(&path).as_deref(), Some("agent-B"));
}

fn add_branch(fx: &Fixture, name: &str, branch: &str, extra: &[&str]) -> super::Run {
    let mut a = vec![
        "worktree",
        "add",
        name,
        "-b",
        branch,
        "--base",
        "origin/main",
        "--owner",
        "a:b",
        "--agent",
        "agent-A",
    ];
    a.extend_from_slice(extra);
    fx.dev(&a)
}

#[test]
fn add_attaches_to_an_existing_local_branch() {
    // Agents ask for a branch that already exists (a review fix round, a pushed branch): attach, never refuse.
    let fx = Fixture::new();
    fx.git(&fx.primary, &["branch", "taken", "origin/main"]);
    fx.git(&fx.primary, &["commit", "-q", "--allow-empty", "-m", "ahead"]);
    fx.git(&fx.primary, &["branch", "-f", "taken", "HEAD"]);
    fx.git(&fx.primary, &["reset", "-q", "--hard", "origin/main"]);
    let path = fx.path("w");
    let r = add_branch(&fx, "w", "taken", &[]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert_eq!(r.json["attached"], "local branch");
    assert_eq!(r.json["drift"]["ahead_of_base"], 1);
    assert_eq!(r.json["drift"]["behind_base"], 0);
    assert!(r.json["drift"]["remote"].is_null(), "no remote branch of that name");
    assert_eq!(fx.git(&path, &["rev-parse", "--abbrev-ref", "HEAD"]), "taken");
    assert_eq!(
        fx.git(&path, &["rev-parse", "HEAD"]),
        fx.git(&fx.primary, &["rev-parse", "taken"]),
        "the branch keeps its own commits; the base is not applied"
    );
    assert!(fx.lock_reason(&path).unwrap().contains("branch=taken"));
}

#[test]
fn add_attaches_to_a_branch_that_exists_only_on_the_remote() {
    let fx = Fixture::new();
    fx.git(&fx.primary, &["push", "-q", "origin", "origin/main:refs/heads/pushed"]);
    fx.git(&fx.primary, &["fetch", "-q", "origin"]);
    assert!(!fx.git_ok(&fx.primary, &["rev-parse", "--verify", "--quiet", "refs/heads/pushed"]));
    let path = fx.path("w");
    let r = add_branch(&fx, "w", "pushed", &[]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert_eq!(r.json["attached"], "remote branch origin/pushed");
    assert_eq!(fx.git(&path, &["rev-parse", "--abbrev-ref", "HEAD"]), "pushed");
    assert_eq!(fx.git(&path, &["rev-parse", "--abbrev-ref", "pushed@{upstream}"]), "origin/pushed");
}

/// Advances origin's main by one commit and fetches it, so `origin/main` is ahead of an older branch.
fn advance_main(fx: &Fixture) {
    fx.git(&fx.primary, &["commit", "-q", "--allow-empty", "-m", "second"]);
    fx.git(&fx.primary, &["push", "-q", "origin", "main"]);
    fx.git(&fx.primary, &["fetch", "-q", "origin"]);
}

#[test]
fn add_refuses_a_branch_that_is_already_merged_into_the_base_and_says_how_far_behind() {
    let fx = Fixture::new();
    fx.git(&fx.primary, &["branch", "old", "origin/main"]);
    advance_main(&fx);
    let path = fx.path("w");
    let r = add_branch(&fx, "w", "old", &[]);
    assert_eq!(r.code, 4, "{}", r.err);
    assert!(
        r.err.contains("already merged into origin/main") && r.err.contains("1 commit(s) behind"),
        "{}",
        r.err
    );
    assert!(r.err.contains("new branch name"), "{}", r.err);
    assert!(!path.exists());
    assert_eq!(fx.owner_of(&path), None);
}

#[test]
fn add_refuses_a_local_branch_that_is_behind_its_remote_and_gives_the_fast_forward() {
    let fx = Fixture::new();
    fx.git(&fx.primary, &["branch", "work", "origin/main"]);
    fx.git(&fx.primary, &["commit", "-q", "--allow-empty", "-m", "remote only"]);
    fx.git(&fx.primary, &["push", "-q", "origin", "HEAD:refs/heads/work"]);
    fx.git(&fx.primary, &["reset", "-q", "--hard", "origin/main"]);
    fx.git(&fx.primary, &["fetch", "-q", "origin"]);
    let path = fx.path("w");
    let r = add_branch(&fx, "w", "work", &[]);
    assert_eq!(r.code, 4, "{}", r.err);
    assert!(r.err.contains("1 commit(s) behind origin/work"), "{}", r.err);
    assert!(r.err.contains("git branch -f work origin/work"), "{}", r.err);
    assert!(!path.exists());
}

#[test]
fn add_reports_how_far_an_attached_branch_is_from_its_remote() {
    let fx = Fixture::new();
    fx.git(&fx.primary, &["push", "-q", "origin", "origin/main:refs/heads/mine"]);
    fx.git(&fx.primary, &["fetch", "-q", "origin"]);
    fx.git(&fx.primary, &["branch", "mine", "origin/mine"]);
    fx.git(&fx.primary, &["commit", "-q", "--allow-empty", "-m", "local only"]);
    fx.git(&fx.primary, &["branch", "-f", "mine", "HEAD"]);
    fx.git(&fx.primary, &["reset", "-q", "--hard", "origin/main"]);
    let r = add_branch(&fx, "w", "mine", &[]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert_eq!(r.json["drift"]["remote"]["ref"], "origin/mine");
    assert_eq!(r.json["drift"]["remote"]["ahead"], 1);
    assert_eq!(r.json["drift"]["remote"]["behind"], 0);
    let text = fx
        .cmd_text(
            &fx.primary,
            &["worktree", "add", "w2", "-b", "mine", "--base", "origin/main", "--owner", "a:b"],
        )
        .output()
        .unwrap();
    // `mine` is now checked out in `w`, so git refuses: the text form is checked on a second branch below.
    assert!(!text.status.success());
    fx.git(&fx.primary, &["branch", "mine2", "mine"]);
    let t = fx
        .cmd_text(
            &fx.primary,
            &["worktree", "add", "w3", "-b", "mine2", "--base", "origin/main", "--owner", "a:b"],
        )
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&t.stdout);
    assert!(out.contains("The branch is 1 ahead of and 0 behind origin/main"), "{out}");
}

#[test]
fn add_for_a_new_branch_says_it_is_new() {
    let fx = Fixture::new();
    let r = add_branch(&fx, "w", "fresh", &[]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert!(r.json["attached"].is_null());
}

#[test]
fn add_refuses_a_branch_another_worktree_has_checked_out_and_leaves_no_entry() {
    let fx = Fixture::new();
    assert_eq!(add_branch(&fx, "one", "shared", &[]).code, 0);
    let path = fx.path("two");
    let r = add_branch(&fx, "two", "shared", &[]);
    assert_ne!(r.code, 0);
    assert!(r.err.contains("shared"), "{}", r.err);
    assert!(!path.exists());
    assert_eq!(fx.owner_of(&path), None);
}

#[test]
fn add_without_a_profile_is_a_full_checkout_and_the_gates_profile_is_the_same() {
    // Agents made sparse worktrees and then hand-added folders for conformance, affected.mjs and `gates check`: a branch
    // worktree is the gates profile, a full checkout (measured: about 1 s and 350 MB more than a cone), unless it is a review copy.
    let fx = Fixture::new();
    std::fs::create_dir_all(fx.primary.join("other")).unwrap();
    std::fs::write(fx.primary.join("other/b.txt"), "b").unwrap();
    fx.git(&fx.primary, &["add", "."]);
    fx.git(&fx.primary, &["commit", "-q", "-m", "dirs"]);
    fx.git(&fx.primary, &["push", "-q", "origin", "main"]);
    fx.git(&fx.primary, &["fetch", "-q", "origin"]);
    for (name, extra) in [("plain", &[][..]), ("gates", &["--profile", "gates"][..])] {
        let r = add_branch(&fx, name, name, extra);
        assert_eq!(r.code, 0, "{} {}", r.json, r.err);
        assert!(fx.path(name).join("other/b.txt").exists());
        assert!(!fx.git_ok(&fx.path(name), &["config", "--get", "core.sparseCheckout"]));
    }
}

#[test]
fn add_refuses_sparse_for_a_branch_worktree_and_says_what_to_use() {
    let fx = Fixture::new();
    let path = fx.path("sp");
    let r = add_branch(&fx, "sp", "sp", &["--sparse", "keep"]);
    assert_eq!(r.code, 2, "{}", r.err);
    assert!(r.err.contains("--detach") && r.err.contains("full checkout"), "{}", r.err);
    assert!(!path.exists());
    assert_eq!(fx.owner_of(&path), None);
    assert_eq!(add_branch(&fx, "sp2", "sp2", &["--profile", "gates", "--sparse", "keep"]).code, 2);
}

#[test]
fn add_with_sparse_makes_a_cone_review_copy_without_changing_the_primary() {
    let fx = Fixture::new();
    std::fs::create_dir_all(fx.primary.join("keep")).unwrap();
    std::fs::create_dir_all(fx.primary.join("other")).unwrap();
    std::fs::write(fx.primary.join("keep/a.txt"), "a").unwrap();
    std::fs::write(fx.primary.join("other/b.txt"), "b").unwrap();
    fx.git(&fx.primary, &["add", "."]);
    fx.git(&fx.primary, &["commit", "-q", "-m", "dirs"]);
    fx.git(&fx.primary, &["push", "-q", "origin", "main"]);
    fx.git(&fx.primary, &["fetch", "-q", "origin"]);
    let path = fx.path("sp");
    let r = fx.dev(&[
        "worktree",
        "add",
        "sp",
        "--detach",
        "--base",
        "origin/main",
        "--owner",
        "a:b",
        "--sparse",
        "keep",
    ]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert!(path.join("keep/a.txt").exists());
    assert!(!path.join("other/b.txt").exists());
    assert!(fx.primary.join("other/b.txt").exists());
    assert!(fx.lock_reason(&path).is_some());
    // The primary stays a full checkout, which the probe also checks.
    assert!(
        !fx.git_ok(&fx.primary, &["config", "--get", "core.sparseCheckout"]),
        "sparse is set only in the worktree"
    );
}

#[test]
fn claim_and_drop_name_the_next_step_when_git_lost_the_worktree_but_its_folder_is_there() {
    // An `add` whose checkout timed out, or a prune, leaves a folder with a `.git` file that git no longer lists.
    let fx = Fixture::new();
    let path = fx.add("w1");
    std::fs::remove_dir_all(fx.primary.join(".git/worktrees/w1")).unwrap();
    assert!(path.join(".git").is_file());
    for r in [
        fx.claim(&path, "agent-A"),
        fx.dev(&["worktree", "drop", p(&path), "--agent", "agent-A"]),
    ] {
        assert_eq!(r.code, 4, "{}", r.err);
        assert!(
            r.err.contains("git no longer lists it") && r.err.contains("./dev worktree add"),
            "{}",
            r.err
        );
    }
    // A plain folder keeps the short message.
    let plain = fx.path("plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert!(!fx.claim(&plain, "agent-A").err.contains("no longer lists"));
}

#[test]
fn claim_binds_the_agent_and_is_idempotent_for_it() {
    let fx = Fixture::new();
    let path = fx.add("w1");
    let r = fx.claim(&path, "agent-A");
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert_eq!(r.json["already"], false);
    assert_eq!(fx.owner_of(&path).as_deref(), Some("agent-A"));
    let again = fx.claim(&path, "agent-A");
    assert_eq!(again.code, 0);
    assert_eq!(again.json["already"], true);
}

#[test]
fn claim_refuses_another_agents_worktree_until_the_coordinator_reassigns() {
    let fx = Fixture::new();
    let path = fx.add("w1");
    assert_eq!(fx.claim(&path, "agent-A").code, 0);
    let r = fx.claim(&path, "agent-B");
    assert_eq!(r.code, 4, "{}", r.err);
    assert!(r.err.contains("claimed by agent-A") && r.err.contains("reassign"), "{}", r.err);
    assert_eq!(fx.owner_of(&path).as_deref(), Some("agent-A"));
    assert_eq!(fx.dev(&["worktree", "reassign", p(&path)]).code, 0);
    assert_eq!(fx.claim(&path, "agent-B").code, 0);
    assert_eq!(fx.owner_of(&path).as_deref(), Some("agent-B"));
}

#[test]
fn claim_refuses_unknown_unlocked_and_primary_targets() {
    let fx = Fixture::new();
    let unknown = fx.path("nowhere");
    let r = fx.claim(&unknown, "agent-A");
    assert_eq!(r.code, 4, "{}", r.err);
    let plain = fx.path("plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(fx.claim(&plain, "agent-A").code, 4);
    let unlocked = fx.add("u");
    assert!(fx.git_ok(&fx.primary, &["worktree", "unlock", p(&unlocked)]));
    let r = fx.claim(&unlocked, "agent-A");
    assert_eq!(r.code, 4, "{}", r.err);
    assert!(r.err.contains("not locked"), "{}", r.err);
    assert_eq!(fx.claim(&fx.primary, "agent-A").code, 4);
    assert_eq!(
        fx.owners()["owners"].as_object().map_or(0, |m| m.len()),
        0,
        "a refused claim records no owner"
    );
}

#[test]
fn claim_takes_the_agent_from_the_environment_and_needs_one() {
    let fx = Fixture::new();
    let path = fx.add("w1");
    let none = fx.dev(&["worktree", "claim", p(&path)]);
    assert_eq!(none.code, 2, "{}", none.err);
    let out = fx
        .cmd(&fx.primary, &["worktree", "claim", p(&path)])
        .env("PADDOCK_AGENT_ID", "agent-E")
        .env("PADDOCK_AGENT_TYPE", "implementer")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(fx.owner_of(&path).as_deref(), Some("agent-E"));
    let bad = fx.dev(&["worktree", "claim", p(&path), "--agent", "a b"]);
    assert_eq!(bad.code, 2, "{}", bad.err);
}

#[test]
fn reassign_is_for_the_coordinator_and_keeps_the_lock() {
    let fx = Fixture::new();
    let path = fx.add("w1");
    assert_eq!(fx.claim(&path, "agent-A").code, 0);
    let by_agent = fx.dev(&["worktree", "reassign", p(&path), "--agent", "agent-B"]);
    assert_eq!(by_agent.code, 4, "{}", by_agent.err);
    assert!(by_agent.err.contains("coordinator"), "{}", by_agent.err);
    assert_eq!(fx.owner_of(&path).as_deref(), Some("agent-A"));
    let r = fx.dev(&["worktree", "reassign", p(&path)]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert_eq!(r.json["previous"], "agent-A");
    assert_eq!(r.json["locked"], true);
    assert_eq!(fx.owner_of(&path), None);
    assert!(fx.lock_reason(&path).is_some(), "the lock stays");
    let unknown = fx.dev(&["worktree", "reassign", p(&fx.path("nowhere"))]);
    assert_eq!(unknown.code, 4, "{}", unknown.err);
}

#[test]
fn release_unlocks_for_the_owner_or_the_coordinator_only() {
    let fx = Fixture::new();
    let path = fx.add("w1");
    assert_eq!(fx.claim(&path, "agent-A").code, 0);
    let other = fx.dev(&["worktree", "release", p(&path), "--agent", "agent-B"]);
    assert_eq!(other.code, 4, "{}", other.err);
    assert!(fx.lock_reason(&path).is_some());
    let owner = fx.dev(&["worktree", "release", p(&path), "--agent", "agent-A"]);
    assert_eq!(owner.code, 0, "{} {}", owner.json, owner.err);
    assert_eq!(fx.lock_reason(&path), None);
    assert_eq!(fx.owner_of(&path), None);
    assert!(path.exists(), "release never removes the folder");
    let again = fx.dev(&["worktree", "release", p(&path), "--agent", "agent-A"]);
    assert_eq!(again.code, 4, "an unlocked worktree cannot be released: {}", again.err);
    // The coordinator can release another's, and an unclaimed one is the coordinator's alone.
    let w2 = fx.add("w2");
    let unclaimed = fx.dev(&["worktree", "release", p(&w2), "--agent", "agent-B"]);
    assert_eq!(unclaimed.code, 4, "{}", unclaimed.err);
    assert_eq!(fx.dev(&["worktree", "release", p(&w2)]).code, 0);
    assert_eq!(fx.lock_reason(&w2), None);
}

fn add_copy(fx: &Fixture, name: &str, agent: &str) -> std::path::PathBuf {
    let path = fx.path(name);
    let r = fx.dev(&[
        "worktree",
        "add",
        name,
        "--base",
        "origin/main",
        "--detach",
        "--owner",
        "reviewer:t",
        "--agent",
        agent,
        "--agent-type",
        "reviewer",
    ]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    path
}

#[test]
fn drop_removes_the_owners_clean_review_copy_with_its_lock_and_entry() {
    let fx = Fixture::new();
    let path = add_copy(&fx, "copy", "agent-R");
    let r = fx.dev(&["worktree", "drop", p(&path), "--agent", "agent-R"]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert!(!path.exists());
    assert!(!fx.porcelain().contains(p(&path)));
    assert_eq!(fx.owner_of(&path), None);
}

#[test]
fn drop_refuses_what_it_does_not_own() {
    let fx = Fixture::new();
    let mine = add_copy(&fx, "copy", "agent-R");
    let other = fx.dev(&["worktree", "drop", p(&mine), "--agent", "agent-S"]);
    assert_eq!(other.code, 4, "{}", other.err);
    let coordinator = fx.dev(&["worktree", "drop", p(&mine)]);
    assert_eq!(coordinator.code, 2, "drop needs an agent: {}", coordinator.err);
    let unclaimed = fx.add("w1");
    let r = fx.dev(&["worktree", "drop", p(&unclaimed), "--agent", "agent-R"]);
    assert_eq!(r.code, 4, "{}", r.err);
    assert!(unclaimed.exists() && mine.exists());
    assert_eq!(fx.dev(&["worktree", "drop", p(&fx.primary), "--agent", "agent-R"]).code, 4);
    assert!(fx.primary.join("README.md").exists());
}

#[test]
fn drop_refuses_uncommitted_changes_and_untracked_files() {
    let fx = Fixture::new();
    let path = add_copy(&fx, "copy", "agent-R");
    std::fs::write(path.join("README.md"), "changed\n").unwrap();
    let r = fx.dev(&["worktree", "drop", p(&path), "--agent", "agent-R"]);
    assert_eq!(r.code, 4, "{}", r.err);
    assert!(r.err.contains("uncommitted"), "{}", r.err);
    assert!(path.join("README.md").exists());
    fx.git(&path, &["checkout", "--", "README.md"]);
    std::fs::write(path.join("new.txt"), "x").unwrap();
    assert_eq!(fx.dev(&["worktree", "drop", p(&path), "--agent", "agent-R"]).code, 4);
    assert!(fx.lock_reason(&path).is_some(), "a refused drop keeps the lock");
    assert_eq!(fx.owner_of(&path).as_deref(), Some("agent-R"));
}

#[test]
fn drop_refuses_staged_modified_and_untracked_work_and_counts_each_kind() {
    // A commit that failed (a hook, a bad message) leaves staged files; a removal chained after it must not take them along.
    let fx = Fixture::new();
    let path = add_copy(&fx, "copy", "agent-R");
    std::fs::write(path.join("staged.txt"), "s").unwrap();
    fx.git(&path, &["add", "staged.txt"]);
    let r = fx.dev(&["worktree", "drop", p(&path), "--agent", "agent-R"]);
    assert_eq!(r.code, 4, "{}", r.err);
    assert!(r.err.contains("1 staged") && !r.err.contains("untracked,"), "{}", r.err);
    std::fs::write(path.join("README.md"), "changed\n").unwrap();
    std::fs::write(path.join("new.txt"), "x").unwrap();
    let r = fx.dev(&["worktree", "drop", p(&path), "--agent", "agent-R"]);
    assert_eq!(r.code, 4, "{}", r.err);
    assert!(r.err.contains("1 staged, 1 modified, 1 untracked"), "{}", r.err);
    for f in ["staged.txt", "README.md", "new.txt"] {
        assert!(path.join(f).exists(), "{f} survives");
    }
    assert!(fx.lock_reason(&path).is_some() && fx.owner_of(&path).as_deref() == Some("agent-R"));
}

#[test]
fn drop_refuses_commits_that_are_on_no_remote_and_allows_them_once_pushed() {
    let fx = Fixture::new();
    let path = fx.path("w1");
    let r = fx.dev(&[
        "worktree",
        "add",
        "w1",
        "-b",
        "w1",
        "--base",
        "origin/main",
        "--owner",
        "a:b",
        "--agent",
        "agent-A",
    ]);
    assert_eq!(r.code, 0, "{}", r.err);
    fx.commit_in(&path, "f.txt");
    let refused = fx.dev(&["worktree", "drop", p(&path), "--agent", "agent-A"]);
    assert_eq!(refused.code, 4, "{}", refused.err);
    assert!(refused.err.contains("no remote"), "{}", refused.err);
    assert!(path.exists());
    fx.git(&path, &["push", "-q", "origin", "w1"]);
    let ok = fx.dev(&["worktree", "drop", p(&path), "--agent", "agent-A"]);
    assert_eq!(ok.code, 0, "{} {}", ok.json, ok.err);
    assert!(!path.exists());
    assert!(
        fx.git_ok(&fx.primary, &["rev-parse", "--verify", "-q", "refs/heads/w1"]),
        "drop keeps the branch"
    );
}

#[test]
fn a_corrupt_owner_map_stops_claims_and_is_not_overwritten() {
    let fx = Fixture::new();
    let path = fx.add("w1");
    let file = fx.primary.join(".git/paddock-worktree-owners.json");
    std::fs::write(&file, "{ not json").unwrap();
    let r = fx.claim(&path, "agent-A");
    assert_eq!(r.code, 1, "{}", r.err);
    assert!(r.err.contains("owner map"), "{}", r.err);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ not json");
}

#[test]
fn the_tool_finds_the_repository_from_inside_a_worktree() {
    let fx = Fixture::new();
    let a = fx.add("w1");
    let b = fx.add("w2");
    let r = fx.dev_in(&a, &["worktree", "claim", p(&b), "--agent", "agent-A"]);
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert_eq!(fx.owner_of(&b).as_deref(), Some("agent-A"));
}

#[test]
fn the_owner_map_file_is_versioned_and_lives_in_the_common_dir_under_the_adrs_names() {
    let fx = Fixture::new();
    let path = fx.add("w1");
    assert_eq!(fx.claim(&path, "agent-A").code, 0);
    let m = fx.owners();
    assert_eq!(m["version"], 1);
    assert!(m["owners"][p(&path)]["claimed"].as_str().unwrap().ends_with('Z'));
    assert!(m["pending"].as_object().unwrap().is_empty(), "no reservation is left behind");
    assert!(fx.primary.join(".git/paddock-worktree-owners.lock").exists());
    assert!(
        !fx.primary.join(".git/paddock-worktree-owners.holder").exists(),
        "the holder file is removed when the command ends"
    );
    assert!(
        std::fs::read_dir(fx.primary.join(".git"))
            .unwrap()
            .all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp"))
    );
}

#[test]
fn a_claim_stays_fast_while_a_slow_add_runs() {
    // P1-1: `add` holds the owner map's lock only to reserve and to confirm, never across git. A fake `git` that sleeps in
    // `worktree add` (the seam: a script first on the PATH) must not delay a claim of another worktree.
    use std::time::{Duration, Instant};
    let fx = Fixture::new();
    let other = fx.add("other");
    let (path_env, reached) = fx.fake_slow_add("slow");
    let mut slow = fx
        .cmd(
            &fx.primary,
            &[
                "worktree",
                "add",
                "slow",
                "--base",
                "origin/main",
                "--owner",
                "a:b",
                "--agent",
                "agent-S",
            ],
        )
        .env("PATH", path_env)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Wait for the add to be inside its slow git call (the reservation is in place).
    fx.wait_inside_add(&reached);
    let t = Instant::now();
    let r = fx.claim(&other, "agent-A");
    let took = t.elapsed();
    assert_eq!(r.code, 0, "{} {}", r.json, r.err);
    assert!(took < Duration::from_millis(1000), "a claim waited {took:?} behind an add");
    assert!(
        slow.try_wait().unwrap().is_none(),
        "the slow add must still be running when the claim finished"
    );
    // A second add of the same path meanwhile is refused by the reservation.
    let dup = fx.dev(&[
        "worktree",
        "add",
        "slow",
        "--base",
        "origin/main",
        "--owner",
        "a:b",
        "--agent",
        "agent-T",
    ]);
    assert_eq!(dup.code, 4, "{}", dup.err);
    assert!(dup.err.contains("being made"), "{}", dup.err);
    let out = slow.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(fx.owner_of(&fx.path("slow")).as_deref(), Some("agent-S"));
    assert!(fx.owners()["pending"].as_object().unwrap().is_empty());
}

#[test]
fn a_failed_add_removes_its_reservation() {
    let fx = Fixture::new();
    assert_eq!(add_branch(&fx, "other", "taken", &[]).code, 0);
    let r = fx.dev(&["worktree", "add", "w", "-b", "taken", "--base", "origin/main", "--owner", "a:b"]);
    assert_ne!(r.code, 0);
    assert!(fx.owners()["pending"].as_object().unwrap().is_empty(), "{}", fx.owners());
    // The same name works at once afterwards.
    let again = fx.dev(&["worktree", "add", "w", "--base", "origin/main", "--owner", "a:b"]);
    assert_eq!(again.code, 0, "{}", again.err);
}

#[test]
fn add_places_the_worktree_under_the_root_and_the_root_comes_from_arg_then_config_then_env() {
    let fx = Fixture::new();
    let (a, b) = (fx.root.join("rootA"), fx.root.join("rootB"));
    // env only (the fixture sets PADDOCK_WORKTREE_ROOT): the first add lands there.
    let r = fx.dev(&["worktree", "add", "e", "--base", "origin/main", "--owner", "a:b"]);
    assert_eq!(r.json["path"], p(&fx.path("e")), "{}", r.err);
    // git config beats env.
    fx.git(&fx.primary, &["config", "paddock.worktreeRoot", p(&b)]);
    let r = fx.dev(&["worktree", "add", "g", "--base", "origin/main", "--owner", "a:b"]);
    assert_eq!(r.json["path"], p(&b.join("g")), "{}", r.err);
    // --root beats both.
    let r = fx.dev(&["worktree", "add", "x", "--base", "origin/main", "--owner", "a:b", "--root", p(&a)]);
    assert_eq!(r.json["path"], p(&a.join("x")), "{}", r.err);
}

#[test]
fn add_without_any_root_fails_with_the_fix() {
    let fx = Fixture::new();
    let out = fx
        .cmd(&fx.primary, &["worktree", "add", "w", "--base", "origin/main", "--owner", "a:b"])
        .env_remove("PADDOCK_WORKTREE_ROOT")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    for fix in ["--root", "paddock.worktreeRoot", "PADDOCK_WORKTREE_ROOT"] {
        assert!(err.contains(fix), "{fix} in {err}");
    }
    assert!(!fx.path("w").exists());
}

#[test]
fn add_refuses_a_path_inside_the_primary_or_inside_any_worktree_and_makes_nothing() {
    // P1-2: one agent must not create a worktree inside another's, or inside the primary.
    let fx = Fixture::new();
    let other = fx.add("other");
    let inside_primary = fx.dev(&[
        "worktree",
        "add",
        "n1",
        "--base",
        "origin/main",
        "--owner",
        "a:b",
        "--root",
        p(&fx.primary),
    ]);
    assert_eq!(inside_primary.code, 4, "{}", inside_primary.err);
    assert!(
        inside_primary.err.contains("inside") && inside_primary.err.contains("primary"),
        "{}",
        inside_primary.err
    );
    let inside_worktree = fx.dev(&[
        "worktree",
        "add",
        "n2",
        "--base",
        "origin/main",
        "--owner",
        "a:b",
        "--root",
        p(&other),
    ]);
    assert_eq!(inside_worktree.code, 4, "{}", inside_worktree.err);
    assert!(inside_worktree.err.contains("another worktree"), "{}", inside_worktree.err);
    // Deeper, and through a path spelled with `..` (canonical comparison).
    let sneaky = format!("{}/../wt/other/sub", p(&fx.wt));
    let r = fx.dev(&[
        "worktree",
        "add",
        "n3",
        "--base",
        "origin/main",
        "--owner",
        "a:b",
        "--root",
        &sneaky,
    ]);
    assert_eq!(r.code, 4, "{}", r.err);
    #[cfg(unix)]
    {
        let link = fx.root.join("link");
        std::os::unix::fs::symlink(&fx.primary, &link).unwrap();
        let r = fx.dev(&[
            "worktree",
            "add",
            "n4",
            "--base",
            "origin/main",
            "--owner",
            "a:b",
            "--root",
            p(&link),
        ]);
        assert_eq!(r.code, 4, "a symlink into the primary is still the primary: {}", r.err);
    }
    for n in ["n1", "n2", "n3", "n4"] {
        assert!(!fx.primary.join(n).exists() && !other.join(n).exists() && !other.join("sub").join(n).exists());
        assert!(
            !fx.git_ok(&fx.primary, &["rev-parse", "--verify", "-q", &format!("refs/heads/{n}")]),
            "{n}: no branch"
        );
    }
}

#[test]
fn add_takes_only_a_plain_folder_name() {
    let fx = Fixture::new();
    for bad in ["../x", "a/b", "/abs", ".hidden", "-x", "", "a b"] {
        let r = fx.dev(&["worktree", "add", bad, "--base", "origin/main", "--owner", "a:b"]);
        assert_eq!(r.code, 2, "{bad:?}: {}", r.err);
    }
}

#[test]
fn drop_says_so_when_the_lock_could_not_be_put_back() {
    let fx = Fixture::new();
    let path = add_copy(&fx, "copy", "agent-R");
    // git refuses the removal, and then refuses to lock again.
    let path_env = fx.fake_git(
        "nolock",
        r#"case "$*" in *"worktree remove"*|*"worktree lock"*) echo "fatal: simulated" >&2; exit 1;; esac"#,
    );
    let out = fx
        .cmd(&fx.primary, &["worktree", "drop", p(&path), "--agent", "agent-R"])
        .env("PATH", path_env)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("could NOT be put back") && err.contains("git worktree lock"), "{err}");
    assert!(
        path.exists() && fx.owner_of(&path).as_deref() == Some("agent-R"),
        "nothing was removed, the entry stays"
    );
}

#[test]
fn two_adds_with_different_roots_cannot_nest_through_a_reservation() {
    // Add A is inside its slow git call (its path is only reserved, nothing on disk yet); add B names A's folder as its root.
    let fx = Fixture::new();
    let (path_env, reached) = fx.fake_slow_add("nest");
    let slow = fx
        .cmd(&fx.primary, &["worktree", "add", "a", "--base", "origin/main", "--owner", "a:b"])
        .env("PATH", path_env)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    fx.wait_inside_add(&reached);
    let inside = fx.dev(&[
        "worktree",
        "add",
        "b",
        "--base",
        "origin/main",
        "--owner",
        "a:b",
        "--root",
        p(&fx.path("a")),
    ]);
    assert_eq!(inside.code, 4, "{}", inside.err);
    assert!(
        inside.err.contains("inside") && inside.err.contains("another `add`"),
        "{}",
        inside.err
    );
    // And the other way round: a root that would put a new worktree around the reserved one.
    let around = fx.dev(&[
        "worktree",
        "add",
        "wt",
        "--base",
        "origin/main",
        "--owner",
        "a:b",
        "--root",
        p(&fx.root),
    ]);
    assert_eq!(around.code, 4, "{}", around.err);
    assert!(around.err.contains("around"), "{}", around.err);
    assert!(slow.wait_with_output().unwrap().status.success());
    assert!(fx.path("a").exists() && !fx.path("a").join("b").exists());
    // Once made, the same nesting is refused against the real worktree.
    let after = fx.dev(&[
        "worktree",
        "add",
        "b",
        "--base",
        "origin/main",
        "--owner",
        "a:b",
        "--root",
        p(&fx.path("a")),
    ]);
    assert_eq!(after.code, 4, "{}", after.err);
}
