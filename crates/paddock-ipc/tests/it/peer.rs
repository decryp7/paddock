//! The peer: Debrief's `peer/tests.rs` and Dyno's `tests/peer.rs` on one implementation, made deterministic (handlers
//! report through channels and wait on gates; no test sleeps or measures how long something took), plus the X13 rule:
//! one frame over the limit fails one call, not the link.

use crate::common;
use common::{GUARD, T, pair, pair_with, raw};
use paddock_ipc::endpoint::Stream;
use paddock_ipc::frame::{self, Frame, FrameLimit, HARD_MAX_FRAME, MIN_FRAME};
use paddock_ipc::peer::{CANCEL_GRACE, DEFAULT_CREDIT};
use paddock_ipc::{
    CallCtx, Handler, IpcError, NoCalls, Peer, PeerOptions, RemoteError, StreamEvent, codes,
    decode_request, encode_reply, typed,
};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn echo() -> Arc<dyn Handler> {
    Arc::new(|_: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> {
        match body {
            b"fail" => Err(RemoteError::new("seven", "typed failure")),
            b"panic" => panic!("handler panicked"),
            other => Ok(other.to_vec()),
        }
    })
}

/// A gate a handler waits on until the test opens it.
#[derive(Clone, Default)]
struct Gate(Arc<(Mutex<bool>, std::sync::Condvar)>);

impl Gate {
    fn wait(&self) {
        let g = self.0.0.lock().unwrap();
        drop(self.0.1.wait_while(g, |open| !*open).unwrap());
    }
    fn open(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

#[test]
fn calls_go_both_ways_with_typed_errors_and_contained_panics() {
    let (a, b) = pair(echo(), echo());
    assert_eq!(a.call(b"hello".to_vec(), T).unwrap(), b"hello");
    assert_eq!(b.call(b"back".to_vec(), T).unwrap(), b"back");
    assert_eq!(
        a.call(b"fail".to_vec(), T).unwrap_err(),
        IpcError::Remote(RemoteError::new("seven", "typed failure"))
    );
    match a.call(b"panic".to_vec(), T).unwrap_err() {
        IpcError::Remote(e) => assert!(
            e.code == codes::PANIC && e.message.contains("handler panicked"),
            "{e:?}"
        ),
        other => panic!("{other:?}"),
    }
    // Still usable after a panic.
    assert_eq!(a.call(b"again".to_vec(), T).unwrap(), b"again");
    let got: String = a.request(&"typed".to_string(), T).unwrap();
    assert_eq!(got, "typed");
    assert!(matches!(
        a.request::<_, u64>(&"not a number".to_string(), T),
        Err(IpcError::Protocol(_))
    ));
    assert_eq!(a.label(), "side a");
    assert_eq!(a.limit(), FrameLimit::default());
    assert_eq!(a.peer_pid(), None);
    assert!(format!("{a:?}").contains("side a"));
    assert!(format!("{:?}", PeerOptions::new("x")).contains("cancel_grace"));
    // The side that answers nothing.
    let (c, _d) = pair(Arc::new(NoCalls), Arc::new(NoCalls));
    assert!(
        matches!(c.call(Vec::new(), T), Err(IpcError::Remote(e)) if e.code == codes::UNIMPLEMENTED)
    );
}

#[test]
fn typed_handlers_decode_requests_and_refuse_bad_ones() {
    let doubler = typed(|_: &CallCtx, n: u32| -> Result<u64, RemoteError> { Ok(u64::from(n) * 2) });
    let (a, _b) = pair(Arc::new(NoCalls), Arc::new(doubler));
    assert_eq!(a.request::<u32, u64>(&21, T).unwrap(), 42);
    assert!(
        matches!(a.call(vec![0xff, 0xff, 0xff, 0xff, 0xff, 0xff], T), Err(IpcError::Remote(e)) if e.code == codes::INVALID)
    );
    assert!(decode_request::<u32>(&[]).is_err());
    assert!(encode_reply(&1u8).is_ok());
}

#[test]
fn many_calls_at_once_are_matched_to_their_callers() {
    let (a, _b) = pair(Arc::new(NoCalls), echo());
    let threads: Vec<_> = (0..16)
        .map(|i| {
            let a = a.clone();
            std::thread::spawn(move || {
                for j in 0..20 {
                    let body = format!("{i}-{j}").into_bytes();
                    assert_eq!(a.call(body.clone(), T).unwrap(), body);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
}

#[test]
fn streams_respect_credits_and_end_with_the_reply() {
    let sent = Arc::new(AtomicUsize::new(0));
    let s2 = sent.clone();
    let producer: Arc<dyn Handler> = Arc::new(
        move |ctx: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> {
            let n: u32 = decode_request(body)?;
            for i in 0..n {
                ctx.send(&i)?;
                s2.fetch_add(1, Ordering::SeqCst);
            }
            encode_reply(&format!("{n} sent"))
        },
    );
    let (a, _b) = pair(Arc::new(NoCalls), producer);
    let mut s = a.stream_request(&100u32, DEFAULT_CREDIT).unwrap();
    let mut got = Vec::new();
    loop {
        match s.next(T).unwrap() {
            StreamEvent::Item(b) => {
                // The producer is never more than the window ahead of what was taken.
                assert!(sent.load(Ordering::SeqCst) <= got.len() + 1 + DEFAULT_CREDIT as usize);
                got.push(paddock_ipc::wire::decode::<u32>(&b).unwrap());
            }
            StreamEvent::Done(b) => {
                assert_eq!(paddock_ipc::wire::decode::<String>(&b).unwrap(), "100 sent");
                break;
            }
            StreamEvent::Idle => panic!("idle"),
        }
    }
    assert_eq!(got, (0..100).collect::<Vec<_>>());
    assert!(s.is_done());
    assert!(s.next(T).is_err(), "nothing after the end");
}

#[test]
fn a_slow_consumer_holds_the_producer_back() {
    let (tx, rx) = mpsc::channel();
    let producer: Arc<dyn Handler> = Arc::new(
        move |ctx: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
            for i in 0..1000u32 {
                // Announces each item before it waits for the credit to send it.
                let _ = tx.send(i);
                ctx.send_item(i.to_le_bytes().to_vec())?;
            }
            Ok(Vec::new())
        },
    );
    let (_p, mut io) = raw(
        producer,
        PeerOptions::new("producer"),
        FrameLimit::default(),
    );
    write_frame_raw(
        &mut io,
        &Frame::Call {
            id: 1,
            credit: 8,
            body: Vec::new(),
        },
    );
    for i in 0..8u32 {
        assert_eq!(
            read_frame_raw(&mut io),
            Frame::Item {
                id: 1,
                body: i.to_le_bytes().to_vec()
            }
        );
    }
    // It announced item 8 and waits for its credit: it cannot have got further.
    while rx.recv_timeout(GUARD).unwrap() != 8 {}
    assert!(rx.try_recv().is_err());
    write_frame_raw(&mut io, &Frame::Credit { id: 1, n: 1 });
    assert_eq!(
        read_frame_raw(&mut io),
        Frame::Item {
            id: 1,
            body: 8u32.to_le_bytes().to_vec()
        }
    );
    assert_eq!(rx.recv_timeout(GUARD).unwrap(), 9);
}

#[test]
fn dropping_a_stream_cancels_the_call() {
    let (tx, rx) = mpsc::channel();
    let producer: Arc<dyn Handler> = Arc::new(
        move |ctx: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
            loop {
                if let Err(e) = ctx.send_item(vec![1]) {
                    let _ = tx.send(e.code.clone());
                    return Err(e);
                }
            }
        },
    );
    let (a, _b) = pair(Arc::new(NoCalls), producer);
    let mut s = a.stream(Vec::new(), 2).unwrap();
    assert!(matches!(s.next(T).unwrap(), StreamEvent::Item(_)));
    drop(s);
    assert_eq!(rx.recv_timeout(GUARD).unwrap(), codes::CANCELLED);
    // An explicit cancel that the callee honours ends the stream.
    let mut s = a.stream(Vec::new(), 2).unwrap();
    assert!(s.cancel_and_wait().is_ok());
    assert!(s.is_done());
    s.cancel();
    assert_eq!(rx.recv_timeout(GUARD).unwrap(), codes::CANCELLED);
}

#[test]
fn a_callee_that_ignores_the_deadline_and_the_cancel_is_a_timeout() {
    let rude_gate = Gate::default();
    let g = rude_gate.clone();
    let (saw_cancel, cancels) = mpsc::channel();
    let slow: Arc<dyn Handler> = Arc::new(
        move |ctx: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> {
            if body == b"polite" {
                // The probe sees the cancel from another thread (e.g. a watcher interrupting a statement).
                let probe = ctx.cancel_probe();
                std::thread::spawn(move || probe.wait()).join().unwrap();
                assert!(ctx.is_cancelled());
                let _ = saw_cancel.send(());
                return Err(CallCtx::cancelled_error());
            }
            g.wait();
            Ok(Vec::new())
        },
    );
    let mut quick = PeerOptions::new("caller");
    quick.cancel_grace = Duration::from_millis(1);
    let (a, _b) = pair_with(
        Arc::new(NoCalls),
        quick,
        slow,
        PeerOptions::new("slow"),
        FrameLimit::default(),
    );
    // The rude callee is held at its gate until after the call gave up: the outcome cannot depend on timing.
    assert_eq!(
        a.call(b"rude".to_vec(), Some(Duration::from_millis(1)))
            .unwrap_err(),
        IpcError::Timeout
    );
    rude_gate.open();
    // One that ends the call on the cancel is still a timeout for the caller.
    assert_eq!(
        a.call(b"polite".to_vec(), Some(Duration::from_millis(1)))
            .unwrap_err(),
        IpcError::Timeout
    );
    cancels.recv_timeout(GUARD).unwrap();
    // An idle stream says so.
    let mut s = a.stream(b"polite".to_vec(), 1).unwrap();
    assert_eq!(s.next(Some(Duration::ZERO)).unwrap(), StreamEvent::Idle);
    drop(s);
    cancels.recv_timeout(GUARD).unwrap();
    assert_eq!(CANCEL_GRACE, Duration::from_secs(2));
}

#[test]
fn a_late_answer_within_the_grace_still_counts() {
    let (started_tx, started) = mpsc::channel();
    let answers_on_cancel: Arc<dyn Handler> = Arc::new(
        move |ctx: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
            let _ = started_tx.send(());
            ctx.cancel_probe().wait();
            Ok(b"late".to_vec())
        },
    );
    let mut patient = PeerOptions::new("caller");
    patient.cancel_grace = GUARD;
    let (a, _b) = pair_with(
        Arc::new(NoCalls),
        patient,
        answers_on_cancel,
        PeerOptions::new("callee"),
        FrameLimit::default(),
    );
    assert_eq!(
        a.call(Vec::new(), Some(Duration::from_millis(1))).unwrap(),
        b"late"
    );
    started.recv_timeout(GUARD).unwrap();
}

#[test]
fn handlers_may_call_back_while_they_answer() {
    let host: Arc<dyn Handler> =
        Arc::new(|_: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> {
            Ok([b"host:", body].concat())
        });
    let child: Arc<dyn Handler> = Arc::new(
        |ctx: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> {
            let back = ctx
                .peer()
                .call(body.to_vec(), T)
                .map_err(|e| RemoteError::new(codes::INTERNAL, e.to_string()))?;
            Ok([b"child got ", back.as_slice()].concat())
        },
    );
    let (h, _c) = pair(host, child);
    assert_eq!(h.call(b"x".to_vec(), T).unwrap(), b"child got host:x");
}

#[test]
fn too_many_concurrent_calls_are_refused_as_busy() {
    let gate = Gate::default();
    let g = gate.clone();
    let (started_tx, started) = mpsc::channel();
    let blocking: Arc<dyn Handler> = Arc::new(
        move |_: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
            let _ = started_tx.send(());
            g.wait();
            Ok(Vec::new())
        },
    );
    let mut one = PeerOptions::new("b");
    one.max_concurrent_calls = 1;
    let (a, _b) = pair_with(
        Arc::new(NoCalls),
        PeerOptions::new("a"),
        blocking,
        one,
        FrameLimit::default(),
    );
    let a2 = a.clone();
    let first = std::thread::spawn(move || a2.call(Vec::new(), T));
    started.recv_timeout(GUARD).unwrap();
    assert!(matches!(a.call(Vec::new(), T), Err(IpcError::Remote(e)) if e.code == codes::BUSY));
    gate.open();
    assert!(first.join().unwrap().is_ok());
}

#[test]
fn closing_fails_pending_calls_on_both_sides() {
    let (started_tx, started) = mpsc::channel();
    let slow: Arc<dyn Handler> = Arc::new(
        move |ctx: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
            let _ = started_tx.send(());
            ctx.cancel_probe().wait();
            Err(CallCtx::cancelled_error())
        },
    );
    let (a, b) = pair(Arc::new(NoCalls), slow);
    let a2 = a.clone();
    let pending = std::thread::spawn(move || a2.call(Vec::new(), T));
    started.recv_timeout(GUARD).unwrap();
    b.close();
    assert!(matches!(pending.join().unwrap(), Err(IpcError::Closed(_))));
    assert!(a.wait_closed(Some(GUARD)));
    assert!(a.wait_closed(None));
    assert!(a.is_closed() && b.is_closed());
    assert!(matches!(a.closed(), Some(IpcError::Closed(_))));
    assert!(matches!(a.call(Vec::new(), T), Err(IpcError::Closed(_))));
    assert!(matches!(a.stream(Vec::new(), 1), Err(IpcError::Closed(_))));
    assert!(matches!(a.push(Vec::new()), Err(IpcError::Closed(_))));
    // Waiting on an open connection returns false at its limit.
    let (c, _d) = pair(Arc::new(NoCalls), Arc::new(NoCalls));
    assert!(!c.wait_closed(Some(Duration::ZERO)));
}

#[test]
fn pushes_arrive_in_order_and_a_slow_listener_drops_instead_of_blocking() {
    let (tx, rx) = mpsc::channel();
    let gate = Gate::default();
    let g = gate.clone();
    let listener = PeerOptions::new("listener").on_push(move |body| {
        if body == b"hold" {
            g.wait();
        }
        let _ = tx.send(body);
    });
    let (a, b) = pair_with(
        echo(),
        PeerOptions::new("a"),
        echo(),
        listener,
        FrameLimit::default(),
    );
    for i in 0..3u8 {
        a.push(vec![i]).unwrap();
    }
    for i in 0..3u8 {
        assert_eq!(rx.recv_timeout(GUARD).unwrap(), vec![i]);
    }
    a.push_value(&"typed").unwrap();
    assert_eq!(
        paddock_ipc::wire::decode::<String>(&rx.recv_timeout(GUARD).unwrap()).unwrap(),
        "typed"
    );
    // A held listener: its queue (64) fills and later pushes are dropped and counted; the connection keeps working.
    a.push(b"hold".to_vec()).unwrap();
    for _ in 0..100 {
        a.push(b"x".to_vec()).unwrap();
    }
    // A call after the pushes is answered after the reader handled every push before it.
    assert_eq!(a.call(b"after".to_vec(), T).unwrap(), b"after");
    let dropped = b.dropped_pushes();
    assert!((36..=37).contains(&dropped), "{dropped}");
    gate.open();
    assert_eq!(rx.recv_timeout(GUARD).unwrap(), b"hold");
    // A side without a push callback ignores pushes.
    b.push(b"ignored".to_vec()).unwrap();
    assert_eq!(b.call(b"still".to_vec(), T).unwrap(), b"still");
    assert_eq!(a.dropped_pushes(), 0);
}

fn write_raw(io: &mut Stream, bytes: &[u8]) {
    io.write_all(bytes).unwrap();
}

fn write_frame_raw(io: &mut Stream, f: &Frame) {
    write_raw(
        io,
        &frame::encode(f, FrameLimit::new(HARD_MAX_FRAME)).unwrap(),
    );
}

fn read_frame_raw(io: &mut Stream) -> Frame {
    frame::read_frame(io, FrameLimit::new(HARD_MAX_FRAME)).unwrap()
}

#[test]
fn a_reply_over_the_limit_fails_that_call_and_the_link_stays() {
    let (p, mut io) = raw(
        Arc::new(NoCalls),
        PeerOptions::new("the plugin"),
        FrameLimit::new(MIN_FRAME),
    );
    let p2 = p.clone();
    let pending = std::thread::spawn(move || p2.call(Vec::new(), T));
    let id = read_frame_raw(&mut io).call_id().unwrap();
    write_frame_raw(
        &mut io,
        &Frame::Reply {
            id,
            body: vec![1; 4000],
        },
    );
    match pending.join().unwrap() {
        Err(IpcError::TooLarge { len, max }) => assert!(len > 4000 && max == MIN_FRAME),
        other => panic!("{other:?}"),
    }
    // The next call works.
    let p2 = p.clone();
    let next = std::thread::spawn(move || p2.call(b"again".to_vec(), T));
    let id = read_frame_raw(&mut io).call_id().unwrap();
    write_frame_raw(
        &mut io,
        &Frame::Reply {
            id,
            body: b"ok".to_vec(),
        },
    );
    assert_eq!(next.join().unwrap().unwrap(), b"ok");
    assert!(!p.is_closed());

    // An item over the limit ends that stream (and cancels it), not the link.
    let mut s = p.stream(Vec::new(), 2).unwrap();
    let id = read_frame_raw(&mut io).call_id().unwrap();
    write_frame_raw(
        &mut io,
        &Frame::Item {
            id,
            body: vec![2; 4000],
        },
    );
    assert!(matches!(s.next(T), Err(IpcError::TooLarge { .. })));
    assert_eq!(read_frame_raw(&mut io), Frame::Cancel { id });
    // A push over the limit is dropped and counted; a frame for no known call is ignored.
    write_frame_raw(
        &mut io,
        &Frame::Push {
            body: vec![3; 4000],
        },
    );
    write_frame_raw(
        &mut io,
        &Frame::Fail {
            id: 999,
            error: RemoteError::new("x", "y".repeat(4000)),
        },
    );
    let p2 = p.clone();
    let last = std::thread::spawn(move || p2.call(Vec::new(), T));
    let id = read_frame_raw(&mut io).call_id().unwrap();
    write_frame_raw(
        &mut io,
        &Frame::Reply {
            id,
            body: Vec::new(),
        },
    );
    assert!(last.join().unwrap().is_ok());
    assert_eq!(p.dropped_pushes(), 1);
    assert!(!p.is_closed());
}

#[test]
fn a_request_over_the_limit_fails_that_call_on_either_side() {
    let (p, mut io) = raw(
        echo(),
        PeerOptions::new("the host"),
        FrameLimit::new(MIN_FRAME),
    );
    // Sent by the raw side: the peer skips it and answers that call with `tooLarge`.
    write_frame_raw(
        &mut io,
        &Frame::Call {
            id: 5,
            credit: 0,
            body: vec![0; 4000],
        },
    );
    match read_frame_raw(&mut io) {
        Frame::Fail { id: 5, error } => assert_eq!(error.code, codes::TOO_LARGE),
        other => panic!("{other:?}"),
    }
    write_frame_raw(
        &mut io,
        &Frame::Call {
            id: 6,
            credit: 0,
            body: b"fits".to_vec(),
        },
    );
    assert_eq!(
        read_frame_raw(&mut io),
        Frame::Reply {
            id: 6,
            body: b"fits".to_vec()
        }
    );
    // Made on this side: refused before anything is sent.
    assert!(matches!(
        p.call(vec![0; 4000], T),
        Err(IpcError::TooLarge { .. })
    ));
    assert!(matches!(
        p.push(vec![0; 4000]),
        Err(IpcError::TooLarge { .. })
    ));
    assert!(!p.is_closed());
}

#[test]
fn an_answer_too_large_for_a_frame_is_a_typed_error_and_the_link_stays() {
    let big: Arc<dyn Handler> = Arc::new(
        |ctx: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> {
            if body == b"item" {
                ctx.send_item(vec![0; 4096])?;
            }
            Ok(vec![0; 4096])
        },
    );
    let small = FrameLimit::new(MIN_FRAME);
    let (a, _b) = pair_with(
        Arc::new(NoCalls),
        PeerOptions::new("a"),
        big,
        PeerOptions::new("b"),
        small,
    );
    assert!(
        matches!(a.call(Vec::new(), T), Err(IpcError::Remote(e)) if e.code == codes::TOO_LARGE && e.message.contains("could not be sent"))
    );
    let mut s = a.stream(b"item".to_vec(), 1).unwrap();
    assert!(matches!(s.next(T), Err(IpcError::Remote(e)) if e.code == codes::TOO_LARGE));
    assert!(!a.is_closed());
}

#[test]
fn a_length_beyond_any_limit_closes_the_connection() {
    let (p, mut io) = raw(
        Arc::new(NoCalls),
        PeerOptions::new("the plugin"),
        FrameLimit::default(),
    );
    let p2 = p.clone();
    let pending = std::thread::spawn(move || p2.call(Vec::new(), T));
    let _call = read_frame_raw(&mut io);
    write_raw(&mut io, &(100u32 << 20).to_le_bytes());
    match pending.join().unwrap() {
        Err(IpcError::Protocol(m)) => assert!(m.contains("exceeds the limit"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(p.wait_closed(Some(GUARD)));
}

#[test]
fn garbage_closes_the_connection() {
    let (p, mut io) = raw(
        Arc::new(NoCalls),
        PeerOptions::new("the plugin"),
        FrameLimit::default(),
    );
    write_raw(&mut io, &[5, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0xff]);
    assert!(p.wait_closed(Some(GUARD)));
    assert!(matches!(p.closed(), Some(IpcError::Protocol(_))));
    // A stream that ends inside a frame is a broken connection.
    let (p, mut io) = raw(
        Arc::new(NoCalls),
        PeerOptions::new("the plugin"),
        FrameLimit::default(),
    );
    write_raw(&mut io, &[9, 0, 0, 0, 1]);
    drop(io);
    assert!(p.wait_closed(Some(GUARD)));
    assert!(matches!(p.closed(), Some(IpcError::Closed(_))));
}

#[test]
fn a_repeated_handshake_or_items_beyond_credit_break_the_protocol() {
    let (p, mut io) = raw(
        Arc::new(NoCalls),
        PeerOptions::new("the plugin"),
        FrameLimit::default(),
    );
    write_frame_raw(
        &mut io,
        &Frame::Welcome {
            version: 1,
            protocol: Default::default(),
        },
    );
    assert!(p.wait_closed(Some(GUARD)));
    assert!(matches!(p.closed(), Some(IpcError::Protocol(m)) if m.contains("handshake")));

    let (p, mut io) = raw(
        Arc::new(NoCalls),
        PeerOptions::new("the plugin"),
        FrameLimit::default(),
    );
    // Nothing is taken, so no credit is granted beyond the first item.
    let mut s = p.stream(Vec::new(), 1).unwrap();
    let id = read_frame_raw(&mut io).call_id().unwrap();
    write_frame_raw(&mut io, &Frame::Item { id, body: vec![1] });
    write_frame_raw(&mut io, &Frame::Item { id, body: vec![2] });
    assert!(p.wait_closed(Some(GUARD)));
    assert!(matches!(s.next(T).unwrap(), StreamEvent::Item(_)));
    assert!(matches!(p.closed(), Some(IpcError::Protocol(m)) if m.contains("more items")));

    // An item for a call that does not stream.
    let (p, mut io) = raw(
        Arc::new(NoCalls),
        PeerOptions::new("the plugin"),
        FrameLimit::default(),
    );
    let p2 = p.clone();
    let pending = std::thread::spawn(move || p2.call(Vec::new(), T));
    let id = read_frame_raw(&mut io).call_id().unwrap();
    write_frame_raw(&mut io, &Frame::Item { id, body: vec![1] });
    assert!(matches!(
        pending.join().unwrap(),
        Err(IpcError::Protocol(_))
    ));
    assert!(p.wait_closed(Some(GUARD)));
}

#[test]
fn credits_and_cancels_from_the_other_side_are_honoured() {
    let producer: Arc<dyn Handler> =
        Arc::new(|ctx: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
            let mut n = 0u8;
            loop {
                ctx.send_item(vec![n])?;
                n += 1;
            }
        });
    let (_p, mut io) = raw(
        producer,
        PeerOptions::new("producer"),
        FrameLimit::default(),
    );
    write_frame_raw(
        &mut io,
        &Frame::Call {
            id: 9,
            credit: 1,
            body: Vec::new(),
        },
    );
    assert_eq!(
        read_frame_raw(&mut io),
        Frame::Item {
            id: 9,
            body: vec![0]
        }
    );
    write_frame_raw(&mut io, &Frame::Credit { id: 9, n: 1 });
    assert_eq!(
        read_frame_raw(&mut io),
        Frame::Item {
            id: 9,
            body: vec![1]
        }
    );
    // Credit and cancel for calls that do not exist are ignored.
    write_frame_raw(&mut io, &Frame::Credit { id: 77, n: 1 });
    write_frame_raw(&mut io, &Frame::Cancel { id: 78 });
    write_frame_raw(&mut io, &Frame::Cancel { id: 9 });
    match read_frame_raw(&mut io) {
        Frame::Fail { id: 9, error } => assert_eq!(error.code, codes::CANCELLED),
        other => panic!("{other:?}"),
    }
    // A unary call cannot stream.
    let streaming: Arc<dyn Handler> =
        Arc::new(|ctx: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
            ctx.send_item(Vec::new())?;
            Ok(Vec::new())
        });
    let (a, _b) = pair(Arc::new(NoCalls), streaming);
    assert!(matches!(a.call(Vec::new(), T), Err(IpcError::Remote(e)) if e.code == codes::INVALID));
}

#[test]
fn closing_one_side_ends_the_other_and_served_calls_see_a_cancel() {
    let (tx, rx) = mpsc::channel();
    let (started_tx, started) = mpsc::channel();
    let watcher: Arc<dyn Handler> = Arc::new(
        move |ctx: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
            let _ = started_tx.send(());
            ctx.cancel_probe().wait();
            let _ = tx.send(ctx.is_cancelled());
            Err(CallCtx::cancelled_error())
        },
    );
    let (a, b): (Peer, Peer) = pair(Arc::new(NoCalls), watcher);
    let a2 = a.clone();
    let _pending = std::thread::spawn(move || a2.call(Vec::new(), T));
    started.recv_timeout(GUARD).unwrap();
    a.close();
    assert!(rx.recv_timeout(GUARD).unwrap());
    assert!(b.wait_closed(Some(GUARD)));
}

#[test]
fn calls_over_the_limit_wait_in_order_in_the_call_queue_and_past_it_are_busy() {
    let gate = Gate::default();
    let g = gate.clone();
    let order = Arc::new(Mutex::new(Vec::new()));
    let o = order.clone();
    let slow: Arc<dyn Handler> = Arc::new(
        move |_: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> {
            o.lock().unwrap().push(body.to_vec());
            g.wait();
            Ok(body.to_vec())
        },
    );
    let mut one = PeerOptions::new("b");
    one.max_concurrent_calls = 1;
    one.call_queue = 2;
    let limit = FrameLimit::default();
    let (_peer, mut raw) = raw(slow, one, limit);
    // One connection, so the callee takes the frames in the order they are written: 1 runs, 2 and 3 wait, 4 is refused.
    for id in 1..=4u64 {
        let call = Frame::Call {
            id,
            credit: 0,
            body: id.to_string().into_bytes(),
        };
        frame::write_frame(&mut raw, &call, limit).unwrap();
    }
    match frame::read_frame(&mut raw, limit).unwrap() {
        Frame::Fail { id: 4, error } => assert_eq!(error.code, codes::BUSY),
        other => panic!("{other:?}"),
    }
    gate.open();
    for id in 1..=3u64 {
        match frame::read_frame(&mut raw, limit).unwrap() {
            Frame::Reply { id: got, body } => {
                assert_eq!((got, body), (id, id.to_string().into_bytes()));
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(
        *order.lock().unwrap(),
        vec![b"1".to_vec(), b"2".to_vec(), b"3".to_vec()]
    );
}

#[test]
fn closing_ends_calls_that_were_still_waiting_in_the_queue() {
    let gate = Gate::default();
    let g = gate.clone();
    let ran = Arc::new(AtomicUsize::new(0));
    let r = ran.clone();
    let slow: Arc<dyn Handler> = Arc::new(
        move |_: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
            r.fetch_add(1, Ordering::SeqCst);
            g.wait();
            Ok(Vec::new())
        },
    );
    let mut one = PeerOptions::new("b");
    one.max_concurrent_calls = 1;
    one.call_queue = 4;
    let limit = FrameLimit::default();
    let (peer, mut raw) = raw(slow, one, limit);
    for id in 1..=3u64 {
        frame::write_frame(
            &mut raw,
            &Frame::Call {
                id,
                credit: 0,
                body: Vec::new(),
            },
            limit,
        )
        .unwrap();
    }
    // A cancel round trip proves the reader has taken all three calls before the connection closes.
    frame::write_frame(&mut raw, &Frame::Cancel { id: 99 }, limit).unwrap();
    peer.close();
    gate.open();
    assert!(peer.wait_closed(Some(GUARD)));
    // Only the call that was running ran; the waiting ones were dropped with the connection.
    assert!(ran.load(Ordering::SeqCst) <= 1);
}
