#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Wake-on-LAN end to end over the fake platform (ER-14): a real IPC connection and the agent's job pool, with `FakeUdp`,
//! `FakeNeighbours`, `FakeProber`, `MemorySecrets` and a fake clock standing in for the network, the neighbour table, the echo, the
//! credential store and time. No datagram is sent, no host is pinged, no test waits for a real second.

use dyno_agent::cli::{ask, connect};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::jobs::{JobOutput, JobSpec};
use dyno_core::protocol::{LiveDetail, Message, Reply, Request};
use dyno_core::wol::fake::{FakeClock, FakeInterfaces, FakeNeighbours, FakeProber, FakeReadiness, FakeUdp};
use dyno_core::wol::readiness::{cant_tell, ReadinessItem, ReadinessKind};
use dyno_core::wol::{Mac, Neighbour, NeighbourState, NetIf, SecureOnEdit, ThisPcWol, WolDeviceInput, WolOutcome, WolView};
use paddock_ipc::{BodyOptions, BodyPeer};
use dyno_platform::fake::Fake;
use dyno_platform::wol::WolPorts;
use paddock_jobs::wire::WireSnapshot;
use paddock_secrets::{MemorySecrets, Name, Secrets};
use std::net::Ipv4Addr;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;

/// How long a test waits for the agent before it calls the run hung (a guard, never the expected wait).
const HANG_GUARD: Duration = Duration::from_secs(30);

const MAC: &str = "02:00:5e:10:00:01";

struct Harness {
    running: Option<Running>,
    udp: Arc<FakeUdp>,
    secrets: Arc<MemorySecrets>,
    dirs: Dirs,
    _tmp: tempfile::TempDir,
}

#[derive(Default)]
struct Scripts {
    neighbours: FakeNeighbours,
    prober: FakeProber,
}

impl Harness {
    fn start() -> Harness {
        Harness::start_with(Scripts::default())
    }

    fn start_with(scripts: Scripts) -> Harness {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        std::fs::create_dir_all(dirs.root()).unwrap();
        let udp = Arc::new(FakeUdp::default());
        let secrets = Arc::new(MemorySecrets::new("Dyno"));
        let fake = Fake::new();
        let mut platform = fake.platform();
        platform.wol = WolPorts {
            udp: udp.clone(),
            interfaces: Arc::new(FakeInterfaces(vec![NetIf {
                name: "en0".into(),
                addr: Ipv4Addr::new(192, 0, 2, 17),
                prefix: 24,
            }])),
            neighbours: Arc::new(scripts.neighbours),
            prober: Arc::new(scripts.prober),
            readiness: Arc::new(FakeReadiness(ThisPcWol {
                items: vec![ReadinessItem {
                    kind: ReadinessKind::Firmware,
                    subject: None,
                    state: cant_tell("not readable"),
                }],
            })),
            secrets: secrets.clone(),
            clock: Arc::new(FakeClock::default()),
        };
        let running = Running::start(dirs.clone(), platform, Box::new(fake), Box::new(|_| Ok(()))).unwrap();
        Harness {
            running: Some(running),
            udp,
            secrets,
            dirs,
            _tmp: tmp,
        }
    }

    fn client(&self) -> BodyPeer<Message> {
        connect(&self.dirs).unwrap().0
    }

    /// A connection that receives the pushed lists and the jobs' statuses.
    fn watcher(&self) -> (BodyPeer<Message>, Receiver<WolView>, Receiver<WireSnapshot>) {
        let d = dyno_agent::discovery::Discovery::read(&self.dirs.discovery()).unwrap();
        let conn = dyno_agent::wire::connect(&d.endpoint, &d.token().unwrap()).unwrap();
        let (tx, rx) = channel();
        let (jtx, jrx) = channel();
        let peer = BodyPeer::start(
            conn,
            BodyOptions::new("the agent").on_push(move |m| match m {
                Message::WolChanged(v) => {
                    let _ = tx.send(*v);
                }
                Message::Job(j) => {
                    let _ = jtx.send(*j);
                }
                _ => {}
            }),
        )
        .unwrap();
        ask(
            &peer,
            Request::Subscribe {
                detail: LiveDetail::Summary,
            },
        )
        .unwrap();
        (peer, rx, jrx)
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop();
        }
    }
}

fn input(name: &str, mac: &str) -> WolDeviceInput {
    WolDeviceInput {
        id: None,
        name: name.into(),
        mac: mac.into(),
        last_ip: Some("192.0.2.40".into()),
        interface: None,
        secure_on: SecureOnEdit::Keep,
    }
}

fn wol(r: Reply) -> WolView {
    match r {
        Reply::Wol(v) => *v,
        other => panic!("expected a list, got {other:?}"),
    }
}

fn save(c: &BodyPeer<Message>, d: WolDeviceInput) -> Result<WolView, String> {
    ask(c, Request::WolSave { device: d }).map(wol)
}

/// Starts a wake and waits for its job to end; returns the output of a done job.
fn wake(c: &BodyPeer<Message>, jobs: &Receiver<WireSnapshot>, id: u64, test: bool) -> Option<WolOutcome> {
    let Reply::Job(started) = ask(c, Request::WolWake { id, test }).unwrap() else {
        panic!("a job")
    };
    loop {
        let s = jobs.recv_timeout(HANG_GUARD).expect("the job ends");
        if s.id == started.id && s.is_terminal() {
            break;
        }
    }
    match ask(c, Request::JobResult { id: started.id }) {
        Ok(Reply::JobOutput(o)) => match *o {
            JobOutput::Wol(o) => Some(*o),
            other => panic!("not a wake: {other:?}"),
        },
        _ => None,
    }
}

fn neighbour(state: NeighbourState, ip: &str) -> Neighbour {
    Neighbour {
        name: None,
        mac: Mac::parse(MAC).unwrap(),
        ip: ip.into(),
        interface: "en0".into(),
        state,
    }
}

#[test]
fn adding_changing_and_removing_is_validated_pushed_and_keeps_the_password_in_the_credential_store() {
    let h = Harness::start();
    let (_w, lists, _jobs) = h.watcher();
    let c = h.client();
    assert!(wol(ask(&c, Request::WolDevices).unwrap()).devices.is_empty());

    let mut with_pw = input("Media PC", MAC);
    with_pw.secure_on = SecureOnEdit::Set {
        password: "01:02:03:04".into(),
    };
    let v = save(&c, with_pw).unwrap();
    assert_eq!(v.devices.len(), 1);
    let d = &v.devices[0];
    assert!(d.secure_on && d.id == 1 && d.port == 9);
    assert_eq!(
        lists.recv_timeout(HANG_GUARD).unwrap().devices.len(),
        1,
        "the change is pushed to every app"
    );
    let name = Name::new("wol", "secure-on-1").unwrap();
    assert_eq!(h.secrets.get(&name).unwrap().unwrap().expose(), "01:02:03:04");
    // The password is in the credential store only: the database row says there is one and nothing more.
    let db = rusqlite::Connection::open(h.dirs.database()).unwrap();
    let row: String = db.query_row("SELECT data FROM wol_devices WHERE id = 1", [], |r| r.get(0)).unwrap();
    assert!(
        row.contains("\"secureOn\":true") && !row.contains("01:02:03:04") && !row.contains("0102"),
        "{row}"
    );

    // Rename keeps the password; a duplicate address and a bad address are refused with words.
    let mut rename = input("Living room PC", MAC);
    rename.id = Some(1);
    assert_eq!(save(&c, rename).unwrap().devices[0].name, "Living room PC");
    assert!(h.secrets.get(&name).unwrap().is_some());
    assert!(save(&c, input("Other", MAC))
        .unwrap_err()
        .contains("already saved as \"Living room PC\""));
    assert!(save(&c, input("Other", "nope")).unwrap_err().contains("six pairs"));
    assert!(save(&c, input("", "02:00:5e:10:00:02")).unwrap_err().contains("name"));

    // Clearing the password removes it from the store; removing the computer removes the rest.
    let mut clear = input("Living room PC", MAC);
    clear.id = Some(1);
    clear.secure_on = SecureOnEdit::Clear;
    assert!(!save(&c, clear).unwrap().devices[0].secure_on);
    assert!(h.secrets.get(&name).unwrap().is_none());
    assert!(wol(ask(&c, Request::WolRemove { id: 1 }).unwrap()).devices.is_empty());
}

#[test]
fn a_wake_sends_three_rounds_of_magic_packets_through_the_card_and_saves_how_it_ended() {
    let h = Harness::start();
    let (_w, lists, jobs) = h.watcher();
    let c = h.client();
    save(&c, input("Media PC", MAC)).unwrap();
    let _ = lists.recv_timeout(HANG_GUARD).unwrap();
    let out = wake(&c, &jobs, 1, false).expect("a done wake");
    assert!(!out.tested && out.answered.is_none());
    assert_eq!(out.sent.len(), 2, "the limited and the directed broadcast of the one card");
    let sent = h.udp.sent();
    assert_eq!(sent.len(), 6);
    let mac = Mac::parse(MAC).unwrap();
    for s in &sent {
        assert_eq!(s.source, Ipv4Addr::new(192, 0, 2, 17));
        assert_eq!((s.port, s.len), (9, 102));
        assert_eq!(&s.payload[..6], &[0xff; 6]);
        assert_eq!(&s.payload[6..12], &mac.0);
    }
    assert!(sent.iter().any(|s| s.dest == Ipv4Addr::BROADCAST) && sent.iter().any(|s| s.dest == Ipv4Addr::new(192, 0, 2, 255)));
    // The ended wake is saved on the device and pushed.
    let pushed = lists.recv_timeout(HANG_GUARD).unwrap();
    let last = pushed.devices[0].last_wake.clone().unwrap();
    assert!(last.ok && !last.tested && last.summary.starts_with("Wake packets sent"));
}

#[test]
fn a_secureon_password_is_appended_to_the_packet() {
    let h = Harness::start();
    let (_w, _lists, jobs) = h.watcher();
    let c = h.client();
    let mut d = input("NAS", MAC);
    d.secure_on = SecureOnEdit::Set {
        password: "0a:0b:0c:0d:0e:0f".into(),
    };
    save(&c, d).unwrap();
    wake(&c, &jobs, 1, false).unwrap();
    let sent = h.udp.sent();
    assert!(sent
        .iter()
        .all(|s| s.len == 108 && s.payload[102..] == [0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f]));
}

#[test]
fn a_test_wake_waits_for_the_answer_and_saves_a_moved_address() {
    // No answer to the first look, then the computer shows up reachable at a new address.
    let h = Harness::start_with(Scripts {
        neighbours: FakeNeighbours::scripted(vec![vec![], vec![], vec![neighbour(NeighbourState::Reachable, "192.0.2.77")]]),
        prober: FakeProber::default(),
    });
    let (_w, lists, jobs) = h.watcher();
    let c = h.client();
    save(&c, input("Media PC", MAC)).unwrap();
    let _ = lists.recv_timeout(HANG_GUARD).unwrap();
    let out = wake(&c, &jobs, 1, true).unwrap();
    let a = out.answered.expect("answered");
    assert_eq!(a.new_ip.as_deref(), Some("192.0.2.77"));
    assert!(out.tested && !out.timed_out);
    let d = &wol(ask(&c, Request::WolDevices).unwrap()).devices[0];
    assert_eq!(d.last_ip.as_deref(), Some("192.0.2.77"));
    assert!(d.last_wake.as_ref().unwrap().ok);
}

#[test]
fn a_test_wake_that_gets_no_answer_says_so_with_the_next_step_and_takes_no_real_time() {
    let h = Harness::start();
    let (_w, _lists, jobs) = h.watcher();
    let c = h.client();
    save(&c, input("Media PC", MAC)).unwrap();
    let t = std::time::Instant::now();
    let out = wake(&c, &jobs, 1, true).unwrap();
    assert!(out.timed_out && out.answered.is_none());
    assert!(out.message.contains("off at the wall"));
    assert!(t.elapsed() < Duration::from_secs(20), "the fake clock waited the 120 s");
    let d = &wol(ask(&c, Request::WolDevices).unwrap()).devices[0];
    assert!(!d.last_wake.as_ref().unwrap().ok);
}

#[test]
fn a_wake_is_refused_for_an_unknown_computer_and_when_the_feature_is_off() {
    let h = Harness::start();
    let c = h.client();
    let e = ask(&c, Request::WolWake { id: 99, test: false }).unwrap_err();
    assert!(e.contains("no longer in the list"), "{e}");
    save(&c, input("Media PC", MAC)).unwrap();
    let Reply::Settings(s) = ask(&c, Request::Settings).unwrap() else {
        panic!("settings")
    };
    let mut s = *s;
    s.power.wol.enabled = false;
    ask(&c, Request::SaveSettings { settings: Box::new(s) }).unwrap();
    assert!(ask(&c, Request::WolWake { id: 1, test: false })
        .unwrap_err()
        .contains("turned off in Settings"));
    assert!(save(&c, input("Other", "02:00:5e:10:00:02"))
        .unwrap_err()
        .contains("turned off in Settings"));
    assert!(!wol(ask(&c, Request::WolDevices).unwrap()).enabled);
    assert!(h.udp.sent().is_empty());
    // Starting the job directly is refused too: the job checks again, because the agent does not trust the webview.
    let (_w, _l, jobs) = h.watcher();
    let Reply::Job(j) = ask(
        &c,
        Request::StartJob {
            spec: JobSpec::WolWake { id: 1, test: false },
        },
    )
    .unwrap() else {
        panic!("a job")
    };
    loop {
        let s = jobs.recv_timeout(HANG_GUARD).expect("the job ends");
        if s.id == j.id && s.is_terminal() {
            assert!(
                matches!(s.state, paddock_jobs::wire::WireState::Failed(ref m) if m.contains("turned off")),
                "{:?}",
                s.state
            );
            break;
        }
    }
    assert!(h.udp.sent().is_empty());
}

#[test]
fn add_from_network_lists_only_unsaved_computers_and_can_be_turned_off() {
    let table = vec![
        neighbour(NeighbourState::Reachable, "192.0.2.40"),
        Neighbour {
            name: None,
            mac: Mac::parse("02:00:5e:10:00:09").unwrap(),
            ip: "192.0.2.90".into(),
            interface: "en0".into(),
            state: NeighbourState::Stale,
        },
    ];
    let h = Harness::start_with(Scripts {
        neighbours: FakeNeighbours::fixed(table),
        prober: FakeProber::default(),
    });
    let c = h.client();
    let Reply::WolCandidates(all) = ask(&c, Request::WolCandidates).unwrap() else {
        panic!("candidates")
    };
    assert_eq!(all.len(), 2);
    save(&c, input("Media PC", MAC)).unwrap();
    let Reply::WolCandidates(rest) = ask(&c, Request::WolCandidates).unwrap() else {
        panic!("candidates")
    };
    assert_eq!(rest.iter().map(|n| n.ip.as_str()).collect::<Vec<_>>(), ["192.0.2.90"]);
    let Reply::Settings(s) = ask(&c, Request::Settings).unwrap() else {
        panic!("settings")
    };
    let mut s = *s;
    s.power.wol.discovery = false;
    ask(&c, Request::SaveSettings { settings: Box::new(s) }).unwrap();
    assert!(ask(&c, Request::WolCandidates).unwrap_err().contains("turned off in Settings"));
}

#[test]
fn a_refused_neighbour_table_is_an_error_with_words_not_a_crash() {
    let h = Harness::start_with(Scripts {
        neighbours: FakeNeighbours::failing(),
        prober: FakeProber::default(),
    });
    let c = h.client();
    assert!(ask(&c, Request::WolCandidates).unwrap_err().contains("could not be read"));
    assert!(wol(ask(&c, Request::WolDevices).unwrap()).enabled, "the agent is still serving");
}

#[test]
fn readiness_is_what_the_platform_reads() {
    let h = Harness::start();
    let c = h.client();
    let Reply::WolReadiness(r) = ask(&c, Request::WolReadiness).unwrap() else {
        panic!("readiness")
    };
    assert_eq!(r.items.len(), 1);
    assert_eq!(r.items[0].kind, ReadinessKind::Firmware);
}
