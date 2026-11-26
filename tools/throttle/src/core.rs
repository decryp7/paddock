//! The pure core of the slot rules: who may take a slot now, and who keeps one when too many were taken. Time, load and the holders
//! are passed in; nothing here touches a file, a process or the clock, so the rules are tested without a disk (the OS adapter is
//! [`crate::slotfile`] and the folder of [`crate::lock`]). Pitcrew will use this crate as a library; this is the part it reuses.
//!
//! The numbers and the load rule are decided in [`crate::gate`]; this module only combines them with the holders.

use crate::gate::{CHEAP_LANE, Pressure, allowed_slots, cheap_admitted};

/// One live holder, as its slot's info record says (informational: that a slot is held is the kernel's word, not the record's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    /// The time the holder wrote its record (epoch seconds); `None` while the record is not there yet.
    pub epoch: Option<u64>,
    /// The holder's pid (0 while the record is not there yet).
    pub pid: u32,
    /// Whether the holder runs a container command (it then runs alone).
    pub container: bool,
}

/// What the rule needs to know to admit one request.
#[derive(Debug, Clone)]
pub struct Inputs<'a> {
    /// The most steps at once (`--slots`).
    pub max_slots: u32,
    /// The cores the load is counted against.
    pub cpus: u32,
    /// The load, CPU idle and memory now.
    pub pressure: &'a Pressure,
    /// The request is a container run.
    pub req_container: bool,
    /// The request is a quick row (the history says under `CHEAP_SECS`).
    pub req_quick: bool,
    /// The request is first in the queue (a quick row and a drain owner may go without being first).
    pub at_head: bool,
    /// The holders of the full slots.
    pub held: &'a [Holder],
    /// How many quick-lane places are held.
    pub cheap_held: usize,
}

/// The decision for one request at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admission {
    /// How many steps may run together now.
    pub allowed: u32,
    /// A container run is involved (the request or a holder): it runs alone.
    pub container: bool,
    /// The request may not take a full slot now (too many run, or it is not first, or a container run waits for the quick lane).
    pub limited: bool,
}

/// Whether a holder runs a container command: the machine then holds that one alone.
pub fn container_running(held: &[Holder]) -> bool {
    held.iter().any(|h| h.container)
}

/// Whether a quick row may take a place in the quick lane beside the full slots now.
pub fn quick_ok(i: &Inputs<'_>) -> bool {
    i.max_slots > 1
        && i.req_quick
        && !container_running(i.held)
        && cheap_admitted(i.cpus, i.pressure, false)
        && i.cheap_held < CHEAP_LANE as usize
}

/// The admission rule for a request: how many may run (one when `--slots` is 1) and whether this one may take a full slot.
pub fn admission(i: &Inputs<'_>) -> Admission {
    let container = i.req_container || container_running(i.held);
    let allowed = if i.max_slots > 1 {
        allowed_slots(i.max_slots, i.cpus, i.pressure, container)
    } else {
        1
    };
    let limited = i.held.len() >= allowed as usize || !i.at_head || (i.req_container && i.cheap_held > 0);
    Admission {
        allowed,
        container,
        limited,
    }
}

/// Whether a taker that holds a slot with the record `mine` (epoch, pid) keeps it, after it looked again at the `held` holders (itself
/// among them) and the rule allows `allowed`: the holders are ordered by the time of their record (then pid; a record that is not there
/// yet sorts last) and only the first `allowed` keep their slot, so two takers that see each other never both step back.
pub fn keeps_slot(held: &[Holder], mine: (u64, u32), allowed: u32) -> bool {
    if held.len() <= allowed as usize {
        return true;
    }
    let mut keys: Vec<(u64, u32)> = held.iter().map(|h| (h.epoch.unwrap_or(u64::MAX), h.pid)).collect();
    keys.sort_unstable();
    keys.iter().position(|k| *k == mine).is_some_and(|p| p < allowed as usize)
}

/// The share of the cores a newly admitted step gets: as many steps as may run now, or as run now with it (a quick row beside them).
pub fn share(allowed: u32, running_with_it: usize) -> u32 {
    allowed.max(u32::try_from(running_with_it).unwrap_or(u32::MAX)).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet() -> Pressure {
        Pressure {
            load1: Some(1.0),
            load5: Some(1.0),
            load15: Some(1.0),
            idle: Some(90.0),
            memory_mb: Some(64_000),
        }
    }

    fn holder(epoch: u64, pid: u32, container: bool) -> Holder {
        Holder {
            epoch: Some(epoch),
            pid,
            container,
        }
    }

    fn inp<'a>(p: &'a Pressure, held: &'a [Holder]) -> Inputs<'a> {
        Inputs {
            max_slots: 2,
            cpus: 16,
            pressure: p,
            req_container: false,
            req_quick: false,
            at_head: true,
            held,
            cheap_held: 0,
        }
    }

    #[test]
    fn a_quiet_machine_admits_a_second_step_and_a_full_one_does_not() {
        let p = quiet();
        let one = [holder(1, 1, false)];
        assert_eq!(
            admission(&inp(&p, &one)),
            Admission {
                allowed: 2,
                container: false,
                limited: false
            }
        );
        let two = [holder(1, 1, false), holder(2, 2, false)];
        assert!(admission(&inp(&p, &two)).limited);
        assert!(
            admission(&Inputs {
                at_head: false,
                ..inp(&p, &[])
            })
            .limited,
            "not first: wait"
        );
    }

    #[test]
    fn one_slot_is_one_and_a_container_run_is_alone() {
        let p = quiet();
        let a = admission(&Inputs {
            max_slots: 1,
            ..inp(&p, &[holder(1, 1, false)])
        });
        assert_eq!((a.allowed, a.limited), (1, true));
        let c = [holder(1, 1, true)];
        let a = admission(&inp(&p, &c));
        assert!(
            a.container && a.allowed == 1 && a.limited,
            "a container holder keeps the machine alone"
        );
        let a = admission(&Inputs {
            req_container: true,
            cheap_held: 1,
            ..inp(&p, &[])
        });
        assert!(a.limited, "a container run also waits for the quick lane to empty");
    }

    #[test]
    fn the_quick_lane_needs_room_and_no_container_run() {
        let p = quiet();
        let q = Inputs {
            req_quick: true,
            ..inp(&p, &[])
        };
        assert!(quick_ok(&q));
        assert!(!quick_ok(&Inputs {
            cheap_held: CHEAP_LANE as usize,
            ..q.clone()
        }));
        let c = [holder(1, 1, true)];
        assert!(!quick_ok(&Inputs { held: &c, ..q.clone() }));
        assert!(!quick_ok(&Inputs { max_slots: 1, ..q.clone() }));
        assert!(!quick_ok(&Inputs { req_quick: false, ..q }));
    }

    #[test]
    fn only_the_oldest_holders_keep_their_slot_when_too_many_were_taken() {
        let held = [holder(10, 5, false), holder(10, 3, false), holder(12, 1, false)];
        assert!(keeps_slot(&held, (10, 3), 2) && keeps_slot(&held, (10, 5), 2));
        assert!(!keeps_slot(&held, (12, 1), 2), "the youngest steps back");
        assert!(keeps_slot(&held, (12, 1), 3), "nobody steps back within the rule");
        let unknown = [
            Holder {
                epoch: None,
                pid: 0,
                container: false,
            },
            holder(10, 3, false),
        ];
        assert!(keeps_slot(&unknown, (10, 3), 1), "a record that is not there yet sorts last");
        assert!(!keeps_slot(&held, (99, 99), 2), "a taker that is not among them keeps nothing");
    }

    #[test]
    fn the_share_is_the_larger_of_the_rule_and_those_running() {
        assert_eq!((share(2, 1), share(1, 3), share(0, 0)), (2, 3, 1));
    }
}
