#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! End to end against the fake VNC server: authentication, every encoding, input, stop and lost connections.

mod common;

use common::*;
use sighter_core::geometry::{Logical, Pt};
use sighter_core::outcome::Outcome;
use sighter_core::ports::{Button, FrameSource, InputDriver, Key, StopFlag, Target, TargetError};
use sighter_target::proto::{ENC_COPYRECT, ENC_DESKTOP_SIZE, ENC_HEXTILE};
use sighter_target::{ConnectOptions, VncAddr, VncTarget};
use std::time::Duration;

fn pt(x: f64, y: f64) -> Pt<Logical> {
    Pt::new(x, y).unwrap()
}

/// Connects and consumes the three set-up messages (pixel format, encodings, first request).
fn session(mode: Mode, pw: Option<&str>, stop: &StopFlag) -> (Fake, VncTarget) {
    let f = start(mode);
    let t = connect(&f, pw, stop.clone()).unwrap();
    assert_eq!((next_event(&f)[0], next_event(&f)[0], next_event(&f)[0]), (0, 2, 3));
    (f, t)
}

#[test]
fn vnc_password_authentication_and_the_session_name() {
    let (f, t) = session(Mode::Password("vm-pass"), Some("vm-pass"), &StopFlag::default());
    assert_eq!(t.desktop_name(), "fake-vm");
    assert_eq!(t.name(), format!("vnc://127.0.0.1:{}", f.port));
}

#[test]
fn security_none_connects_without_a_password() {
    let (_f, mut t) = session(Mode::NoAuth, None, &StopFlag::default());
    _f.to_client.send(vec![2]).unwrap(); // a bell is ignored
    _f.to_client.send(raw_screen([1, 2, 3])).unwrap();
    assert_eq!(t.grab().unwrap().get(0, 0), Some(0x030201));
}

#[test]
fn a_wrong_password_is_refused_without_echoing_it() {
    let f = start(Mode::Password("right"));
    let err = connect(&f, Some("wrong-secret"), StopFlag::default()).err().unwrap();
    let text = err.to_string();
    assert!(matches!(&err, TargetError::Lost(m) if m.contains("authentication failed") && m.contains("wrong password")));
    assert!(!text.contains("wrong-secret") && !format!("{err:?}").contains("wrong-secret"));
    assert_eq!(err.outcome(), Outcome::Error);
}

#[test]
fn a_missing_or_unsupported_security_type_is_a_clear_error() {
    let f = start(Mode::OnlyTypes(vec![2]));
    let err = connect(&f, None, StopFlag::default()).err().unwrap();
    assert!(matches!(&err, TargetError::Lost(m) if m.contains("needs a password")), "{err:?}");
    let f = start(Mode::OnlyTypes(vec![18]));
    let err = connect(&f, Some("x"), StopFlag::default()).err().unwrap();
    assert!(matches!(&err, TargetError::Lost(m) if m.contains("18")), "{err:?}");
}

#[test]
fn every_encoding_reaches_the_frame() {
    let (f, mut t) = session(Mode::NoAuth, None, &StopFlag::default());
    // Raw.
    f.to_client.send(raw_screen([0x10, 0x20, 0x30])).unwrap();
    let (seq, frame) = t.latest().unwrap();
    assert_eq!((frame.width(), frame.height(), frame.get(0, 0)), (64, 48, Some(0x302010)));
    assert_eq!(next_event(&f)[0], 3, "an incremental request follows each update");

    // ZRLE: one solid 64x48 tile, then a second message through the same zlib stream.
    let mut z = zlib();
    f.to_client.send(zrle_update(&mut z, 0, 0, W, H, &[1, 9, 8, 7])).unwrap();
    let seq = t.wait_newer(seq, WAIT).unwrap();
    assert_eq!(t.grab().unwrap().get(63, 47), Some(0x070809));
    f.to_client.send(zrle_update(&mut z, 0, 0, 2, 2, &[1, 4, 5, 6])).unwrap();
    let seq = t.wait_newer(seq, WAIT).unwrap();
    assert_eq!(
        (t.grab().unwrap().get(1, 1), t.grab().unwrap().get(2, 2)),
        (Some(0x060504), Some(0x070809))
    );

    // Hextile: a 16x16 background tile.
    let mut hex = rect(0, 0, 16, 16, ENC_HEXTILE);
    hex.extend([2, 0x01, 0x02, 0x03, 0]);
    f.to_client.send(hex).unwrap();
    let seq = t.wait_newer(seq, WAIT).unwrap();
    let frame = t.grab().unwrap();
    assert_eq!((frame.get(0, 0), frame.get(20, 20)), (Some(0x030201), Some(0x070809)));

    // CopyRect: copy the 16x16 corner to (32, 16).
    let mut copy = rect(32, 16, 16, 16, ENC_COPYRECT);
    copy.extend([0, 0, 0, 0]);
    f.to_client.send(copy).unwrap();
    let seq = t.wait_newer(seq, WAIT).unwrap();
    assert_eq!(t.grab().unwrap().get(40, 20), Some(0x030201));

    // DesktopSize: the frame is reallocated.
    f.to_client.send(rect(0, 0, 20, 10, ENC_DESKTOP_SIZE)).unwrap();
    t.wait_newer(seq, WAIT).unwrap();
    let frame = t.grab().unwrap();
    assert_eq!((frame.width(), frame.height()), (20, 10));
}

#[test]
fn ctrl_alt_del_keys_and_pointer_reach_the_server() {
    let (f, mut t) = session(Mode::NoAuth, None, &StopFlag::default());
    t.chord(&[Key::Ctrl, Key::Alt, Key::Delete]).unwrap();
    let keys: Vec<_> = (0..6).map(|_| next_key(&f)).collect();
    assert_eq!(
        keys,
        vec![(0xffe3, 1), (0xffe9, 1), (0xffff, 1), (0xffff, 0), (0xffe9, 0), (0xffe3, 0)]
    );
    t.click(pt(5.0, 6.0), Button::Left).unwrap();
    let pointer: Vec<_> = std::iter::from_fn(|| Some(next_event(&f))).filter(|e| e[0] == 5).take(3).collect();
    assert_eq!(
        pointer,
        vec![vec![5, 0, 0, 5, 0, 6], vec![5, 1, 0, 5, 0, 6], vec![5, 0, 0, 5, 0, 6]]
    );
    t.move_to(pt(7.4, 8.6)).unwrap();
    t.type_text("hi").unwrap();
    assert_eq!(next_event(&f), vec![5, 0, 0, 7, 0, 9]);
    assert_eq!((next_key(&f), next_key(&f)), ((0x68, 1), (0x68, 0)));
}

#[test]
fn stop_blocks_input_and_nothing_more_is_sent() {
    let stop = StopFlag::default();
    let (f, mut t) = session(Mode::NoAuth, None, &stop);
    stop.stop();
    assert_eq!(t.type_text("x"), Err(TargetError::Stopped));
    assert_eq!(t.click(pt(1.0, 1.0), Button::Right), Err(TargetError::Stopped));
    assert_eq!(TargetError::Stopped.outcome(), Outcome::Stopped);
    assert!(t.release_input().is_ok());
    drop(t);
    // Dropping closes the socket; the fake sees the end without any key or pointer event having been sent.
    assert!(f.events.recv_timeout(WAIT).is_err());
}

#[test]
fn a_stop_during_a_long_wait_ends_it_at_once() {
    let stop = StopFlag::default();
    let f = start(Mode::NoFrames);
    let opts = ConnectOptions {
        frame_timeout: Duration::from_secs(600),
        ..fast()
    };
    let mut t = VncTarget::connect(&addr(&f), None, stop.clone(), opts, &dial()).unwrap();
    // The three set-up messages and the first request have arrived: the target is now waiting for a frame that never comes.
    let setup: Vec<u8> = (0..3).map(|_| next_event(&f)[0]).collect();
    assert_eq!(setup, vec![0, 2, 3]);
    let stopper = {
        let stop = stop.clone();
        std::thread::spawn(move || stop.stop())
    };
    assert_eq!(t.grab().unwrap_err(), TargetError::Stopped);
    stopper.join().unwrap();
    // Waiting for a newer frame stops the same way.
    assert_eq!(t.wait_newer(0, Duration::from_secs(600)).unwrap_err(), TargetError::Stopped);
}

#[test]
fn a_dropped_connection_is_an_error_not_a_failure() {
    let (f, mut t) = session(Mode::NoAuth, None, &StopFlag::default());
    f.to_client.send(raw_screen([1, 2, 3])).unwrap();
    let (seq, _) = t.latest().unwrap();
    f.to_client.send(Vec::new()).unwrap(); // the server closes the socket
    let err = t.wait_newer(seq, WAIT).unwrap_err();
    assert!(matches!(err, TargetError::Lost(_)), "{err:?}");
    assert_eq!(err.outcome(), Outcome::Error);
    assert_eq!(t.grab().unwrap_err().outcome(), Outcome::Error);
}

#[test]
fn a_silent_server_times_out_in_the_handshake() {
    let f = start(Mode::Silent);
    let opts = ConnectOptions {
        handshake_timeout: Duration::from_millis(150),
        ..fast()
    };
    let err = VncTarget::connect(&addr(&f), None, StopFlag::default(), opts, &dial())
        .err()
        .unwrap();
    assert_eq!(err, TargetError::Timeout(Duration::from_millis(150)));
}

#[test]
fn no_first_frame_times_out_the_grab() {
    let f = start(Mode::NoFrames);
    let opts = ConnectOptions {
        frame_timeout: Duration::from_millis(100),
        ..fast()
    };
    let mut t = VncTarget::connect(&addr(&f), None, StopFlag::default(), opts, &dial()).unwrap();
    assert_eq!(t.grab().unwrap_err(), TargetError::Timeout(Duration::from_millis(100)));
    // wait_newer just reports the unchanged sequence on timeout.
    assert_eq!(t.wait_newer(0, Duration::from_millis(50)).unwrap(), 0);
}

#[test]
fn unreachable_targets_are_lost_with_a_hint() {
    // Port 0 is never connectable, so no port is probed or freed.
    let err = VncTarget::connect(
        &VncAddr {
            host: "127.0.0.1".into(),
            port: 0,
        },
        None,
        StopFlag::default(),
        fast(),
        &dial(),
    )
    .err()
    .unwrap();
    assert!(matches!(&err, TargetError::Lost(m) if m.contains("is the VM running")), "{err:?}");
    let err = VncTarget::connect(
        &VncAddr {
            host: "unknown.test".into(),
            port: 5900,
        },
        None,
        StopFlag::default(),
        fast(),
        &dial(),
    )
    .err()
    .unwrap();
    assert!(matches!(err, TargetError::Lost(_)));
}

#[test]
fn names_resolve_through_the_injected_lookup_and_the_checked_address_is_dialled() {
    let f = start(Mode::NoAuth);
    let a = VncAddr {
        host: "vm.test".into(),
        port: f.port,
    };
    let t = VncTarget::connect(&a, None, StopFlag::default(), fast(), &dial()).unwrap();
    assert_eq!(t.name(), format!("vnc://vm.test:{}", f.port));
}

#[test]
fn a_public_name_is_refused_by_default() {
    let f = start(Mode::NoAuth);
    let a = VncAddr {
        host: "public.test".into(),
        port: f.port,
    };
    let err = VncTarget::connect(&a, None, StopFlag::default(), fast(), &dial()).err().unwrap();
    assert!(
        matches!(&err, TargetError::Lost(m) if m.contains("public address") || m.contains("reach")),
        "{err:?}"
    );
    assert_eq!(err.outcome(), Outcome::Error);
}

#[test]
fn the_reader_thread_has_ended_after_drop() {
    let (_f, t) = session(Mode::NoAuth, None, &StopFlag::default());
    let probe = t.reader_probe();
    assert!(probe.upgrade().is_some(), "the reader runs while the target lives");
    drop(t);
    assert!(probe.upgrade().is_none(), "drop joins the reader thread");
}

#[test]
fn an_expired_handshake_budget_ends_the_connect() {
    let f = start(Mode::Silent);
    let opts = ConnectOptions {
        handshake_total: Duration::ZERO,
        ..fast()
    };
    let err = VncTarget::connect(&addr(&f), None, StopFlag::default(), opts, &dial())
        .err()
        .unwrap();
    assert!(matches!(err, TargetError::Timeout(_)), "{err:?}");
}
