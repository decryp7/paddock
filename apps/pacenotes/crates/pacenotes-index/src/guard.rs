//! `GuardedHistory`: the denylist in front of `ReadAt` (ADR 0005).

use pacenotes_core::ports::{ChangeSet, History, HistoryError};
use pacenotes_core::{RepoPath, Rev};
use paddock_jobs::CancelToken;

use crate::denylist::Denylist;

/// A [`History`] that never serves a denied path's content. The object database holds whatever was ever committed (ADR 0004), so
/// a committed `.env` is readable through the bare port; every consumer that serves content (the host's `ReadAt`, MCP, the index)
/// takes its history through this wrapper.
pub struct GuardedHistory<H: History> {
    inner: H,
    denylist: Denylist,
}

impl<H: History> GuardedHistory<H> {
    /// Wraps `inner` with `denylist`.
    pub fn new(inner: H, denylist: Denylist) -> GuardedHistory<H> {
        GuardedHistory { inner, denylist }
    }

    /// The wrapped history, for callers that must not serve content (the churn log reads names only).
    pub fn inner(&self) -> &H {
        &self.inner
    }
}

impl<H: History> History for GuardedHistory<H> {
    fn head(&self, cancel: &CancelToken) -> Result<Rev, HistoryError> {
        self.inner.head(cancel)
    }

    fn read_at(&self, rev: &Rev, path: &RepoPath, max_bytes: u64, cancel: &CancelToken) -> Result<Vec<u8>, HistoryError> {
        if self.denylist.check(path).is_some() {
            return Err(HistoryError::Denied(path.clone()));
        }
        self.inner.read_at(rev, path, max_bytes, cancel)
    }

    /// Names are not content: the list is passed through as it is, so a denied file still shows as changed.
    fn changed_between(&self, from: &Rev, to: &Rev, limit: usize, cancel: &CancelToken) -> Result<ChangeSet, HistoryError> {
        self.inner.changed_between(from, to, limit, cancel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pacenotes_core::CommitHash;
    use pacenotes_core::contract::{HistoryFixture, MemoryHistory, history_contract};

    fn p(s: &str) -> RepoPath {
        RepoPath::parse(s).unwrap()
    }

    fn rev(c: char) -> Rev {
        Rev::Commit(CommitHash::parse(&c.to_string().repeat(40)).unwrap())
    }

    fn guarded() -> (GuardedHistory<MemoryHistory>, Rev, Rev) {
        let (a, b) = (rev('a'), rev('b'));
        let files = vec![
            (0, p("src/keep.rs"), b"fn keep() {}\n".to_vec()),
            (1, p("src/keep.rs"), b"fn keep() {}\n".to_vec()),
            (0, p("src/empty.txt"), Vec::new()),
            (1, p("src/empty.txt"), Vec::new()),
            (0, p("src/changed.rs"), b"v1".to_vec()),
            (1, p("src/changed.rs"), b"v2".to_vec()),
            (1, p("src/added.rs"), b"new".to_vec()),
            (0, p("src/gone.rs"), b"old".to_vec()),
            (1, p(".env"), b"API_TOKEN=PN1-DUMMY".to_vec()),
        ];
        let inner = MemoryHistory::new(vec![a.clone(), b.clone()], files);
        (GuardedHistory::new(inner, Denylist::new()), a, b)
    }

    #[test]
    fn a_denied_path_is_never_served_but_everything_else_is() {
        let (g, a, b) = guarded();
        let live = CancelToken::new();
        assert_eq!(g.read_at(&b, &p(".env"), 1000, &live), Err(HistoryError::Denied(p(".env"))));
        assert_eq!(
            g.inner().read_at(&b, &p(".env"), 1000, &live).unwrap(),
            b"API_TOKEN=PN1-DUMMY",
            "the bare port does serve it"
        );
        assert!(HistoryError::Denied(p(".env")).to_string().contains("withheld"));
        assert_eq!(g.read_at(&b, &p("src/changed.rs"), 100, &live).unwrap(), b"v2");
        let names: Vec<_> = g
            .changed_between(&a, &b, 10, &live)
            .unwrap()
            .changes
            .into_iter()
            .map(|c| c.path.to_string())
            .collect();
        assert!(names.contains(&".env".to_owned()), "names pass, content does not");
    }

    #[test]
    fn the_guard_still_honours_the_history_contract() {
        let (g, a, b) = guarded();
        history_contract(&HistoryFixture {
            history: &g,
            rev_a: a,
            rev_b: b,
            present: (p("src/changed.rs"), b"v2".to_vec()),
            empty: p("src/empty.txt"),
            missing: p("nope.rs"),
        });
    }
}
