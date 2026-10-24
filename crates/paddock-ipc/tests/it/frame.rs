//! The frame codec: Debrief's and Dyno's frame cases, the skip of an oversized frame, and "never panics" properties
//! (the fuzz target in `fuzz/` runs the same decoder without a time limit).

use paddock_ipc::RemoteError;
use paddock_ipc::frame::*;
use proptest::prelude::*;
use std::io::{self, Read};

fn limit() -> FrameLimit {
    FrameLimit::default()
}

fn all() -> Vec<Frame> {
    vec![
        Frame::Hello {
            version: VERSION,
            protocol: ProtocolVersion::new(1, 3),
            token: [7; 32],
            pid: 42,
        },
        Frame::Welcome {
            version: VERSION,
            protocol: ProtocolVersion::new(1, 4),
        },
        Frame::Call {
            id: 1,
            credit: 4,
            body: b"req".to_vec(),
        },
        Frame::Reply {
            id: 1,
            body: Vec::new(),
        },
        Frame::Item {
            id: u64::MAX,
            body: vec![0; 300],
        },
        Frame::Fail {
            id: 2,
            error: RemoteError::new("bad", "no — café").with_data(serde_json::json!({"limit": 5})),
        },
        Frame::Cancel { id: 3 },
        Frame::Credit { id: 3, n: 8 },
        Frame::Push {
            body: b"event".to_vec(),
        },
    ]
}

#[test]
fn every_frame_roundtrips_through_bytes_and_every_prefix_asks_for_more() {
    for f in all() {
        let bytes = encode(&f, limit()).unwrap();
        let (back, used) = decode(&bytes, limit()).unwrap().unwrap();
        assert_eq!((back, used), (f.clone(), bytes.len()));
        for cut in 0..bytes.len() {
            assert_eq!(
                decode(&bytes[..cut], limit()).unwrap(),
                None,
                "{f:?} cut at {cut}"
            );
        }
        // And through a reader.
        let mut r = &bytes[..];
        assert_eq!(read_frame(&mut r, limit()).unwrap(), f);
        assert_eq!(read_frame(&mut r, limit()), Err(FrameError::Closed));
    }
    let ids: Vec<Option<u64>> = all().iter().map(Frame::call_id).collect();
    assert_eq!(
        ids,
        [
            None,
            None,
            Some(1),
            Some(1),
            Some(u64::MAX),
            Some(2),
            Some(3),
            Some(3),
            None
        ]
    );
}

#[test]
fn limits_are_clamped() {
    assert_eq!(FrameLimit::new(0).get(), MIN_FRAME);
    assert_eq!(FrameLimit::new(usize::MAX).get(), HARD_MAX_FRAME);
    assert_eq!(FrameLimit::default().get(), DEFAULT_MAX_FRAME);
    assert_eq!(FrameLimit::new(5000).get(), 5000);
}

#[test]
fn protocol_versions_say_which_side_to_update() {
    let host = ProtocolVersion::new(2, 3);
    assert!(host.accepts(ProtocolVersion::new(2, 0)) && host.accepts(host));
    assert!(!host.accepts(ProtocolVersion::new(2, 4)) && !host.accepts(ProtocolVersion::new(1, 9)));
    assert_eq!(
        host.mismatch(ProtocolVersion::new(2, 1), "p", "Debrief"),
        None
    );
    let old = host
        .mismatch(ProtocolVersion::new(1, 7), "plugin \"x\"", "Debrief")
        .unwrap();
    assert!(
        old.contains("too old")
            && old.contains("1.7")
            && old.contains("2.x")
            && old.contains("Debrief"),
        "{old}"
    );
    let new = host
        .mismatch(ProtocolVersion::new(2, 4), "plugin \"x\"", "Dyno")
        .unwrap();
    assert!(
        new.contains("needs a newer Dyno") && new.contains("2.4"),
        "{new}"
    );
    assert!(
        host.mismatch(ProtocolVersion::new(3, 0), "p", "Dyno")
            .unwrap()
            .contains("newer Dyno")
    );
    assert_eq!(
        "1.12".parse::<ProtocolVersion>(),
        Ok(ProtocolVersion::new(1, 12))
    );
    for bad in ["1", "a.b", "1.2.3", "", "70000.1", "1.x"] {
        assert!(bad.parse::<ProtocolVersion>().is_err(), "{bad}");
    }
    assert_eq!(ProtocolVersion::UNVERSIONED.to_string(), "0.0");
}

#[test]
fn skew_names_the_side_to_update_and_since_finds_the_release() {
    use paddock_ipc::frame::Skew;
    let host = ProtocolVersion::new(2, 3);
    assert_eq!(host.skew(ProtocolVersion::new(2, 3)), None);
    assert_eq!(
        host.skew(ProtocolVersion::new(1, 9)),
        Some(Skew::ChildBehind)
    );
    assert_eq!(
        host.skew(ProtocolVersion::new(2, 4)),
        Some(Skew::HostBehind)
    );
    assert_eq!(
        host.skew(ProtocolVersion::new(3, 0)),
        Some(Skew::HostBehind)
    );
    let since = [
        (ProtocolVersion::new(2, 5), "1.6"),
        (ProtocolVersion::new(2, 4), "1.4"),
        (ProtocolVersion::new(2, 1), "1.1"),
    ];
    let found = |v| ProtocolVersion::introduced_in(since, v);
    assert_eq!(found(ProtocolVersion::new(2, 4)), Some("1.4"));
    assert_eq!(found(ProtocolVersion::new(2, 2)), Some("1.4"));
    assert_eq!(found(ProtocolVersion::new(2, 6)), None);
}

#[test]
fn handshake_versions_are_read_from_any_hello() {
    let hello = all().remove(0);
    let body = encode(&hello, limit()).unwrap()[LEN_BYTES..].to_vec();
    assert_eq!(
        handshake_versions(&body),
        Some((VERSION, Some(ProtocolVersion::new(1, 3))))
    );
    // A version-1 hello (no protocol) still yields its frame version.
    let v1 = postcard::to_stdvec(&(0u32, 1u16, [1u8; 32], 7u32)).unwrap();
    assert_eq!(handshake_versions(&v1), Some((1, None)));
    assert_eq!(handshake_versions(&[]), None);
    let cancel = encode(&Frame::Cancel { id: 1 }, limit()).unwrap();
    assert_eq!(handshake_versions(&cancel[LEN_BYTES..]), None);
}

#[test]
fn heads_name_the_call_of_a_frame() {
    let head = |f: &Frame| peek_head(&encode(f, limit()).unwrap()[LEN_BYTES..]);
    let expect = [
        (FrameKind::Other, None),
        (FrameKind::Other, None),
        (FrameKind::Call, Some(1)),
        (FrameKind::Reply, Some(1)),
        (FrameKind::Item, Some(u64::MAX)),
        (FrameKind::Fail, Some(2)),
        (FrameKind::Other, None),
        (FrameKind::Other, None),
        (FrameKind::Push, None),
    ];
    for (f, (kind, id)) in all().iter().zip(expect) {
        assert_eq!(head(f), Some(FrameHead { kind, id }), "{f:?}");
    }
    assert_eq!(peek_head(&[]), None);
    assert_eq!(peek_head(&[99]), None, "an unknown variant");
    assert_eq!(peek_head(&[2]), None, "a call without its id");
}

#[test]
fn an_oversized_frame_is_skipped_and_the_stream_stays_in_step() {
    let small = FrameLimit::new(MIN_FRAME);
    let big = Frame::Reply {
        id: 9,
        body: vec![1; 5000],
    };
    let next = Frame::Cancel { id: 10 };
    let mut bytes = encode(&big, FrameLimit::new(HARD_MAX_FRAME)).unwrap();
    bytes.extend(encode(&next, small).unwrap());
    let mut r = &bytes[..];
    match read_frame(&mut r, small) {
        Err(FrameError::Skipped { len, max, head }) => {
            assert!(len > 5000 && max == MIN_FRAME, "{len} {max}");
            assert_eq!(
                head,
                Some(FrameHead {
                    kind: FrameKind::Reply,
                    id: Some(9)
                })
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        read_frame(&mut r, small).unwrap(),
        next,
        "the next frame is read normally"
    );
    // Encoding refuses before anything is written.
    assert!(matches!(
        encode(&big, small),
        Err(FrameError::TooLarge { .. })
    ));
    // A stream that ends inside a skipped frame is an I/O error.
    let cut = encode(&big, FrameLimit::new(HARD_MAX_FRAME)).unwrap();
    assert!(matches!(
        read_frame(&mut &cut[..3000], small),
        Err(FrameError::Io(_))
    ));
    assert!(matches!(
        read_frame(&mut &cut[..6], small),
        Err(FrameError::Io(_))
    ));
}

#[test]
fn a_length_beyond_the_hard_limit_is_refused_from_the_prefix_alone() {
    let prefix = u32::MAX.to_le_bytes();
    assert_eq!(
        decode(&prefix, limit()),
        Err(FrameError::TooLarge {
            len: u32::MAX as usize,
            max: DEFAULT_MAX_FRAME
        })
    );
    let mut r = &prefix[..];
    assert!(matches!(
        read_frame(&mut r, limit()),
        Err(FrameError::TooLarge { .. })
    ));
    // The skip never reads past the hard limit.
    let mut r = &((HARD_MAX_FRAME + 1) as u32).to_le_bytes()[..];
    assert!(matches!(
        read_frame(&mut r, FrameLimit::new(MIN_FRAME)),
        Err(FrameError::TooLarge { .. })
    ));
}

#[test]
fn garbage_truncation_and_trailing_bytes_are_errors() {
    let bytes = [3, 0, 0, 0, 0xff, 0xff, 0xff];
    assert!(matches!(
        decode(&bytes, limit()),
        Err(FrameError::Invalid(_))
    ));
    let ok = encode(&Frame::Cancel { id: 1 }, limit()).unwrap();
    let mut long = ok.clone();
    long[0] += 1;
    long.push(0);
    assert!(
        matches!(decode(&long, limit()), Err(FrameError::Invalid(m)) if m.contains("trailing"))
    );
    assert_eq!(
        decode_body(&ok[LEN_BYTES..]).unwrap(),
        Frame::Cancel { id: 1 }
    );
    // A stream that ends inside the prefix or the body.
    assert!(matches!(
        read_frame(&mut &[9u8, 0][..], limit()),
        Err(FrameError::Io(_))
    ));
    assert!(matches!(
        read_frame(&mut &[9u8, 0, 0, 0, 1][..], limit()),
        Err(FrameError::Io(_))
    ));
    // An unknown variant.
    assert!(matches!(
        read_frame(&mut &[1u8, 0, 0, 0, 99][..], limit()),
        Err(FrameError::Invalid(_))
    ));
    for e in [
        FrameError::Closed,
        FrameError::Io("x".into()),
        FrameError::TooLarge { len: 2, max: 1 },
        FrameError::Skipped {
            len: 2,
            max: 1,
            head: None,
        },
        FrameError::Invalid("y".into()),
        FrameError::TimedOut,
    ] {
        assert!(!e.to_string().is_empty());
    }
}

#[test]
fn read_errors_keep_their_meaning() {
    struct Failing(io::ErrorKind);
    impl Read for Failing {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::from(self.0))
        }
    }
    assert_eq!(
        read_frame(&mut Failing(io::ErrorKind::ConnectionReset), limit()),
        Err(FrameError::Closed)
    );
    assert_eq!(
        read_frame(&mut Failing(io::ErrorKind::TimedOut), limit()),
        Err(FrameError::TimedOut)
    );
    assert!(matches!(
        read_frame(&mut Failing(io::ErrorKind::PermissionDenied), limit()),
        Err(FrameError::Io(_))
    ));
    // An interrupted read is retried.
    struct Interrupted(bool, Vec<u8>);
    impl Read for Interrupted {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if !self.0 {
                self.0 = true;
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            (&self.1[..]).read(buf).inspect(|n| {
                self.1.drain(..*n);
            })
        }
    }
    let bytes = encode(&Frame::Cancel { id: 4 }, limit()).unwrap();
    assert_eq!(
        read_frame(&mut Interrupted(false, bytes), limit()).unwrap(),
        Frame::Cancel { id: 4 }
    );
}

#[test]
fn writing_reports_a_broken_stream() {
    struct Broken;
    impl io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(io::ErrorKind::BrokenPipe))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert_eq!(
        write_frame(&mut Broken, &Frame::Cancel { id: 1 }, limit()),
        Err(FrameError::Closed)
    );
    let mut out = Vec::new();
    write_frame(&mut out, &Frame::Cancel { id: 1 }, limit()).unwrap();
    assert_eq!(
        decode(&out, limit()).unwrap().unwrap().0,
        Frame::Cancel { id: 1 }
    );
}

proptest! {
    #[test]
    fn decoding_random_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _ = decode(&bytes, FrameLimit::new(MIN_FRAME));
        let _ = decode_body(&bytes);
        let _ = peek_head(&bytes);
        let _ = handshake_versions(&bytes);
        let mut r = &bytes[..];
        while read_frame(&mut r, FrameLimit::new(MIN_FRAME)).is_ok() {}
    }

    #[test]
    fn any_call_roundtrips(id in any::<u64>(), credit in any::<u32>(), body in proptest::collection::vec(any::<u8>(), 0..256)) {
        let f = Frame::Call { id, credit, body };
        let bytes = encode(&f, limit()).unwrap();
        prop_assert_eq!(decode(&bytes, limit()).unwrap().unwrap().0, f);
    }
}
