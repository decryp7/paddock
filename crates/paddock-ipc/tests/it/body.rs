//! `BodyPeer`: one body type for calls, answers and pushes, over the same frames as any peer.

use crate::common::GUARD;
use paddock_ipc::endpoint;
use paddock_ipc::{
    BodyOptions, BodyPeer, Connection, FrameLimit, IpcError, Peer, RemoteError, codes,
};
use serde::{Deserialize, Serialize};
use std::sync::mpsc;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Msg {
    Ask(String),
    Answer(String),
    Event(u32),
}

fn pair(a: BodyOptions<Msg>, b: BodyOptions<Msg>) -> (BodyPeer<Msg>, BodyPeer<Msg>) {
    let (x, y) = endpoint::pair().unwrap();
    let limit = FrameLimit::default();
    (
        BodyPeer::start(Connection::raw(x, limit), a).unwrap(),
        BodyPeer::start(Connection::raw(y, limit), b).unwrap(),
    )
}

fn echo() -> impl Fn(Msg) -> Result<Msg, RemoteError> {
    |m| match m {
        Msg::Ask(s) if s == "panic" => panic!("handler panicked"),
        Msg::Ask(s) if s == "fail" => Err(RemoteError::new("nope", "it failed")),
        Msg::Ask(s) => Ok(Msg::Answer(s)),
        _ => Err(RemoteError::new(codes::INVALID, "asks only")),
    }
}

#[test]
fn calls_return_typed_answers_failures_and_contained_panics() {
    let (a, _b) = pair(BodyOptions::new("a"), BodyOptions::new("b").serve(echo()));
    assert_eq!(
        a.call(Msg::Ask("hi".into()), GUARD).unwrap(),
        Msg::Answer("hi".into())
    );
    assert_eq!(
        a.call(Msg::Ask("fail".into()), GUARD).unwrap_err(),
        IpcError::Remote(RemoteError::new("nope", "it failed"))
    );
    assert!(matches!(
        a.call(Msg::Ask("panic".into()), GUARD),
        Err(IpcError::Remote(e)) if e.code == codes::PANIC
    ));
    // The connection survives the panic.
    assert_eq!(
        a.call(Msg::Ask("again".into()), GUARD).unwrap(),
        Msg::Answer("again".into())
    );
}

#[test]
fn a_side_without_a_handler_refuses_calls() {
    let (a, b) = pair(BodyOptions::new("a"), BodyOptions::new("b"));
    assert!(matches!(
        a.call(Msg::Ask("x".into()), GUARD),
        Err(IpcError::Remote(e)) if e.code == codes::UNIMPLEMENTED
    ));
    assert!(matches!(
        b.call(Msg::Ask("x".into()), GUARD),
        Err(IpcError::Remote(e)) if e.code == codes::UNIMPLEMENTED
    ));
}

#[test]
fn pushes_arrive_in_order_and_close_is_seen_by_both_sides() {
    let (tx, rx) = mpsc::channel();
    let (a, b) = pair(
        BodyOptions::new("a"),
        BodyOptions::new("b").on_push(move |m| {
            let _ = tx.send(m);
        }),
    );
    for i in 0..3 {
        a.push(Msg::Event(i)).unwrap();
    }
    for i in 0..3 {
        assert_eq!(rx.recv_timeout(GUARD).unwrap(), Msg::Event(i));
    }
    assert!(!a.is_closed());
    assert_eq!(a.dropped_pushes(), 0);
    assert_eq!(b.undecodable_pushes(), 0);
    assert!(a.peer().label() == "a");
    a.clone().close();
    assert!(b.wait_closed(Some(GUARD)));
    assert!(a.is_closed() && a.closed().is_some());
    assert!(a.push(Msg::Event(9)).is_err());
    assert!(format!("{a:?}").contains("BodyPeer"));
}

#[test]
fn a_body_that_does_not_decode_fails_the_call_or_is_counted_not_fatal() {
    let (x, y) = endpoint::pair().unwrap();
    let limit = FrameLimit::default();
    let (tx, rx) = mpsc::channel();
    let typed = BodyPeer::start(
        Connection::raw(x, limit),
        BodyOptions::new("typed")
            .serve(echo())
            .on_push(move |m: Msg| {
                let _ = tx.send(m);
            }),
    )
    .unwrap();
    let plain = Peer::start(
        Connection::raw(y, limit),
        std::sync::Arc::new(paddock_ipc::NoCalls),
        paddock_ipc::PeerOptions::new("plain"),
    )
    .unwrap();
    // A byte that is no `Msg` variant.
    let junk = vec![0xff, 0xff, 0xff];
    let err = plain.call(junk.clone(), Some(GUARD)).unwrap_err();
    assert!(matches!(err, IpcError::Remote(e) if e.code == codes::INVALID));
    plain.push(junk).unwrap();
    // A good push after the bad one still arrives, and the bad one was counted.
    plain.push_value(&Msg::Event(1)).unwrap();
    assert_eq!(rx.recv_timeout(GUARD).unwrap(), Msg::Event(1));
    assert_eq!(typed.undecodable_pushes(), 1);
}

#[test]
fn peer_options_reach_the_underlying_peer() {
    let (x, _y) = endpoint::pair().unwrap();
    let p = BodyPeer::<Msg>::start(
        Connection::raw(x, FrameLimit::default()),
        BodyOptions::new("a").peer_options(|o| {
            let mut o = o;
            o.max_concurrent_calls = 3;
            o
        }),
    )
    .unwrap();
    assert!(format!("{:?}", BodyOptions::<Msg>::new("z")).contains("serves"));
    drop(p);
}
