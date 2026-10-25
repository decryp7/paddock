//! Statistics over stored issues, all pure: the daily stability score, per-app and per-kind leaderboards, the trend
//! against the window before, and the per-day crash counts the history chart reads. Local days are computed from a
//! UTC offset the caller passes (the app's clock), never from the machine's time zone database.

use super::{Issue, IssueKind, SourceState, SourceStatus, MAX_LISTED};
use crate::anomaly::{Anomaly, AnomalyKind, Evidence};
use crate::protocol::DayCount;
use crate::time::{Timestamp, DAY, HOUR, MINUTE};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Most apps on the leaderboard.
pub const TOP_APPS: usize = 8;
/// Occurrences of one kind beyond this many on one day no longer lower the score further.
const EXTRA_CAP: u32 = 4;

/// How steady the machine was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stability {
    /// 90 or more.
    Stable,
    /// 70 to 89.
    SomeProblems,
    /// Below 70.
    Unstable,
}

/// The rating for a score.
pub fn stability(score: u32) -> Stability {
    match score {
        90.. => Stability::Stable,
        70..=89 => Stability::SomeProblems,
        _ => Stability::Unstable,
    }
}

impl Stability {
    /// The user-facing word.
    pub fn label(self) -> &'static str {
        match self {
            Stability::Stable => "Stable",
            Stability::SomeProblems => "Some problems",
            Stability::Unstable => "Unstable",
        }
    }
}

/// The stability score of one day, 0 to 100, from how often each kind of issue happened. Each kind costs its weight once;
/// every further occurrence that day costs half as much, up to four more, so a single crash loop cannot flatten the score
/// by itself but does not go unnoticed either. A day with no issues scores 100.
pub fn day_score(counts: &[(IssueKind, u32)]) -> u32 {
    let doubled: u32 = counts
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(k, n)| 2 * k.weight() + k.weight() * (n - 1).min(EXTRA_CAP))
        .sum();
    100u32.saturating_sub(doubled / 2)
}

/// The start of the local day containing `at` (as a UTC timestamp).
pub fn day_start(at: Timestamp, utc_offset_minutes: i32) -> Timestamp {
    let shift = i64::from(utc_offset_minutes) * MINUTE;
    Timestamp((at.millis().saturating_add(shift)).div_euclid(DAY) * DAY - shift)
}

/// One day's score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayScore {
    /// The start of the local day.
    pub day: Timestamp,
    /// 0 to 100.
    pub score: u32,
    /// Issues that day.
    pub issues: u32,
}

/// Scores for the `days` local days ending with the day that contains `now`, oldest first.
pub fn daily_scores(issues: &[Issue], now: Timestamp, days: u32, utc_offset_minutes: i32) -> Vec<DayScore> {
    let today = day_start(now, utc_offset_minutes);
    (0..days)
        .rev()
        .map(|back| {
            let start = today.minus(i64::from(back) * DAY);
            let end = start.plus(DAY);
            let mut counts: HashMap<IssueKind, u32> = HashMap::new();
            for i in issues.iter().filter(|i| i.at >= start && i.at < end) {
                *counts.entry(i.kind).or_default() += 1;
            }
            let list: Vec<(IssueKind, u32)> = counts.into_iter().collect();
            DayScore {
                day: start,
                score: day_score(&list),
                issues: list.iter().map(|(_, n)| n).sum(),
            }
        })
        .collect()
}

/// Application crashes per local day in `[from, to)`, one entry per day (zero days included). `restarts` is 0: no OS log
/// says an app was relaunched.
pub fn crash_days(issues: &[Issue], from: Timestamp, to: Timestamp, utc_offset_minutes: i32) -> Vec<DayCount> {
    let mut out = Vec::new();
    let mut day = day_start(from, utc_offset_minutes);
    while day < to && out.len() < 400 {
        let end = day.plus(DAY);
        let crashes = issues
            .iter()
            .filter(|i| i.kind == IssueKind::AppCrash && i.at >= day && i.at < end)
            .count() as u32;
        out.push(DayCount { day, crashes, restarts: 0 });
        day = end;
    }
    out
}

/// Crashes of one app within [`LOOP_WINDOW_MS`] that make a crash loop.
pub const LOOP_CRASHES: u32 = 3;
/// The window a crash loop is counted in.
pub const LOOP_WINDOW_MS: i64 = HOUR;

/// Apps that crashed [`LOOP_CRASHES`] times or more in the last hour, as restart-loop anomalies (the alert and the health
/// report read these).
pub fn crash_loops(issues: &[Issue], now: Timestamp) -> Vec<Anomaly> {
    let from = now.minus(LOOP_WINDOW_MS);
    let mut by_app: HashMap<&str, Vec<Timestamp>> = HashMap::new();
    for i in issues
        .iter()
        .filter(|i| i.kind == IssueKind::AppCrash && !i.app.is_empty() && i.at > from && i.at <= now)
    {
        by_app.entry(i.app.as_str()).or_default().push(i.at);
    }
    let mut out: Vec<Anomaly> = by_app
        .into_iter()
        .filter(|(_, v)| v.len() as u32 >= LOOP_CRASHES)
        .map(|(app, v)| {
            let n = v.len();
            Anomaly {
                id: format!("restartLoop:{app}"),
                kind: AnomalyKind::RestartLoop,
                app: app.to_string(),
                since: v.iter().copied().min().unwrap_or(now),
                title: format!("{app} crashed {n} times in the last hour"),
                explanation:
                    "Repeated crashes point at a broken update, a bad setting or failing hardware. Check for an update, or reinstall it."
                        .into(),
                evidence: vec![Evidence::new("Crashes in the last hour", n.to_string())],
            }
        })
        .collect();
    out.sort_by(|a, b| a.app.cmp(&b.app));
    out
}

/// Drops an issue another source already reported: the same kind and app within 30 seconds, from a different source (a
/// crash is both a core dump and a journal line). Earlier sources in `issues` win.
pub fn collapse_across_sources(issues: Vec<Issue>) -> Vec<Issue> {
    let mut kept: Vec<Issue> = Vec::with_capacity(issues.len());
    for i in issues {
        let twin = kept
            .iter()
            .any(|k| k.source != i.source && k.kind == i.kind && k.app == i.app && (k.at.millis() - i.at.millis()).abs() <= 30_000);
        if !twin {
            kept.push(i);
        }
    }
    kept
}

/// An application and how often it had trouble.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tally {
    /// The application (or service, or device).
    pub name: String,
    /// Issues in the window.
    pub count: u32,
    /// The latest one.
    pub last: Timestamp,
    /// The kind it had most often.
    pub kind: IssueKind,
}

/// How many issues of one kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindTally {
    /// The kind.
    pub kind: IssueKind,
    /// Issues in the window.
    pub count: u32,
}

/// This window against the one before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trend {
    /// Issues in the window.
    pub current: u32,
    /// Issues in the window of the same length before it.
    pub previous: u32,
}

/// The apps with the most issues in `issues` (issues without an app name are left out), most first.
pub fn leaderboard(issues: &[Issue], limit: usize) -> Vec<Tally> {
    let mut by_app: HashMap<&str, Vec<&Issue>> = HashMap::new();
    for i in issues.iter().filter(|i| !i.app.is_empty()) {
        by_app.entry(i.app.as_str()).or_default().push(i);
    }
    let mut out: Vec<Tally> = by_app
        .into_iter()
        .map(|(name, list)| {
            let mut kinds: HashMap<IssueKind, u32> = HashMap::new();
            for i in &list {
                *kinds.entry(i.kind).or_default() += 1;
            }
            let kind = kinds
                .into_iter()
                .max_by_key(|(k, n)| (*n, std::cmp::Reverse(*k)))
                .map_or(IssueKind::AppCrash, |(k, _)| k);
            Tally {
                name: name.to_string(),
                count: list.len() as u32,
                last: list.iter().map(|i| i.at).max().unwrap_or_default(),
                kind,
            }
        })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(b.last.cmp(&a.last)).then(a.name.cmp(&b.name)));
    out.truncate(limit);
    out
}

/// Issues per kind, most first (kinds with none are left out).
pub fn kind_tally(issues: &[Issue]) -> Vec<KindTally> {
    let mut out: Vec<KindTally> = IssueKind::ALL
        .iter()
        .map(|k| KindTally {
            kind: *k,
            count: issues.iter().filter(|i| i.kind == *k).count() as u32,
        })
        .filter(|t| t.count > 0)
        .collect();
    out.sort_by_key(|k| std::cmp::Reverse(k.count));
    out
}

/// Everything the Issues page shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuesOverview {
    /// Window start.
    pub from: Timestamp,
    /// Window end.
    pub to: Timestamp,
    /// Days in the window.
    pub days: u32,
    /// Today's stability score; none when no source could be read (nothing is known, so nothing is claimed).
    pub score: Option<u32>,
    /// The rating for `score`.
    pub rating: Option<Stability>,
    /// One score per day, oldest first (empty when no source could be read).
    pub daily: Vec<DayScore>,
    /// This window against the one before.
    pub trend: Trend,
    /// The apps with the most issues.
    pub apps: Vec<Tally>,
    /// Issues per kind.
    pub kinds: Vec<KindTally>,
    /// The issues, newest first (at most [`MAX_LISTED`]).
    pub issues: Vec<Issue>,
    /// How many issues the window holds in all.
    pub total: u32,
    /// How each source was last read.
    pub sources: Vec<SourceStatus>,
    /// When the sources were last read.
    pub scanned_at: Option<Timestamp>,
}

/// Builds the page's data. `issues` holds the window and the window before it (twice `days` back from `now`).
pub fn build_overview(
    now: Timestamp,
    days: u32,
    utc_offset_minutes: i32,
    issues: &[Issue],
    sources: Vec<SourceStatus>,
    scanned_at: Option<Timestamp>,
) -> IssuesOverview {
    let today = day_start(now, utc_offset_minutes);
    let from = today.minus(i64::from(days.saturating_sub(1)) * DAY);
    let previous_from = from.minus(i64::from(days) * DAY);
    let mut window: Vec<Issue> = issues.iter().filter(|i| i.at >= from && i.at <= now).cloned().collect();
    window.sort_by(|a, b| b.at.cmp(&a.at).then(a.id.cmp(&b.id)));
    let previous = issues.iter().filter(|i| i.at >= previous_from && i.at < from).count() as u32;
    let any_readable = sources.iter().any(SourceStatus::is_readable);
    let daily = if any_readable {
        daily_scores(&window, now, days, utc_offset_minutes)
    } else {
        Vec::new()
    };
    let score = daily.last().map(|d| d.score);
    let total = window.len() as u32;
    let apps = leaderboard(&window, TOP_APPS);
    let kinds = kind_tally(&window);
    window.truncate(MAX_LISTED);
    IssuesOverview {
        from,
        to: now,
        days,
        rating: score.map(stability),
        score,
        daily,
        trend: Trend { current: total, previous },
        apps,
        kinds,
        issues: window,
        total,
        sources,
        scanned_at,
    }
}

impl SourceState {
    /// Whether the source was read.
    pub fn is_read(&self) -> bool {
        matches!(self, SourceState::Read(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issues::IssueSourceId;

    const T0: i64 = 1_761_091_200_000; // 2025-10-22 00:00 UTC

    fn at(day: i64, hour: i64, kind: IssueKind, app: &str) -> Issue {
        Issue::new(
            IssueSourceId::MacReports,
            Timestamp(T0 + day * DAY + hour * 3_600_000),
            kind,
            app,
            "t",
            "e",
            &format!("{day}-{hour}-{app}"),
        )
    }

    fn ok(source: IssueSourceId) -> SourceStatus {
        SourceStatus {
            source,
            label: "x".into(),
            state: SourceState::Read(1),
            at: Some(Timestamp(0)),
        }
    }

    #[test]
    fn a_quiet_day_is_perfect_and_costs_follow_the_weights() {
        assert_eq!(day_score(&[]), 100);
        assert_eq!(day_score(&[(IssueKind::AppCrash, 0)]), 100);
        assert_eq!(day_score(&[(IssueKind::AppCrash, 1)]), 92);
        assert_eq!(day_score(&[(IssueKind::BadShutdown, 1)]), 80);
        assert_eq!(day_score(&[(IssueKind::AppCrash, 1), (IssueKind::AppHang, 1)]), 87);
    }

    #[test]
    fn repeats_count_half_and_stop_counting() {
        // 8 + 4 per further crash, at most four further.
        assert_eq!(day_score(&[(IssueKind::AppCrash, 2)]), 88);
        assert_eq!(day_score(&[(IssueKind::AppCrash, 5)]), 76);
        assert_eq!(day_score(&[(IssueKind::AppCrash, 5)]), day_score(&[(IssueKind::AppCrash, 500)]));
    }

    #[test]
    fn the_score_never_goes_below_zero() {
        let all: Vec<(IssueKind, u32)> = IssueKind::ALL.iter().map(|k| (*k, 9)).collect();
        assert_eq!(day_score(&all), 0);
    }

    #[test]
    fn ratings_have_thresholds() {
        assert_eq!(stability(100), Stability::Stable);
        assert_eq!(stability(90), Stability::Stable);
        assert_eq!(stability(89), Stability::SomeProblems);
        assert_eq!(stability(70), Stability::SomeProblems);
        assert_eq!(stability(69), Stability::Unstable);
        assert_eq!(Stability::SomeProblems.label(), "Some problems");
        assert_eq!(Stability::Stable.label(), "Stable");
        assert_eq!(Stability::Unstable.label(), "Unstable");
    }

    #[test]
    fn local_days_follow_the_offset() {
        let t = Timestamp(T0 + 23 * 3_600_000); // 23:00 UTC
        assert_eq!(day_start(t, 0), Timestamp(T0));
        // In UTC+2 it is already the next day, which started at 22:00 UTC.
        assert_eq!(day_start(t, 120), Timestamp(T0 + 22 * 3_600_000));
        // In UTC-5 the day started at 05:00 UTC.
        assert_eq!(day_start(t, -300), Timestamp(T0 + 5 * 3_600_000));
        assert_eq!(day_start(Timestamp(T0), 0), Timestamp(T0));
    }

    #[test]
    fn daily_scores_cover_every_day_oldest_first() {
        let issues = vec![
            at(0, 3, IssueKind::AppCrash, "A"),
            at(0, 4, IssueKind::AppCrash, "A"),
            at(2, 9, IssueKind::BadShutdown, ""),
        ];
        let now = Timestamp(T0 + 2 * DAY + 12 * 3_600_000);
        let d = daily_scores(&issues, now, 3, 0);
        assert_eq!(d.iter().map(|x| x.score).collect::<Vec<_>>(), vec![88, 100, 80]);
        assert_eq!(d.iter().map(|x| x.issues).collect::<Vec<_>>(), vec![2, 0, 1]);
        assert_eq!(d[0].day, Timestamp(T0));
        assert_eq!(d[2].day, Timestamp(T0 + 2 * DAY));
        assert!(daily_scores(&[], now, 0, 0).is_empty());
    }

    #[test]
    fn crash_days_count_only_crashes_per_day_with_zero_days() {
        let issues = vec![
            at(0, 1, IssueKind::AppCrash, "A"),
            at(0, 2, IssueKind::AppHang, "A"),
            at(2, 1, IssueKind::AppCrash, "B"),
            at(2, 5, IssueKind::AppCrash, "B"),
        ];
        let days = crash_days(&issues, Timestamp(T0), Timestamp(T0 + 3 * DAY), 0);
        assert_eq!(days.iter().map(|d| d.crashes).collect::<Vec<_>>(), vec![1, 0, 2]);
        assert!(days.iter().all(|d| d.restarts == 0));
        assert!(crash_days(&issues, Timestamp(T0 + DAY), Timestamp(T0), 0).is_empty());
    }

    #[test]
    fn leaderboards_rank_by_count_then_recency() {
        let issues = vec![
            at(0, 1, IssueKind::AppCrash, "Mail"),
            at(0, 2, IssueKind::AppCrash, "Mail"),
            at(0, 3, IssueKind::AppHang, "Mail"),
            at(1, 1, IssueKind::AppCrash, "Safari"),
            at(0, 9, IssueKind::AppCrash, "Notes"),
            at(0, 5, IssueKind::DiskError, ""),
        ];
        let top = leaderboard(&issues, 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].name, "Mail");
        assert_eq!(top[0].count, 3);
        assert_eq!(top[0].kind, IssueKind::AppCrash);
        // Safari and Notes have one each; Safari is newer.
        assert_eq!(top[1].name, "Safari");
        let kinds = kind_tally(&issues);
        assert_eq!(
            kinds[0],
            KindTally {
                kind: IssueKind::AppCrash,
                count: 4
            }
        );
        assert_eq!(kinds.len(), 3);
        assert!(kinds.iter().all(|k| k.count > 0));
    }

    #[test]
    fn three_crashes_in_an_hour_are_a_loop() {
        let now = Timestamp(T0 + 5 * 3_600_000);
        let mk = |min: i64, app: &str, key: &str| {
            Issue::new(
                IssueSourceId::MacReports,
                now.minus(min * MINUTE),
                IssueKind::AppCrash,
                app,
                "t",
                "e",
                key,
            )
        };
        let issues = vec![
            mk(5, "Loopy", "a"),
            mk(20, "Loopy", "b"),
            mk(50, "Loopy", "c"),
            mk(70, "Loopy", "d"),
            mk(10, "Twice", "e"),
            mk(30, "Twice", "f"),
            mk(1, "", "g"),
            mk(2, "", "h"),
            mk(3, "", "i"),
        ];
        let loops = crash_loops(&issues, now);
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].app, "Loopy");
        assert_eq!(loops[0].kind, AnomalyKind::RestartLoop);
        assert_eq!(loops[0].title, "Loopy crashed 3 times in the last hour");
        assert_eq!(loops[0].id, "restartLoop:Loopy");
        assert_eq!(loops[0].since, now.minus(50 * MINUTE));
        assert!(crash_loops(&[], now).is_empty());
        // Hangs are not crashes.
        let hangs: Vec<Issue> = (0..5)
            .map(|n| {
                Issue::new(
                    IssueSourceId::MacReports,
                    now.minus(n * MINUTE),
                    IssueKind::AppHang,
                    "H",
                    "t",
                    "e",
                    &n.to_string(),
                )
            })
            .collect();
        assert!(crash_loops(&hangs, now).is_empty());
    }

    #[test]
    fn the_same_crash_from_two_sources_is_one() {
        let a = Issue::new(
            IssueSourceId::Coredumps,
            Timestamp(1_000),
            IssueKind::AppCrash,
            "foo",
            "t",
            "e",
            "1",
        );
        let b = Issue::new(IssueSourceId::Journal, Timestamp(5_000), IssueKind::AppCrash, "foo", "t", "e", "2");
        let far = Issue::new(
            IssueSourceId::Journal,
            Timestamp(500_000),
            IssueKind::AppCrash,
            "foo",
            "t",
            "e",
            "3",
        );
        let other = Issue::new(IssueSourceId::Journal, Timestamp(2_000), IssueKind::AppCrash, "bar", "t", "e", "4");
        let same_source = Issue::new(
            IssueSourceId::Coredumps,
            Timestamp(2_000),
            IssueKind::AppCrash,
            "foo",
            "t",
            "e",
            "5",
        );
        let kept = collapse_across_sources(vec![a.clone(), b, far.clone(), other.clone(), same_source.clone()]);
        assert_eq!(kept, vec![a, far, other, same_source]);
    }

    #[test]
    fn overview_windows_trends_and_unreadable_sources() {
        let now = Timestamp(T0 + 5 * DAY + 3_600_000);
        let issues = vec![
            at(0, 1, IssueKind::AppCrash, "old"), // previous window (the window is days 3 to 5)
            at(2, 1, IssueKind::AppCrash, "prev"),
            at(3, 1, IssueKind::AppCrash, "a"),
            at(5, 0, IssueKind::AppHang, "b"),
        ];
        let o = build_overview(now, 3, 0, &issues, vec![ok(IssueSourceId::MacReports)], Some(Timestamp(7)));
        assert_eq!(o.total, 2);
        assert_eq!(o.trend, Trend { current: 2, previous: 2 });
        assert_eq!(o.issues[0].app, "b");
        assert_eq!(o.daily.len(), 3);
        assert_eq!(o.score, Some(95));
        assert_eq!(o.rating, Some(Stability::Stable));
        assert_eq!(o.from, Timestamp(T0 + 3 * DAY));
        assert_eq!(o.scanned_at, Some(Timestamp(7)));
        // No readable source: nothing is known, so no score is claimed.
        let blind = build_overview(
            now,
            3,
            0,
            &[],
            vec![SourceStatus {
                source: IssueSourceId::Journal,
                label: "j".into(),
                state: SourceState::Unavailable("no journalctl".into()),
                at: None,
            }],
            None,
        );
        assert_eq!(blind.score, None);
        assert_eq!(blind.rating, None);
        assert!(blind.daily.is_empty());
        assert!(!blind.sources[0].state.is_read());
    }

    #[test]
    fn the_listing_is_capped_but_the_total_is_not() {
        let now = Timestamp(T0 + DAY);
        let issues: Vec<Issue> = (0..(MAX_LISTED as i64 + 25))
            .map(|n| {
                Issue::new(
                    IssueSourceId::MacReports,
                    Timestamp(T0 + n * 1_000),
                    IssueKind::AppCrash,
                    "A",
                    "t",
                    "e",
                    &n.to_string(),
                )
            })
            .collect();
        let o = build_overview(now, 2, 0, &issues, vec![ok(IssueSourceId::MacReports)], None);
        assert_eq!(o.issues.len(), MAX_LISTED);
        assert_eq!(o.total as usize, MAX_LISTED + 25);
    }
}
