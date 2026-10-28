#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The disk map through a real agent connection over the fake platform: a scan is a job whose status is pushed (no polling),
//! the tree is read one level at a time, Cancel and unknown ids answer with typed errors. Only temp folders are scanned.

use dyno_agent::cli::{ask, connect};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::disk::scan::{DiskScanStatus, DiskTarget, ScanState};
use dyno_core::protocol::{ErrorCode, Message, Reply, Request};
use dyno_ipc::{Client, Peer, PeerOptions};
use dyno_platform::fake::Fake;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

fn start() -> (Running, Dirs, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let fake = Fake::new();
    let running = Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
    (running, dirs, tmp)
}

/// A connection that forwards the pushed disk scan statuses.
fn listener(dirs: &Dirs) -> (Peer<Message>, Receiver<DiskScanStatus>) {
    let d = dyno_agent::discovery::Discovery::read(&dirs.discovery()).unwrap();
    let conn = Client::connect(&d.endpoint, &d.token().unwrap()).unwrap();
    let (tx, rx) = channel();
    let peer = Peer::start(
        conn,
        PeerOptions::new().on_push(move |m| {
            if let Message::DiskScan(s) = m {
                let _ = tx.send(*s);
            }
        }),
    )
    .unwrap();
    (peer, rx)
}

#[test]
fn a_folder_scan_is_pushed_and_read_one_level_at_a_time() {
    let (running, dirs, tmp) = start();
    let work = tmp.path().join("work");
    std::fs::create_dir_all(work.join("big")).unwrap();
    std::fs::create_dir_all(work.join("small")).unwrap();
    std::fs::write(work.join("big/a.bin"), vec![1u8; 200_000]).unwrap();
    std::fs::write(work.join("small/b.bin"), vec![1u8; 10]).unwrap();
    let work = std::fs::canonicalize(work).unwrap().to_string_lossy().into_owned();
    let (peer, statuses) = listener(&dirs);

    let Reply::DiskScan(started) = ask(
        &peer,
        Request::StartDiskScan {
            target: DiskTarget::Folder { path: work.clone() },
        },
    )
    .unwrap() else {
        panic!("a scan status")
    };
    let last = loop {
        let s = statuses.recv().unwrap();
        assert_eq!(s.scan, started.scan);
        if s.state.is_final() {
            break s;
        }
    };
    assert_eq!(last.state, ScanState::Done);
    assert!(last.progress.bytes >= 200_000);
    assert_eq!(last.progress.total_bytes, None, "a folder has no known total");

    let Reply::DiskChildren(top) = ask(
        &peer,
        Request::DiskChildren {
            scan: started.scan,
            node: None,
            offset: 0,
            limit: 10,
            min_fraction: 0.0,
        },
    )
    .unwrap() else {
        panic!("children")
    };
    assert_eq!(top.children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["big", "small"]);
    let Reply::DiskPath(path) = ask(
        &peer,
        Request::DiskPath {
            scan: started.scan,
            node: top.children[0].id,
        },
    )
    .unwrap() else {
        panic!("a path")
    };
    assert_eq!(std::path::PathBuf::from(path), std::path::Path::new(&work).join("big"));
    let Reply::DiskScans(all) = ask(&peer, Request::DiskScans).unwrap() else {
        panic!("scans")
    };
    assert_eq!(all.len(), 1);
    assert!(matches!(ask(&peer, Request::DiskVolumes).unwrap(), Reply::DiskVolumes(_)));
    assert!(matches!(
        ask(&peer, Request::CancelDiskScan { scan: started.scan }).unwrap(),
        Reply::Ok
    ));
    assert!(
        work.contains("work") && std::path::Path::new(&work).join("big/a.bin").exists(),
        "scanning moves nothing"
    );
    drop(peer);
    running.stop();
}

#[test]
fn unknown_scans_and_bad_folders_answer_with_typed_errors() {
    let (running, dirs, _tmp) = start();
    let peer = connect(&dirs).unwrap().0;
    let code = |r: Request| match peer.call(Message::Request(Box::new(r)), Duration::from_secs(15)) {
        Ok(Message::Reply(reply)) => (*reply).unwrap_err().code,
        other => panic!("expected an error reply, got {other:?}"),
    };
    assert_eq!(code(Request::CancelDiskScan { scan: 7 }), ErrorCode::NotFound);
    assert_eq!(
        code(Request::DiskChildren {
            scan: 7,
            node: None,
            offset: 0,
            limit: 1,
            min_fraction: 0.0
        }),
        ErrorCode::NotFound
    );
    assert_eq!(code(Request::DiskPath { scan: 7, node: 0 }), ErrorCode::NotFound);
    assert_eq!(
        code(Request::StartDiskScan {
            target: DiskTarget::Folder { path: "relative".into() }
        }),
        ErrorCode::BadRequest
    );
    assert_eq!(
        code(Request::StartDiskScan {
            target: DiskTarget::Volume {
                id: "no-such-volume".into()
            }
        }),
        ErrorCode::NotFound
    );
    drop(peer);
    running.stop();
}
