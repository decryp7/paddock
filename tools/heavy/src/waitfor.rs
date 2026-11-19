//! Who waits for whom in the queue, as a graph, so a cycle is reported at once (docs/research/2026-11-19-gate-hang-detection.md, C2)
//! instead of showing only as two waits that never end (the drain deadlock of 2026-11-18, fixed in 7a4aafbee).
//!
//! The edges are what `Acquirer::poll` waits on, read off the folder. They come from the same predicates `poll` itself asks
//! ([`follows_queue`], [`yields_to_high`], [`waits_for_drain`]), so a change to a rule changes the graph with it (review P2-6):
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

/// What a run is, for the rules of the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// A normal step: it queues behind the head and yields to high-priority runs and to a drain.
    Normal,
    /// A high-priority run: no ticket, it goes before the queue.
    High,
    /// A container run that asked for the machine to drain: it goes by its marker.
    DrainOwner,
}

impl Role {
    /// The role of a run that is high-priority, or that owns a drain marker (which wins).
    pub fn of(high: bool, drain_owner: bool) -> Role {
        if drain_owner {
            Role::DrainOwner
        } else if high {
            Role::High
        } else {
            Role::Normal
        }
    }
}

/// Whether a run waits for its place in the FIFO queue (`Acquirer::poll`'s queue check). A drain owner does not (7a4aafbee), unless the
/// old rule is put back for a test.
pub fn follows_queue(role: Role, rules: Rules) -> bool {
    match role {
        Role::Normal => true,
        Role::High => false,
        Role::DrainOwner => rules.drain_owner_follows_queue,
    }
}

/// Whether a run yields to the high-priority runs that still count (`Acquirer::poll`'s high-marker check): only a normal step.
pub fn yields_to_high(role: Role) -> bool {
    role == Role::Normal
}

/// Whether a run past the queue waits for another run's drain marker: every run that is not itself a container run (a container run,
/// the drain owner above all, does not wait for a drain).
pub fn waits_for_drain(container_run: bool) -> bool {
    !container_run
}

/// What a waiter is, as the folder shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiter {
    /// Who it is: the file name of its ticket (or, for a high-priority run, of its marker), which holds its pid and the time it was made
    /// (review P2-6: two runs of one agent name and pid are told apart by it, a name is not an identity).
    pub id: String,
    /// The agent's name, for people.
    pub name: String,
    /// The pid of the heavy that waits.
    pub pid: u32,
    /// Its place in the queue (1 is the head); `None` for a high-priority run, which has no ticket.
    pub position: Option<usize>,
    /// It holds a live high-priority marker.
    pub high: bool,
    /// It holds a live drain marker (a container run that asked for the machine to drain).
    pub drain_owner: bool,
    /// It is a container run (its ticket says so; a drain owner is one by definition): `Acquirer::poll` asks `waits_for_drain` of exactly
    /// this, so the graph does too. A high-priority run has no ticket and is taken as no container run (its edges are over-stated, which
    /// can only add an edge, never hide one).
    pub container: bool,
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
        let role = Role::of(x.high, x.drain_owner);
        if yields_to_high(role) {
            for &h in &high {
                out.push(Edge {
                    from: i,
                    to: h,
                    why: "a high-priority run goes first",
                });
            }
        }
        if follows_queue(role, rules) && x.position.is_some_and(|p| p > 1) {
            if let Some(h) = head.filter(|&h| h != i) {
                out.push(Edge {
                    from: i,
                    to: h,
                    why: "behind the head of the queue",
                });
            }
            continue;
        }
        // Past the queue (the head, a high-priority run, a drain owner that goes by its marker): wait for a drain it does not own.
        if waits_for_drain(x.container || x.drain_owner) {
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

/// A cycle in the wait-for graph: the indices of its members in order (each waits for the next, the last for the first), or `None`.
pub fn find_cycle(w: &[Waiter], rules: Rules) -> Option<Vec<usize>> {
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
            return Some(c);
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

/// What the waiter `me` (an index into `w`) does about `cycle`: only a drain owner that is a member breaks it, by withdrawing its own
/// marker.
pub fn break_action(w: &[Waiter], cycle: &[usize], me: usize) -> Break {
    if !cycle.contains(&me) {
        Break::Outside
    } else if w[me].drain_owner {
        Break::WithdrawDrain
    } else {
        Break::ReportOnly
    }
}

/// The names of the members, `a -> b -> a`, for the log and `heavy status`.
pub fn describe(w: &[Waiter], cycle: &[usize]) -> String {
    let names: Vec<&str> = cycle.iter().map(|&i| w[i].name.as_str()).collect();
    let mut s = names.join(" -> ");
    if let Some(first) = names.first() {
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
            id: format!("ticket-{name}"),
            name: name.into(),
            pid: 1,
            position,
            high,
            drain_owner,
            container: drain_owner,
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
        let said = describe(&state, &cycle);
        assert_eq!(said.split(" -> ").count(), 3, "{said}");
        assert!(said.contains('H') && said.contains('C'), "{said}");
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
        assert_eq!(break_action(&state, &cycle, 1), Break::WithdrawDrain);
        assert_eq!(break_action(&state, &cycle, 0), Break::ReportOnly);
        assert_eq!(break_action(&state, &cycle, 2), Break::Outside);
    }

    /// The graph and `poll` ask the same predicates: the roles' answers are what the edges are built from.
    #[test]
    fn the_roles_answer_what_poll_asks() {
        let old = Rules {
            drain_owner_follows_queue: true,
        };
        assert!(follows_queue(Role::Normal, Rules::default()) && !follows_queue(Role::High, Rules::default()));
        assert!(!follows_queue(Role::DrainOwner, Rules::default()) && follows_queue(Role::DrainOwner, old));
        assert!(yields_to_high(Role::Normal) && !yields_to_high(Role::High) && !yields_to_high(Role::DrainOwner));
        assert!(waits_for_drain(false) && !waits_for_drain(true));
        assert_eq!(Role::of(true, true), Role::DrainOwner, "a drain owner is a drain owner first");
    }

    /// Review (b): a container run that has no drain marker (it has not waited long enough to ask for one) does not wait for another
    /// container run's drain: `poll` asks `waits_for_drain(container_req)`, and the graph asks it of the same fact. Taken as a drain
    /// owner it would have shown an edge that poll never waits on.
    #[test]
    fn a_container_run_without_a_drain_marker_does_not_wait_for_a_drain() {
        let mut head = w("C", Some(1), false, false);
        head.container = true;
        let owner = w("D", Some(2), false, true);
        let state = vec![head, owner];
        let es = edges(&state, Rules::default());
        assert!(
            !es.iter().any(|e| e.from == 0),
            "the container head waits for nobody's drain: {es:?}"
        );
        // the same head as a normal step waits for the drain
        let normal = vec![w("N", Some(1), false, false), state[1].clone()];
        assert!(edges(&normal, Rules::default()).iter().any(|e| e.from == 0 && e.to == 1));
    }

    /// Two runs with one agent name are two waiters: the graph names them by their tickets, not by what they are called.
    #[test]
    fn waiters_are_told_apart_by_their_ticket_not_by_name() {
        let mut a = w("same", Some(1), false, false);
        let mut b = w("same", Some(2), false, true);
        a.id = "ticket-1-pid7".into();
        b.id = "ticket-2-pid8".into();
        let state = vec![a, b];
        let cycle = find_cycle(
            &state,
            Rules {
                drain_owner_follows_queue: true,
            },
        )
        .unwrap();
        assert_eq!(
            break_action(&state, &cycle, 1),
            Break::WithdrawDrain,
            "only the drain owner of the two breaks"
        );
        assert_eq!(break_action(&state, &cycle, 0), Break::ReportOnly);
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
