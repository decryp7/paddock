#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The disk map through a real agent connection over the fake platform: a scan is a job whose status is pushed (no polling),
//! the tree is read one level at a time, Cancel and unknown ids answer with typed errors. Only temp folders are scanned.

use dyno_agent::cli::{ask, connect};
use dyno_agent::disk::centre::AccessProbe;
use dyno_agent::disk::volumes::{ListedVolume, VolumeLister};
use dyno_agent::disk::DiskPorts;
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::disk::scan::{DiskScanStatus, DiskTarget, ScanState};
use dyno_core::disk::volume::{Volume, VolumeLocation};
use dyno_core::ports::PlatformError;
use dyno_core::protocol::{ErrorCode, Message, Reply, Request};
use dyno_ipc::{Client, Peer, PeerOptions};
use dyno_platform::fake::Fake;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One invented volume: the temp folder of the test. No test lists the machine's mounts or runs `diskutil` (ER-14).
struct OneVolume(PathBuf);

impl VolumeLister for OneVolume {
    fn list(&self) -> Vec<ListedVolume> {
        vec![ListedVolume {
            volume: Volume {
                id: "Test disk".into(),
                name: "Test disk".into(),
                mount: self.0.to_string_lossy().into_owned(),
                fs: "apfs".into(),
                location: VolumeLocation::Internal,
                capacity: Some(1000),
                free: Some(400),
                rotational: Some(false),
                read_only: false,
                reachable: true,
                whole_volume: true,
            },
            root: self.0.clone(),
        }]
    }
}

/// A volume list that holds its first answer until the test lets it go (the system being slow), and says when it was asked.
struct SlowVolumes {
    inner: OneVolume,
    open: Arc<(Mutex<bool>, std::sync::Condvar)>,
    asked: Mutex<std::sync::mpsc::Sender<()>>,
}

impl VolumeLister for SlowVolumes {
    fn list(&self) -> Vec<ListedVolume> {
        let _ = self.asked.lock().unwrap().send(());
        let (flag, cv) = &*self.open;
        let mut released = flag.lock().unwrap();
        while !*released {
            released = cv.wait(released).unwrap();
        }
        drop(released);
        self.inner.list()
    }
}

struct NoAccess(bool);

impl AccessProbe for NoAccess {
    fn full_disk_access_missing(&self) -> bool {
        self.0
    }

    fn open_settings(&self) -> Result<(), PlatformError> {
        if self.0 {
            Ok(())
        } else {
            Err(PlatformError::Unsupported("no such pane".into()))
        }
    }
}

/// What the agent was asked to show in the file manager.
type Shown = Arc<Mutex<Vec<PathBuf>>>;

fn start() -> (Running, Dirs, tempfile::TempDir, Shown) {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let fake = Fake::new();
    let shown: Shown = Arc::default();
    let record = shown.clone();
    let reveal = Box::new(move |p: &std::path::Path| {
        if p.ends_with("vanished") {
            return Err(PlatformError::NotFound("gone".into()));
        }
        record.lock().unwrap().push(p.to_path_buf());
        Ok(())
    });
    let ports = DiskPorts {
        volumes: Arc::new(OneVolume(tmp.path().to_path_buf())),
        access: Arc::new(NoAccess(true)),
    };
    let running = Running::start_with(dirs.clone(), fake.platform(), Box::new(fake.clone()), reveal, ports).unwrap();
    (running, dirs, tmp, shown)
}

/// The final status of the scan's current pass. The agent never drops a final state, and the wait is on the scan itself, so
/// the test cannot hang on a lost event.
fn final_status(running: &Running, statuses: &Receiver<DiskScanStatus>, scan: u64) -> DiskScanStatus {
    running.shared.disk.wait_finished(scan).unwrap();
    loop {
        let s = statuses.recv().unwrap();
        if s.scan == scan && s.state.is_final() {
            return s;
        }
    }
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
    let (running, dirs, tmp, _) = start();
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
    let last = final_status(&running, &statuses, started.scan);
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
    let Reply::DiskVolumes(volumes) = ask(&peer, Request::DiskVolumes).unwrap() else {
        panic!("volumes")
    };
    assert_eq!(
        volumes.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(),
        ["Test disk"],
        "the invented list, not the machine's"
    );
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
    let (running, dirs, _tmp, _) = start();
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

#[test]
fn a_node_is_revealed_by_id_and_only_a_node_of_a_kept_scan_can_be() {
    let (running, dirs, tmp, shown) = start();
    let work = tmp.path().join("work");
    std::fs::create_dir_all(work.join("big")).unwrap();
    std::fs::write(work.join("big/a.bin"), vec![1u8; 20_000]).unwrap();
    let work = std::fs::canonicalize(work).unwrap();
    let (peer, statuses) = listener(&dirs);
    let Reply::DiskScan(started) = ask(
        &peer,
        Request::StartDiskScan {
            target: DiskTarget::Folder {
                path: work.to_string_lossy().into_owned(),
            },
        },
    )
    .unwrap() else {
        panic!("a scan status")
    };
    final_status(&running, &statuses, started.scan);
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
    let big = top.children[0].id;
    assert!(matches!(
        ask(
            &peer,
            Request::DiskReveal {
                scan: started.scan,
                node: big
            }
        )
        .unwrap(),
        Reply::Ok
    ));
    assert_eq!(*shown.lock().unwrap(), vec![work.join("big")], "the path is the scan's own");
    let err = |r: Request| match peer.call(Message::Request(Box::new(r)), Duration::from_secs(15)) {
        Ok(Message::Reply(reply)) => (*reply).unwrap_err().code,
        other => panic!("expected an error reply, got {other:?}"),
    };
    assert!(matches!(ask(&peer, Request::OpenFullDiskAccessSettings).unwrap(), Reply::Ok));
    assert_eq!(err(Request::DiskReveal { scan: 99, node: 0 }), ErrorCode::NotFound);
    assert_eq!(
        err(Request::DiskReveal {
            scan: started.scan,
            node: 9999
        }),
        ErrorCode::NotFound
    );
    assert_eq!(err(Request::RetryDiskScan { scan: 99 }), ErrorCode::NotFound);
    assert_eq!(
        err(Request::RetryDiskScan { scan: started.scan }),
        ErrorCode::BadRequest,
        "every folder was read"
    );
    assert_eq!(shown.lock().unwrap().len(), 1, "a refused request shows nothing");
    drop(peer);
    running.stop();
}

#[cfg(unix)]
#[allow(unsafe_code)]
#[test]
fn a_folder_that_was_refused_is_read_again_by_a_retry_and_full_disk_access_is_not_claimed_for_a_file_mode() {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: `geteuid` has no arguments and cannot fail.
    if unsafe { libc::geteuid() } == 0 {
        return; // root can read every folder: nothing is refused
    }
    let (running, dirs, tmp, _) = start();
    let work = tmp.path().join("work");
    let locked = work.join("Library");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::write(locked.join("mail.db"), vec![1u8; 30_000]).unwrap();
    std::fs::write(work.join("a.bin"), vec![1u8; 10]).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let work = std::fs::canonicalize(work).unwrap();
    let (peer, statuses) = listener(&dirs);
    let target = DiskTarget::Folder {
        path: work.to_string_lossy().into_owned(),
    };
    let Reply::DiskScan(started) = ask(&peer, Request::StartDiskScan { target }).unwrap() else {
        panic!("a scan status")
    };
    let first = final_status(&running, &statuses, started.scan);
    assert_eq!(first.progress.unreadable, 1);
    // A folder refused by its mode is EACCES, an ordinary rights problem, even while the permission probe says Full Disk Access
    // is missing: the button that opens its settings would not help.
    assert!(!first.needs_full_disk_access, "a file mode is not a privacy permission");

    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    let Reply::DiskScan(again) = ask(&peer, Request::RetryDiskScan { scan: started.scan }).unwrap() else {
        panic!("a scan status")
    };
    assert_eq!(again.scan, started.scan);
    let second = final_status(&running, &statuses, started.scan);
    assert_eq!(second.state, ScanState::Done);
    assert_eq!(second.progress.unreadable, 0);
    assert!(second.progress.bytes > first.progress.bytes, "what was inside is now counted");
    assert!(!second.needs_full_disk_access);
    drop(peer);
    running.stop();
}

#[test]
fn the_volume_list_is_asked_for_in_the_background_and_never_holds_other_requests() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let fake = Fake::new();
    let open = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let (asked, was_asked) = channel();
    let ports = DiskPorts {
        volumes: Arc::new(SlowVolumes {
            inner: OneVolume(tmp.path().to_path_buf()),
            open: open.clone(),
            asked: Mutex::new(asked),
        }),
        access: Arc::new(NoAccess(false)),
    };
    // The agent starts although the system has not answered yet.
    let running = Running::start_with(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(())), ports).unwrap();
    was_asked.recv().unwrap();
    let peer = connect(&dirs).unwrap().0;
    assert!(
        matches!(ask(&peer, Request::DiskScans).unwrap(), Reply::DiskScans(s) if s.is_empty()),
        "other requests answer meanwhile"
    );
    // The system answers: the first DiskVolumes then has its list.
    *open.0.lock().unwrap() = true;
    open.1.notify_all();
    let Reply::DiskVolumes(v) = ask(&peer, Request::DiskVolumes).unwrap() else {
        panic!("volumes")
    };
    assert_eq!(v.len(), 1);
    drop(peer);
    running.stop();
}
