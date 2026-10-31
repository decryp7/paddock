//! The tray popup's widget values (family ADR 0035, Spotter's catalogue in `spotter_core::widgets`): one pure function
//! from the watches the engine already holds, the settings and the clock to what each placed widget shows. The UI draws
//! these and decides nothing (AGENTS.md "Where logic lives"): what is attention, what is new, what is next and when quiet
//! hours are in force are all decided here.
//!
//! Nothing here reads a clock, a file or the network: time is passed in, and no widget starts a check on its own.

use crate::view::WatchSummary;
use paddock_widgets::{Layout, PlacementState, Progress, SurfaceNotice, SurfaceValues, Tone, WidgetAction, WidgetState};
use spotter_core::state::WatchStatus;
use spotter_core::time::ClockTime;
use spotter_core::watch::Settings;
use spotter_core::widgets::{
    ChangeRow, FailureKind, FailureRow, NotificationMode, PopoverState, PopoverStatus, PopoverValue, PopoverValues, PopoverWidget, SiteRow,
    SiteState, WidgetActionId, CHANGE_ROWS, FAILURE_ROWS, SITE_ROWS,
};

/// Watches due within this long after the next one are counted with it (milliseconds).
const SAME_TIME_MS: i64 = 60_000;

/// What the builders read: the watches, the settings and the time (milliseconds since the epoch, and the local time of day).
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    /// Every watch.
    pub watches: &'a [WatchSummary],
    /// The app settings.
    pub settings: &'a Settings,
    /// Now, in milliseconds since the epoch.
    pub now_ms: i64,
    /// The local time of day (for quiet hours).
    pub local: ClockTime,
}

fn act(action: WidgetActionId, label: &str) -> Option<WidgetAction<WidgetActionId>> {
    Some(WidgetAction {
        action,
        label: label.to_string(),
    })
}

fn ready(value: PopoverValue, now_ms: i64) -> PopoverState {
    WidgetState::Ready {
        value,
        as_of: now_ms,
        stale_since: None,
    }
}

/// "Nothing here yet": the widget tells what it will show and offers the step that starts it.
fn empty(title: &str, detail: &str, next: Option<WidgetAction<WidgetActionId>>) -> PopoverState {
    WidgetState::Empty {
        title: title.into(),
        detail: detail.into(),
        next,
        calm: false,
    }
}

/// "All good": the widget has looked and there is nothing to act on (drawn as one calm row with a check).
fn calm(title: &str, detail: &str, next: Option<WidgetAction<WidgetActionId>>) -> PopoverState {
    WidgetState::Empty {
        title: title.into(),
        detail: detail.into(),
        next,
        calm: true,
    }
}

/// "12 min ago": how long ago `then_ms` was (a time of the future reads as just now).
fn ago(now_ms: i64, then_ms: i64) -> String {
    let minutes = (now_ms - then_ms).max(0) / 60_000;
    match minutes {
        0 => "just now".to_string(),
        1..=59 => format!("{minutes} min ago"),
        60..=1439 => format!("{} h ago", minutes / 60),
        _ => format!("{} d ago", minutes / 1440),
    }
}

/// Active (unpaused) watches that were never checked.
fn unchecked(i: &Inputs) -> usize {
    i.watches.iter().filter(|w| !w.paused && w.last_check_ms.is_none()).count()
}

/// How the watches are doing overall: the header pill. Failures first, then "never checked", else all good.
pub fn popover_status(i: &Inputs) -> PopoverStatus {
    let failing = i.watches.iter().filter(|w| !w.paused && failure_kind(&w.status).is_some()).count();
    if failing > 0 {
        return PopoverStatus::Failing { count: count(failing) };
    }
    let never = unchecked(i);
    if never > 0 && never == i.watches.iter().filter(|w| !w.paused).count() {
        return PopoverStatus::Unknown;
    }
    if never > 0 {
        return PopoverStatus::Unchecked { count: count(never) };
    }
    if i.watches.iter().all(|w| w.paused) {
        PopoverStatus::Unknown
    } else {
        PopoverStatus::AllGood
    }
}

fn site_state(w: &WatchSummary) -> SiteState {
    if w.paused {
        SiteState::Paused
    } else if w.checking {
        SiteState::Checking
    } else if w.status.is_failure() {
        SiteState::Attention
    } else if w.unseen {
        SiteState::New
    } else if matches!(w.status, WatchStatus::Pending) {
        SiteState::Pending
    } else {
        SiteState::Ok
    }
}

fn failure_kind(s: &WatchStatus) -> Option<(FailureKind, &str)> {
    match s {
        WatchStatus::Unreachable(m) => Some((FailureKind::Unreachable, m)),
        WatchStatus::AuthNeeded(m) => Some((FailureKind::AuthNeeded, m)),
        WatchStatus::NeedsYou(m) => Some((FailureKind::NeedsYou, m)),
        WatchStatus::Invalid(m) => Some((FailureKind::Invalid, m)),
        WatchStatus::Failed(m) => Some((FailureKind::Failed, m)),
        WatchStatus::Pending | WatchStatus::Ok | WatchStatus::NoMatch => None,
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn watched_sites(i: &Inputs) -> PopoverState {
    if i.watches.is_empty() {
        return empty(
            "See your watches here",
            "No watches yet.",
            act(WidgetActionId::AddWatch, "Add a watch"),
        );
    }
    let mut rows: Vec<SiteRow> = i
        .watches
        .iter()
        .map(|w| SiteRow {
            id: w.id.clone(),
            name: w.name.clone(),
            state: site_state(w),
            version: w.latest.as_ref().map(|l| l.version.clone()),
        })
        .collect();
    // Attention first, then what is new, running and quiet; the person's own order breaks ties (stable sort).
    let rank = |s: SiteState| match s {
        SiteState::Attention => 0,
        SiteState::New => 1,
        SiteState::Checking => 2,
        SiteState::Pending => 3,
        SiteState::Ok => 4,
        SiteState::Paused => 5,
    };
    rows.sort_by_key(|r| rank(r.state));
    let total = rows.len();
    rows.truncate(SITE_ROWS);
    let more = count(total - rows.len());
    ready(
        PopoverValue::WatchedSites {
            total: count(total),
            rows,
            more,
        },
        i.now_ms,
    )
}

fn recent_changes(i: &Inputs) -> PopoverState {
    let mut found: Vec<ChangeRow> = i
        .watches
        .iter()
        .filter_map(|w| {
            w.latest.as_ref().map(|l| ChangeRow {
                id: w.id.clone(),
                name: w.name.clone(),
                version: l.version.clone(),
                first_seen_ms: l.first_seen_ms,
                unseen: w.unseen,
            })
        })
        .collect();
    if found.is_empty() {
        return if i.watches.is_empty() {
            empty(
                "See new releases here",
                "No watches yet.",
                act(WidgetActionId::AddWatch, "Add a watch"),
            )
        } else {
            empty(
                "See new releases here",
                "Nothing found yet.",
                act(WidgetActionId::CheckAll, "Check all now"),
            )
        };
    }
    found.sort_by(|a, b| b.first_seen_ms.cmp(&a.first_seen_ms).then_with(|| a.name.cmp(&b.name)));
    let total = found.len();
    let unseen = count(found.iter().filter(|c| c.unseen).count());
    found.truncate(CHANGE_ROWS);
    let more = count(total - found.len());
    ready(PopoverValue::RecentChanges { rows: found, more, unseen }, i.now_ms)
}

fn next_check(i: &Inputs) -> PopoverState {
    let running = i.watches.iter().filter(|w| w.checking).count();
    let due = || {
        i.watches
            .iter()
            .filter(|w| !w.paused && !w.checking)
            .filter_map(|w| w.next_check_ms.map(|at| (at, w)))
            .min_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name.cmp(&b.1.name)))
    };
    let next = due().map(|(at, w)| {
        let also = i
            .watches
            .iter()
            .filter(|o| !o.paused && !o.checking && o.id != w.id)
            .filter(|o| o.next_check_ms.is_some_and(|t| t <= at.saturating_add(SAME_TIME_MS)))
            .count();
        PopoverValue::NextCheck {
            id: w.id.clone(),
            name: w.name.clone(),
            at_ms: at,
            also_due: count(also),
            due: at <= i.now_ms,
        }
    });
    if running > 0 {
        // A busy source is Working, never an error (ADR 0035); the next check stays under it.
        return WidgetState::Working {
            title: "Checking now".into(),
            // The engine knows which watches run, not how many finished this round: an indeterminate bar, never "n so far".
            phase: format!("Checking {running} {}", if running == 1 { "watch" } else { "watches" }),
            progress: Progress::Unknown,
            last: next,
            cancel: None,
        };
    }
    match next {
        Some(v) => ready(v, i.now_ms),
        None if i.watches.is_empty() => empty(
            "See when Spotter looks next",
            "No watches yet.",
            act(WidgetActionId::AddWatch, "Add a watch"),
        ),
        None if i.watches.iter().all(|w| w.paused) => empty(
            "See when Spotter looks next",
            "Every watch is paused.",
            act(WidgetActionId::OpenWatches, "Open watches"),
        ),
        None => empty(
            "See when Spotter looks next",
            "No check is scheduled yet.",
            act(WidgetActionId::CheckAll, "Check all now"),
        ),
    }
}

fn quiet_hours(i: &Inputs) -> PopoverState {
    match i.settings.quiet_hours {
        None => empty(
            "Mute notifications overnight",
            "No quiet hours are set.",
            act(WidgetActionId::OpenSettings, "Set quiet hours"),
        ),
        Some(q) => ready(
            PopoverValue::QuietHours {
                start: String::from(q.start),
                end: String::from(q.end),
                active: q.contains(i.local),
            },
            i.now_ms,
        ),
    }
}

fn failures(i: &Inputs) -> PopoverState {
    let mut rows: Vec<FailureRow> = i
        .watches
        .iter()
        .filter(|w| !w.paused)
        .filter_map(|w| {
            failure_kind(&w.status).map(|(kind, message)| FailureRow {
                id: w.id.clone(),
                name: w.name.clone(),
                kind,
                message: message.to_string(),
                failures: w.failures,
            })
        })
        .collect();
    if rows.is_empty() {
        // Calm only when watches exist and have been looked at: no watches, or none checked yet, is "nothing here yet".
        if i.watches.is_empty() {
            return empty("See failures here", "No watches yet.", act(WidgetActionId::AddWatch, "Add a watch"));
        }
        return match i.watches.iter().filter_map(|w| w.last_check_ms).max() {
            None => empty(
                "See failures here",
                "Nothing is checked yet.",
                act(WidgetActionId::CheckAll, "Check all now"),
            ),
            // Some never checked: "no failures" would claim more than the engine knows.
            Some(_) if unchecked(i) > 0 => empty(
                "No failures so far",
                &format!("{} not checked yet.", unchecked(i)),
                act(WidgetActionId::CheckAll, "Check all now"),
            ),
            Some(at) => calm(
                "No failures",
                &format!("Last checked {}.", ago(i.now_ms, at)),
                act(WidgetActionId::OpenWatches, "Open watches"),
            ),
        };
    }
    rows.sort_by(|a, b| b.failures.cmp(&a.failures).then_with(|| a.name.cmp(&b.name)));
    let total = rows.len();
    rows.truncate(FAILURE_ROWS);
    let more = count(total - rows.len());
    ready(PopoverValue::Failures { rows, more }, i.now_ms)
}

/// Whether, and until when, notifications go out.
pub fn notification_mode(settings: &Settings, now_ms: i64) -> NotificationMode {
    if !settings.notifications {
        NotificationMode::Off
    } else {
        match settings.notifications_paused_until {
            Some(until_ms) if until_ms > now_ms => NotificationMode::Paused { until_ms },
            _ => NotificationMode::On,
        }
    }
}

fn notifications(i: &Inputs) -> PopoverState {
    ready(
        PopoverValue::Notifications {
            mode: notification_mode(i.settings, i.now_ms),
        },
        i.now_ms,
    )
}

/// One widget's state.
pub fn state(kind: PopoverWidget, i: &Inputs) -> PopoverState {
    match kind {
        PopoverWidget::WatchedSites => watched_sites(i),
        PopoverWidget::RecentChanges => recent_changes(i),
        PopoverWidget::NextCheck => next_check(i),
        PopoverWidget::QuietHours => quiet_hours(i),
        PopoverWidget::Failures => failures(i),
        PopoverWidget::Notifications => notifications(i),
    }
}

/// The surface notice: paused notifications are said once above the widgets, unless the Notifications widget already says it.
fn notice(layout: &Layout<PopoverWidget>, i: &Inputs) -> Option<SurfaceNotice<WidgetActionId>> {
    let NotificationMode::Paused { until_ms } = notification_mode(i.settings, i.now_ms) else {
        return None;
    };
    if layout.placements.iter().any(|p| p.widget == PopoverWidget::Notifications) {
        return None;
    }
    let minutes = (until_ms - i.now_ms + 59_999) / 60_000;
    let detail = if minutes >= 120 {
        format!("Paused for {} more hours.", (minutes + 30) / 60)
    } else if minutes >= 60 {
        "Paused for about an hour more.".to_string()
    } else {
        format!("Paused for {minutes} more minute{}.", if minutes == 1 { "" } else { "s" })
    };
    Some(SurfaceNotice {
        tone: Tone::Info,
        title: "Notifications are paused".into(),
        detail,
        action: act(WidgetActionId::ResumeNotifications, "Resume"),
    })
}

/// All values of the popup for `layout`.
pub fn popover_values(layout: &Layout<PopoverWidget>, i: &Inputs) -> PopoverValues {
    SurfaceValues {
        notice: notice(layout, i),
        placements: layout
            .placements
            .iter()
            .map(|p| PlacementState {
                id: p.id.clone(),
                state: state(p.widget, i),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::ReleaseView;
    use paddock_widgets::{Edit, Stored};
    use spotter_core::time::QuietHours;
    use spotter_core::widgets::PopoverCatalogue;

    const NOW: i64 = 1_800_000_000_000;

    fn release(version: &str, first_seen_ms: i64) -> ReleaseView {
        ReleaseView {
            version: version.into(),
            prerelease: false,
            name: "Tool".into(),
            location: "https://example.org/".into(),
            published_at_ms: None,
            first_seen_ms,
            size: None,
            sha256: None,
            product: None,
            channel: String::new(),
            fields: Vec::new(),
            artifacts: Vec::new(),
            updated_in_place: false,
            upgrade_command: None,
        }
    }

    fn watch(id: &str) -> WatchSummary {
        WatchSummary {
            id: id.into(),
            name: format!("Tool {id}"),
            source_kind: "github".into(),
            source_label: "GitHub".into(),
            source_description: "example.org/tool".into(),
            status: WatchStatus::Ok,
            paused: false,
            checking: false,
            latest: Some(release("1.2.0", NOW - 1000)),
            acknowledged: None,
            installed: None,
            unseen: false,
            newer_than_installed: None,
            last_check_ms: None,
            next_check_ms: None,
            failures: 0,
            schedule_text: "Every hour".into(),
            product_count: 0,
        }
    }

    fn settings() -> Settings {
        Settings::default()
    }

    fn with<R>(watches: &[WatchSummary], s: &Settings, f: impl FnOnce(&Inputs) -> R) -> R {
        let i = Inputs {
            watches,
            settings: s,
            now_ms: NOW,
            local: ClockTime::new(12, 0).unwrap(),
        };
        f(&i)
    }

    fn value_of(state: PopoverState) -> PopoverValue {
        match state {
            WidgetState::Ready { value, .. } => value,
            other => panic!("not ready: {other:?}"),
        }
    }

    fn empty_of(state: PopoverState) -> (String, String, Option<WidgetActionId>) {
        match state {
            WidgetState::Empty { title, detail, next, .. } => (title, detail, next.map(|n| n.action)),
            other => panic!("not empty: {other:?}"),
        }
    }

    #[test]
    fn no_watches_is_an_empty_state_that_starts_a_watch() {
        let s = settings();
        with(&[], &s, |i| {
            for kind in [PopoverWidget::WatchedSites, PopoverWidget::RecentChanges, PopoverWidget::NextCheck] {
                let (title, detail, next) = empty_of(state(kind, i));
                assert!(title.starts_with("See "), "{title}");
                assert_eq!(detail, "No watches yet.");
                assert_eq!(next, Some(WidgetActionId::AddWatch));
            }
        });
    }

    #[test]
    fn watched_sites_put_attention_first_and_cap_the_rows() {
        let mut ws: Vec<WatchSummary> = (0..6).map(|n| watch(&format!("w{n}"))).collect();
        ws[5].status = WatchStatus::Unreachable("offline".into());
        ws[4].unseen = true;
        ws[3].paused = true;
        ws[2].checking = true;
        let s = settings();
        let PopoverValue::WatchedSites { total, rows, more } = with(&ws, &s, |i| value_of(state(PopoverWidget::WatchedSites, i))) else {
            panic!("watched sites")
        };
        assert_eq!((total, rows.len(), more), (6, 4, 2));
        let states: Vec<SiteState> = rows.iter().map(|r| r.state).collect();
        assert_eq!(states, [SiteState::Attention, SiteState::New, SiteState::Checking, SiteState::Ok]);
        assert_eq!(rows[0].id, "w5");
        assert_eq!(rows[0].version.as_deref(), Some("1.2.0"));
    }

    #[test]
    fn a_pending_watch_is_listed_before_an_ok_one_and_after_a_running_one() {
        let mut ok = watch("ok");
        ok.name = "Ok tool".into();
        let mut pending = watch("pending");
        pending.status = WatchStatus::Pending;
        let mut running = watch("running");
        running.checking = true;
        let s = settings();
        let PopoverValue::WatchedSites { rows, .. } =
            with(&[ok, pending, running], &s, |i| value_of(state(PopoverWidget::WatchedSites, i)))
        else {
            panic!("watched sites")
        };
        let order: Vec<(&str, SiteState)> = rows.iter().map(|r| (r.id.as_str(), r.state)).collect();
        assert_eq!(
            order,
            [
                ("running", SiteState::Checking),
                ("pending", SiteState::Pending),
                ("ok", SiteState::Ok)
            ]
        );
    }

    #[test]
    fn the_pause_notice_words_the_minute_hour_and_hours_boundaries() {
        let layout = Stored::<PopoverCatalogue>::default().layout().clone();
        let detail = |minutes: i64| {
            let mut s = settings();
            s.notifications_paused_until = Some(NOW + minutes * 60_000);
            with(&[], &s, |i| popover_values(&layout, i)).notice.unwrap().detail
        };
        assert_eq!(detail(59), "Paused for 59 more minutes.");
        assert_eq!(detail(60), "Paused for about an hour more.");
        assert_eq!(detail(119), "Paused for about an hour more.");
        assert_eq!(detail(120), "Paused for 2 more hours.");
        assert_eq!(detail(149), "Paused for 2 more hours.");
        assert_eq!(detail(150), "Paused for 3 more hours.");
    }

    #[test]
    fn a_pending_watch_is_pending_and_a_paused_failing_watch_is_paused() {
        let mut a = watch("a");
        a.status = WatchStatus::Pending;
        let mut b = watch("b");
        b.paused = true;
        b.status = WatchStatus::Failed("x".into());
        assert_eq!(site_state(&a), SiteState::Pending);
        assert_eq!(site_state(&b), SiteState::Paused);
    }

    #[test]
    fn recent_changes_list_the_newest_releases_first() {
        let mut ws: Vec<WatchSummary> = (0..5).map(|n| watch(&format!("w{n}"))).collect();
        for (n, w) in ws.iter_mut().enumerate() {
            w.latest = Some(release("2.0.0", NOW - 10_000 + n as i64));
        }
        ws[0].latest = None;
        ws[4].unseen = true;
        let s = settings();
        let PopoverValue::RecentChanges { rows, more, unseen } = with(&ws, &s, |i| value_of(state(PopoverWidget::RecentChanges, i))) else {
            panic!("recent changes")
        };
        // w0 has no release, so four are found; the newest first, the first CHANGE_ROWS listed, the rest counted.
        assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["w4"]);
        assert_eq!((more, unseen), (3, 1), "four found, one listed, one of them unseen");
        assert!(rows[0].unseen);
    }

    #[test]
    fn recent_changes_with_watches_but_no_release_offers_a_check() {
        let mut w = watch("a");
        w.latest = None;
        let s = settings();
        let (_, detail, next) = empty_of(with(&[w], &s, |i| state(PopoverWidget::RecentChanges, i)));
        assert_eq!(detail, "Nothing found yet.");
        assert_eq!(next, Some(WidgetActionId::CheckAll));
    }

    #[test]
    fn next_check_is_the_earliest_unpaused_watch_and_counts_those_due_with_it() {
        let mut ws: Vec<WatchSummary> = (0..4).map(|n| watch(&format!("w{n}"))).collect();
        ws[0].next_check_ms = Some(NOW + 600_000);
        ws[1].next_check_ms = Some(NOW + 300_000);
        ws[2].next_check_ms = Some(NOW + 330_000);
        ws[3].next_check_ms = Some(NOW + 1000);
        ws[3].paused = true;
        let s = settings();
        let PopoverValue::NextCheck { id, at_ms, also_due, .. } = with(&ws, &s, |i| value_of(state(PopoverWidget::NextCheck, i))) else {
            panic!("next check")
        };
        assert_eq!((id.as_str(), at_ms, also_due), ("w1", NOW + 300_000, 1));
    }

    #[test]
    fn next_check_while_a_check_runs_is_working_and_indeterminate_and_keeps_the_next_one() {
        let mut ws = vec![watch("a"), watch("b"), watch("c")];
        ws[0].checking = true;
        ws[1].checking = true;
        ws[2].next_check_ms = Some(NOW + 5000);
        let s = settings();
        match with(&ws, &s, |i| state(PopoverWidget::NextCheck, i)) {
            WidgetState::Working {
                phase,
                progress: Progress::Unknown,
                last: Some(PopoverValue::NextCheck { id, .. }),
                cancel: None,
                ..
            } => assert_eq!((phase.as_str(), id.as_str()), ("Checking 2 watches", "c")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn next_check_explains_why_nothing_is_scheduled() {
        let s = settings();
        let mut paused = watch("a");
        paused.paused = true;
        let (_, detail, next) = empty_of(with(&[paused], &s, |i| state(PopoverWidget::NextCheck, i)));
        assert_eq!(
            (detail.as_str(), next),
            ("Every watch is paused.", Some(WidgetActionId::OpenWatches))
        );
        let (_, detail, next) = empty_of(with(&[watch("b")], &s, |i| state(PopoverWidget::NextCheck, i)));
        assert_eq!(
            (detail.as_str(), next),
            ("No check is scheduled yet.", Some(WidgetActionId::CheckAll))
        );
    }

    #[test]
    fn quiet_hours_say_whether_they_are_in_force_now() {
        let mut s = settings();
        let (title, _, next) = empty_of(with(&[], &s, |i| state(PopoverWidget::QuietHours, i)));
        assert!(title.starts_with("Mute"));
        assert_eq!(next, Some(WidgetActionId::OpenSettings));
        s.quiet_hours = Some(QuietHours {
            start: ClockTime::new(22, 0).unwrap(),
            end: ClockTime::new(7, 0).unwrap(),
        });
        let day = value_of(with(&[], &s, |i| state(PopoverWidget::QuietHours, i)));
        assert_eq!(
            day,
            PopoverValue::QuietHours {
                start: "22:00".into(),
                end: "07:00".into(),
                active: false
            }
        );
        let night = Inputs {
            watches: &[],
            settings: &s,
            now_ms: NOW,
            local: ClockTime::new(23, 30).unwrap(),
        };
        assert!(matches!(
            value_of(state(PopoverWidget::QuietHours, &night)),
            PopoverValue::QuietHours { active: true, .. }
        ));
    }

    #[test]
    fn failures_list_the_worst_first_skip_paused_watches_and_offer_the_watch() {
        let mut ws: Vec<WatchSummary> = (0..5).map(|n| watch(&format!("w{n}"))).collect();
        ws[0].status = WatchStatus::Unreachable("The share is offline.".into());
        ws[0].failures = 2;
        ws[1].status = WatchStatus::NeedsYou("Log in.".into());
        ws[1].failures = 7;
        ws[2].status = WatchStatus::Failed("Bad reply.".into());
        ws[2].failures = 1;
        ws[3].status = WatchStatus::AuthNeeded("Token refused.".into());
        ws[3].failures = 3;
        ws[4].status = WatchStatus::Invalid("No such kind.".into());
        ws[4].paused = true;
        let s = settings();
        let PopoverValue::Failures { rows, more } = with(&ws, &s, |i| value_of(state(PopoverWidget::Failures, i))) else {
            panic!("failures")
        };
        // Worst first: the failing, unpaused watches are w1 (7), w3 (3), w0 (2) and w2 (1); one is listed, three are counted.
        assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["w1"]);
        assert_eq!(
            (more, rows[0].kind, rows[0].message.as_str()),
            (3, FailureKind::NeedsYou, "Log in.")
        );
    }

    #[test]
    fn no_failures_is_calm_only_once_watches_were_checked_and_says_when() {
        let s = settings();
        let mut a = watch("a");
        a.last_check_ms = Some(NOW - 12 * 60_000);
        let calm_of = |state: PopoverState| match state {
            WidgetState::Empty { title, detail, next, calm } => (title, detail, next.map(|n| n.action), calm),
            other => panic!("not empty: {other:?}"),
        };
        let (title, detail, next, calm) = calm_of(with(&[a], &s, |i| state(PopoverWidget::Failures, i)));
        assert_eq!(
            (title.as_str(), detail.as_str(), next, calm),
            ("No failures", "Last checked 12 min ago.", Some(WidgetActionId::OpenWatches), true)
        );
        // No watches: nothing to be fine about, and the step starts one.
        let (title, detail, next, calm) = calm_of(with(&[], &s, |i| state(PopoverWidget::Failures, i)));
        assert_eq!(
            (title.as_str(), detail.as_str(), next, calm),
            ("See failures here", "No watches yet.", Some(WidgetActionId::AddWatch), false)
        );
        // Watches that were never checked: not "fine" yet, and the step checks them.
        let (_, detail, next, calm) = calm_of(with(&[watch("b")], &s, |i| state(PopoverWidget::Failures, i)));
        assert_eq!(
            (detail.as_str(), next, calm),
            ("Nothing is checked yet.", Some(WidgetActionId::CheckAll), false)
        );
    }

    #[test]
    fn a_never_checked_watch_keeps_the_failures_row_from_claiming_all_good() {
        let s = settings();
        let mut a = watch("a");
        a.last_check_ms = Some(NOW - 60_000);
        let (title, detail, next, calm) = match with(&[a, watch("b"), watch("c")], &s, |i| state(PopoverWidget::Failures, i)) {
            WidgetState::Empty { title, detail, next, calm } => (title, detail, next.map(|n| n.action), calm),
            other => panic!("not empty: {other:?}"),
        };
        assert_eq!(
            (title.as_str(), detail.as_str(), next, calm),
            ("No failures so far", "2 not checked yet.", Some(WidgetActionId::CheckAll), false)
        );
    }

    #[test]
    fn the_header_status_is_failing_unchecked_all_good_or_unknown() {
        let s = settings();
        let st = |ws: &[WatchSummary]| with(ws, &s, popover_status);
        let mut ok = watch("ok");
        ok.last_check_ms = Some(NOW);
        let mut bad = watch("bad");
        bad.status = WatchStatus::Failed("x".into());
        let mut paused_bad = watch("pb");
        paused_bad.status = WatchStatus::Failed("y".into());
        paused_bad.paused = true;
        assert_eq!(st(&[]), PopoverStatus::Unknown);
        assert_eq!(st(&[watch("never")]), PopoverStatus::Unknown, "none checked yet says nothing");
        assert_eq!(st(&[ok.clone()]), PopoverStatus::AllGood);
        assert_eq!(st(&[ok.clone(), watch("never")]), PopoverStatus::Unchecked { count: 1 });
        assert_eq!(
            st(&[ok.clone(), bad.clone(), paused_bad.clone(), bad.clone()]),
            PopoverStatus::Failing { count: 2 },
            "paused ones do not count"
        );
        assert_eq!(st(&[paused_bad]), PopoverStatus::Unknown);
    }

    #[test]
    fn the_next_check_is_due_when_its_time_is_not_after_now() {
        let s = settings();
        let due = |offset: i64| {
            let mut w = watch("a");
            w.next_check_ms = Some(NOW + offset);
            match with(&[w], &s, |i| value_of(state(PopoverWidget::NextCheck, i))) {
                PopoverValue::NextCheck { due, .. } => due,
                other => panic!("{other:?}"),
            }
        };
        assert!(due(-90_000), "overdue");
        assert!(due(0), "exactly now");
        assert!(!due(1), "a moment away");
    }

    #[test]
    fn how_long_ago_is_worded_in_minutes_hours_and_days() {
        assert_eq!(ago(NOW, NOW + 5), "just now");
        assert_eq!(ago(NOW, NOW - 30_000), "just now");
        assert_eq!(ago(NOW, NOW - 59 * 60_000), "59 min ago");
        assert_eq!(ago(NOW, NOW - 60 * 60_000), "1 h ago");
        assert_eq!(ago(NOW, NOW - 1439 * 60_000), "23 h ago");
        assert_eq!(ago(NOW, NOW - 1440 * 60_000), "1 d ago");
    }

    #[test]
    fn notifications_are_on_paused_or_off() {
        let mut s = settings();
        assert_eq!(notification_mode(&s, NOW), NotificationMode::On);
        s.notifications_paused_until = Some(NOW + 1);
        assert_eq!(notification_mode(&s, NOW), NotificationMode::Paused { until_ms: NOW + 1 });
        s.notifications_paused_until = Some(NOW);
        assert_eq!(notification_mode(&s, NOW), NotificationMode::On, "a pause that ended is over");
        s.notifications = false;
        s.notifications_paused_until = Some(NOW + 1);
        assert_eq!(notification_mode(&s, NOW), NotificationMode::Off);
        let v = value_of(with(&[], &s, |i| state(PopoverWidget::Notifications, i)));
        assert_eq!(
            v,
            PopoverValue::Notifications {
                mode: NotificationMode::Off
            }
        );
    }

    #[test]
    fn the_surface_says_paused_once_unless_the_notifications_widget_does() {
        let mut s = settings();
        s.notifications_paused_until = Some(NOW + 20 * 60_000);
        // The fresh-install popup has no Notifications widget; a layout that has one is built by adding it.
        let layout = Stored::<PopoverCatalogue>::default().layout().clone();
        let mut shown = Stored::<PopoverCatalogue>::default();
        shown
            .edit(paddock_widgets::Edit::Add {
                widget: PopoverWidget::Notifications,
                span: None,
                at: None,
            })
            .unwrap();
        let n = with(&[], &s, |i| popover_values(&layout, i)).notice.unwrap();
        assert_eq!(
            (n.tone, n.title.as_str(), n.detail.as_str()),
            (Tone::Info, "Notifications are paused", "Paused for 20 more minutes.")
        );
        assert_eq!(n.action.unwrap().action, WidgetActionId::ResumeNotifications);
        assert!(with(&[], &s, |i| popover_values(shown.layout(), i)).notice.is_none());
        s.notifications_paused_until = None;
        assert!(with(&[], &s, |i| popover_values(&layout, i)).notice.is_none());
    }

    #[test]
    fn the_pause_is_worded_in_minutes_an_hour_or_hours() {
        let layout = Stored::<PopoverCatalogue>::default().layout().clone();
        let mut layout = layout;
        layout.placements.clear();
        let detail = |mins: i64| {
            let mut s = settings();
            s.notifications_paused_until = Some(NOW + mins * 60_000);
            with(&[], &s, |i| popover_values(&layout, i)).notice.unwrap().detail
        };
        assert_eq!(detail(1), "Paused for 1 more minute.");
        assert_eq!(detail(90), "Paused for about an hour more.");
        assert_eq!(detail(24 * 60), "Paused for 24 more hours.");
    }

    #[test]
    fn values_come_in_layout_order_one_per_placement() {
        let s = settings();
        let stored = Stored::<PopoverCatalogue>::default();
        let values = with(&[], &s, |i| popover_values(stored.layout(), i));
        let ids: Vec<&str> = values.placements.iter().map(|p| p.id.as_str()).collect();
        let layout_ids: Vec<&str> = stored.layout().placements.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, layout_ids);
        let mut edited = stored.clone();
        edited
            .edit(Edit::Remove {
                id: stored.layout().placements[0].id.clone(),
            })
            .unwrap();
        assert_eq!(
            with(&[], &s, |i| popover_values(edited.layout(), i)).placements.len(),
            ids.len() - 1
        );
    }

    // ---- fixtures for the UI's mock ----

    /// Failing watches with the messages the engine really words them with.
    fn failing(id: &str, name: &str, status: WatchStatus, failures: u32) -> WatchSummary {
        let mut w = watch(id);
        w.name = name.into();
        w.status = status;
        w.failures = failures;
        w.latest = Some(release("1.0.0", NOW - 3 * 86_400_000));
        w.last_check_ms = Some(NOW - 8 * 60_000);
        w.next_check_ms = Some(NOW + 15 * 60_000);
        w
    }

    fn found(id: &str, name: &str, version: &str, ago_ms: i64, unseen: bool) -> WatchSummary {
        let mut w = watch(id);
        w.name = name.into();
        w.latest = Some(release(version, NOW - ago_ms));
        w.unseen = unseen;
        w.last_check_ms = Some(NOW - 12 * 60_000);
        w.next_check_ms = Some(NOW + 40 * 60_000);
        w
    }

    /// The named scenarios of the UI's mock (`demo`, `empty`, `worst`): the watches and settings that make them.
    fn scenario(name: &str) -> (Vec<WatchSummary>, Settings) {
        let mut s = settings();
        match name {
            "empty" => (Vec::new(), s),
            "demo" => {
                let mut ws = vec![
                    found("w-1", "Orbit API image", "2.2.0", 60_000, true),
                    found("w-2", "tinyparse", "2.1.0", 4 * 3_600_000, true),
                    found("w-3", "Acme Installer", "R3.10.00T32", 5 * 3_600_000, true),
                    found("w-4", "Lumen CLI", "3.6.5", 3 * 86_400_000, false),
                    found("w-5", "Fjord UI kit", "3.1.5", 2 * 86_400_000, true),
                ];
                ws.push(failing(
                    "w-6",
                    "Orbit server",
                    WatchStatus::Unreachable(
                        "gitlab.example did not answer within 20 seconds. It will be tried again in 15 minutes.".into(),
                    ),
                    3,
                ));
                ws.push(failing(
                    "w-7",
                    "Vendor registry",
                    WatchStatus::AuthNeeded("The registry asked for a token. Add one in the watch settings.".into()),
                    1,
                ));
                let mut paused = found("w-8", "Nightly docs feed", "2026.10.30", 6 * 86_400_000, false);
                paused.paused = true;
                ws.push(paused);
                (ws, s)
            }
            "worst" => {
                // The worst the default popup meets: more failures than rows, a check running, notifications paused.
                let mut ws = vec![
                    failing(
                        "w-1",
                        "Vendor download page",
                        WatchStatus::NeedsYou(
                            "The page asks for a login before it shows the download. Open it, sign in, then check again.".into(),
                        ),
                        7,
                    ),
                    failing(
                        "w-2",
                        "Orbit server",
                        WatchStatus::Unreachable(
                            "gitlab.example did not answer within 20 seconds. It will be tried again in 15 minutes.".into(),
                        ),
                        3,
                    ),
                    failing(
                        "w-3",
                        "Vendor registry",
                        WatchStatus::AuthNeeded("The registry asked for a token. Add one in the watch settings.".into()),
                        2,
                    ),
                    failing(
                        "w-4",
                        "Internal mirror",
                        WatchStatus::Failed("The server answered 502 Bad Gateway.".into()),
                        1,
                    ),
                    found("w-5", "Orbit API image", "2.2.0", 60_000, true),
                    found("w-6", "tinyparse", "2.1.0", 4 * 3_600_000, true),
                ];
                ws[4].checking = true;
                s.notifications_paused_until = Some(NOW + 95 * 60_000);
                (ws, s)
            }
            other => panic!("no scenario {other}"),
        }
    }

    /// The UI's mock serves these files' contents instead of re-deriving the engine's rules in TypeScript: for every scenario the
    /// state of every widget kind (as if all were placed) and the surface notice of a popup without the Notifications widget.
    /// `PADDOCK_WRITE_TS=1 cargo test -p spotter-engine --lib widgets::tests::the_mock_fixtures` rewrites it.
    #[test]
    fn the_mock_fixtures_are_the_engines_output() {
        use paddock_commands::testing::{self, WriteMode};
        let mut all = Stored::<PopoverCatalogue>::default();
        for widget in [PopoverWidget::WatchedSites, PopoverWidget::QuietHours, PopoverWidget::Notifications] {
            all.edit(Edit::Add {
                widget,
                span: None,
                at: None,
            })
            .unwrap();
        }
        let without_notifications = Stored::<PopoverCatalogue>::default();
        let mut scenarios = serde_json::Map::new();
        for name in ["demo", "empty", "worst"] {
            let (ws, s) = scenario(name);
            let values = with(&ws, &s, |i| popover_values(all.layout(), i));
            let by_kind: serde_json::Map<String, serde_json::Value> = all
                .layout()
                .placements
                .iter()
                .zip(&values.placements)
                .map(|(p, st)| {
                    (
                        serde_json::to_value(p.widget).unwrap().as_str().unwrap().to_string(),
                        serde_json::to_value(&st.state).unwrap(),
                    )
                })
                .collect();
            let notice = with(&ws, &s, |i| popover_values(without_notifications.layout(), i)).notice;
            let status = with(&ws, &s, popover_status);
            scenarios.insert(
                name.into(),
                serde_json::json!({ "byKind": by_kind, "noticeWithoutNotificationsWidget": notice, "status": status }),
            );
        }
        // The catalogue (kinds, spans, limits), the default layout and the loading shape come from the same code the engine runs.
        let view: spotter_core::widgets::PopoverView = paddock_widgets::view(&without_notifications);
        let text = serde_json::to_string_pretty(
            &serde_json::json!({ "nowMs": NOW, "view": view, "shape": spotter_core::widgets::loading_shape(), "scenarios": scenarios }),
        )
        .unwrap()
            + "\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src/app/core/mock/popover-fixtures.json");
        testing::sync_generated(&path, &text, WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
    }
}
