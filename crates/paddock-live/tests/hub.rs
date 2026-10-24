//! The hub: numbering, merging, bounded queues, resynchronisation, closing, a panicking fold.

mod common;

use common::{Change, Overview, event, memory};
use paddock_live::{Frame, Hub, Link, MAX_QUEUE_CAP, MIN_QUEUE_CAP};

type F = Frame<Overview, Change>;

fn seqs(frames: &[F]) -> Vec<(u64, u64)> {
    frames
        .iter()
        .filter_map(|f| match f {
            Frame::Delta { from, seq, .. } => Some((*from, *seq)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_subscriber_starts_with_a_snapshot_and_then_gets_ordered_numbered_deltas() {
    let hub = Hub::new(Overview::default());
    assert_eq!((hub.seq(), hub.link()), (0, Link::Connecting));
    let sub = hub.subscribe();
    assert_eq!(hub.publish(event("a")), 1);
    assert_eq!(hub.publish(Change::Bump), 2);
    let frames = sub.drain();
    assert!(matches!(
        &frames[0],
        Frame::Snapshot {
            seq: 0,
            link: Link::Connecting,
            ..
        }
    ));
    assert_eq!(seqs(&frames), vec![(1, 1), (2, 2)]);
    assert_eq!(hub.state().events, vec!["a"]);
    assert_eq!(hub.state().count, 1);
    assert_eq!(sub.pending(), 0);
    assert!(sub.try_next().is_none());
}

#[test]
fn a_late_subscriber_starts_from_the_state_of_now() {
    let hub = Hub::new(Overview::default());
    hub.publish(event("a"));
    hub.publish(event("b"));
    let sub = hub.subscribe();
    hub.publish(event("c"));
    let frames = sub.drain();
    let Frame::Snapshot { seq, state, .. } = &frames[0] else {
        panic!("{frames:?}")
    };
    assert_eq!((*seq, state.events.len()), (2, 2));
    assert_eq!(seqs(&frames), vec![(3, 3)]);
}

#[test]
fn latest_wins_deltas_merge_into_one_frame_that_covers_their_numbers() {
    let hub = Hub::new(Overview::default());
    let sub = hub.subscribe();
    hub.publish(memory(1));
    hub.publish(memory(2));
    hub.publish(memory(3));
    hub.publish(event("x"));
    hub.publish(memory(4));
    let frames = sub.drain();
    assert_eq!(seqs(&frames), vec![(1, 3), (4, 4), (5, 5)]);
    assert!(matches!(
        &frames[1],
        Frame::Delta {
            delta: Change::Memory { bytes: 3 },
            ..
        }
    ));
    assert_eq!(
        hub.seq(),
        5,
        "the sequence counts every change, merged or not"
    );
}

#[test]
fn a_consumer_that_falls_behind_gets_one_snapshot_and_the_queue_stays_bounded() {
    let hub = Hub::with_queue_cap(Overview::default(), 4);
    let slow = hub.subscribe();
    let keen = hub.subscribe();
    let mut kept = Vec::new();
    for i in 0..10 {
        hub.publish(event(&i.to_string()));
        assert!(slow.pending() <= 4, "bounded at every step");
        kept.extend(keen.drain());
    }
    assert!(slow.resyncs() >= 1);
    assert_eq!(keen.resyncs(), 0);
    let frames = slow.drain();
    let Frame::Snapshot { seq, state, .. } = &frames[0] else {
        panic!("{frames:?}")
    };
    assert!(
        *seq >= 4 && state.events.len() == *seq as usize,
        "the snapshot holds everything up to its number"
    );
    // After the snapshot the numbers continue without a gap.
    let mut next = *seq + 1;
    for (from, to) in seqs(&frames) {
        assert_eq!(from, next);
        next = to + 1;
    }
    assert_eq!(next, 11);
    assert_eq!(seqs(&kept).len(), 10);
}

#[test]
fn queue_caps_are_clamped() {
    let low = Hub::with_queue_cap(Overview::default(), 0);
    let sub = low.subscribe();
    for i in 0..MIN_QUEUE_CAP + 2 {
        low.publish(event(&i.to_string()));
    }
    assert!(sub.pending() <= MIN_QUEUE_CAP);
    let high = Hub::with_queue_cap(Overview::default(), usize::MAX);
    let sub = high.subscribe();
    for i in 0..MAX_QUEUE_CAP + 5 {
        high.publish(event(&i.to_string()));
    }
    assert!(sub.pending() <= MAX_QUEUE_CAP);
    assert!(sub.resyncs() >= 1);
}

#[test]
fn a_reset_replaces_the_state_and_what_was_queued() {
    let hub = Hub::new(Overview::default());
    let sub = hub.subscribe();
    hub.publish(event("old"));
    let seq = hub.reset(Overview {
        memory: 7,
        ..Overview::default()
    });
    assert_eq!(seq, 2);
    hub.publish(Change::Bump);
    let frames = sub.drain();
    let Frame::Snapshot { seq, state, link } = &frames[0] else {
        panic!("{frames:?}")
    };
    assert_eq!(
        (*seq, state.memory, state.events.len(), link),
        (2, 7, 0, &Link::Connecting)
    );
    assert_eq!(seqs(&frames), vec![(3, 3)]);
}

#[test]
fn a_reset_with_a_link_shows_the_new_state_with_the_new_link_in_one_frame() {
    let hub = Hub::new(Overview::default());
    let sub = hub.subscribe();
    hub.reset_with(Overview::default(), Link::Connected);
    let frames = sub.drain();
    assert!(matches!(
        &frames[0],
        Frame::Snapshot {
            link: Link::Connected,
            ..
        }
    ));
    assert_eq!(frames.len(), 1);
    assert_eq!(hub.link(), Link::Connected);
}

#[test]
fn link_changes_are_sent_once_and_the_newest_replaces_an_unread_one() {
    let hub = Hub::new(Overview::default());
    let sub = hub.subscribe();
    sub.drain();
    hub.set_link(Link::Connecting);
    assert_eq!(sub.pending(), 0, "no change, nothing sent");
    hub.set_link(Link::Connected);
    hub.set_link(Link::Reconnecting {
        attempt: 1,
        retry_in_ms: 1000,
        reason: "closed".into(),
    });
    let frames = sub.drain();
    assert_eq!(frames.len(), 1);
    assert!(matches!(
        &frames[0],
        Frame::Link {
            link: Link::Reconnecting { attempt: 1, .. }
        }
    ));
    hub.publish(Change::Bump);
    hub.set_link(Link::Stopped);
    hub.set_link(Link::Down {
        reason: "gave up".into(),
    });
    assert_eq!(sub.drain().len(), 2, "a delta then one link frame");
}

#[test]
fn a_link_change_to_a_full_queue_resynchronises_instead_of_growing_it() {
    let hub = Hub::with_queue_cap(Overview::default(), 4);
    let sub = hub.subscribe();
    for i in 0..3 {
        hub.publish(event(&i.to_string()));
    }
    assert_eq!(
        sub.pending(),
        4,
        "the snapshot and three deltas fill the queue"
    );
    hub.set_link(Link::Stopped);
    let frames = sub.drain();
    assert_eq!(frames.len(), 1);
    assert!(matches!(
        &frames[0],
        Frame::Snapshot {
            link: Link::Stopped,
            seq: 3,
            ..
        }
    ));
    assert_eq!(sub.resyncs(), 1);
}

#[test]
fn a_snapshot_frame_is_what_a_reloading_window_asks_for() {
    let hub = Hub::new(Overview::default());
    hub.publish(event("a"));
    hub.set_link(Link::Connected);
    let Frame::Snapshot { seq, state, link } = hub.snapshot() else {
        panic!("not a snapshot")
    };
    assert_eq!(
        (seq, state.events, link),
        (1, vec!["a".to_string()], Link::Connected)
    );
}

#[test]
fn dropped_subscribers_are_forgotten() {
    let hub = Hub::new(Overview::default());
    let a = hub.subscribe();
    let b = hub.subscribe();
    assert_eq!(hub.subscribers(), 2);
    drop(a);
    hub.publish(Change::Bump);
    assert_eq!(hub.subscribers(), 1);
    assert_eq!(b.pending(), 2);
}

#[test]
fn closing_ends_every_subscription_after_what_is_queued_and_refuses_changes() {
    let hub = Hub::new(Overview::default());
    let sub = hub.subscribe();
    hub.publish(event("a"));
    let clone = hub.clone();
    assert!(!hub.is_closed());
    hub.close();
    assert!(clone.is_closed());
    assert_eq!(
        clone.publish(event("late")),
        1,
        "ignored: the number does not move"
    );
    assert_eq!(clone.reset(Overview::default()), 1);
    clone.reset_with(Overview::default(), Link::Connected);
    clone.set_link(Link::Stopped);
    assert_eq!(hub.state().events, vec!["a"]);
    assert_eq!(hub.link(), Link::Connecting);
    assert!(sub.next().is_some(), "the snapshot");
    assert!(sub.next().is_some(), "the delta");
    assert!(sub.next().is_none(), "then the end");
    let late = hub.subscribe();
    assert!(late.next().is_some());
    assert!(late.next().is_none());
}

#[test]
fn next_waits_for_a_frame_published_from_another_thread() {
    let hub = Hub::new(Overview::default());
    let sub = hub.subscribe();
    assert!(sub.next().is_some());
    let publisher = hub.clone();
    let handle = std::thread::spawn(move || {
        publisher.publish(event("from another thread"));
        publisher.close();
    });
    let frame = sub.next().expect("the delta");
    assert!(matches!(frame, Frame::Delta { seq: 1, .. }));
    assert!(sub.next().is_none());
    handle.join().unwrap();
}

#[test]
fn a_fold_that_panics_does_not_poison_the_feed() {
    let hub = Hub::new(Overview::default());
    let sub = hub.subscribe();
    let h = hub.clone();
    let crashed = std::thread::spawn(move || h.publish(Change::Explode)).join();
    assert!(crashed.is_err());
    // The lock was poisoned by the panic; the hub carries on.
    hub.publish(Change::Bump);
    assert_eq!(hub.state().count, 1);
    assert!(sub.drain().iter().any(|f| matches!(
        f,
        Frame::Delta {
            delta: Change::Bump,
            ..
        }
    )));
}

#[test]
fn the_wire_format_is_camel_case_with_a_kind_tag() {
    let hub = Hub::new(Overview::default());
    let sub = hub.subscribe();
    hub.set_link(Link::Reconnecting {
        attempt: 2,
        retry_in_ms: 2000,
        reason: "closed".into(),
    });
    hub.publish(memory(5));
    let json: Vec<serde_json::Value> = sub
        .drain()
        .iter()
        .map(|f| serde_json::to_value(f).unwrap())
        .collect();
    assert_eq!(json[0]["kind"], "snapshot");
    assert_eq!(json[0]["link"]["state"], "connecting");
    assert_eq!(
        json[1],
        serde_json::json!({"kind": "link", "link": {"state": "reconnecting", "attempt": 2, "retryInMs": 2000, "reason": "closed"}})
    );
    assert_eq!(
        json[2],
        serde_json::json!({"kind": "delta", "from": 1, "seq": 1, "delta": {"kind": "memory", "bytes": 5}})
    );
    let back: F = serde_json::from_value(json[2].clone()).unwrap();
    assert!(matches!(back, Frame::Delta { seq: 1, .. }));
}

/// A state that does not override `merge`: nothing merges.
#[derive(Clone, serde::Serialize)]
struct Plain(Vec<u32>);

impl paddock_live::LiveState for Plain {
    type Delta = u32;
    fn apply(&mut self, delta: &u32) {
        self.0.push(*delta);
    }
}

#[test]
fn without_a_merge_every_change_is_its_own_frame() {
    let hub = Hub::new(Plain(vec![]));
    let sub = hub.subscribe();
    hub.publish(1);
    hub.publish(1);
    assert_eq!(sub.drain().len(), 3);
    assert_eq!(hub.state().0, vec![1, 1]);
}
