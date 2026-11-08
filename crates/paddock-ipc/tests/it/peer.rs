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
        Err(IpcError::ReplyTooLarge { len, max }) => assert!(len > 4000 && max == MIN_FRAME),
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
    assert!(matches!(s.next(T), Err(IpcError::ReplyTooLarge { .. })));
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
    let (ran_tx, ran) = mpsc::channel::<()>();
    let slow: Arc<dyn Handler> = Arc::new(
        move |_: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
            let _ = ran_tx.send(());
            g.wait();
            Ok(Vec::new())
        },
    );
    let mut one = PeerOptions::new("b");
    one.max_concurrent_calls = 1;
    one.call_queue = 2;
    let limit = FrameLimit::default();
    let (peer, mut raw) = raw(slow, one, limit);
    for id in 1..=4u64 {
        let call = Frame::Call {
            id,
            credit: 0,
            body: Vec::new(),
        };
        frame::write_frame(&mut raw, &call, limit).unwrap();
    }
    // The refusal of the fourth proves the reader took the first three: one runs, two wait.
    match frame::read_frame(&mut raw, limit).unwrap() {
        Frame::Fail { id: 4, error } => assert_eq!(error.code, codes::BUSY),
        other => panic!("{other:?}"),
    }
    // The running call has started (its handler reported in) before the connection closes.
    ran.recv_timeout(GUARD).unwrap();
    peer.close();
    gate.open();
    assert!(peer.wait_closed(Some(GUARD)));
    // Every thread that holds the handler ends once the running call is done and the queue is dropped; the handler's sender
    // goes with the last of them, so the channel disconnecting is the condition "nothing else will run". The two calls that
    // waited never ran.
    assert!(matches!(
        ran.recv_timeout(GUARD),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));
}

#[test]
fn a_call_cancelled_while_it_waits_is_answered_as_cancelled_and_never_runs_and_queued_bytes_are_capped()
 {
    let gate = Gate::default();
    let g = gate.clone();
    let ran = Arc::new(Mutex::new(Vec::new()));
    let r = ran.clone();
    let slow: Arc<dyn Handler> = Arc::new(
        move |_: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> {
            r.lock().unwrap().push(body.to_vec());
            g.wait();
            Ok(Vec::new())
        },
    );
    let mut one = PeerOptions::new("b");
    one.max_concurrent_calls = 1;
    one.call_queue = 4;
    one.call_queue_bytes = 8;
    let limit = FrameLimit::default();
    let (_peer, mut raw) = raw(slow, one, limit);
    let call = |id: u64, len: usize| Frame::Call {
        id,
        credit: 0,
        body: vec![b'x'; len],
    };
    // 1 runs; 2 (5 bytes) waits; 3 (5 bytes) would make 10 > 8 queued bytes and is refused.
    frame::write_frame(&mut raw, &call(1, 1), limit).unwrap();
    frame::write_frame(&mut raw, &call(2, 5), limit).unwrap();
    frame::write_frame(&mut raw, &call(3, 5), limit).unwrap();
    match frame::read_frame(&mut raw, limit).unwrap() {
        Frame::Fail { id: 3, error } => assert_eq!(error.code, codes::BUSY),
        other => panic!("{other:?}"),
    }
    // The caller cancels 2 while it waits. A fourth call that the byte cap refuses is answered after the cancel was taken (one
    // reader, frames in order), so its refusal is the condition "the cancel is in" before the running call is let go.
    frame::write_frame(&mut raw, &Frame::Cancel { id: 2 }, limit).unwrap();
    frame::write_frame(&mut raw, &call(4, 5), limit).unwrap();
    match frame::read_frame(&mut raw, limit).unwrap() {
        Frame::Fail { id: 4, error } => assert_eq!(error.code, codes::BUSY),
        other => panic!("{other:?}"),
    }
    gate.open();
    let mut answers = Vec::new();
    while answers.len() < 2 {
        match frame::read_frame(&mut raw, limit).unwrap() {
            Frame::Reply { id, .. } => answers.push((id, true)),
            Frame::Fail { id, error } => {
                assert_eq!((id, error.code.as_str()), (2, codes::CANCELLED));
                answers.push((id, false));
            }
            other => panic!("{other:?}"),
        }
    }
    answers.sort_unstable();
    assert_eq!(answers, vec![(1, true), (2, false)]);
    // Only the first call ran; 2 was answered as cancelled without running.
    assert_eq!(*ran.lock().unwrap(), vec![vec![b'x']]);
}

/// IP1 item 1: a reply over THIS side's limit is the other side's doing. It must not look like our own request being too
/// large (`IpcError::TooLarge`, which callers word as "the request is N bytes").
#[test]
fn an_oversized_reply_is_reported_as_the_other_sides_not_as_our_request() {
    let (p, mut io) = raw(
        Arc::new(NoCalls),
        PeerOptions::new("the plugin"),
        FrameLimit::new(MIN_FRAME),
    );
    let p2 = p.clone();
    let pending = std::thread::spawn(move || p2.call(Vec::new(), T));
    let Frame::Call { id, .. } = read_frame_raw(&mut io) else {
        panic!("a call was expected");
    };
    let big = frame::encode(
        &Frame::Reply {
            id,
            body: vec![0; MIN_FRAME + 4096],
        },
        FrameLimit::default(),
    )
    .unwrap();
    write_raw(&mut io, &big);
    let err = pending.join().unwrap().unwrap_err();
    assert!(
        matches!(err, IpcError::ReplyTooLarge { len, max } if len > max && max == MIN_FRAME),
        "an oversized reply came back as {err:?}"
    );
    assert!(!err.is_process_failure());
    assert!(err.to_string().starts_with("its answer of"), "{err}");
    assert!(
        !p.is_closed(),
        "one oversized reply fails one call, not the link"
    );
}

/// IP1 item 2: a peer that stops reading makes this side's sends block and then fail at the bound; memory does not grow.
#[test]
fn a_peer_that_stops_reading_makes_sends_block_and_then_fail_at_the_bound() {
    let clock = Arc::new(paddock_jobs::time::FakeClock::new());
    let mut opts = PeerOptions::new("the plugin");
    opts.write_queue = 2;
    opts.write_queue_bytes = 8 << 20;
    opts.write_stall = Duration::from_secs(30);
    opts.clock = clock.clone();
    let (p, io) = raw(Arc::new(NoCalls), opts, FrameLimit::default());
    // A call is waiting for its answer (no timeout of its own) when the other side stops reading.
    let waiting = {
        let p = p.clone();
        std::thread::spawn(move || p.call(b"waiting".to_vec(), None))
    };
    // `io` is never read: the writer blocks once the socket buffer is full, then the queue fills (2 frames).
    let sender = {
        let p = p.clone();
        std::thread::spawn(move || {
            let mut sent = 0usize;
            loop {
                match p.push(vec![0u8; 1 << 20]) {
                    Ok(()) => sent += 1,
                    Err(e) => return (sent, e),
                }
            }
        })
    };
    assert!(clock.wait_for_waiters(1, GUARD), "the sender never blocked");
    assert!(
        !sender.is_finished(),
        "a full queue blocks, it does not fail at once"
    );
    clock.advance(Duration::from_secs(30));
    let (sent, err) = sender.join().unwrap();
    assert!(matches!(err, IpcError::Timeout), "{err:?}");
    // At most the socket buffer's worth, the frame in the writer's hands and the two queued ones went out.
    assert!(sent < 64, "sent {sent} MiB before the bound");
    // A stalled send drops its frame, so the connection cannot go on: it is closed with the reason, and the call that waited
    // for an answer fails with it instead of waiting for ever.
    assert!(p.is_closed(), "a stalled send must close the connection");
    let reason = p.closed().unwrap().to_string();
    assert!(
        reason.contains("the plugin stopped reading for 30 s"),
        "{reason}"
    );
    let released = waiting.join().unwrap().unwrap_err();
    assert!(
        released.to_string().contains("stopped reading"),
        "{released:?}"
    );
    drop(io);
}

/// IP1 review P1-1: a reply that stalls is a frame dropped. The connection closes, and a call the other side was waiting
/// on is released by the close (here the test is the other side and sees the stream end).
#[test]
fn a_served_calls_stalled_reply_closes_the_connection_and_releases_the_caller() {
    let clock = Arc::new(paddock_jobs::time::FakeClock::new());
    let mut opts = PeerOptions::new("the plugin");
    opts.write_queue = 1;
    opts.write_queue_bytes = 8 << 20;
    opts.write_stall = Duration::from_secs(30);
    opts.clock = clock.clone();
    let big: Arc<dyn Handler> = Arc::new(|_: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
        Ok(vec![7u8; 1 << 20])
    });
    let (p, mut io) = raw(big, opts, FrameLimit::default());
    // 40 calls whose 1 MiB replies the test never reads. The end state is stable and exact: the writer holds reply 1 (blocked on
    // the full socket), the one-frame queue holds reply 2, and replies 3..=40 wait for room on the clock. Waiting for the first
    // waiter is not enough: while the writer is still taking reply 1, a waiter appears and is released again by that pop, and an
    // advance at that moment makes every later reply start its 30 s from the advanced clock, so none ever expires.
    for id in 1..=40u64 {
        write_frame_raw(
            &mut io,
            &Frame::Call {
                id,
                credit: 0,
                body: Vec::new(),
            },
        );
    }
    assert!(
        clock.wait_for_waiters(38, GUARD),
        "the replies never all waited for room"
    );
    clock.advance(Duration::from_secs(30));
    assert!(
        p.wait_closed(Some(GUARD)),
        "a stalled reply left the connection open"
    );
    assert!(p.closed().unwrap().to_string().contains("stopped reading"));
    // The other side sees the stream end after what was already written: it is released, not left waiting.
    let mut ended = false;
    for _ in 0..80 {
        match frame::read_frame(&mut io, FrameLimit::default()) {
            Ok(_) => {}
            Err(_) => {
                ended = true;
                break;
            }
        }
    }
    assert!(ended, "the stream never ended");
}

/// IP1 item 3: a call id that is still in progress, sent again, is a protocol error that closes the connection.
#[test]
fn a_repeated_call_id_closes_the_connection() {
    let gate = Gate::default();
    let (started_tx, started_rx) = mpsc::channel();
    let started_tx = Mutex::new(started_tx);
    let waiting = gate.clone();
    let handler: Arc<dyn Handler> = Arc::new(
        move |_: &CallCtx, _: &[u8]| -> Result<Vec<u8>, RemoteError> {
            let _ = started_tx.lock().unwrap().send(());
            waiting.wait();
            Ok(Vec::new())
        },
    );
    let (p, mut io) = raw(
        handler,
        PeerOptions::new("the plugin"),
        FrameLimit::default(),
    );
    let call = |id| {
        frame::encode(
            &Frame::Call {
                id,
                credit: 0,
                body: Vec::new(),
            },
            FrameLimit::default(),
        )
        .unwrap()
    };
    write_raw(&mut io, &call(7));
    started_rx
        .recv_timeout(GUARD)
        .expect("the first call never started");
    write_raw(&mut io, &call(7));
    assert!(p.wait_closed(Some(GUARD)));
    match p.closed() {
        Some(IpcError::Protocol(m)) => assert!(m.contains("call 7"), "{m}"),
        other => panic!("{other:?}"),
    }
    gate.open();
}

/// A different id while the first call runs is fine, and an id may be used again once its call has ended.
#[test]
fn distinct_ids_and_reuse_after_the_call_ended_are_not_errors() {
    let (p, mut io) = raw(
        echo(),
        PeerOptions::new("the plugin"),
        FrameLimit::default(),
    );
    let call = |id: u64, body: &[u8]| {
        frame::encode(
            &Frame::Call {
                id,
                credit: 0,
                body: body.to_vec(),
            },
            FrameLimit::default(),
        )
        .unwrap()
    };
    for (id, body) in [(1, b"a"), (2, b"b")] {
        write_raw(&mut io, &call(id, body));
        assert_eq!(
            read_frame_raw(&mut io),
            Frame::Reply {
                id,
                body: body.to_vec()
            }
        );
    }
    write_raw(&mut io, &call(1, b"c"));
    assert_eq!(
        read_frame_raw(&mut io),
        Frame::Reply {
            id: 1,
            body: b"c".to_vec()
        }
    );
    assert!(!p.is_closed());
}

/// IP1 item 2: when the writer dies (a write fails), nothing more is queued for it: later sends fail at once with the
/// reason the connection closed, and do not wait for the stall.
#[test]
fn sends_after_the_writer_died_fail_at_once_with_the_close_reason() {
    let (p, io) = raw(
        Arc::new(NoCalls),
        PeerOptions::new("the plugin"),
        FrameLimit::default(),
    );
    drop(io);
    assert!(p.wait_closed(Some(GUARD)));
    let reason = p.closed().expect("closed");
    assert_eq!(p.push(b"late".to_vec()).unwrap_err(), reason);
    assert_eq!(p.call(b"late".to_vec(), T).unwrap_err(), reason);
}

/// IP1 review P2: an answer that is too large reaches the other side as `tooLarge`, whichever direction was too large.
#[test]
fn both_directions_of_an_oversized_frame_answer_too_large() {
    assert_eq!(
        IpcError::TooLarge { len: 9, max: 5 }.to_remote().code,
        codes::TOO_LARGE
    );
    assert_eq!(
        IpcError::ReplyTooLarge { len: 9, max: 5 }.to_remote().code,
        codes::TOO_LARGE
    );
    assert_eq!(IpcError::Timeout.to_remote().code, codes::INTERNAL);
}

/// IP1 review P2: a call's own timeout is honoured while it waits for room in the queue: the call fails with Timeout, the
/// connection stays (the other side has not been silent for the whole stall), and nothing grew.
#[test]
fn a_calls_own_timeout_ends_its_wait_for_room_without_closing_the_connection() {
    let clock = Arc::new(paddock_jobs::time::FakeClock::new());
    let mut opts = PeerOptions::new("the plugin");
    opts.write_queue = 1;
    opts.write_queue_bytes = 8 << 20;
    opts.write_stall = Duration::from_secs(30);
    opts.clock = clock.clone();
    let (p, io) = raw(Arc::new(NoCalls), opts, FrameLimit::default());
    // Fill the socket buffer, the writer's hand and the one-frame queue with pushes from a thread that is left waiting.
    let filler = {
        let p = p.clone();
        std::thread::spawn(move || while p.push(vec![0u8; 1 << 20]).is_ok() {})
    };
    assert!(clock.wait_for_waiters(1, GUARD), "the queue never filled");
    let caller = {
        let p = p.clone();
        std::thread::spawn(move || p.call(b"x".to_vec(), Some(Duration::from_secs(2))))
    };
    assert!(
        clock.wait_for_waiters(2, GUARD),
        "the call never waited for room"
    );
    clock.advance(Duration::from_secs(2));
    let err = caller.join().unwrap().unwrap_err();
    assert!(matches!(err, IpcError::Timeout), "{err:?}");
    assert!(
        !p.is_closed(),
        "one call running out of time is not the other side's silence"
    );
    // The stall itself, later, closes the connection and ends the filler.
    clock.advance(Duration::from_secs(30));
    filler.join().unwrap();
    assert!(p.is_closed());
    drop(io);
}

/// A peer whose write queue (one frame) is full because the other side (the test) has not read for a while: a filler thread
/// pushes 1 MiB frames until it blocks on the fake clock. `stop` ends the filler after its current push.
fn full_queue_peer(
    reader_refuses: bool,
) -> (
    Peer,
    Stream,
    Arc<paddock_jobs::time::FakeClock>,
    Arc<std::sync::atomic::AtomicBool>,
    std::thread::JoinHandle<()>,
) {
    let clock = Arc::new(paddock_jobs::time::FakeClock::new());
    let mut opts = PeerOptions::new("the plugin");
    opts.write_queue = 1;
    opts.write_queue_bytes = 8 << 20;
    if reader_refuses {
        opts.max_concurrent_calls = 0;
        opts.call_queue = 0;
    }
    opts.clock = clock.clone();
    let (p, io) = raw(Arc::new(NoCalls), opts, FrameLimit::default());
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let filler = {
        let (p, stop) = (p.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) && p.push(vec![0u8; 1 << 20]).is_ok() {}
        })
    };
    assert!(clock.wait_for_waiters(1, GUARD), "the queue never filled");
    (p, io, clock, stop, filler)
}

/// Review of IP1 (follow-up 1): a write queue that is merely full is back-pressure, not silence. A refusal the reader thread
/// must send then goes into the small reserve beyond the bounds, and the connection stays up (no time has passed, so
/// nothing says the other side stopped reading).
#[test]
fn a_refusal_on_the_reader_thread_fits_the_reserve_when_the_queue_is_merely_full() {
    let (p, mut io, _clock, stop, filler) = full_queue_peer(true);
    write_frame_raw(
        &mut io,
        &Frame::Call {
            id: 1,
            credit: 0,
            body: Vec::new(),
        },
    );
    // The test starts reading again: the refusal arrives, in order, after what was queued.
    stop.store(true, Ordering::SeqCst);
    let refusal = loop {
        match frame::read_frame(&mut io, FrameLimit::new(HARD_MAX_FRAME)) {
            Ok(Frame::Fail { id: 1, error }) => break error,
            Ok(_) => {}
            Err(e) => panic!(
                "the link closed before the refusal arrived: {e:?} / {:?}",
                p.closed()
            ),
        }
    };
    assert_eq!(refusal.code, codes::BUSY);
    assert!(
        !p.is_closed(),
        "back-pressure alone closed the link: {:?}",
        p.closed()
    );
    filler.join().unwrap();
}

/// Review of IP1 (follow-up 1): the reserve is bounded. When even it is used up (the other side sends calls and reads
/// nothing), the connection closes, and the reason says what happened instead of naming a time that did not pass.
#[test]
fn a_used_up_reserve_closes_the_link_with_an_accurate_reason() {
    let (p, mut io, _clock, _stop, filler) = full_queue_peer(true);
    // One queued filler frame plus the 16-frame reserve of a peer that runs no calls; the 17th refusal does not fit.
    for id in 1..=17u64 {
        write_frame_raw(
            &mut io,
            &Frame::Call {
                id,
                credit: 0,
                body: Vec::new(),
            },
        );
    }
    assert!(
        p.wait_closed(Some(GUARD)),
        "an exhausted reserve left the link open"
    );
    let reason = p.closed().unwrap().to_string();
    assert!(
        reason.contains("the plugin had 17 frames unread when a reply was due"),
        "{reason}"
    );
    assert!(!reason.contains("stopped reading for"), "{reason}");
    filler.join().unwrap();
}

/// Review of IP1 (follow-up 2): a cancel that cannot be queued within the grace is a frame dropped, and a callee that never
/// hears it keeps running while the link looks healthy. The link closes, like every other dropped frame.
#[test]
fn a_cancel_that_cannot_be_queued_closes_the_link() {
    let clock = Arc::new(paddock_jobs::time::FakeClock::new());
    let mut opts = PeerOptions::new("the plugin");
    opts.write_queue = 1;
    opts.write_queue_bytes = 8 << 20;
    opts.clock = clock.clone();
    let (p, io) = raw(Arc::new(NoCalls), opts, FrameLimit::default());
    let caller = {
        let p = p.clone();
        std::thread::spawn(move || p.call(b"x".to_vec(), Some(Duration::from_secs(2))))
    };
    assert!(clock.wait_for_waiters(1, GUARD), "the call was never sent");
    let filler = {
        let p = p.clone();
        std::thread::spawn(move || while p.push(vec![0u8; 1 << 20]).is_ok() {})
    };
    assert!(clock.wait_for_waiters(2, GUARD), "the queue never filled");
    let started = clock.waits_started();
    clock.advance(Duration::from_secs(2));
    // The call timed out and its cancel now waits for room.
    assert!(
        clock.wait_for_started(started + 1, GUARD),
        "no cancel waited for room"
    );
    clock.advance(CANCEL_GRACE);
    assert!(
        p.wait_closed(Some(GUARD)),
        "a cancel that was dropped left the link open"
    );
    let reason = p.closed().unwrap().to_string();
    assert!(reason.contains("cancel"), "{reason}");
    assert!(caller.join().unwrap().is_err());
    filler.join().unwrap();
    drop(io);
}

/// Review of IP1 (follow-up 3): a call's timeout covers the wait for room and the wait for the answer together. A call
/// that spent 6 s of 10 s waiting for room times out 4 s after it was sent, not 10 s.
#[test]
fn a_calls_timeout_is_shared_by_the_wait_for_room_and_the_wait_for_the_answer() {
    let (p, mut io, clock, stop, filler) = full_queue_peer(false);
    let (tx, rx) = mpsc::channel();
    {
        let p = p.clone();
        std::thread::spawn(move || {
            let _ = tx.send(p.call(b"x".to_vec(), Some(Duration::from_secs(10))));
        });
    }
    assert!(
        clock.wait_for_waiters(2, GUARD),
        "the call never waited for room"
    );
    clock.advance(Duration::from_secs(6));
    // Room appears: the test reads until the call itself came through.
    stop.store(true, Ordering::SeqCst);
    loop {
        if let Frame::Call { .. } = read_frame_raw(&mut io) {
            break;
        }
    }
    filler.join().unwrap();
    assert!(
        clock.wait_for_waiters(1, GUARD),
        "the call never waited for its answer"
    );
    let started = clock.waits_started();
    clock.advance(Duration::from_secs(4));
    assert!(
        clock.wait_for_started(started + 1, GUARD),
        "10 s are not over, but the call still waited for its answer after the remaining 4 s"
    );
    clock.advance(CANCEL_GRACE);
    let err = rx
        .recv_timeout(GUARD)
        .expect("the call never ended")
        .unwrap_err();
    assert!(matches!(err, IpcError::Timeout), "{err:?}");
}
