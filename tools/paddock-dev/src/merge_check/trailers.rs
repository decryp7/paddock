//! The reasons a merge message can give for a deliberate drop (ADR 0089: a reason is data, never prose the check guesses at).
//!
//! - `Tests-removed: <test name>: <reason>`
//! - `Hunks-dropped: <path>: <reason>` (a folder with a trailing slash covers the files under it)
//! - `Reverted: <path>: <reason>` (answers a backwards finding; `Hunks-dropped` does too)
//!
//! and the facts `./dev merge member` writes into a train merge it makes (a user decision of 2026-11-27: readers had parsed
//! the free-text merge subject, and that needed three fix rounds):
//!
//! - `Paddock-Member: <member branch>`, `Paddock-Sync-Ref: sync/<member>/<short sha>` (a sync ref was merged), `Paddock-Sync-Sha:
//!   <full sha of the member commit>` and `Paddock-Reland: <revert sha>` (one per re-landed revert).
//!
//! [`merge_member`] is the ONE reader of them: every reader of a train merge (the sync-member own-revert logic, the trial reader,
//! the stale-base check, `--write-reverted`) calls it. The trailers come from git's own parser (`%(trailers:only,unfold)` in the
//! `--format` of the call that reads the commit, [`TRAILERS_FORMAT`], so no process is started for them and no regex reads the text);
//! only a merge made by hand, without them, falls back to the old reading of its subject.

/// The `--format` placeholder that gives a commit's trailer block, as git's own parser reads it: `Key: value` lines of the last
/// paragraph when it is a trailer block, unfolded; empty when there is none.
pub const TRAILERS_FORMAT: &str = "%(trailers:only,unfold)";
/// `Paddock-Member`: the member's branch name.
pub const MEMBER: &str = "Paddock-Member";
/// `Paddock-Sync-Ref`: the sync ref that was merged.
pub const SYNC_REF: &str = "Paddock-Sync-Ref";
/// `Paddock-Sync-Sha`: the full sha of the commit that was merged.
pub const SYNC_SHA: &str = "Paddock-Sync-Sha";
/// `Paddock-Reland`: a revert of the target that the member re-lands.
pub const RELAND: &str = "Paddock-Reland";

/// What a train merge says about its member.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MergeMember {
    /// The member's branch name (the subject's reading for a merge made by hand).
    pub branch: String,
    /// The sync ref that was merged (`sync/<member>/<short sha>`): the trailer, else `synced as <ref>` in a hand-made subject.
    pub sync_ref: Option<String>,
    /// The full sha of the member commit that was merged, from the trailer only.
    pub sync_sha: Option<String>,
    /// The reverts of the target the member re-lands, from the trailers only.
    pub relands: Vec<String>,
    /// Whether the member came from `Paddock-Member` (else it is the subject's reading, or empty).
    pub from_trailer: bool,
}

/// The values of git's trailer block `trailers` (its `Key: value` lines) under `key`, case-insensitive as git's tokens are.
fn values<'t>(trailers: &'t str, key: &str) -> impl Iterator<Item = &'t str> {
    let key = key.to_string();
    trailers
        .lines()
        .filter_map(|l| l.split_once(':'))
        .filter(move |(k, _)| k.trim().eq_ignore_ascii_case(&key))
        .map(|(_, v)| v.trim())
        .filter(|v| !v.is_empty())
}

/// The member of a train merge: the trailers first (`trailers` is the output of [`TRAILERS_FORMAT`] for the merge commit), the old
/// reading of the merge `subject` only when `Paddock-Member` is missing (a merge made by hand, or before `merge member`).
pub fn merge_member(subject: &str, trailers: &str) -> MergeMember {
    let from = values(trailers, MEMBER).next();
    MergeMember {
        branch: from.map_or_else(|| branch_of(subject), str::to_string),
        // The trailer first; a merge made by hand names its sync ref in the subject (`synced as <ref>`).
        sync_ref: values(trailers, SYNC_REF)
            .next()
            .map(str::to_string)
            .or_else(|| synced_ref(subject)),
        sync_sha: values(trailers, SYNC_SHA).next().map(str::to_string),
        relands: values(trailers, RELAND).map(str::to_string).collect(),
        from_trailer: from.is_some(),
    }
}

/// The member a ref names: the branch (`origin/` dropped), and for `sync/<member>/<hex>` the member's own branch and the ref.
#[derive(Debug, PartialEq, Eq)]
pub struct Named {
    /// The member's branch name.
    pub branch: String,
    /// The sync ref, when one was given.
    pub sync_ref: Option<String>,
}

/// What `arg` (a branch, a remote branch or a `sync/<member>/<hex>` ref) names.
pub fn named(arg: &str) -> Named {
    let name = arg.strip_prefix("origin/").unwrap_or(arg);
    // `sync/<member>/<short sha>` is the one spelling [`super::trial_dispatch::sync_ref`] writes and reads.
    match super::trial_dispatch::split_sync_ref(name) {
        Some((member, _)) => Named {
            branch: member.to_string(),
            sync_ref: Some(name.to_string()),
        },
        None => Named {
            branch: name.to_string(),
            sync_ref: None,
        },
    }
}

/// The trailer lines every tool-made train merge carries (`merge member`, the trial's merge, the queue's): the writer side of
/// [`merge_member`], so they cannot drift.
pub fn member_lines(who: &Named, sha: &str, relands: &[String]) -> Vec<String> {
    let mut lines = vec![format!("{MEMBER}: {}", who.branch)];
    if let Some(r) = &who.sync_ref {
        lines.push(format!("{SYNC_REF}: {r}"));
    }
    lines.push(format!("{SYNC_SHA}: {sha}"));
    lines.extend(relands.iter().map(|r| format!("{RELAND}: {r}")));
    lines
}

/// The `Reverted:` lines for a re-land: one per file the revert changed, so the merge check accepts the member's re-add of work the
/// target took out. The one generator of them (`merge member --reland` and the trial's merge).
pub fn reland_lines(revert: &str, files: &[String]) -> Vec<String> {
    files
        .iter()
        .map(|f| {
            format!(
                "Reverted: {f}: the member re-lands the work the revert {} took out (--reland)",
                revert.get(..9).unwrap_or(revert)
            )
        })
        .collect()
}

/// `subject`, a blank line, then `lines` as one trailer block, ending in a newline.
pub fn with_block(subject: &str, lines: &[String]) -> String {
    format!("{subject}\n\n{}\n", lines.join("\n"))
}

/// The branch a merge commit's subject names: `Merge branch 'x'`, `... merge x into train N`, `Merge x (hex) into trial of y`;
/// a trailing `(hex)` is dropped; else the subject itself. The fallback of [`merge_member`], for merges made by hand.
pub(super) fn branch_of(subject: &str) -> String {
    let subject = strip_hex_suffix(subject.trim());
    if let Some(rest) = subject.split("Merge branch '").nth(1)
        && let Some((name, _)) = rest.split_once('\'')
    {
        return name.to_string();
    }
    let named = subject
        .split(": merge ")
        .nth(1)
        .or_else(|| subject.strip_prefix("Merge ").filter(|r| r.contains(" into ")));
    match named {
        Some(rest) => strip_hex_suffix(rest.split(" into ").next().unwrap_or(rest).trim()).to_string(),
        None => subject.to_string(),
    }
}

/// `name (abc1234)` becomes `name`; so does `name (abc1234, synced as sync/m/abc123456)`, the integrator's merge subject: the
/// parenthesis opens with the member's commit and may go on after a comma.
fn strip_hex_suffix(s: &str) -> &str {
    if let Some(body) = s.strip_suffix(')')
        && let Some((head, inside)) = body.rsplit_once(" (")
    {
        let hex = inside.split_once(',').map_or(inside, |(h, _)| h).trim();
        if hex.len() >= 7 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return head.trim_end();
        }
    }
    s
}

/// The sync ref a hand-made merge subject names, `Merge <member> (<sha>, synced as sync/<member>/<short sha>) into <train>`: the text
/// after `synced as ` up to the closing parenthesis. `None` when the subject names none.
fn synced_ref(subject: &str) -> Option<String> {
    let rest = subject.split("synced as ").nth(1)?;
    let name = rest.split([')', ' ', ',']).next()?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The reason given for `subject` under `key` (`Tests-removed` or `Hunks-dropped`): the text after `<key>: <subject>: `, which
/// must not be empty. Looks at every line of the message, so a trailer block anywhere counts. For a path, `subject` may also
/// be a folder written with a trailing slash (`Hunks-dropped: docs/images/: ...`), which covers the files under it
/// ([`path_reason`]).
pub fn reason<'m>(message: &'m str, key: &str, subject: &str) -> Option<&'m str> {
    let prefix = format!("{key}: {subject}: ");
    message
        .lines()
        .filter_map(|l| l.trim().strip_prefix(prefix.as_str()))
        .map(str::trim)
        .find(|r| !r.is_empty())
}

/// [`reason`] for a file path: the path itself, or a folder prefix ending in `/` that contains it.
pub fn path_reason<'m>(message: &'m str, key: &str, path: &str) -> Option<&'m str> {
    if let Some(r) = reason(message, key, path) {
        return Some(r);
    }
    let prefix = format!("{key}: ");
    message
        .lines()
        .filter_map(|l| l.trim().strip_prefix(prefix.as_str()))
        .find_map(|rest| {
            let (dir, why) = rest.split_once(": ")?;
            (dir.ends_with('/') && path.starts_with(dir) && !why.trim().is_empty()).then_some(why.trim())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_trailers_name_the_member_and_the_subject_is_only_the_fallback() {
        let t = "Paddock-Member: feat/x\nPaddock-Sync-Ref: sync/feat/x/abc123456\nPaddock-Sync-Sha: abc1234567890\nPaddock-Reland: r1\nPaddock-Reland: r2\nReverted: a.txt: why\n";
        let m = merge_member("chore: take it, whatever the words", t);
        assert_eq!(
            m,
            MergeMember {
                branch: "feat/x".into(),
                sync_ref: Some("sync/feat/x/abc123456".into()),
                sync_sha: Some("abc1234567890".into()),
                relands: vec!["r1".into(), "r2".into()],
                from_trailer: true,
            }
        );
        // The key is git's token: any case.
        assert_eq!(merge_member("x", "paddock-member: feat/y\n").branch, "feat/y");
        // A trailer beats a subject that names another branch.
        assert_eq!(merge_member("Merge branch 'feat/other' into train", t).branch, "feat/x");
    }

    #[test]
    fn a_sync_ref_names_its_member_and_a_branch_names_itself() {
        let w = named("sync/feat/x/abc123456");
        assert_eq!(
            (w.branch.as_str(), w.sync_ref.as_deref()),
            ("feat/x", Some("sync/feat/x/abc123456"))
        );
        assert_eq!(
            named("origin/sync/feat/x/abc123456").sync_ref.as_deref(),
            Some("sync/feat/x/abc123456")
        );
        assert_eq!(
            named("origin/feat/x"),
            Named {
                branch: "feat/x".into(),
                sync_ref: None
            }
        );
        // Not hex: a branch that happens to start with sync/.
        assert_eq!(named("sync/feat/x").sync_ref, None);
    }

    /// The writer and the reader agree: what `member_lines` writes, `merge_member` reads back from one trailer block.
    #[test]
    fn what_the_writer_makes_the_one_reader_reads_back() {
        let who = named("sync/feat/x/abc123456");
        let mut lines = member_lines(&who, "abc1234567890", &["r1".into()]);
        lines.push("Reverted: a.txt: why".into());
        let m = with_block("Merge feat/x into train/t", &lines);
        assert_eq!(m.lines().next(), Some("Merge feat/x into train/t"));
        let block = m.split_once("\n\n").unwrap().1;
        assert!(!block.contains("\n\n"), "one block: {block}");
        let read = merge_member("Merge feat/x into train/t", block);
        assert_eq!(read.branch, "feat/x");
        assert_eq!(read.sync_ref.as_deref(), Some("sync/feat/x/abc123456"));
        assert_eq!(read.sync_sha.as_deref(), Some("abc1234567890"));
        assert_eq!(read.relands, ["r1"]);
    }

    #[test]
    fn a_merge_without_the_trailer_is_read_from_its_subject_as_before() {
        for (s, want) in [
            ("Merge branch 'feat/x' into main", "feat/x"),
            ("chore(train): merge feat/x into train 12", "feat/x"),
            ("chore(train): merge feat/x (abc1234) into train 12", "feat/x"),
            ("Merge feat/x (abc1234def) into trial of main", "feat/x"),
            ("chore(train): merge feat/x (abc1234)", "feat/x"),
            (
                "Merge feat/x (d5b1ce0e4, synced as sync/feat/x/2ab388102) into train/2026-11-26-u",
                "feat/x",
            ),
            ("Merge feat/x (b6d3953b3, synced as sync/feat/x) into train/2026-11-26-t", "feat/x"),
            ("something else", "something else"),
            ("fix: a (note)", "fix: a (note)"),
        ] {
            let m = merge_member(s, "");
            assert_eq!((m.branch.as_str(), m.from_trailer), (want, false), "{s}");
        }
        // The integrator's subject names the sync ref too.
        let s = merge_member("Merge feat/x (d5b1ce0e4, synced as sync/feat/x/2ab388102) into train/u", "");
        assert_eq!(s.sync_ref.as_deref(), Some("sync/feat/x/2ab388102"));
        assert_eq!(merge_member("Merge branch 'feat/x'", "").sync_ref, None);
        // Other trailers do not name a member.
        assert!(!merge_member("Merge branch 'a' into b", "Reverted: f: why\n").from_trailer);
    }

    #[test]
    fn a_reason_is_needed_and_the_subject_must_match_exactly() {
        let m = "Merge x\n\nTests-removed: old_case: replaced by new_case\nTests-removed: bare: \nTests-removed: other_old_case: no\n";
        assert_eq!(reason(m, "Tests-removed", "old_case"), Some("replaced by new_case"));
        assert_eq!(reason(m, "Tests-removed", "bare"), None);
        assert_eq!(reason(m, "Tests-removed", "old"), None);
        assert_eq!(reason(m, "Hunks-dropped", "old_case"), None);
    }

    #[test]
    fn a_folder_with_a_trailing_slash_covers_its_files() {
        let m = "Hunks-dropped: docs/images/: the hero is redrawn\nHunks-dropped: src/: \n";
        assert!(path_reason(m, "Hunks-dropped", "docs/images/a/b.svg").is_some());
        assert!(path_reason(m, "Hunks-dropped", "docs/imagesx/b.svg").is_none());
        assert!(path_reason(m, "Hunks-dropped", "src/lib.rs").is_none());
    }
}
