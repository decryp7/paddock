//! Who waits for whom in the queue, as a graph, so a cycle is reported at once (docs/research/2026-11-19-gate-hang-detection.md, C2)
//! instead of showing only as two waits that never end (the drain deadlock of 2026-11-18, fixed in 7a4aafbee).
//!
//! The edges are what `Acquirer::poll` waits on, read off the folder:
//! - a waiter that is not high-priority and holds ticket number `p > 1` waits for the head of the queue (every ticket ahead of it, so
//!   the head stands for them);
//! - a waiter that is the head (or high-priority, which has no ticket) waits for the owner of a live drain marker, unless it is that
//!   owner: every other step waits for a container run's drain;
//! - a waiter that is not high-priority waits for the owner of a high-priority marker that still counts.
//!
//! A drain owner goes by its marker, not its place in the queue: it has no edge to the head (the fix of 7a4aafbee). [`Rules`] can put
//! that edge back, so a test can show that the detector finds the old deadlock and names its members.
//!
//! Pure: a list of waiters in, a cycle out.

/// What a waiter is, as the folder shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiter {
    /// The agent's name.
    pub name: String,
    /// The pid of the heavy that waits.
    pub pid: u32,
    /// Its place in the queue (1 is the head); `None` for a high-priority run, which has no ticket.
    pub position: Option<usize>,
    /// It holds a live high-priority marker.
    pub high: bool,
    /// It holds a live drain marker (a container run that asked for the machine to drain).
    pub drain_owner: bool,
}

/// The rules the edges follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rules {
    /// The drain owner waits for the head of the queue like any other ticket: the behaviour before 7a4aafbee, kept only so a test
    /// can reproduce the old cycle. Production never sets it.
    pub drain_owner_follows_queue: bool,
}

/// One edge: waiter `from` waits for waiter `to`, for the reason given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// Index of the waiter that waits.
    pub from: usize,
    /// Index of the waiter it waits for.
    pub to: usize,
    /// Why, in a few words.
    pub why: &'static str,
}

/// The index of the waiter at queue position 1.
fn head(w: &[Waiter]) -> Option<usize> {
    w.iter().position(|x| x.position == Some(1))
}

/// Every edge of the wait-for graph of `w` under `rules`.
pub fn edges(w: &[Waiter], rules: Rules) -> Vec<Edge> {
    let mut out = Vec::new();
    let head = head(w);
    let drain: Vec<usize> = w.iter().enumerate().filter(|(_, x)| x.drain_owner).map(|(i, _)| i).collect();
    let high: Vec<usize> = w.iter().enumerate().filter(|(_, x)| x.high).map(|(i, _)| i).collect();
    for (i, x) in w.iter().enumerate() {
        // A high-priority run yields to nobody but a drain; the others yield to high-priority runs.
        if !x.high && !x.drain_owner {
            for &h in &high {
                out.push(Edge {
                    from: i,
                    to: h,
                    why: "a high-priority run goes first",
                });
            }
        }
        let queued_behind_head = x.position.is_some_and(|p| p > 1);
        if queued_behind_head && (!x.drain_owner || rules.drain_owner_follows_queue) {
            if let Some(h) = head.filter(|&h| h != i) {
                out.push(Edge {
                    from: i,
                    to: h,
                    why: "behind the head of the queue",
                });
            }
            continue;
        }
        // The head, a high-priority run and a drain owner are past the queue: they wait for a drain they do not own.
        if !x.drain_owner || rules.drain_owner_follows_queue {
            for &d in drain.iter().filter(|&&d| d != i) {
                out.push(Edge {
                    from: i,
                    to: d,
                    why: "a container run waits for the machine to drain",
                });
            }
        }
    }
    out
}

/// A cycle in the wait-for graph: the names of its members in order (each waits for the next, the last for the first), or `None`.
pub fn find_cycle(w: &[Waiter], rules: Rules) -> Option<Vec<String>> {
    let es = edges(w, rules);
    // 0 unvisited, 1 on the path, 2 done
    let mut state = vec![0u8; w.len()];
    let mut path: Vec<usize> = Vec::new();
    fn go(n: usize, es: &[Edge], state: &mut [u8], path: &mut Vec<usize>) -> Option<Vec<usize>> {
        state[n] = 1;
        path.push(n);
        for e in es.iter().filter(|e| e.from == n) {
            match state[e.to] {
                1 => {
                    let at = path.iter().position(|&p| p == e.to)?;
                    return Some(path[at..].to_vec());
                }
                0 => {
                    if let Some(c) = go(e.to, es, state, path) {
                        return Some(c);
                    }
                }
                _ => {}
            }
        }
        path.pop();
        state[n] = 2;
        None
    }
    for s in 0..w.len() {
        if state[s] == 0
            && let Some(c) = go(s, &es, &mut state, &mut path)
        {
            return Some(c.into_iter().map(|i| w[i].name.clone()).collect());
        }
    }
    None
}

/// What the run that sees a cycle does about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Break {
    /// It owns the drain marker that is part of the cycle: it withdraws it (a drain is only an optimisation for fairness: the
    /// others go on, and it asks again after its wait), which cannot lose a lock or start anything unsafely.
    WithdrawDrain,
    /// It is in the cycle but owns nothing it may give up: it reports it and waits.
    ReportOnly,
    /// It is not in the cycle: it says nothing.
    Outside,
}

/// What `me` does about `cycle`: only a drain owner that is a member breaks it, by withdrawing its own marker.
pub fn break_action(cycle: &[String], me: &Waiter) -> Break {
    if !cycle.iter().any(|n| n == &me.name) {
        Break::Outside
    } else if me.drain_owner {
        Break::WithdrawDrain
    } else {
        Break::ReportOnly
    }
}

/// `a -> b -> a`, for the log and `heavy status`.
pub fn describe(cycle: &[String]) -> String {
    let mut s = cycle.join(" -> ");
    if let Some(first) = cycle.first() {
        s.push_str(" -> ");
        s.push_str(first);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(name: &str, position: Option<usize>, high: bool, drain_owner: bool) -> Waiter {
        Waiter {
            name: name.into(),
            pid: 1,
            position,
            high,
            drain_owner,
        }
    }

    /// The deadlock of 2026-11-18: the head waits for a drain whose owner sits behind it in the queue. With the old rule the detector
    /// names both; with the rule as fixed there is nothing to find.
    #[test]
    fn the_old_drain_deadlock_is_a_cycle_the_detector_names() {
        let state = vec![w("H", Some(1), false, false), w("C", Some(2), false, true)];
        let old = Rules {
            drain_owner_follows_queue: true,
        };
        let cycle = find_cycle(&state, old).expect("the old rule has a cycle");
        assert_eq!(describe(&cycle).split(" -> ").count(), 3, "{cycle:?}");
        assert!(cycle.contains(&"H".to_string()) && cycle.contains(&"C".to_string()), "{cycle:?}");
        assert_eq!(
            find_cycle(&state, Rules::default()),
            None,
            "the fix: a drain owner goes by its marker"
        );
    }

    #[test]
    fn only_a_member_that_owns_the_drain_breaks_the_cycle_and_it_withdraws_its_marker() {
        let state = vec![
            w("H", Some(1), false, false),
            w("C", Some(2), false, true),
            w("X", Some(3), false, false),
        ];
        let cycle = find_cycle(
            &state,
            Rules {
                drain_owner_follows_queue: true,
            },
        )
        .unwrap();
        assert_eq!(break_action(&cycle, &state[1]), Break::WithdrawDrain);
        assert_eq!(break_action(&cycle, &state[0]), Break::ReportOnly);
        assert_eq!(break_action(&cycle, &state[2]), Break::Outside);
    }

    #[test]
    fn a_plain_queue_and_a_wait_for_a_high_priority_run_have_no_cycle() {
        let state = vec![
            w("A", Some(1), false, false),
            w("B", Some(2), false, false),
            w("H", None, true, false),
        ];
        assert_eq!(find_cycle(&state, Rules::default()), None);
        let es = edges(&state, Rules::default());
        assert!(es.iter().any(|e| e.from == 1 && e.to == 0), "B waits for the head");
        assert!(
            es.iter().any(|e| e.from == 0 && e.to == 2),
            "the head waits for the high-priority run"
        );
    }

    /// A model over random waiters (a seeded generator, no clock): whatever mix of normal, high-priority and drain-owning waiters the
    /// folder holds, the rules as they are give no cycle, and the old rule gives one exactly when a drain owner sits behind a head
    /// that is not itself the drain owner. The control: take the fix out and the model finds cycles (`cycles_with(old)` > 0).
    fn random_state(seed: u64) -> Vec<Waiter> {
        let mut x = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let mut rnd = |n: u64| {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (x >> 33) % n
        };
        let tickets = 1 + rnd(6) as usize;
        let highs = rnd(3) as usize;
        let mut out: Vec<Waiter> = (0..tickets).map(|i| w(&format!("t{i}"), Some(i + 1), false, false)).collect();
        // at most one drain owner among the ticket holders (two would also need two markers; one is the case that deadlocked)
        if rnd(2) == 0 {
            let i = rnd(tickets as u64) as usize;
            out[i].drain_owner = true;
        }
        for i in 0..highs {
            out.push(w(&format!("h{i}"), None, true, false));
        }
        out
    }

    fn cycles_with(rules: Rules) -> usize {
        (0..1200u64).filter(|&s| find_cycle(&random_state(s), rules).is_some()).count()
    }

    #[test]
    fn the_model_has_no_cycle_under_the_rules_and_finds_the_old_deadlock_without_the_fix() {
        assert_eq!(cycles_with(Rules::default()), 0, "the rules as they are never wait in a circle");
        let old = cycles_with(Rules {
            drain_owner_follows_queue: true,
        });
        assert!(
            old > 100,
            "the control: with the fix taken out the detector finds the deadlock in {old} of 1200 states"
        );
    }
}
