//! Churn and hot spots: pure functions over commit records. They read no clock (`now` is an argument) and no repository.

use std::collections::{BTreeMap, BTreeSet};

use pacenotes_core::{CommitHash, RepoPath};

/// One commit as the log records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitChanges {
    /// The commit.
    pub commit: CommitHash,
    /// Commit time, seconds since the Unix epoch.
    pub time: i64,
    /// The author's identity as the commit gives it (used to count distinct authors; never logged).
    pub author: String,
    /// The files the commit changed.
    pub files: Vec<RepoPath>,
}

/// How often and how recently one file changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChurn {
    /// The file.
    pub path: RepoPath,
    /// Commits that changed it.
    pub commits: u32,
    /// Distinct authors of those commits.
    pub authors: u32,
    /// Time of the newest such commit.
    pub last_changed: i64,
}

/// A file ranked by recency-weighted churn.
#[derive(Debug, Clone, PartialEq)]
pub struct HotSpot {
    /// The file.
    pub path: RepoPath,
    /// The recency-weighted number of commits: a change `half_life` seconds old counts half as much as one just now.
    pub score: f64,
    /// Commits that changed it.
    pub commits: u32,
    /// Distinct authors.
    pub authors: u32,
    /// Time of the newest change.
    pub last_changed: i64,
}

/// Per-file churn over `commits`, sorted by path.
pub fn churn(commits: &[CommitChanges]) -> Vec<FileChurn> {
    let mut by_file: BTreeMap<&RepoPath, (u32, BTreeSet<&str>, i64)> = BTreeMap::new();
    for c in commits {
        for f in &c.files {
            let e = by_file.entry(f).or_insert((0, BTreeSet::new(), i64::MIN));
            e.0 = e.0.saturating_add(1);
            e.1.insert(c.author.as_str());
            e.2 = e.2.max(c.time);
        }
    }
    by_file
        .into_iter()
        .map(|(path, (commits, authors, last_changed))| FileChurn {
            path: path.clone(),
            commits,
            authors: u32::try_from(authors.len()).unwrap_or(u32::MAX),
            last_changed,
        })
        .collect()
}

/// The `limit` files with the highest recency-weighted churn, best first; equal scores are ordered by path, so the result is
/// deterministic. Each commit counts `2^(-age / half_life_secs)` (age 0 for a commit in the future); `half_life_secs` of 0 is
/// treated as 1.
pub fn rank_hot_spots(commits: &[CommitChanges], now: i64, half_life_secs: u64, limit: usize) -> Vec<HotSpot> {
    let half_life = half_life_secs.max(1) as f64;
    let mut score: BTreeMap<&RepoPath, f64> = BTreeMap::new();
    for c in commits {
        let age = now.saturating_sub(c.time).max(0) as f64;
        let weight = (-age / half_life).exp2();
        for f in &c.files {
            *score.entry(f).or_insert(0.0) += weight;
        }
    }
    let figures: BTreeMap<RepoPath, FileChurn> = churn(commits).into_iter().map(|c| (c.path.clone(), c)).collect();
    let mut ranked: Vec<HotSpot> = score
        .into_iter()
        .filter_map(|(path, score)| {
            let c = figures.get(path)?;
            Some(HotSpot {
                path: path.clone(),
                score,
                commits: c.commits,
                authors: c.authors,
                last_changed: c.last_changed,
            })
        })
        .collect();
    ranked.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    ranked.truncate(limit);
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> RepoPath {
        RepoPath::parse(s).unwrap()
    }

    fn commit(n: u8, time: i64, author: &str, files: &[&str]) -> CommitChanges {
        CommitChanges {
            commit: CommitHash::parse(&format!("{n:02x}").repeat(20)).unwrap(),
            time,
            author: author.into(),
            files: files.iter().map(|f| p(f)).collect(),
        }
    }

    const DAY: i64 = 86_400;

    #[test]
    fn churn_counts_commits_authors_and_the_newest_change() {
        let log = [
            commit(3, 300, "a", &["x.rs", "y.rs"]),
            commit(2, 200, "b", &["x.rs"]),
            commit(1, 100, "a", &["x.rs"]),
        ];
        let c = churn(&log);
        assert_eq!(c.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["x.rs", "y.rs"]);
        assert_eq!((c[0].commits, c[0].authors, c[0].last_changed), (3, 2, 300));
        assert_eq!((c[1].commits, c[1].authors), (1, 1));
    }

    #[test]
    fn recent_changes_outweigh_old_ones() {
        let now = 1000 * DAY;
        // old.rs changed 5 times long ago, fresh.rs 2 times this week
        let mut log = vec![
            commit(1, now - DAY, "a", &["fresh.rs"]),
            commit(2, now - 2 * DAY, "a", &["fresh.rs"]),
        ];
        for i in 0..5 {
            log.push(commit(10 + i, now - 400 * DAY, "a", &["old.rs"]));
        }
        let ranked = rank_hot_spots(&log, now, 30 * DAY as u64, 10);
        assert_eq!(ranked[0].path.as_str(), "fresh.rs");
        assert!(ranked[0].score > 1.8 && ranked[1].score < 0.01, "{ranked:?}");
        assert_eq!((ranked[1].commits, ranked[0].commits), (5, 2));
    }

    #[test]
    fn ranking_is_deterministic_cut_and_total() {
        let log = [commit(1, 100, "a", &["b.rs", "a.rs", "c.rs"])];
        let ranked = rank_hot_spots(&log, 100, 10, 2);
        assert_eq!(
            ranked.iter().map(|h| h.path.as_str()).collect::<Vec<_>>(),
            ["a.rs", "b.rs"],
            "equal scores order by path"
        );
        assert!(rank_hot_spots(&log, 100, 0, 0).is_empty());
        assert!(rank_hot_spots(&[], 0, 1, 5).is_empty());
        // a commit dated after `now` counts fully, a half-life of 0 does not divide by zero
        assert!(rank_hot_spots(&log, 0, 0, 1)[0].score <= 1.0);
        assert_eq!(rank_hot_spots(&log, 100, 10, 3)[0].score, 1.0);
    }
}
