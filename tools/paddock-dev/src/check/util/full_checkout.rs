//! A sparse checkout that lacks folders the gates read makes conformance fail with a bare ENOENT, and agents then add folders one
//! by one. This says it once, with the folders and the fix (ER-27). It compares the tracked folders (the top level, and the
//! children of `apps/`) with the disk, so it needs no list of what the gates read. The port of `util/full-checkout.mjs`.

use paddock_proc::{Limits, command, run_bounded};
use std::path::Path;
use std::time::Duration;

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = command("git");
    cmd.arg("-C").arg(root).args(args);
    let out = run_bounded(cmd, Limits::new(Duration::from_secs(60)).max_output(1 << 24)).ok()?;
    out.success().then(|| out.stdout_text())
}

/// `None` when `root` is not a sparse checkout or has every tracked folder, else the `::error::` line to print. A workflow run
/// (`GITHUB_ACTIONS=true`) is never judged (CI checks out sparse on purpose); `PADDOCK_ALLOW_SPARSE` skips the check elsewhere.
pub fn problem(root: &Path, env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    if env("GITHUB_ACTIONS").as_deref() == Some("true") || env("PADDOCK_ALLOW_SPARSE").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    if git(root, &["config", "--get", "core.sparseCheckout"])?.trim() != "true" {
        return None;
    }
    let mut folders: Vec<String> = Vec::new();
    for args in [
        ["ls-tree", "-d", "--name-only", "HEAD"].as_slice(),
        &["ls-tree", "-d", "--name-only", "HEAD", "apps/"],
    ] {
        folders.extend(git(root, args)?.split('\n').map(|f| f.trim().to_string()));
    }
    let mut missing: Vec<String> = Vec::new();
    for f in folders.into_iter().filter(|f| !f.is_empty()) {
        if !missing.contains(&f) && !root.join(&f).exists() {
            missing.push(f);
        }
    }
    if missing.is_empty() {
        return None;
    }
    Some(format!(
        "::error::This checkout is sparse and lacks {}, which the gates read. Fix: run `git sparse-checkout disable`, or `git sparse-checkout add {}`. New worktrees from `./dev worktree add` are full checkouts.",
        missing.join(", "),
        missing.join(" ")
    ))
}
