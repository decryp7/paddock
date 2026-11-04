//! Per-program network traffic where the OS makes it cheap to read (macOS `nettop`): cumulative byte counters per
//! process become bytes per second between two reads. Pure: the platform adapter reads the text, the agent passes it
//! and the time in.

use crate::helper_policy::{OthersPolicy, Visibility};
use crate::helper_processes::{RawOwner, Sweep};
use crate::helper_wire::HelperUser;
use crate::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Most programs listed in one snapshot (the busiest first).
pub const MAX_TRAFFIC_ROWS: usize = 50;
/// Most counters one read may contain (a bound on untrusted tool output).
pub const MAX_COUNTERS: usize = 8_192;

/// What the OS reports for one process: bytes since it started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrafficCounter {
    /// Process id.
    pub pid: u32,
    /// Program name as the tool prints it (may be shortened).
    pub name: String,
    /// Bytes received.
    pub bytes_in: u64,
    /// Bytes sent.
    pub bytes_out: u64,
}

/// What one program sent and received per second since the previous read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficRow {
    /// Process id.
    pub pid: u32,
    /// Program name.
    pub name: String,
    /// Bytes per second received.
    pub rx_bps: f64,
    /// Bytes per second sent.
    pub tx_bps: f64,
}

/// What the helper tells a caller about other accounts' programs when the policy says "total": how many there are and what they
/// moved together, never their names or process ids.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OthersTraffic {
    /// Programs of other accounts that have counters.
    pub programs: u32,
    /// Bytes they received together.
    pub bytes_in: u64,
    /// Bytes they sent together.
    pub bytes_out: u64,
}

/// The helper's `Traffic` answer: the counters this caller may see, and the rest as one total.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficAnswer {
    /// The caller's own programs and the system's (and every account's for a caller whose scope allows it).
    pub counters: Vec<TrafficCounter>,
    /// Other accounts' programs as one total, for a caller who may see that they exist but not which they are.
    pub others: Option<OthersTraffic>,
}

/// One read of the traffic port with its source.
#[derive(Debug, Clone)]
pub struct TrafficRead {
    /// The counters, or why there are none.
    pub answer: Result<TrafficAnswer, crate::ports::PlatformError>,
    /// Who read them.
    pub via: crate::read_via::ReadVia,
}

/// Who a counter's process belongs to, from the same sweep the process list is built from.
enum Whose {
    /// The caller's own, or the system's or a service's: shown in detail.
    Shown,
    /// Another account's: shown as the scope says.
    Other,
}

fn whose(pid: u32, sweep: &Sweep, caller: &HelperUser) -> Whose {
    let Some(p) = sweep.processes.iter().find(|p| p.pid == pid) else {
        // A program that has gone, or one the sweep could not see, is not known to be the caller's: fail closed.
        return Whose::Other;
    };
    match &p.owner {
        RawOwner::System | RawOwner::Service(..) => Whose::Shown,
        RawOwner::Account { id, .. } if id == caller => Whose::Shown,
        RawOwner::Account { .. } | RawOwner::Unknown => Whose::Other,
    }
}

/// `counters` scoped to `caller` (ADR 0027 section 5, plan decision D6): the caller's own programs and the system's in detail;
/// other accounts' programs in detail, as one total, or not at all, as `scope` says (the same rule as [`crate::helper_processes::build_page`]).
/// A counter whose process is not in `sweep` counts as another account's. At most [`MAX_COUNTERS`] are kept.
pub fn traffic_answer(counters: Vec<TrafficCounter>, sweep: &Sweep, caller: &HelperUser, scope: Visibility) -> TrafficAnswer {
    let detail_for_others = matches!(scope, Visibility::Full | Visibility::Own(OthersPolicy::Detail));
    let mut answer = TrafficAnswer::default();
    for c in counters.into_iter().take(MAX_COUNTERS) {
        match whose(c.pid, sweep, caller) {
            Whose::Shown => answer.counters.push(c),
            Whose::Other if detail_for_others => answer.counters.push(c),
            Whose::Other if scope == Visibility::Own(OthersPolicy::Total) => {
                let o = answer.others.get_or_insert_with(OthersTraffic::default);
                o.programs = o.programs.saturating_add(1);
                o.bytes_in = o.bytes_in.saturating_add(c.bytes_in);
                o.bytes_out = o.bytes_out.saturating_add(c.bytes_out);
            }
            Whose::Other => {}
        }
    }
    answer
}

/// Parses `nettop -P -L 1 -x -J bytes_in,bytes_out` output: a header line, then `name.pid,bytes_in,bytes_out,` per
/// process (the name may contain dots and spaces; the pid follows the last dot). Lines that do not fit are skipped.
pub fn parse_nettop(text: &str) -> Vec<TrafficCounter> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let mut parts = line.trim_end_matches(',').rsplitn(3, ',');
        let (Some(out_s), Some(in_s), Some(label)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let (Ok(bytes_out), Ok(bytes_in)) = (out_s.trim().parse::<u64>(), in_s.trim().parse::<u64>()) else {
            continue;
        };
        let Some((name, pid)) = label.rsplit_once('.') else { continue };
        let Ok(pid) = pid.trim().parse::<u32>() else { continue };
        out.push(TrafficCounter {
            pid,
            name: name.trim().to_string(),
            bytes_in,
            bytes_out,
        });
        if out.len() >= MAX_COUNTERS {
            break;
        }
    }
    out
}

/// Turns consecutive counter reads into rates.
#[derive(Debug, Default)]
pub struct TrafficTracker {
    last: HashMap<u32, (u64, u64)>,
    at: Option<Timestamp>,
}

impl TrafficTracker {
    /// A tracker that has seen nothing (the first update has no rates yet).
    pub fn new() -> TrafficTracker {
        TrafficTracker::default()
    }

    /// Learns `counters` read at `at` and returns the programs that moved bytes since the previous read, busiest first.
    /// A counter that went backwards (a pid reused by a new process) restarts from zero instead of going negative.
    pub fn update(&mut self, at: Timestamp, counters: &[TrafficCounter]) -> Vec<TrafficRow> {
        let secs = self.at.map(|prev| at.since(prev) as f64 / 1000.0).filter(|s| *s > 0.0);
        let mut rows = Vec::new();
        if let Some(secs) = secs {
            for c in counters {
                let Some((pin, pout)) = self.last.get(&c.pid).copied() else {
                    continue;
                };
                let rx = c.bytes_in.saturating_sub(pin);
                let tx = c.bytes_out.saturating_sub(pout);
                if rx == 0 && tx == 0 {
                    continue;
                }
                rows.push(TrafficRow {
                    pid: c.pid,
                    name: c.name.clone(),
                    rx_bps: rx as f64 / secs,
                    tx_bps: tx as f64 / secs,
                });
            }
        }
        rows.sort_by(|a, b| (b.rx_bps + b.tx_bps).total_cmp(&(a.rx_bps + a.tx_bps)).then(a.pid.cmp(&b.pid)));
        rows.truncate(MAX_TRAFFIC_ROWS);
        self.last = counters.iter().map(|c| (c.pid, (c.bytes_in, c.bytes_out))).collect();
        self.at = Some(at);
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = ",bytes_in,bytes_out,\nkernel_task.0,3112760137,9478161228,\nGoogle Chrome H.905,1000,200,\nweird.name.with.dots.77,5,6,\nbroken line\nnopid,1,2,\nx.notapid,1,2,\ny.9,abc,2,\n";

    #[test]
    fn a_counter_whose_process_has_no_known_owner_is_only_in_the_total() {
        use crate::helper_processes::{ProcState, RawProcess};
        let me = HelperUser::Uid(1_000);
        let process = |pid, owner| RawProcess {
            pid,
            started: Some(1),
            parent: None,
            name: format!("p{pid}"),
            path: None,
            owner,
            state: ProcState::Running,
            threads: None,
            cpu_time_ms: None,
            memory_bytes: None,
            disk_read_bytes: None,
            disk_written_bytes: None,
        };
        let sweep = Sweep {
            processes: vec![
                process(
                    1,
                    RawOwner::Account {
                        id: me.clone(),
                        name: None,
                    },
                ),
                process(2, RawOwner::Unknown),
            ],
            homes: vec![],
            taken_at: 1,
        };
        let counters = vec![
            TrafficCounter {
                pid: 1,
                name: "mine".into(),
                bytes_in: 1,
                bytes_out: 1,
            },
            TrafficCounter {
                pid: 2,
                name: "mystery".into(),
                bytes_in: 5,
                bytes_out: 6,
            },
        ];
        let a = traffic_answer(counters, &sweep, &me, Visibility::Own(OthersPolicy::Total));
        assert_eq!(a.counters.iter().map(|c| c.pid).collect::<Vec<_>>(), vec![1]);
        assert_eq!(
            a.others,
            Some(OthersTraffic {
                programs: 1,
                bytes_in: 5,
                bytes_out: 6
            })
        );
    }

    #[test]
    fn nettop_text_becomes_counters_and_bad_lines_are_skipped() {
        let c = parse_nettop(SAMPLE);
        assert_eq!(c.len(), 3);
        assert_eq!(
            (c[0].pid, c[0].name.as_str(), c[0].bytes_in, c[0].bytes_out),
            (0, "kernel_task", 3_112_760_137, 9_478_161_228)
        );
        assert_eq!(c[1].name, "Google Chrome H");
        assert_eq!((c[2].pid, c[2].name.as_str()), (77, "weird.name.with.dots"));
        assert!(parse_nettop("").is_empty());
        assert!(parse_nettop(",bytes_in,bytes_out,\n").is_empty());
    }

    #[test]
    fn counters_are_bounded() {
        let mut text = String::from(",bytes_in,bytes_out,\n");
        for i in 0..MAX_COUNTERS + 10 {
            text.push_str(&format!("p.{i},1,1,\n"));
        }
        assert_eq!(parse_nettop(&text).len(), MAX_COUNTERS);
    }

    fn counter(pid: u32, i: u64, o: u64) -> TrafficCounter {
        TrafficCounter {
            pid,
            name: format!("app{pid}"),
            bytes_in: i,
            bytes_out: o,
        }
    }

    #[test]
    fn rates_come_from_the_difference_between_two_reads() {
        let mut t = TrafficTracker::new();
        assert!(
            t.update(Timestamp(0), &[counter(1, 100, 100), counter(2, 0, 0)]).is_empty(),
            "the first read has no rates"
        );
        let rows = t.update(Timestamp(2_000), &[counter(1, 2_100, 300), counter(2, 0, 0), counter(3, 50, 50)]);
        assert_eq!(rows.len(), 1, "idle programs and programs first seen now are not listed");
        assert_eq!((rows[0].pid, rows[0].rx_bps, rows[0].tx_bps), (1, 1_000.0, 100.0));
    }

    #[test]
    fn busiest_first_and_a_reset_counter_never_goes_negative() {
        let mut t = TrafficTracker::new();
        t.update(Timestamp(0), &[counter(1, 1_000, 1_000), counter(2, 0, 0)]);
        let rows = t.update(Timestamp(1_000), &[counter(1, 10, 10), counter(2, 5_000, 0)]);
        assert_eq!(rows.iter().map(|r| r.pid).collect::<Vec<_>>(), vec![2]);
        assert_eq!(rows[0].rx_bps, 5_000.0);
        // A clock that did not move gives no rates instead of dividing by zero.
        let rows = t.update(Timestamp(1_000), &[counter(2, 9_000, 0)]);
        assert!(rows.is_empty());
    }

    #[test]
    fn the_list_is_capped() {
        let mut t = TrafficTracker::new();
        let first: Vec<_> = (0..100).map(|p| counter(p, 0, 0)).collect();
        let next: Vec<_> = (0..100).map(|p| counter(p, 10 * u64::from(p + 1), 0)).collect();
        t.update(Timestamp(0), &first);
        let rows = t.update(Timestamp(1_000), &next);
        assert_eq!(rows.len(), MAX_TRAFFIC_ROWS);
        assert_eq!(rows[0].pid, 99);
    }
}
