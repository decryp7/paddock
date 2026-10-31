//! The tray popup's values (family ADR 0035 §5): what the app builds from the agent's live push, the agent's answers for
//! the slow kinds and the connection, and when it sends them to the popup. Pure: time, visibility and the zone are
//! arguments; [`crate::shell`] owns the threads and the Tauri event.
//!
//! - The live kinds (CPU and memory, network, top apps) are built here from the live push.
//! - The slow kinds (alarms, disk space, backup, last issue, pinned counters) are the agent's `PopoverValues` answers,
//!   asked on open, on the agent's change events and by each kind's refresh period ([`PopoverModel::due`]).
//! - Values go to the popup **only while it is visible** ([`PopoverModel::emission`]).
//! - When the agent is lost the model freezes the last values as stale and the surface carries the "Agent stopped"
//!   notice with Restart agent; nothing is drawn as if it were still live.

use dyno_core::protocol::LiveUpdate;
use dyno_core::time::{LocalZone, Timestamp};
use dyno_core::widgets::popover_values::{
    agent_stopped, is_live_push, mark_stale, state, surface_values, Inputs, Minute, PopoverState, PopoverSurfaceValues, Source,
};
use dyno_core::widgets::{PopoverCatalogue, PopoverWidget};
use paddock_widgets::{Catalogue, Placement, PlacementId, Refresh};
use std::collections::HashMap;

/// A kind that refreshes live is asked of the agent at most this often (seconds): it is read from the agent's caches, but
/// the popup is no reason to ask every second.
pub const LIVE_SLOW_SECS: i64 = 5;

/// Whether the agent connection is up.
#[derive(Debug, Clone, PartialEq)]
enum Link {
    Up,
    /// Lost at this time; the values the popup showed then, marked stale.
    Down {
        since: Timestamp,
        frozen: Vec<PopoverState>,
    },
}

/// Everything the popup shows, kept between ticks.
#[derive(Debug)]
pub struct PopoverModel {
    placements: Vec<Placement<PopoverWidget>>,
    minute: Minute,
    live: Option<LiveUpdate>,
    slow: HashMap<PlacementId, PopoverState>,
    asked_at: HashMap<PlacementId, Timestamp>,
    force: bool,
    visible: bool,
    link: Link,
}

impl Default for PopoverModel {
    fn default() -> Self {
        PopoverModel::new(PopoverCatalogue::defaults())
    }
}

impl PopoverModel {
    /// A model for `placements` (the popup layout).
    pub fn new(placements: Vec<Placement<PopoverWidget>>) -> PopoverModel {
        PopoverModel {
            placements,
            minute: Minute::default(),
            live: None,
            slow: HashMap::new(),
            asked_at: HashMap::new(),
            force: true,
            visible: false,
            link: Link::Up,
        }
    }

    /// The popup layout changed: answers of removed placements are forgotten and every slow kind is asked again.
    pub fn set_layout(&mut self, placements: Vec<Placement<PopoverWidget>>) {
        if placements == self.placements {
            return;
        }
        self.slow.retain(|id, _| placements.iter().any(|p| &p.id == id));
        self.asked_at.retain(|id, _| placements.iter().any(|p| &p.id == id));
        // A placement whose widget changed (another volume) must not show the old answer.
        for p in &placements {
            if self.placements.iter().find(|o| o.id == p.id).is_some_and(|o| o.widget != p.widget) {
                self.slow.remove(&p.id);
                self.asked_at.remove(&p.id);
            }
        }
        self.placements = placements;
        self.force = true;
    }

    /// The popup layout.
    pub fn placements(&self) -> &[Placement<PopoverWidget>] {
        &self.placements
    }

    /// A live update arrived.
    pub fn on_live(&mut self, update: &LiveUpdate) {
        if matches!(self.link, Link::Up) {
            self.minute.push(update);
            self.live = Some(update.clone());
        }
    }

    /// The agent answered `PopoverValues`.
    pub fn on_slow(&mut self, answers: Vec<PopoverState>) {
        for a in answers {
            if self.placements.iter().any(|p| p.id == a.id) {
                self.slow.insert(a.id.clone(), a);
            }
        }
    }

    /// Something the slow kinds show may have changed (an alarm, a job, the volumes): ask for all of them at the next tick.
    pub fn force_refresh(&mut self) {
        self.force = true;
    }

    /// The popover was shown or hidden. Showing it asks for every slow kind and sends the values at once. Returns whether it
    /// just became visible.
    pub fn set_visible(&mut self, visible: bool) -> bool {
        let opened = visible && !self.visible;
        self.visible = visible;
        if opened {
            self.force = true;
        }
        opened
    }

    /// Whether the popup is visible.
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// The agent is gone: freeze what the popup shows as stale, until it is back.
    pub fn agent_lost(&mut self, now: Timestamp) {
        if matches!(self.link, Link::Down { .. }) {
            return;
        }
        let mut frozen = self.states(now);
        mark_stale(&mut frozen);
        self.link = Link::Down {
            since: self.last_reading().unwrap_or(now),
            frozen,
        };
    }

    /// The agent is back: live values resume and everything is asked again.
    pub fn agent_back(&mut self) {
        if !matches!(self.link, Link::Up) {
            self.link = Link::Up;
            self.force = true;
        }
    }

    /// Whether the agent is up.
    pub fn agent_up(&self) -> bool {
        matches!(self.link, Link::Up)
    }

    fn last_reading(&self) -> Option<Timestamp> {
        self.live.as_ref().map(|l| l.system.at)
    }

    /// The slow placements to ask the agent for at `now`, and remembers that they were asked. Nothing while the popup is
    /// hidden or the agent is away (a hidden popup costs nothing, ADR 0035 §8).
    pub fn due(&mut self, now: Timestamp) -> Vec<PlacementId> {
        if !self.visible || !self.agent_up() {
            return Vec::new();
        }
        let force = std::mem::take(&mut self.force);
        let due: Vec<PlacementId> = self
            .placements
            .iter()
            .filter(|p| !is_live_push(dyno_core::widgets::Widget::kind(&p.widget)))
            .filter(|p| force || self.is_due(p, now))
            .map(|p| p.id.clone())
            .collect();
        for id in &due {
            self.asked_at.insert(id.clone(), now);
        }
        due
    }

    fn is_due(&self, p: &Placement<PopoverWidget>, now: Timestamp) -> bool {
        let Some(asked) = self.asked_at.get(&p.id) else { return true };
        let kind = dyno_core::widgets::Widget::kind(&p.widget);
        let period = match PopoverCatalogue::kinds().iter().find(|k| k.kind == kind).map(|k| k.refresh) {
            Some(Refresh::Every { secs }) => i64::from(secs),
            Some(Refresh::Live) => LIVE_SLOW_SECS,
            // On change: only when something said so.
            Some(Refresh::OnChange) | None => return false,
        };
        now.since(*asked) >= period * 1_000
    }

    /// Every placement's state, in the layout's order; a placement with no state yet is left out (the popup shows it loading).
    fn states(&self, now: Timestamp) -> Vec<PopoverState> {
        let at = self.last_reading().unwrap_or(now);
        let mut inputs = Inputs::empty(at, &self.minute);
        if let Some(l) = &self.live {
            inputs.live = Source::Ready(l);
        }
        self.placements
            .iter()
            .filter_map(|p| {
                if is_live_push(dyno_core::widgets::Widget::kind(&p.widget)) {
                    state(&p.widget, &p.id, &inputs).map(|s| PopoverState {
                        id: p.id.clone(),
                        state: s,
                    })
                } else {
                    self.slow.get(&p.id).cloned()
                }
            })
            .collect()
    }

    /// The surface values now: the states, and the "Agent stopped" notice while the agent is away.
    pub fn values(&self, now: Timestamp, zone: &dyn LocalZone) -> PopoverSurfaceValues {
        match &self.link {
            Link::Up => surface_values(None, self.states(now)),
            Link::Down { since, frozen } => surface_values(Some(agent_stopped(Some(*since), zone)), frozen.clone()),
        }
    }

    /// What to send the popup, or nothing while it is hidden.
    pub fn emission(&self, now: Timestamp, zone: &dyn LocalZone) -> Option<PopoverSurfaceValues> {
        self.visible.then(|| self.values(now, zone))
    }
}

#[cfg(test)]
mod tests;
