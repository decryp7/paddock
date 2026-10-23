#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Right-click search, hash search, startup filter, watched folders and network change events end to end over the fake
//! platform: a real IPC connection and a real SQLite file in a temp folder. Files are created only in temp folders.

use dyno_agent::cli::{ask, connect};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::activity::UndoState;
use dyno_core::history::HistoryRange;
use dyno_core::netchange::{Link, LinkKind, NetState};
use dyno_core::process::ProcessRef;
use dyno_core::protocol::{Message, Reply, Request, TimelineEventKind};
use dyno_core::startup::{ImpactRating, StartupEntry, StartupFilter, StartupImpact, StartupLocation, StartupState};
use dyno_core::time::Timestamp;
use dyno_core::weblinks::HashSource;
use dyno_ipc::Peer;
use dyno_platform::fake::{process, Fake};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

struct Harness {
    running: Option<Running>,
    fake: Fake,
    dirs: Dirs,
    tmp: tempfile::TempDir,
}

impl Harness {
    fn start(setup: impl FnOnce(&Fake)) -> Harness {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let fake = Fake::new();
        fake.add_process(process(100, "calm", 1.0));
        setup(&fake);
        let running = Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap();
        Harness {
            running: Some(running),
            fake,
            dirs,
            tmp,
        }
    }

    fn client(&self) -> Peer<Message> {
        connect(&self.dirs).unwrap().0
    }

    fn shared(&self) -> Arc<dyno_agent::agent::Shared> {
        self.running.as_ref().unwrap().shared.clone()
    }

    /// A folder made for the test, canonical (macOS temp folders are links).
    fn folder(&self, name: &str) -> PathBuf {
        let p = std::fs::canonicalize(self.tmp.path()).unwrap().join(name);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop();
        }
    }
}

fn old_file(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, name.as_bytes()).unwrap();
    let f = std::fs::File::open(&p).unwrap();
    f.set_modified(std::time::SystemTime::now() - Duration::from_secs(5 * 86_400))
        .unwrap();
    p
}

#[test]
fn a_search_seed_is_redacted_and_the_url_is_built_from_the_confirmed_text_only() {
    let h = Harness::start(|_| {});
    let peer = h.client();
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
    let text = format!("Slack crashed for {user} at 10.1.2.3 {{123e4567-e89b-12d3-a456-426614174000}}");
    let Reply::SearchSeed(seed) = ask(&peer, Request::SearchSeed { text }).unwrap() else {
        panic!("seed")
    };
    assert!(seed.text.starts_with("Slack crashed"));
    assert!(!seed.text.contains("10.1.2.3") && !seed.text.contains("123e4567"));
    assert!(seed.removed.iter().any(|r| r.contains("IP address")) && seed.removed.iter().any(|r| r.contains("GUID")));
    let Reply::Url(u) = ask(
        &peer,
        Request::WebSearch {
            query: "kernel_task high cpu".into(),
        },
    )
    .unwrap() else {
        panic!("url")
    };
    assert_eq!(u, "https://duckduckgo.com/?q=kernel_task+high+cpu");
    assert!(ask(&peer, Request::WebSearch { query: "  ".into() })
        .unwrap_err()
        .contains("Type something"));
}

#[test]
fn a_file_hash_is_computed_here_and_the_virustotal_address_holds_only_the_hash() {
    let h = Harness::start(|_| {});
    let peer = h.client();
    let f = h.folder("progs").join("tool.bin");
    std::fs::write(&f, b"abc").unwrap();
    let Reply::FileHash(info) = ask(
        &peer,
        Request::FileHash {
            source: HashSource::Path(f.to_string_lossy().into_owned()),
        },
    )
    .unwrap() else {
        panic!("hash")
    };
    assert_eq!(info.sha256, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    let Reply::Url(u) = ask(
        &peer,
        Request::VirusTotalUrl {
            sha256: info.sha256.clone(),
        },
    )
    .unwrap() else {
        panic!("url")
    };
    assert_eq!(u, format!("https://www.virustotal.com/gui/file/{}", info.sha256));
    assert!(ask(&peer, Request::VirusTotalUrl { sha256: "nope".into() })
        .unwrap_err()
        .contains("64 hexadecimal"));
    // A relative path, a folder and a missing file are refused with a reason.
    for bad in ["relative", h.tmp.path().to_str().unwrap(), "/definitely/not/here"] {
        let e = ask(
            &peer,
            Request::FileHash {
                source: HashSource::Path(bad.into()),
            },
        )
        .unwrap_err();
        assert!(!e.is_empty(), "{bad}");
    }
    // A process that is not running (a reused pid) is not found.
    let gone = ProcessRef {
        pid: 100,
        started: Timestamp(1),
    };
    assert!(ask(
        &peer,
        Request::FileHash {
            source: HashSource::Process(gone)
        }
    )
    .unwrap_err()
    .contains("exited"));
}

#[test]
fn a_running_process_is_hashed_from_the_file_it_runs_from() {
    let tmp_exe = tempfile::tempdir().unwrap();
    let exe = tmp_exe.path().join("runner.bin");
    std::fs::write(&exe, b"abc").unwrap();
    let mut p = process(300, "runner", 1.0);
    p.exe = Some(exe.to_string_lossy().into_owned());
    let target = p.reference();
    let h = Harness::start(|f| f.add_process(p));
    let peer = h.client();
    // The sampler fills the process table within a tick or two.
    let mut answer = None;
    for _ in 0..40 {
        match ask(
            &peer,
            Request::FileHash {
                source: HashSource::Process(target),
            },
        ) {
            Ok(Reply::FileHash(i)) => {
                answer = Some(i);
                break;
            }
            _ => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    assert_eq!(
        answer.unwrap().sha256,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

fn startup(name: &str, exe: &str, publisher: Option<&str>) -> StartupEntry {
    StartupEntry {
        id: format!("agent:user:{name}"),
        name: name.into(),
        command: exe.into(),
        exe: Some(exe.into()),
        location: StartupLocation::UserLaunchAgent,
        state: StartupState::Enabled,
        publisher: publisher.map(str::to_string),
        impact: StartupImpact {
            cpu_secs: 0.0,
            disk_bytes: 0,
            rating: ImpactRating::NotMeasured,
        },
    }
}

#[test]
fn the_startup_list_is_filtered_by_the_engine_with_counts() {
    // Names that are OS-owned on every OS the test may run on: system folders of macOS, Linux and a Windows folder.
    let (os_exe, os_name) = if cfg!(windows) {
        ("C:\\Windows\\System32\\sec.exe", "SecurityHealth")
    } else if cfg!(target_os = "macos") {
        ("/System/Library/CoreServices/x", "com.apple.thing")
    } else {
        ("/usr/libexec/thing", "thing")
    };
    let h = Harness::start(|f| {
        f.with(|s| {
            s.startup = vec![
                startup(os_name, os_exe, None),
                startup("Slack", "/Applications/Slack.app/slack", Some("Slack Technologies")),
                startup("mystery", "/tmp/mystery", None),
            ]
        })
    });
    let peer = h.client();
    let get = |filter: StartupFilter| {
        let Reply::StartupOverview(o) = ask(&peer, Request::StartupOverview { filter }).unwrap() else {
            panic!("overview")
        };
        *o
    };
    let all = get(StartupFilter::default());
    assert_eq!((all.entries.len(), all.total, all.os_count, all.signed_count), (3, 3, 1, 1));
    let hidden = get(StartupFilter {
        hide_os: true,
        hide_signed: true,
    });
    assert_eq!(hidden.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["mystery"]);
    assert_eq!((hidden.total, hidden.os_count, hidden.signed_count), (3, 1, 1));
    // A failing startup source is an error with a reason, not an empty list.
    h.fake.fail(
        dyno_platform::fake::FakeOp::StartupList,
        dyno_core::ports::PlatformError::AccessDenied("no".into()),
    );
    assert!(ask(
        &peer,
        Request::StartupOverview {
            filter: StartupFilter::default()
        }
    )
    .is_err());
}

fn net(links: Vec<Link>, ssid: Option<&str>) -> NetState {
    NetState {
        links,
        ssid: ssid.map(str::to_string),
    }
}

fn link(name: &str, kind: LinkKind, addr: &str) -> Link {
    Link {
        name: name.into(),
        kind,
        addresses: vec![addr.parse().unwrap()],
    }
}

#[test]
fn network_changes_land_on_the_timeline() {
    let h = Harness::start(|f| f.with(|s| s.net = Some(net(vec![link("en0", LinkKind::Wifi, "192.168.1.5")], Some("Home")))));
    let shared = h.shared();
    // The first look is a baseline.
    assert!(shared.netwatch.look(&shared).is_empty());
    h.fake.with(|s| {
        s.net = Some(net(
            vec![link("en0", LinkKind::Wifi, "10.0.0.9"), link("utun4", LinkKind::Vpn, "10.8.0.2")],
            Some("Cafe"),
        ))
    });
    let changes = shared.netwatch.look(&shared);
    assert_eq!(changes.len(), 4, "left, joined, vpn up, ip change: {changes:?}");
    let peer = h.client();
    let Reply::History(hist) = ask(
        &peer,
        Request::History {
            range: HistoryRange::Hour,
            end: None,
        },
    )
    .unwrap() else {
        panic!("history")
    };
    let labels: Vec<&str> = hist
        .events
        .iter()
        .filter(|e| e.kind == TimelineEventKind::Network)
        .map(|e| e.label.as_str())
        .collect();
    assert!(labels.contains(&"Left Wi-Fi “Home”"), "{labels:?}");
    assert!(labels.contains(&"Joined Wi-Fi “Cafe”"));
    assert!(labels.contains(&"VPN up (utun4)"));
    assert!(labels.iter().any(|l| l.starts_with("IP address on en0 changed")));
    // A machine that cannot be read is skipped quietly and keeps the baseline.
    h.fake.with(|s| s.net = None);
    assert!(shared.netwatch.look(&shared).is_empty());
}

fn watched(peer: &Peer<Message>) -> dyno_core::watch::WatchOverview {
    let Reply::Watched(o) = ask(peer, Request::WatchedFolders).unwrap() else {
        panic!("watched")
    };
    *o
}

#[test]
fn a_watched_folder_is_off_until_previewed_and_only_then_sweeps_into_an_undoable_quarantine() {
    let h = Harness::start(|_| {});
    let peer = h.client();
    let dir = h.folder("downloads");
    let old = old_file(&dir, "installer.dmg");
    let keep = old_file(&dir, "notes.txt");
    let outside = old_file(&h.folder("elsewhere"), "other.dmg");
    // Adding switches nothing on.
    let Reply::Watched(o) = ask(
        &peer,
        Request::AddWatchedFolder {
            path: dir.to_string_lossy().into_owned(),
            template: Some("installers".into()),
            keep_for_secs: 86_400,
            recursive: false,
        },
    )
    .unwrap() else {
        panic!("add")
    };
    let id = o.folders[0].folder.id.clone();
    assert!(!o.folders[0].folder.enabled);
    // Turning on or cleaning without a preview is refused.
    let on = Request::SetWatchedFolder {
        id: id.clone(),
        keep_for_secs: None,
        enabled: Some(true),
        recursive: None,
    };
    assert!(ask(&peer, on.clone()).unwrap_err().contains("preview first"));
    assert!(ask(&peer, Request::SweepWatchedFolder { id: id.clone() })
        .unwrap_err()
        .contains("preview first"));
    assert!(old.exists(), "nothing moved before the person agreed");
    let Reply::WatchPreview(p) = ask(&peer, Request::PreviewWatchedFolder { id: id.clone() }).unwrap() else {
        panic!("preview")
    };
    assert_eq!(p.id, id);
    assert!(old.exists(), "a preview moves nothing");
    ask(&peer, on).unwrap();
    assert!(watched(&peer).folders[0].folder.enabled);
    // A sweep (creation times are ignored, as temp files made a moment ago would otherwise never be due).
    let folder = watched(&peer).folders[0].folder.clone();
    dyno_agent::handlers_watch::sweep_one(&h.shared(), &folder, false).unwrap();
    assert!(
        !old.exists() && keep.exists() && outside.exists(),
        "only the matching old file of the watched folder moved"
    );
    let last = watched(&peer).folders[0].folder.last_sweep.clone().unwrap();
    assert_eq!((last.moved, last.failed), (1, 0));
    let Reply::Activities(log) = ask(&peer, Request::Activity { limit: 20 }).unwrap() else {
        panic!("activity")
    };
    let entry = log
        .iter()
        .find(|a| a.title.contains("old file"))
        .expect("the sweep is in the activity log");
    assert_eq!(entry.undo, UndoState::Available);
    ask(
        &peer,
        Request::Undo {
            activity_id: entry.id.clone(),
        },
    )
    .unwrap();
    assert!(old.exists(), "undo put the file back");
    // Stopping the watch leaves the folder alone.
    ask(&peer, Request::RemoveWatchedFolder { id }).unwrap();
    assert!(watched(&peer).folders.is_empty() && old.exists());
}

#[test]
fn unsafe_and_overlapping_folders_are_refused_with_a_reason() {
    let h = Harness::start(|_| {});
    let peer = h.client();
    let add = |path: String| {
        ask(
            &peer,
            Request::AddWatchedFolder {
                path,
                template: None,
                keep_for_secs: 600,
                recursive: false,
            },
        )
    };
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
    for bad in [
        "/".to_string(),
        home,
        h.dirs.root().to_string_lossy().into_owned(),
        "relative".to_string(),
        "/no/such/folder".to_string(),
    ] {
        let e = add(bad.clone()).unwrap_err();
        assert!(!e.is_empty(), "{bad}");
    }
    let dir = h.folder("a");
    add(dir.to_string_lossy().into_owned()).unwrap();
    std::fs::create_dir_all(dir.join("inner")).unwrap();
    assert!(add(dir.join("inner").to_string_lossy().into_owned())
        .unwrap_err()
        .contains("overlap"));
    assert!(ask(
        &peer,
        Request::SetWatchedFolder {
            id: "w-none".into(),
            keep_for_secs: Some(600),
            enabled: None,
            recursive: None
        }
    )
    .is_err());
    let id = watched(&peer).folders[0].folder.id.clone();
    assert!(ask(
        &peer,
        Request::SetWatchedFolder {
            id,
            keep_for_secs: Some(5),
            enabled: None,
            recursive: None
        }
    )
    .unwrap_err()
    .contains("between 10 minutes and 10 days"));
}

#[test]
fn the_list_survives_an_agent_restart_and_a_damaged_file_starts_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let dir = std::fs::canonicalize(tmp.path()).unwrap().join("watched");
    std::fs::create_dir_all(&dir).unwrap();
    let start = |dirs: &Dirs| {
        let fake = Fake::new();
        fake.add_process(process(1, "calm", 1.0));
        Running::start(dirs.clone(), fake.platform(), Box::new(fake.clone()), Box::new(|_| Ok(()))).unwrap()
    };
    let r = start(&dirs);
    let peer = connect(&dirs).unwrap().0;
    ask(
        &peer,
        Request::AddWatchedFolder {
            path: dir.to_string_lossy().into_owned(),
            template: None,
            keep_for_secs: 600,
            recursive: false,
        },
    )
    .unwrap();
    drop(peer);
    r.stop();
    let r = start(&dirs);
    let peer = connect(&dirs).unwrap().0;
    assert_eq!(watched(&peer).folders.len(), 1);
    drop(peer);
    r.stop();
    std::fs::write(dirs.watched_folders(), "{broken").unwrap();
    let _ = std::fs::remove_file(dirs.watched_folders().with_extension("json.bak"));
    let r = start(&dirs);
    let peer = connect(&dirs).unwrap().0;
    assert!(watched(&peer).folders.is_empty());
    drop(peer);
    r.stop();
}
