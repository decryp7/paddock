#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! A hostile or broken server never panics the client and never makes it allocate without bound: every case ends as a
//! typed `TargetError` (an infrastructure fault, outcome `error`).

mod common;

use common::*;
use sighter_core::outcome::Outcome;
use sighter_core::ports::{FrameSource, StopFlag, TargetError};
use sighter_target::VncTarget;
use sighter_target::proto::{ENC_RAW, ENC_ZRLE};

fn connected() -> (Fake, VncTarget) {
    let f = start(Mode::NoAuth);
    let t = connect(&f, None, StopFlag::default()).unwrap();
    (f, t)
}

/// Sends one hostile message and returns the error the target ends with.
fn after(msg: Vec<u8>) -> TargetError {
    let (f, mut t) = connected();
    f.to_client.send(msg).unwrap();
    let err = loop {
        match t.grab() {
            Err(e) => break e,
            Ok(_) => continue,
        }
    };
    assert_eq!(err.outcome(), Outcome::Error);
    err
}

fn handshake_error(bytes: Vec<u8>) -> TargetError {
    let f = start(Mode::Bytes(bytes));
    connect(&f, None, StopFlag::default()).err().unwrap()
}

#[test]
fn a_peer_that_does_not_speak_rfb_is_refused() {
    let err = handshake_error(b"HTTP/1.1 200 OK\r\n".to_vec());
    assert!(matches!(&err, TargetError::Lost(m) if m.contains("not a VNC server")), "{err:?}");
}

#[test]
fn a_truncated_handshake_is_a_lost_connection() {
    for bytes in [
        b"RFB 003".to_vec(),
        b"RFB 003.008\n".to_vec(),
        [b"RFB 003.008\n".to_vec(), vec![1]].concat(),
    ] {
        let err = handshake_error(bytes);
        assert!(matches!(err, TargetError::Lost(_)), "{err:?}");
    }
}

#[test]
fn a_refusal_reason_and_oversized_text_are_handled() {
    let reason = b"too many sessions";
    let msg = [
        b"RFB 003.008\n".to_vec(),
        vec![0],
        (reason.len() as u32).to_be_bytes().to_vec(),
        reason.to_vec(),
    ]
    .concat();
    let err = handshake_error(msg);
    assert!(matches!(&err, TargetError::Lost(m) if m.contains("too many sessions")), "{err:?}");
    let msg = [b"RFB 003.008\n".to_vec(), vec![0], u32::MAX.to_be_bytes().to_vec()].concat();
    assert!(
        matches!(handshake_error(msg), TargetError::Protocol(_)),
        "a 4 GiB reason is refused, not allocated"
    );
}

#[test]
fn an_oversized_desktop_is_refused_before_any_allocation() {
    // ServerInit claims 65535 x 65535 pixels.
    let pf = sighter_target::proto::PIXEL_FORMAT;
    let init = [vec![0xff, 0xff, 0xff, 0xff], pf.to_vec(), 0u32.to_be_bytes().to_vec()].concat();
    let msg = [b"RFB 003.008\n".to_vec(), vec![1, 1], 0u32.to_be_bytes().to_vec(), init].concat();
    let err = handshake_error(msg);
    assert!(matches!(&err, TargetError::Protocol(m) if m.contains("unusable")), "{err:?}");
}

#[test]
fn an_unknown_message_and_an_unrequested_encoding_break_the_protocol() {
    assert!(matches!(after(vec![99]), TargetError::Protocol(m) if m.contains("unknown server message")));
    assert!(matches!(after(rect(0, 0, 1, 1, 7)), TargetError::Protocol(m) if m.contains("encoding 7")));
}

#[test]
fn rectangles_outside_the_desktop_or_too_many_are_refused() {
    assert!(matches!(after(rect(60, 40, 10, 10, ENC_RAW)), TargetError::Protocol(m) if m.contains("outside")));
    let too_many = vec![0, 0, 0xff, 0xff];
    assert!(matches!(after(too_many), TargetError::Protocol(m) if m.contains("rectangles")));
}

#[test]
fn a_connection_cut_in_the_middle_of_a_rectangle_is_lost() {
    let mut half = rect(0, 0, W, H, ENC_RAW);
    half.extend([1, 2, 3, 0].repeat(10));
    // The fake closes after the short body only when asked to; send the bytes, then the close marker.
    let (f, mut t) = connected();
    f.to_client.send(half).unwrap();
    f.to_client.send(Vec::new()).unwrap();
    let err = loop {
        if let Err(e) = t.grab() {
            break e;
        }
    };
    assert!(matches!(err, TargetError::Lost(_)), "{err:?}");
}

#[test]
fn malformed_zlib_and_zlib_bombs_are_refused() {
    let zrle = |len: u32, body: &[u8]| {
        let mut m = rect(0, 0, 4, 4, ENC_ZRLE);
        m.extend(len.to_be_bytes());
        m.extend_from_slice(body);
        m
    };
    assert!(matches!(after(zrle(4, &[1, 2, 3, 4])), TargetError::Protocol(m) if m.contains("zlib")));
    assert!(matches!(after(zrle(u32::MAX, &[])), TargetError::Protocol(m) if m.contains("larger")));
    // A megabyte of zeros deflates to a few hundred bytes and would inflate far beyond a 4x4 rectangle.
    let mut z = zlib();
    let bomb = zrle_update(&mut z, 0, 0, 4, 4, &vec![0u8; 1 << 20]);
    assert!(matches!(after(bomb), TargetError::Protocol(m) if m.contains("expands")));
}
