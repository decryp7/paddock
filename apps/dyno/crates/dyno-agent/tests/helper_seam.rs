#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The helper seam end to end (helper-wiring plan sections 3 and 4.2): a real agent over the fake platform whose `Shared.helper`
//! is served by a real helper `Dispatcher` (`common/helper_rig.rs`). Memory modules are the second instance of the seam.

use dyno_agent::cli::{ask_within, connect};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::helper::HelperHost;
use dyno_core::helper_wire::HelperState;
use dyno_core::inventory::{MemoryLayout, SystemInventory};
use dyno_core::protocol::{Reply, Request};
use dyno_core::read_via::{NextStep, ReadVia};
use dyno_platform::fake::Fake;
use dyno_platform::inventory::linux;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[path = "common/helper_rig.rs"]
mod helper_rig;
use helper_rig::{dispatcher, wiring, Caller, DispatcherTransport, Readers};

const SLOW: Duration = Duration::from_secs(60);
const TOTAL: u64 = 8 << 30;

/// A small SMBIOS table: a memory array of four slots and one 8 GiB DDR4 stick (synthetic, the same shape the parser's own tests use).
fn dmi_table() -> Vec<u8> {
    fn structure(kind: u8, len: usize, set: &dyn Fn(&mut Vec<u8>), strings: &[&str]) -> Vec<u8> {
        let mut d = vec![0u8; len];
        d[0] = kind;
        d[1] = len as u8;
        set(&mut d);
        for s in strings {
            d.extend_from_slice(s.as_bytes());
            d.push(0);
        }
        if strings.is_empty() {
            d.push(0);
        }
        d.push(0);
        d
    }
    let mut t = structure(
        16,
        23,
        &|d| {
            d[0x07..0x0b].copy_from_slice(&(64u32 << 20).to_le_bytes());
            d[0x0d..0x0f].copy_from_slice(&4u16.to_le_bytes());
        },
        &[],
    );
    t.extend(structure(
        17,
        0x28,
        &|d| {
            d[0x0c..0x0e].copy_from_slice(&8192u16.to_le_bytes());
            d[0x0e] = 8;
            d[0x10] = 1;
            d[0x12] = 26;
            d[0x15..0x17].copy_from_slice(&3200u16.to_le_bytes());
            d[0x17] = 3;
            d[0x1a] = 4;
        },
        &["ChannelA-DIMM0", "BANK 0", "Acme", "AC-1"],
    ));
    t.extend(structure(127, 4, &|_| {}, &[]));
    t
}

struct Rig {
    running: Option<Running>,
    dirs: Dirs,
    up: std::sync::Arc<std::sync::atomic::AtomicBool>,
    served: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    _tmp: tempfile::TempDir,
}

impl Rig {
    fn start(caller: Caller, host: HelperHost, up: bool) -> Rig {
        Rig::start_with(caller, host, up, Box::new(dyno_core::helper_holders::NoHolders), |_| {})
    }

    fn start_with(
        caller: Caller,
        host: HelperHost,
        up: bool,
        holders: Box<dyn dyno_core::helper_holders::HolderSource>,
        seed: impl FnOnce(&Fake),
    ) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let fake = Fake::new();
        seed(&fake);
        let readers = Readers {
            memory: linux::parse(&dmi_table(), TOTAL),
            holders,
            ..Readers::default()
        };
        let transport = DispatcherTransport::new(dispatcher(host, readers, tmp.path()), caller);
        transport.up.store(up, Ordering::SeqCst);
        let (up, served) = (transport.up.clone(), transport.served.clone());
        let running = Running::start_with_helper(
            dirs.clone(),
            fake.platform(),
            Box::new(fake.clone()),
            Box::new(|_| Ok(())),
            wiring(transport, host),
        )
        .unwrap();
        Rig {
            running: Some(running),
            dirs,
            up,
            served,
            _tmp: tmp,
        }
    }

    fn inventory(&self) -> SystemInventory {
        let peer = connect(&self.dirs).unwrap().0;
        let Reply::Inventory(i) = ask_within(&peer, Request::Inventory, SLOW).unwrap() else {
            panic!("inventory")
        };
        *i
    }

    fn network(&self) -> dyno_core::network::NetworkSnapshot {
        let peer = connect(&self.dirs).unwrap().0;
        let query = dyno_core::network::NetworkQuery {
            show_loopback: true,
            ..Default::default()
        };
        let Reply::Network(n) = ask_within(&peer, Request::Network { query }, SLOW).unwrap() else {
            panic!("network")
        };
        *n
    }

    /// The network snapshot once `done` holds (the first sample carries the process list the names come from).
    fn network_when(&self, done: impl Fn(&dyno_core::network::NetworkSnapshot) -> bool) -> dyno_core::network::NetworkSnapshot {
        let until = std::time::Instant::now() + SLOW;
        loop {
            let n = self.network();
            if done(&n) || std::time::Instant::now() > until {
                return n;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[cfg(unix)]
    fn lockers(&self, path: &std::path::Path) -> Result<Vec<dyno_core::process::Locker>, String> {
        let peer = connect(&self.dirs).unwrap().0;
        match ask_within(
            &peer,
            Request::Lockers {
                path: path.to_string_lossy().into_owned(),
            },
            SLOW,
        )? {
            Reply::Lockers(l) => Ok(l),
            other => panic!("{other:?}"),
        }
    }

    fn state(&self) -> HelperState {
        let peer = connect(&self.dirs).unwrap().0;
        let Reply::HelperStatus(s) = ask_within(&peer, Request::HelperStatus, SLOW).unwrap() else {
            panic!("status")
        };
        s.state
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        if let Some(r) = self.running.take() {
            r.stop();
        }
    }
}

#[test]
fn memory_modules_come_from_a_real_helper_dispatcher_and_say_so() {
    let rig = Rig::start(Caller::Owner, HelperHost::Linux, true);
    let inv = rig.inventory();
    assert!(
        matches!(&inv.memory, MemoryLayout::Slots(a) if a.slots == 4 && a.modules.len() == 1),
        "{:?}",
        inv.memory
    );
    assert_eq!(inv.memory_via, ReadVia::Helper);
    assert!(
        rig.served.lock().unwrap().iter().any(|k| k == "MemoryModules"),
        "the helper was asked"
    );
}

#[test]
fn the_helper_path_and_the_local_path_give_the_same_domain_type_for_the_same_input() {
    // The same SMBIOS bytes: the helper's reader parses them (served through the real dispatcher, over the wire), and the agent's
    // own reader on a computer where the table is readable parses them with the same function.
    let local = linux::parse(&dmi_table(), TOTAL);
    let rig = Rig::start(Caller::Owner, HelperHost::Linux, true);
    assert_eq!(rig.inventory().memory, local);
}

#[test]
fn without_the_helper_the_total_is_shown_and_the_source_asks_for_the_helper() {
    let rig = Rig::start(Caller::Owner, HelperHost::Linux, false);
    let inv = rig.inventory();
    assert!(matches!(inv.memory, MemoryLayout::TotalOnly { .. }));
    let ReadVia::NeedsHelper { reason, next } = inv.memory_via else {
        panic!("{:?}", inv.memory_via)
    };
    assert!(reason.contains("administrator"), "{reason}");
    assert_eq!(next, NextStep::OpenSettings("helper".into()));
    assert_eq!(rig.state(), HelperState::Off);
    assert!(rig.served.lock().unwrap().is_empty(), "nothing reached an absent helper");
    rig.up.store(true, Ordering::SeqCst);
    assert_eq!(rig.state(), HelperState::Ready);
    // The inventory was cached while the helper was off; the helper coming up moves the epoch and it is read again.
    let inv = rig.inventory();
    assert!(matches!(&inv.memory, MemoryLayout::Slots(a) if a.slots == 4), "{:?}", inv.memory);
    assert_eq!(inv.memory_via, ReadVia::Helper);
}

#[test]
fn a_caller_who_may_not_read_the_audit_log_still_sees_a_ready_helper() {
    let rig = Rig::start(Caller::Other, HelperHost::Linux, true);
    assert_eq!(rig.state(), HelperState::Ready);
    assert_eq!(rig.inventory().memory_via, ReadVia::Helper, "memory is open to any caller");
}

/// A helper reader of who holds what: one system program holding socket 900, one other account's holding socket 901, and one
/// system program holding the file with the device and inode it is told; records what it was asked.
struct Holders {
    file: std::sync::Arc<std::sync::Mutex<Vec<(u64, u64)>>>,
    want: Option<(u64, u64)>,
}

impl Holders {
    fn raw(pid: u32, owner: dyno_core::helper_processes::RawOwner, sockets: &[u64]) -> dyno_core::helper_holders::RawHolder {
        dyno_core::helper_holders::RawHolder {
            pid,
            started: Some(1_700_000_000),
            name: format!("prog{pid}"),
            owner,
            sockets: sockets.to_vec(),
        }
    }
}

impl dyno_core::helper_holders::HolderSource for Holders {
    fn sockets(&self) -> Result<dyno_core::helper_holders::HolderScan, dyno_core::ports::PlatformError> {
        use dyno_core::helper_processes::RawOwner;
        Ok(dyno_core::helper_holders::HolderScan {
            holders: vec![
                Holders::raw(7001, RawOwner::System, &[900]),
                Holders::raw(
                    7002,
                    RawOwner::Account {
                        id: dyno_core::helper_wire::HelperUser::parse("uid:1999").unwrap(),
                        name: Some("someone".into()),
                    },
                    &[901],
                ),
            ],
            truncated: false,
        })
    }

    fn file(&self, dev: u64, ino: u64) -> Result<dyno_core::helper_holders::HolderScan, dyno_core::ports::PlatformError> {
        self.file.lock().unwrap().push((dev, ino));
        let hit = self.want == Some((dev, ino));
        Ok(dyno_core::helper_holders::HolderScan {
            holders: if hit {
                vec![Holders::raw(7001, dyno_core::helper_processes::RawOwner::System, &[])]
            } else {
                vec![]
            },
            truncated: false,
        })
    }
}

fn hidden_listener(port: u16, inode: u64) -> dyno_core::network::SocketRow {
    dyno_core::network::SocketRow {
        proto: dyno_core::network::Proto::Tcp,
        local: std::net::IpAddr::from([10, 1, 2, 3]),
        local_port: port,
        remote: None,
        state: dyno_core::network::SocketState::Listen,
        pid: None,
        inode: Some(inode),
    }
}

fn no_holders() -> Holders {
    Holders {
        file: Default::default(),
        want: None,
    }
}

#[test]
fn a_socket_the_kernel_hides_gets_its_program_from_the_helper_and_the_page_says_so() {
    let rig = Rig::start_with(Caller::Owner, HelperHost::Linux, true, Box::new(no_holders()), |f| {
        f.add_process(dyno_platform::fake::process(7001, "prog7001", 0.0));
        f.with(|s| s.sockets.push(hidden_listener(5432, 900)));
    });
    let n = rig.network_when(|n| {
        matches!(
            n.listeners.first().map(|l| &l.owner),
            Some(dyno_core::network::Owner::Process { .. })
        )
    });
    assert_eq!(n.listeners.len(), 1, "{:?}", n.listeners);
    assert_eq!(n.listeners[0].port, 5432);
    assert!(
        matches!(&n.listeners[0].owner, dyno_core::network::Owner::Process { pid: 7001, .. }),
        "{:?}",
        n.listeners[0].owner
    );
    assert_eq!(n.owners_via, ReadVia::Helper);
}

#[test]
fn without_the_helper_a_hidden_socket_stays_hidden_and_the_source_asks_for_the_helper() {
    let rig = Rig::start_with(Caller::Owner, HelperHost::Linux, false, Box::new(no_holders()), |f| {
        f.with(|s| s.sockets.push(hidden_listener(5432, 900)));
    });
    let n = rig.network();
    assert_eq!(n.listeners.len(), 1);
    assert!(!matches!(&n.listeners[0].owner, dyno_core::network::Owner::Process { .. }));
    let ReadVia::NeedsHelper { reason, next } = n.owners_via else {
        panic!("{:?}", n.owners_via)
    };
    assert!(reason.contains("hides which program"), "{reason}");
    assert_eq!(next, NextStep::OpenSettings("helper".into()));
}

#[test]
fn another_accounts_socket_stays_hidden_from_a_caller_who_may_not_see_it() {
    let rig = Rig::start_with(Caller::Other, HelperHost::Linux, true, Box::new(no_holders()), |f| {
        f.with(|s| s.sockets.push(hidden_listener(5433, 901)));
    });
    let n = rig.network();
    assert!(
        !matches!(&n.listeners[0].owner, dyno_core::network::Owner::Process { .. }),
        "scoped by the connection's account"
    );
}

#[cfg(unix)]
#[test]
fn what_holds_this_file_is_asked_by_device_and_inode_and_answered_for_every_account() {
    use std::os::unix::fs::MetadataExt;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("held.log");
    std::fs::write(&file, "x").unwrap();
    let m = std::fs::metadata(&file).unwrap();
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let holders = Holders {
        file: asked.clone(),
        want: Some((m.dev(), m.ino())),
    };
    let rig = Rig::start_with(Caller::Owner, HelperHost::Linux, true, Box::new(holders), |_| {});
    let got = rig.lockers(&file).unwrap();
    assert_eq!(
        *asked.lock().unwrap(),
        [(m.dev(), m.ino())],
        "only the device and inode crossed, never the path"
    );
    assert_eq!(got.len(), 1);
    assert_eq!(
        (got[0].process.pid, got[0].name.as_str(), got[0].via.clone()),
        (7001, "prog7001", ReadVia::Helper)
    );
}

#[cfg(unix)]
#[test]
fn without_the_helper_what_holds_this_file_is_the_agents_own_answer_marked_local() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("held.log");
    std::fs::write(&file, "x").unwrap();
    let mine = dyno_core::process::Locker::new(dyno_core::process::ProcessRef { pid: 9, started: None }, "me".into(), None);
    let canonical = file.clone();
    let rig = Rig::start_with(Caller::Owner, HelperHost::Linux, false, Box::new(no_holders()), |f| {
        f.with(|s| {
            s.lockers.insert(canonical, vec![mine.clone()]);
        });
    });
    let got = rig.lockers(&file).unwrap();
    assert_eq!((got.len(), got[0].via.clone()), (1, ReadVia::Local));
}
#[test]
fn the_cached_inventory_reads_the_memory_slots_again_when_the_helper_comes_up() {
    let rig = Rig::start(Caller::Owner, HelperHost::Linux, false);
    let before = rig.inventory();
    assert!(matches!(before.memory, MemoryLayout::TotalOnly { .. }), "{:?}", before.memory);
    assert!(matches!(before.memory_via, ReadVia::NeedsHelper { .. }), "{:?}", before.memory_via);
    rig.up.store(true, Ordering::SeqCst);
    // The readiness answer is kept a few seconds: wait on the condition, not on a fixed time.
    let until = std::time::Instant::now() + SLOW;
    let after = loop {
        let inv = rig.inventory();
        if inv.memory_via == ReadVia::Helper || std::time::Instant::now() > until {
            break inv;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(after.memory_via, ReadVia::Helper);
    assert!(
        matches!(&after.memory, MemoryLayout::Slots(a) if a.slots == 4),
        "{:?}",
        after.memory
    );
}
