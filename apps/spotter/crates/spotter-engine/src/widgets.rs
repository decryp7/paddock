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
    ChangeRow, FailureKind, FailureRow, NotificationMode, PopoverState, PopoverValue, PopoverValues, PopoverWidget, SiteRow, SiteState,
    WidgetActionId,
};

/// Rows Watched sites lists.
pub const SITE_ROWS: usize = 4;
/// Rows Recent changes lists.
pub const CHANGE_ROWS: usize = 3;
/// Rows Failures lists.
pub const FAILURE_ROWS: usize = 3;
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

fn empty(title: &str, detail: &str, next: Option<WidgetAction<WidgetActionId>>) -> PopoverState {
    WidgetState::Empty {
        title: title.into(),
        detail: detail.into(),
        next,
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
    found.truncate(CHANGE_ROWS);
    let more = count(total - found.len());
    ready(PopoverValue::RecentChanges { rows: found, more }, i.now_ms)
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
        }
    });
    if running > 0 {
        // A busy source is Working, never an error (ADR 0035); the next check stays under it.
        return WidgetState::Working {
            title: "Checking now".into(),
            phase: "Checking watches".into(),
            progress: Progress::Count {
                done: running as u64,
                unit: if running == 1 { "watch".into() } else { "watches".into() },
            },
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
        return empty(
            "No failures",
            "Every watch is fine.",
            act(WidgetActionId::OpenWatches, "Open watches"),
        );
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
        action: act(WidgetActionId::ResumeNotifications, "Resume notifications"),
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
            WidgetState::Empty { title, detail, next } => (title, detail, next.map(|n| n.action)),
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
        assert_eq!((total, rows.len(), more), (6, SITE_ROWS, 2));
        let states: Vec<SiteState> = rows.iter().map(|r| r.state).collect();
        assert_eq!(states, [SiteState::Attention, SiteState::New, SiteState::Checking, SiteState::Ok]);
        assert_eq!(rows[0].id, "w5");
        assert_eq!(rows[0].version.as_deref(), Some("1.2.0"));
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
        let PopoverValue::RecentChanges { rows, more } = with(&ws, &s, |i| value_of(state(PopoverWidget::RecentChanges, i))) else {
            panic!("recent changes")
        };
        assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["w4", "w3", "w2"]);
        assert_eq!(more, 1, "w0 has no release, so four are found and one is not listed");
        assert!(rows[0].unseen && !rows[1].unseen);
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
    fn next_check_while_a_check_runs_is_working_with_a_growing_count_and_keeps_the_next_one() {
        let mut ws = vec![watch("a"), watch("b"), watch("c")];
        ws[0].checking = true;
        ws[1].checking = true;
        ws[2].next_check_ms = Some(NOW + 5000);
        let s = settings();
        match with(&ws, &s, |i| state(PopoverWidget::NextCheck, i)) {
            WidgetState::Working {
                phase,
                progress: Progress::Count { done, unit },
                last: Some(PopoverValue::NextCheck { id, .. }),
                cancel: None,
                ..
            } => {
                assert_eq!(
                    (phase.as_str(), done, unit.as_str(), id.as_str()),
                    ("Checking watches", 2, "watches", "c")
                );
            }
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
        assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["w1", "w3", "w0"]);
        assert_eq!(
            (more, rows[0].kind, rows[0].message.as_str()),
            (1, FailureKind::NeedsYou, "Log in.")
        );
        assert_eq!(rows[1].kind, FailureKind::AuthNeeded);
        assert_eq!(rows[2].kind, FailureKind::Unreachable);
    }

    #[test]
    fn no_failures_is_a_calm_empty_that_opens_the_watches() {
        let s = settings();
        let (title, _, next) = empty_of(with(&[watch("a")], &s, |i| state(PopoverWidget::Failures, i)));
        assert_eq!((title.as_str(), next), ("No failures", Some(WidgetActionId::OpenWatches)));
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
        let default = Stored::<PopoverCatalogue>::default();
        let mut layout = default.layout().clone();
        layout.placements.retain(|p| p.widget != PopoverWidget::Notifications);
        let n = with(&[], &s, |i| popover_values(&layout, i)).notice.unwrap();
        assert_eq!(
            (n.tone, n.title.as_str(), n.detail.as_str()),
            (Tone::Info, "Notifications are paused", "Paused for 20 more minutes.")
        );
        assert_eq!(n.action.unwrap().action, WidgetActionId::ResumeNotifications);
        assert!(with(&[], &s, |i| popover_values(default.layout(), i)).notice.is_none());
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
}
