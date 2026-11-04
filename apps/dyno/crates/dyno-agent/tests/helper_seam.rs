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
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let fake = Fake::new();
        let readers = Readers {
            memory: linux::parse(&dmi_table(), TOTAL),
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
