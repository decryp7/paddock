//! The source runner: reconnect schedule, seeding, resetting the delay, giving up, cancelling. No sleeps: the fake clock
//! moves only when a test moves it, and the runner waits on it.

use crate::common::{Overview, event};
use paddock_jobs::CancelToken;
use paddock_jobs::time::{BackoffPolicy, Clock, FakeClock, GiveUp};
use paddock_live::{Frame, Hub, Link, Publisher, RunEnd, Source, default_policy, run};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

/// What one scripted connection does.
enum Step {
    /// Fails at once with this reason.
    Fail(&'static str),
    /// Seeds, publishes `n` events, then advances the clock by `lasts` while connected and ends with this reason.
    Seeded {
        events: usize,
        lasts: Duration,
        reason: Option<&'static str>,
    },
    /// Cancels the token and returns.
    Cancel,
}

struct Scripted {
    steps: VecDeque<Step>,
    clock: FakeClock,
    cancel: CancelToken,
    done: Sender<()>,
}

impl Source<Overview> for Scripted {
    fn session(
        &mut self,
        feed: &Publisher<'_, Overview>,
        _cancel: &CancelToken,
    ) -> Result<(), String> {
        let step = self
            .steps
            .pop_front()
            .expect("the script has a step for every connection");
        let out = match step {
            Step::Fail(reason) => Err(reason.to_string()),
            Step::Seeded {
                events,
                lasts,
                reason,
            } => {
                feed.seed(Overview::default());
                assert!(feed.is_seeded());
                for i in 0..events {
                    feed.publish(event(&i.to_string()));
                }
                self.clock.advance(lasts);
                reason.map_or(Ok(()), |r| Err(r.to_string()))
            }
            Step::Cancel => {
                self.cancel.cancel();
                Ok(())
            }
        };
        let _ = self.done.send(());
        out
    }
}

struct Rig {
    hub: Hub<Overview>,
    clock: FakeClock,
    cancel: CancelToken,
    done: Receiver<()>,
    handle: std::thread::JoinHandle<RunEnd>,
}

fn start(steps: Vec<Step>, policy: BackoffPolicy) -> Rig {
    let hub = Hub::new(Overview::default());
    let clock = FakeClock::new();
    let cancel = CancelToken::new();
    let (tx, done) = channel();
    let mut source = Scripted {
        steps: steps.into(),
        clock: clock.clone(),
        cancel: cancel.clone(),
        done: tx,
    };
    let (h, c, t) = (hub.clone(), clock.clone(), cancel.clone());
    let handle = std::thread::spawn(move || {
        let clock: Arc<dyn Clock> = Arc::new(c);
        run(&h, &mut source, &clock, policy, &t)
    });
    Rig {
        hub,
        clock,
        cancel,
        done,
        handle,
    }
}

impl Rig {
    /// Waits for the connection that was running to end, and for the runner to wait out its delay on the clock.
    fn wait_for_delay(&self) {
        self.done.recv().unwrap();
        // conformance: hang-guard — the runner reaches its wait at once; the guard only stops a broken runner hanging the suite
        assert!(
            self.clock.wait_for_waiters(1, Duration::from_secs(30)),
            "the runner did not wait for its delay"
        );
    }
}

fn retries(link: Link) -> (u32, u64, String) {
    match link {
        Link::Reconnecting {
            attempt,
            retry_in_ms,
            reason,
        } => (attempt, retry_in_ms, reason),
        other => panic!("not reconnecting: {other:?}"),
    }
}

#[test]
fn failures_back_off_one_two_four_eight_ten_and_stay_at_ten() {
    let steps = (0..7)
        .map(|_| Step::Fail("refused"))
        .chain([Step::Cancel])
        .collect();
    let rig = start(steps, default_policy());
    let mut seen = Vec::new();
    for _ in 0..7 {
        rig.wait_for_delay();
        let (attempt, ms, reason) = retries(rig.hub.link());
        assert_eq!(reason, "refused");
        seen.push((attempt, ms));
        rig.clock.advance(Duration::from_millis(ms));
    }
    assert_eq!(
        seen,
        vec![
            (1, 1000),
            (2, 2000),
            (3, 4000),
            (4, 8000),
            (5, 10_000),
            (6, 10_000),
            (7, 10_000)
        ]
    );
    assert_eq!(rig.handle.join().unwrap(), RunEnd::Cancelled);
    assert_eq!(rig.hub.link(), Link::Stopped);
}

#[test]
fn a_seeded_connection_is_live_then_reconnects_and_seeds_again_with_a_fresh_snapshot() {
    let steps = vec![
        Step::Seeded {
            events: 2,
            lasts: Duration::ZERO,
            reason: Some("the server closed the stream"),
        },
        Step::Seeded {
            events: 1,
            lasts: Duration::ZERO,
            reason: None,
        },
        Step::Cancel,
    ];
    let rig = start(steps, default_policy());
    let sub = rig.hub.subscribe();
    rig.wait_for_delay();
    assert_eq!(
        retries(rig.hub.link()),
        (1, 1000, "the server closed the stream".to_string())
    );
    assert_eq!(
        rig.hub.state().events.len(),
        2,
        "the last known state stays while reconnecting"
    );
    rig.clock.advance(Duration::from_secs(1));
    rig.wait_for_delay();
    assert_eq!(
        retries(rig.hub.link()),
        (2, 2000, "The connection was closed".to_string())
    );
    rig.clock.advance(Duration::from_secs(2));
    assert_eq!(rig.handle.join().unwrap(), RunEnd::Cancelled);
    let frames = sub.drain();
    let reseeds = frames
        .iter()
        .filter(|f| {
            matches!(
                f,
                Frame::Snapshot {
                    link: Link::Connected,
                    ..
                }
            )
        })
        .count();
    assert!(
        reseeds >= 1,
        "each seed is a snapshot with the link connected: {frames:?}"
    );
}

#[test]
fn a_connection_that_lasted_starts_the_delays_over() {
    let policy = BackoffPolicy::retry_forever(
        Duration::from_secs(1),
        Duration::from_secs(10),
        Duration::from_secs(30),
    );
    let steps = vec![
        Step::Fail("refused"),
        Step::Fail("refused"),
        Step::Seeded {
            events: 0,
            lasts: Duration::from_secs(31),
            reason: Some("dropped"),
        },
        Step::Seeded {
            events: 0,
            lasts: Duration::from_secs(5),
            reason: Some("dropped"),
        },
        Step::Cancel,
    ];
    let rig = start(steps, policy);
    let mut seen = Vec::new();
    for _ in 0..4 {
        rig.wait_for_delay();
        let (attempt, ms, _) = retries(rig.hub.link());
        seen.push((attempt, ms));
        rig.clock.advance(Duration::from_millis(ms));
    }
    // 1 s, 2 s; then a connection of 31 s (>= 30 s) resets: 1 s again; a short one does not: 2 s.
    assert_eq!(seen, vec![(1, 1000), (2, 2000), (1, 1000), (2, 2000)]);
    assert_eq!(rig.handle.join().unwrap(), RunEnd::Cancelled);
}

#[test]
fn a_policy_that_gives_up_leaves_the_link_down_with_the_reason() {
    let policy = BackoffPolicy {
        initial: Duration::from_secs(1),
        max: Duration::from_secs(2),
        reset_after: Duration::from_secs(60),
        give_up: Some(GiveUp {
            max_failures: 1,
            window: Duration::from_secs(600),
        }),
    };
    let rig = start(
        vec![Step::Fail("no agent"), Step::Fail("still no agent")],
        policy,
    );
    rig.wait_for_delay();
    rig.clock.advance(Duration::from_secs(1));
    assert_eq!(rig.handle.join().unwrap(), RunEnd::GaveUp);
    assert_eq!(
        rig.hub.link(),
        Link::Down {
            reason: "still no agent".into()
        }
    );
}

#[test]
fn cancelling_during_the_wait_stops_the_feed_at_once() {
    let rig = start(vec![Step::Fail("refused")], default_policy());
    rig.wait_for_delay();
    rig.cancel.cancel();
    assert_eq!(rig.handle.join().unwrap(), RunEnd::Cancelled);
    assert_eq!(rig.hub.link(), Link::Stopped);
}

#[test]
fn a_token_cancelled_before_the_start_never_connects() {
    let hub = Hub::new(Overview::default());
    let (tx, _rx) = channel();
    let clock = FakeClock::new();
    let cancel = CancelToken::new();
    cancel.cancel();
    let mut source = Scripted {
        steps: VecDeque::new(),
        clock: clock.clone(),
        cancel: cancel.clone(),
        done: tx,
    };
    let clock: Arc<dyn Clock> = Arc::new(clock);
    assert_eq!(
        run(&hub, &mut source, &clock, default_policy(), &cancel),
        RunEnd::Cancelled
    );
    assert_eq!(hub.link(), Link::Stopped);
}

#[test]
fn a_session_that_cancels_ends_the_run_without_a_retry() {
    let rig = start(vec![Step::Cancel], default_policy());
    assert_eq!(rig.handle.join().unwrap(), RunEnd::Cancelled);
    assert_eq!(rig.hub.link(), Link::Stopped);
}
