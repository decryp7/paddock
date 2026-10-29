//! The change plan: pure, deterministic, and the place where a rename keeps its id.

use std::collections::{BTreeMap, BTreeSet};

use pacenotes_core::{FileId, Fingerprint, RepoPath};

/// A file as the index knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    /// Its id, issued once.
    pub file_id: FileId,
    /// Where it was.
    pub path: RepoPath,
    /// Its content fingerprint then.
    pub fingerprint: Fingerprint,
}

/// A file as the workspace has it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Current {
    /// Where it is.
    pub path: RepoPath,
    /// Its content fingerprint now.
    pub fingerprint: Fingerprint,
}

/// What changed. Every list is sorted by path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    /// Same path, same content.
    pub unchanged: Vec<RepoPath>,
    /// Same path, new content: facts are extracted again, the id is kept.
    pub modified: Vec<RepoPath>,
    /// Gone from `from`, present at `to` with the same content: the id moves with the file.
    pub renamed: Vec<(RepoPath, RepoPath)>,
    /// New files.
    pub added: Vec<RepoPath>,
    /// Files that are no longer there.
    pub deleted: Vec<RepoPath>,
}

/// Matches paths first, then fingerprints. Several identical files pair in path order (the n-th vanished with the n-th new), so
/// the plan is the same on every run. A rename that also edits the file is a delete and an add.
pub fn plan_changes(previous: &[Known], current: &[Current]) -> Plan {
    let before: BTreeMap<&RepoPath, &Fingerprint> = previous.iter().map(|k| (&k.path, &k.fingerprint)).collect();
    let after: BTreeMap<&RepoPath, &Fingerprint> = current.iter().map(|c| (&c.path, &c.fingerprint)).collect();
    let mut plan = Plan::default();
    let mut vanished: Vec<&RepoPath> = Vec::new();
    for (path, fingerprint) in &before {
        match after.get(path) {
            Some(now) if now == fingerprint => plan.unchanged.push((*path).clone()),
            Some(_) => plan.modified.push((*path).clone()),
            None => vanished.push(path),
        }
    }
    let new: Vec<&RepoPath> = after.keys().filter(|p| !before.contains_key(*p)).copied().collect();
    let mut gone_by_content: BTreeMap<&Fingerprint, Vec<&RepoPath>> = BTreeMap::new();
    for path in &vanished {
        gone_by_content.entry(before[*path]).or_default().push(path);
    }
    for list in gone_by_content.values_mut() {
        list.reverse(); // pop() takes the smallest path first
    }
    let mut paired: BTreeSet<&RepoPath> = BTreeSet::new();
    for path in new {
        match gone_by_content.get_mut(after[path]).and_then(Vec::pop) {
            Some(from) => {
                paired.insert(from);
                plan.renamed.push((from.clone(), path.clone()));
            }
            None => plan.added.push(path.clone()),
        }
    }
    plan.deleted = vanished.into_iter().filter(|p| !paired.contains(*p)).cloned().collect();
    plan.renamed.sort_by(|a, b| a.1.cmp(&b.1));
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use pacenotes_core::FileIdAllocator;

    fn p(s: &str) -> RepoPath {
        RepoPath::parse(s).unwrap()
    }

    fn fp(s: &str) -> Fingerprint {
        Fingerprint::of_content(s.as_bytes())
    }

    fn known(ids: &mut FileIdAllocator, files: &[(&str, &str)]) -> Vec<Known> {
        files
            .iter()
            .map(|(path, content)| Known {
                file_id: ids.issue().unwrap(),
                path: p(path),
                fingerprint: fp(content),
            })
            .collect()
    }

    fn current(files: &[(&str, &str)]) -> Vec<Current> {
        files
            .iter()
            .map(|(path, content)| Current {
                path: p(path),
                fingerprint: fp(content),
            })
            .collect()
    }

    fn paths(v: &[RepoPath]) -> Vec<&str> {
        v.iter().map(RepoPath::as_str).collect()
    }

    #[test]
    fn each_kind_of_change_is_found() {
        let mut ids = FileIdAllocator::new();
        let before = known(
            &mut ids,
            &[("same.rs", "1"), ("edit.rs", "2"), ("move_me.rs", "3"), ("gone.rs", "4")],
        );
        let after = current(&[("same.rs", "1"), ("edit.rs", "2b"), ("moved.rs", "3"), ("new.rs", "5")]);
        let plan = plan_changes(&before, &after);
        assert_eq!(paths(&plan.unchanged), ["same.rs"]);
        assert_eq!(paths(&plan.modified), ["edit.rs"]);
        assert_eq!(plan.renamed, [(p("move_me.rs"), p("moved.rs"))]);
        assert_eq!(paths(&plan.added), ["new.rs"]);
        assert_eq!(paths(&plan.deleted), ["gone.rs"]);
    }

    #[test]
    fn identical_files_pair_in_path_order_and_leftovers_are_added_or_deleted() {
        let mut ids = FileIdAllocator::new();
        let before = known(&mut ids, &[("a.txt", "x"), ("b.txt", "x"), ("c.txt", "x")]);
        let after = current(&[("n1.txt", "x"), ("n2.txt", "x")]);
        let plan = plan_changes(&before, &after);
        assert_eq!(plan.renamed, [(p("a.txt"), p("n1.txt")), (p("b.txt"), p("n2.txt"))]);
        assert_eq!(paths(&plan.deleted), ["c.txt"]);
        let more = plan_changes(
            &known(&mut FileIdAllocator::new(), &[("a.txt", "x")]),
            &current(&[("n1.txt", "x"), ("n2.txt", "x")]),
        );
        assert_eq!(more.renamed.len(), 1);
        assert_eq!(paths(&more.added), ["n2.txt"]);
    }

    /// The path wins over the content: a file that took the place of another is modified, not a rename.
    #[test]
    fn a_path_match_beats_a_fingerprint_match() {
        let mut ids = FileIdAllocator::new();
        let before = known(&mut ids, &[("a.rs", "A"), ("b.rs", "B")]);
        let after = current(&[("a.rs", "B"), ("b.rs", "B")]);
        let plan = plan_changes(&before, &after);
        assert_eq!(paths(&plan.modified), ["a.rs"]);
        assert_eq!(paths(&plan.unchanged), ["b.rs"]);
        assert!(plan.renamed.is_empty() && plan.added.is_empty() && plan.deleted.is_empty());
    }

    #[test]
    fn the_plan_does_not_depend_on_input_order() {
        let mut ids = FileIdAllocator::new();
        let before = known(&mut ids, &[("a", "1"), ("b", "2"), ("c", "2")]);
        let after = current(&[("x", "2"), ("y", "2"), ("z", "9")]);
        let mut reversed_before = before.clone();
        reversed_before.reverse();
        let mut reversed_after = after.clone();
        reversed_after.reverse();
        assert_eq!(plan_changes(&before, &after), plan_changes(&reversed_before, &reversed_after));
        assert_eq!(plan_changes(&[], &[]), Plan::default());
    }
}
