//! A bounded model check: every interleaving of a world model, breadth first, up to a state bound. The world is a
//! nondeterministic model of everything around a machine (processes, locks, late answers, the person); the check asks
//! for every successor of a state and stops at the first violation, with the shortest trace that reaches it.

use std::collections::{HashSet, VecDeque};
use std::hash::Hash;

/// What the exploration found.
#[derive(Debug)]
pub struct Explored {
    /// Distinct states visited.
    pub states: usize,
    /// The first violation (the shortest trace), if any.
    pub violation: Option<Violation>,
    /// The bound was reached before the search ended (the result is then only "none up to the bound").
    pub truncated: bool,
}

/// A violated invariant and how the world got there.
#[derive(Debug)]
pub struct Violation {
    /// The invariant.
    pub what: String,
    /// The steps from the start, one label each.
    pub trace: Vec<String>,
}

/// Explores `start`. `key` identifies a state (equal keys are the same state), `next` lists every step from a state with a
/// label, and `check` names the invariant a state violates. The search keeps `parent` links only, so a trace costs
/// nothing until a violation is reported.
pub fn explore<W, K: Hash + Eq>(
    start: W,
    max_states: usize,
    key: impl Fn(&W) -> K,
    next: impl Fn(&W) -> Vec<(String, W)>,
    check: impl Fn(&W) -> Option<String>,
) -> Explored {
    let mut seen = HashSet::new();
    let mut parents: Vec<(usize, String)> = vec![(usize::MAX, String::new())];
    let mut queue = VecDeque::new();
    seen.insert(key(&start));
    queue.push_back((start, 0usize));
    let mut truncated = false;
    while let Some((world, id)) = queue.pop_front() {
        if let Some(what) = check(&world) {
            let mut trace = Vec::new();
            let mut at = id;
            while at != 0 {
                trace.push(parents[at].1.clone());
                at = parents[at].0;
            }
            trace.reverse();
            return Explored {
                states: seen.len(),
                violation: Some(Violation { what, trace }),
                truncated,
            };
        }
        for (label, n) in next(&world) {
            if seen.len() >= max_states {
                truncated = true;
                continue;
            }
            if seen.insert(key(&n)) {
                parents.push((id, label));
                queue.push_back((n, parents.len() - 1));
            }
        }
    }
    Explored {
        states: seen.len(),
        violation: None,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_finds_the_shortest_trace_to_a_violation_and_counts_distinct_states() {
        // A counter that may add 1 or 2; 5 is forbidden.
        let r = explore(
            0u32,
            1000,
            |w| *w,
            |w| vec![("+1".into(), w + 1), ("+2".into(), w + 2)],
            |w| (*w == 5).then(|| "five".to_string()),
        );
        let v = r.violation.expect("five is reachable");
        assert_eq!(v.what, "five");
        assert_eq!(v.trace.len(), 3, "{:?}", v.trace);
    }

    #[test]
    fn it_reports_a_clean_search_and_a_truncated_one() {
        let clean = explore(
            0u32,
            1000,
            |w| *w,
            |w| if *w < 4 { vec![("+".into(), w + 1)] } else { vec![] },
            |_| None,
        );
        assert!(clean.violation.is_none() && !clean.truncated);
        assert_eq!(clean.states, 5);
        let cut = explore(0u32, 3, |w| *w, |w| vec![("+".into(), w + 1)], |_| None);
        assert!(cut.truncated);
    }
}
