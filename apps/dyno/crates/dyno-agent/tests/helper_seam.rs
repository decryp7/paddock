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
        Rig::start_with(caller, host, up, Readers::default())
    }

    fn start_with(caller: Caller, host: HelperHost, up: bool, readers: Readers) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = Dirs::at(tmp.path().join("data"));
        let fake = Fake::new();
        let readers = Readers {
            memory: linux::parse(&dmi_table(), TOTAL),
            ..readers
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

// ---- WP-CNT and WP-DEAD: counters, the helper log and Wake-on-LAN modes through the same real dispatcher ----

use dyno_agent::helper_first::CentrePort;
use dyno_agent::helper_readers::wol::HelperReadiness;
use dyno_core::helper_counters::{CounterAnswer, CounterValue, EnergyDomain};
use dyno_core::helper_requests::CounterId;
use dyno_core::wake::{WolMode, WolReply};
use dyno_core::wol::readiness::{linux_wakeup_item, ItemState};
use dyno_core::wol::{ReadinessSource, ThisPcWol};

const HANG_GUARD: Duration = Duration::from_secs(30);

fn apple_silicon() -> Vec<CounterAnswer> {
    vec![
        CounterAnswer {
            id: CounterId::Thermal,
            value: CounterValue::Unavailable("macOS reports a thermal pressure level, not temperatures, to software.".into()),
        },
        CounterAnswer {
            id: CounterId::Fans,
            value: CounterValue::Unavailable("Apple silicon Macs have no smc sampler.".into()),
        },
        CounterAnswer {
            id: CounterId::CpuEnergy,
            value: CounterValue::CpuEnergy(vec![EnergyDomain {
                label: "CPU Power".into(),
                microjoules: None,
                wraps_at_microjoules: None,
                milliwatts: Some(3_000),
            }]),
        },
    ]
}

fn counters_reach_the_energy_report_and_the_temperature_source(caller: Caller) {
    let rig = Rig::start_with(
        caller,
        HelperHost::Macos,
        true,
        Readers {
            counters: apple_silicon(),
            ..Readers::default()
        },
    );
    let shared = rig.running.as_ref().unwrap().shared.clone();
    let began = std::time::Instant::now();
    loop {
        let (_, cpu) = shared.climate.cpu_energy();
        if cpu == ReadVia::Helper && shared.climate.live_via() != ReadVia::Local {
            break;
        }
        assert!(began.elapsed() < HANG_GUARD, "the climate never read through the helper");
        std::thread::yield_now();
    }
    let ReadVia::Unavailable { reason, .. } = shared.climate.live_via() else {
        panic!("{:?}", shared.climate.live_via())
    };
    assert!(reason.contains("thermal pressure"), "{reason}");
    let peer = connect(&rig.dirs).unwrap().0;
    let Reply::Energy(report) = ask_within(&peer, Request::EnergyReport { days: 1 }, SLOW).unwrap() else {
        panic!("energy")
    };
    assert_eq!(report.cpu_via, ReadVia::Helper);
    assert!(rig.served.lock().unwrap().iter().any(|k| k == "Counters"), "the helper was asked");
}

#[test]
fn the_owner_gets_cpu_energy_and_the_reason_for_missing_temperatures() {
    counters_reach_the_energy_report_and_the_temperature_source(Caller::Owner);
}

#[test]
fn any_other_account_gets_the_same_counters() {
    counters_reach_the_energy_report_and_the_temperature_source(Caller::Other);
}

fn bundle_file(rig: &Rig, path: &str) -> Option<String> {
    let bundle = dyno_agent::diagnostics::collect(&rig.running.as_ref().unwrap().shared).unwrap();
    bundle.files.iter().find(|f| f.path == path).map(|f| f.text.clone())
}

#[test]
fn the_owner_s_bundle_holds_the_helper_log() {
    let rig = Rig::start_with(
        Caller::Owner,
        HelperHost::Linux,
        true,
        Readers {
            log: vec!["helper started".into(), "request served".into()],
            ..Readers::default()
        },
    );
    let text = bundle_file(&rig, "helper/helper.log").expect("the helper log is in the bundle");
    assert!(text.contains("helper started") && text.contains("request served"), "{text}");
    assert!(bundle_file(&rig, "helper/helper-log.txt").is_none());
}

#[test]
fn another_account_s_bundle_says_why_the_helper_log_is_missing() {
    let rig = Rig::start_with(
        Caller::Other,
        HelperHost::Linux,
        true,
        Readers {
            log: vec!["helper started".into()],
            ..Readers::default()
        },
    );
    assert!(
        bundle_file(&rig, "helper/helper.log").is_none(),
        "the helper refuses the log to another account"
    );
    let why = bundle_file(&rig, "helper/helper-log.txt").expect("the reason is in the bundle");
    assert!(!why.trim().is_empty() && !why.contains("helper started"), "{why}");
}

struct OneCard;
impl ReadinessSource for OneCard {
    fn read(&self) -> ThisPcWol {
        ThisPcWol {
            items: vec![linux_wakeup_item("enp3s0", "enabled\n")],
        }
    }
}

fn wol_mode_is_judged_through_the_helper(caller: Caller) {
    let tmp = tempfile::tempdir().unwrap();
    let readers = Readers {
        wol: Some(WolReply {
            enabled: vec![],
            supported: vec![WolMode::Magic, WolMode::Phy],
        }),
        ..Readers::default()
    };
    let transport = DispatcherTransport::new(dispatcher(HelperHost::Linux, readers, tmp.path()), caller);
    let served = transport.served.clone();
    let centre = std::sync::Arc::new(helper_rig::centre_over(transport, HelperHost::Linux));
    let readiness = HelperReadiness::new(std::sync::Arc::new(OneCard), Box::new(CentrePort(centre)));
    let items = readiness.read().items;
    assert!(
        matches!(&items[0].state, ItemState::Problem { fix, .. } if fix.contains("ethtool -s enp3s0 wol g")),
        "{:?}",
        items[0]
    );
    assert!(served.lock().unwrap().iter().any(|k| k == "WolState"), "the helper was asked");
}

#[test]
fn the_owner_s_card_mode_comes_from_the_helper() {
    wol_mode_is_judged_through_the_helper(Caller::Owner);
}

#[test]
fn any_account_s_card_mode_comes_from_the_helper() {
    wol_mode_is_judged_through_the_helper(Caller::Other);
}

// ---- the rig and the helper's server agree on who may ask what ----

/// One request of every kind that matters to the table: refused to another account, counts only, or open.
fn sample_requests() -> Vec<dyno_core::helper_wire::HelperRequest> {
    use dyno_core::helper_wire::HelperRequest as R;
    vec![
        R::Info,
        R::Audit(5),
        R::LogTail(5),
        R::Uninstall,
        R::Firewall,
        R::MemoryModules,
        R::ListWakes,
        R::Traffic,
        R::WolState {
            ifname: dyno_core::wake::IfName::parse("eth0").unwrap(),
        },
    ]
}

#[test]
fn the_rig_refuses_exactly_what_the_helper_server_refuses() {
    use dyno_core::helper_policy::{access, Access, Standing};
    for caller in [Caller::Owner, Caller::Admin, Caller::Other] {
        for request in sample_requests() {
            // The server's own check, on the caller as the helper's identity code decides it.
            let server = dyno_helper::auth::authorize(&caller.peer(), &helper_rig::owner(), &request);
            let tmp = tempfile::tempdir().unwrap();
            let transport = DispatcherTransport::new(dispatcher(HelperHost::Linux, Readers::default(), tmp.path()), caller);
            let rig_refused = matches!(
                dyno_agent::helper_link::HelperTransport::call(&transport, request.clone()),
                Err(dyno_agent::helper_link::TransportError::Remote { ref code, .. }) if code == dyno_core::helper_wire::codes::REFUSED
            );
            assert_eq!(rig_refused, server.is_err(), "{caller:?} {:?}", request.kind());
            // And the table itself says the same for another account.
            if caller == Caller::Other {
                assert_eq!(
                    server.is_err(),
                    access(request.kind(), Standing::Other) == Access::Denied,
                    "{:?}",
                    request.kind()
                );
            }
        }
    }
}
