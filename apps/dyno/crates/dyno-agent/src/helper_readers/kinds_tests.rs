//! Every reading asks the helper only for the kinds its `KINDS` says (and asks for at least one): the registry of the seam
//! (`helper_first::wired_kinds_while`, `tests/helper_wiring.rs`) trusts `KINDS`, and this is where `KINDS` is held to what
//! `via_helper` really sends. The recording `HelperCalls` below says which kind each typed call is; a new typed call that is not
//! mapped here fails to compile, because the trait has no default methods.

use crate::diagnostics::HelperLog;
use crate::firewall::HelperFirewall;
use crate::helper_first::{HelperCalls, HelperReading, NOT_A_READING_KINDS};
use crate::helper_link::CallError;
use crate::helper_readers::counters::MacCounters;
use crate::helper_readers::extras::WakeRequests;
use crate::helper_readers::holders::{FileHolderReading, SocketOwnerReading};
use crate::helper_readers::login_items::LoginItems;
use crate::helper_readers::memory::MemoryModules;
use crate::helper_readers::processes::Sweeper;
use crate::helper_readers::traffic::ProgramTraffic;
use crate::helper_readers::wol::WolModes;
use crate::helper_wear::SmartWear;
use dyno_core::btm::LoginItemsReply;
use dyno_core::firewall::{FirewallSource, RuleSource};
use dyno_core::helper::HelperHost;
use dyno_core::helper_counters::CounterAnswer;
use dyno_core::helper_extras::ExtrasAnswer;
use dyno_core::helper_holders::{FileHolders, SocketOwners};
use dyno_core::helper_policy::RequestKind;
use dyno_core::helper_processes::ProcessPage;
use dyno_core::helper_requests::{CounterQuery, ExtrasKind, ProcessQuery, SmartQuery};
use dyno_core::inventory::{MemoryLayout, Unavailable};
use dyno_core::metrics::Reading;
use dyno_core::network_traffic::TrafficAnswer;
use dyno_core::ports::{DiskWearSource, FileLocks, PlatformError};
use dyno_core::smart::SmartReply;
use dyno_core::wake::{IfName, WolReply};
use std::sync::{Arc, Mutex};

/// `HelperCalls` that answers every call with a refusal and remembers which kind was asked.
#[derive(Default)]
struct Recorder(Mutex<Vec<RequestKind>>);

impl Recorder {
    fn asked(&self, kind: RequestKind) -> CallError {
        self.0.lock().unwrap().push(kind);
        CallError::from(PlatformError::Failed("recorded".into()))
    }
}

impl HelperCalls for Recorder {
    fn processes(&self, _: ProcessQuery) -> Result<ProcessPage, CallError> {
        Err(self.asked(RequestKind::Processes))
    }
    fn traffic(&self) -> Result<TrafficAnswer, CallError> {
        Err(self.asked(RequestKind::Traffic))
    }
    fn socket_owners(&self) -> Result<SocketOwners, CallError> {
        Err(self.asked(RequestKind::SocketOwners))
    }
    fn file_holders(&self, _: u64, _: u64) -> Result<FileHolders, CallError> {
        Err(self.asked(RequestKind::FileHolders))
    }
    fn counters(&self, _: CounterQuery) -> Result<Vec<CounterAnswer>, CallError> {
        Err(self.asked(RequestKind::Counters))
    }
    fn memory(&self) -> Result<MemoryLayout, CallError> {
        Err(self.asked(RequestKind::MemoryModules))
    }
    fn extras(&self, _: ExtrasKind) -> Result<ExtrasAnswer, CallError> {
        Err(self.asked(RequestKind::Extras))
    }
    fn login_items(&self) -> Result<LoginItemsReply, CallError> {
        Err(self.asked(RequestKind::LoginItems))
    }
    fn smart(&self, _: SmartQuery) -> Result<SmartReply, CallError> {
        Err(self.asked(RequestKind::Smart))
    }
    fn firewall(&self) -> Result<FirewallSource, CallError> {
        Err(self.asked(RequestKind::Firewall))
    }
    fn log_tail(&self, _: u32) -> Result<Vec<String>, CallError> {
        Err(self.asked(RequestKind::LogTail))
    }
    fn wol_state(&self, _: IfName) -> Result<WolReply, CallError> {
        Err(self.asked(RequestKind::WolState))
    }
}

struct NoWear;
impl DiskWearSource for NoWear {
    fn read(&mut self) -> Reading<Vec<dyno_core::hardware::DiskWear>> {
        Reading::Unavailable("none".into())
    }
}

struct NoLocks;
impl FileLocks for NoLocks {
    fn lockers(&mut self, _: &std::path::Path) -> Result<Vec<dyno_core::process::Locker>, PlatformError> {
        Ok(vec![])
    }
}

/// The kinds `via_helper` of `reading` asked for, and the kinds it declares.
fn asked<R: HelperReading>(name: &str, reading: R) -> (String, Vec<RequestKind>, &'static [RequestKind]) {
    let recorder = Recorder::default();
    let _ = reading.via_helper(&recorder);
    let asked = recorder.0.lock().unwrap().clone();
    (name.to_string(), asked, R::KINDS)
}

type Row = (String, Vec<RequestKind>, &'static [RequestKind]);

/// One row per `HelperReading` the agent has.
fn rows() -> Vec<Row> {
    let memory = |t| MemoryLayout::TotalOnly {
        total: t,
        reason: Unavailable::failed("x"),
    };
    vec![
        asked("HelperLog", HelperLog),
        asked("HelperFirewall", HelperFirewall::new(RuleSource::Pf)),
        asked("SmartWear", SmartWear::new(Box::new(NoWear), HelperHost::Linux)),
        asked("LoginItems", LoginItems),
        asked("WakeRequests", WakeRequests::new(Box::new(|| Ok(vec![])), |_| String::new())),
        asked("MemoryModules", MemoryModules::new(HelperHost::Linux, Box::new(memory))),
        asked("SocketOwnerReading", SocketOwnerReading),
        asked("FileHolderReading", FileHolderReading::new(Box::new(NoLocks))),
        asked("Sweeper", Sweeper::new(Arc::new(paddock_jobs::time::SystemClock))),
        asked("WolModes", WolModes::for_card(IfName::parse("eth0").unwrap())),
        asked("MacCounters", MacCounters { active: true }),
        asked("ProgramTraffic", ProgramTraffic::new(Box::new(dyno_platform::fake::Fake::new()))),
    ]
}

/// The request kinds that are readings but that no row of `rows` declares: a reading added without a row here would never be held
/// to what its `via_helper` sends.
fn kinds_without_a_row(rows: &[Row]) -> Vec<RequestKind> {
    RequestKind::ALL
        .into_iter()
        .filter(|k| !NOT_A_READING_KINDS.contains(k) && !rows.iter().any(|(_, _, declared)| declared.contains(k)))
        .collect()
}

#[test]
fn every_reading_kind_has_a_row_here() {
    let missing = kinds_without_a_row(&rows());
    assert!(
        missing.is_empty(),
        "{missing:?} are request kinds that are readings (not in NOT_A_READING_KINDS) but no row of rows() declares them; add the HelperReading \
         to rows(), or list the kind in helper_first::NOT_A_READING_KINDS if it is not a reading"
    );
}

#[test]
fn a_reading_missing_from_the_rows_is_named() {
    let without_wear: Vec<Row> = rows().into_iter().filter(|(name, _, _)| name != "SmartWear").collect();
    assert_eq!(kinds_without_a_row(&without_wear), vec![RequestKind::Smart]);
}

#[test]
fn every_reading_asks_only_for_the_kinds_it_declares_and_asks_for_at_least_one() {
    let rows = rows();
    let mut problems = Vec::new();
    for (name, asked, declared) in &rows {
        if asked.is_empty() {
            problems.push(format!(
                "{name}: via_helper asked the helper for nothing; its KINDS {declared:?} claim a wiring it does not have"
            ));
        }
        for kind in asked {
            if !declared.contains(kind) {
                problems.push(format!(
                    "{name}: via_helper asks for {kind:?}, which its KINDS {declared:?} do not list; add it to KINDS"
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
