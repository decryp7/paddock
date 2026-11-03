//! The popup's model: what it builds, when it asks the agent, when it sends, and what it shows without the agent.

use super::*;
use dyno_core::time::{FixedZone, HOUR, SECOND};
use dyno_core::widgets::popover_values::PopoverNoticeKind;
use dyno_core::widgets::popover_values::{PopoverValue, WidgetActionId};
use dyno_core::widgets::VolumeChoice;
use dyno_platform::fake::quiet_sample;
use paddock_widgets::{assign_spans, WidgetState};

const T0: Timestamp = Timestamp(10 * HOUR);

fn zone() -> FixedZone {
    FixedZone(0)
}

fn update(at: Timestamp, cpu: f32) -> LiveUpdate {
    let mut system = quiet_sample(at);
    system.cpu.total = cpu;
    LiveUpdate {
        system,
        pressure: dyno_core::metrics::MemoryPressure::Normal,
        top_cpu: vec![],
        top_memory: vec![],
        overhead: Default::default(),
        processes: None,
        groups: None,
        other_users: None,
    }
}

fn ids(ask: Option<Ask>) -> Vec<PlacementId> {
    ask.map(|a| a.ids).unwrap_or_default()
}

fn model() -> PopoverModel {
    // alarms (slow, on change), disk space (slow, every 60 s), cpu and memory (live).
    PopoverModel::default()
}

fn id(s: &str) -> PlacementId {
    PlacementId::new(s).unwrap()
}

fn slow_answer(name: &str) -> PopoverState {
    PlacementState {
        id: id(name),
        state: WidgetState::Empty {
            title: "t".into(),
            detail: "d".into(),
            next: None,
            calm: false,
        },
    }
}

use paddock_widgets::PlacementState;

#[test]
fn nothing_is_asked_and_nothing_is_sent_while_the_popup_is_hidden() {
    let mut m = model();
    m.on_live(&update(T0, 10.0));
    assert_eq!(m.due(T0), None);
    assert_eq!(m.emission(T0, &zone()), None);
    assert!(!m.visible());
}

#[test]
fn opening_asks_every_slow_kind_once_and_sends_the_values_at_once() {
    let mut m = model();
    m.on_live(&update(T0, 10.0));
    assert!(m.set_visible(true));
    assert!(!m.set_visible(true), "only the edge opens");
    assert_eq!(ids(m.due(T0)), [id("active-alarms"), id("disk-space")]);
    assert_eq!(m.due(T0), None, "asked once");
    let v = m.emission(T0, &zone()).expect("visible");
    assert_eq!(v.notice, None);
    // The live kind is built; the slow ones have no answer yet and are left out (the popup shows them loading).
    assert_eq!(v.placements.len(), 1);
    assert_eq!(v.placements[0].id, id("cpu-memory"));
    let WidgetState::Ready {
        value: PopoverValue::CpuMemory { cpu_percent, .. },
        as_of,
        stale_since,
    } = &v.placements[0].state
    else {
        panic!("{:?}", v.placements[0].state)
    };
    assert_eq!((*cpu_percent, *as_of, *stale_since), (10.0, T0.0, None));
}

#[test]
fn the_slow_answers_come_back_in_the_layouts_order_with_the_live_ones() {
    let mut m = model();
    m.on_live(&update(T0, 10.0));
    m.set_visible(true);
    m.on_slow(
        0,
        vec![
            slow_answer("disk-space"),
            slow_answer("active-alarms"),
            slow_answer("not-in-the-layout"),
        ],
    );
    let ids: Vec<String> = m.values(T0, &zone()).placements.iter().map(|p| p.id.to_string()).collect();
    assert_eq!(ids, ["active-alarms", "disk-space", "cpu-memory"]);
}

#[test]
fn each_kind_is_asked_again_by_its_own_period_and_on_change_kinds_only_when_told() {
    let mut m = model();
    m.set_visible(true);
    m.due(T0);
    // Disk space refreshes every 60 s; active alarms only on change.
    assert_eq!(m.due(T0.plus(59 * SECOND)), None);
    assert_eq!(ids(m.due(T0.plus(60 * SECOND))), [id("disk-space")]);
    assert_eq!(m.due(T0.plus(61 * SECOND)), None);
    m.force_refresh();
    assert_eq!(ids(m.due(T0.plus(62 * SECOND))), [id("active-alarms"), id("disk-space")]);
}

#[test]
fn a_live_kind_asked_of_the_agent_is_asked_every_few_seconds() {
    let layout = assign_spans([(2, PopoverWidget::PinnedCounters)]);
    let mut m = PopoverModel::new(layout);
    m.set_visible(true);
    assert_eq!(ids(m.due(T0)), [id("pinned-counters")]);
    assert_eq!(m.due(T0.plus((LIVE_SLOW_SECS - 1) * SECOND)), None);
    assert_eq!(ids(m.due(T0.plus(LIVE_SLOW_SECS * SECOND))), [id("pinned-counters")]);
}

#[test]
fn a_new_layout_forgets_removed_and_changed_answers_and_asks_again() {
    let mut m = model();
    m.set_visible(true);
    m.due(T0);
    m.on_slow(0, vec![slow_answer("active-alarms"), slow_answer("disk-space")]);
    let mut layout = m.placements().to_vec();
    layout.retain(|p| p.id != id("active-alarms"));
    layout.iter_mut().find(|p| p.id == id("disk-space")).unwrap().widget = PopoverWidget::disk(VolumeChoice::System);
    m.set_layout(layout.clone());
    assert!(m.values(T0, &zone()).placements.is_empty());
    assert_eq!(ids(m.due(T0.plus(SECOND))), [id("disk-space")]);
    m.set_layout(layout);
    assert_eq!(m.due(T0.plus(2 * SECOND)), None, "the same layout asks nothing");
}

#[test]
fn losing_the_agent_gives_one_notice_and_stale_values_and_restart_is_offered() {
    let mut m = model();
    let at = Timestamp(T0.0 + 12 * HOUR + 28 * 60 * SECOND);
    m.on_live(&update(at, 42.0));
    m.set_visible(true);
    m.on_slow(
        0,
        vec![PlacementState {
            id: id("active-alarms"),
            state: WidgetState::Ready {
                value: PopoverValue::TopApps { rows: vec![] },
                as_of: at.0 - 5 * SECOND,
                stale_since: None,
            },
        }],
    );
    m.agent_lost(at.plus(30 * SECOND));
    m.agent_lost(at.plus(31 * SECOND));
    let v = m.values(at.plus(40 * SECOND), &zone());
    let notice = v.notice.clone().expect("one notice");
    assert_eq!(notice.title, "Agent stopped");
    assert_eq!(notice.detail, "Readings paused at 22:28");
    assert_eq!(notice.action.unwrap().action, WidgetActionId::RestartAgent);
    for p in &v.placements {
        let WidgetState::Ready { stale_since, as_of, .. } = &p.state else {
            panic!()
        };
        assert_eq!(*stale_since, Some(*as_of), "{}", p.id);
    }
    assert_eq!(v.placements.len(), 2);
    // New live updates are not shown as live while the agent is away, and nothing is asked of it.
    m.on_live(&update(at.plus(50 * SECOND), 99.0));
    assert_eq!(m.values(at.plus(50 * SECOND), &zone()), v);
    assert_eq!(m.due(at.plus(60 * SECOND)), None);
}

#[test]
fn when_the_agent_is_back_the_notice_goes_and_everything_is_asked_again() {
    let mut m = model();
    m.on_live(&update(T0, 10.0));
    m.set_visible(true);
    m.due(T0);
    m.agent_lost(T0.plus(SECOND));
    m.agent_back();
    assert!(m.agent_up());
    m.on_live(&update(T0.plus(5 * SECOND), 20.0));
    assert_eq!(m.values(T0.plus(5 * SECOND), &zone()).notice, None);
    assert_eq!(ids(m.due(T0.plus(5 * SECOND))), [id("active-alarms"), id("disk-space")]);
}

#[test]
fn hiding_the_popup_stops_the_values_and_showing_it_again_sends_them() {
    let mut m = model();
    m.on_live(&update(T0, 10.0));
    m.set_visible(true);
    assert!(m.emission(T0, &zone()).is_some());
    m.set_visible(false);
    assert_eq!(m.emission(T0, &zone()), None);
    assert_eq!(m.due(T0.plus(120 * SECOND)), None);
    assert!(m.set_visible(true));
    assert!(m.emission(T0, &zone()).is_some());
}

#[test]
fn an_answer_for_an_older_layout_is_dropped() {
    let mut m = model();
    m.set_visible(true);
    let ask = m.due(T0).expect("asked on open");
    // The person changes the popup while the agent is still answering.
    let mut layout = m.placements().to_vec();
    layout.iter_mut().find(|p| p.id == id("disk-space")).unwrap().widget = PopoverWidget::disk(VolumeChoice::System);
    m.set_layout(layout);
    assert!(!m.on_slow(ask.generation, vec![slow_answer("disk-space")]), "late answer");
    assert!(m.values(T0, &zone()).placements.is_empty());
    let again = m.due(T0.plus(SECOND)).expect("asked again for the new layout");
    assert!(again.generation > ask.generation);
    assert!(m.on_slow(again.generation, vec![slow_answer("disk-space")]));
    assert_eq!(m.values(T0, &zone()).placements.len(), 1);
}

#[test]
fn losing_the_agent_before_any_reading_does_not_claim_a_time() {
    let mut m = model();
    m.agent_lost(T0);
    let notice = m.values(T0, &zone()).notice.clone().expect("the notice");
    assert_eq!(notice.title, "Agent stopped");
    assert_eq!(notice.detail, "No readings yet");
    assert_eq!(m.values(T0, &zone()).notice_kind, Some(PopoverNoticeKind::AgentStopped));
}
