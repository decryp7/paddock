#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The app-agent wire guard (ER-21). Dyno's IPC is postcard (`dyno-ipc`), which is not self-describing: it cannot carry serde
//! internally tagged enums (`#[serde(tag = "..")]`), adjacently tagged ones, `#[serde(flatten)]`, `#[serde(untagged)]` or
//! `skip_serializing_if`; such a type serialises but fails to decode, and only at run time, on the first message that uses it.
//!
//! This test round-trips a sample of EVERY variant of the wire enums through postcard (encode, decode, equal), so a type that
//! postcard cannot carry fails here with the type and variant named. The enums covered: [`Message`], [`Request`], [`Reply`],
//! [`ErrorCode`], [`LiveDetail`], [`JobSpec`], [`JobOutput`], [`HelperRequest`], [`HelperReply`], [`HelperMessage`].
//!
//! Adding a variant:
//! 1. the `tag_*` function of its enum stops compiling ("non-exhaustive patterns: `Request::Foo { .. }` not covered"): add its arm;
//! 2. add a sample for it in the enum's `*_samples()`;
//! 3. `every_variant_has_a_sample` compares the samples with the variants written in the source and names a missing one.
//!
//! This file is written against the types on `main`; a branch that adds variants adds its samples when it merges (the guard fails
//! the merge result until it does, which is the point).

use dyno_core::extras::files::FinderQuery;
use dyno_core::helper_wire::{HelperMessage, HelperReply, HelperRequest};
use dyno_core::history::HistoryRange;
use dyno_core::issues::IssuesQuery;
use dyno_core::jobs::{JobOutput, JobSpec};
use dyno_core::process::ProcessRef;
use dyno_core::protocol::*;
use dyno_core::settings::Settings;
use dyno_core::time::Timestamp;
use dyno_core::weblinks::HashSource;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::BTreeSet;
use std::fmt::Debug;

/// Encode and decode `v` with postcard; the error names the type and says what postcard cannot carry.
fn try_round<T: Serialize + DeserializeOwned>(v: &T) -> Result<T, String> {
    let ty = std::any::type_name::<T>();
    let bytes = postcard::to_stdvec(v).map_err(|e| format!("{ty}: postcard cannot encode it: {e}"))?;
    postcard::from_bytes(&bytes).map_err(|e| {
        format!("{ty}: postcard encodes it but cannot decode it ({e}); wire types must not use internally or adjacently tagged enums, flatten, untagged or skip_serializing_if")
    })
}

fn check<T: Serialize + DeserializeOwned + PartialEq + Debug>(what: &str, samples: &[T]) {
    for s in samples {
        match try_round(s) {
            Ok(back) => assert_eq!(&back, s, "{what}: decoded value differs from {s:?}"),
            Err(e) => panic!("{what}: {e} (sample {s:?})"),
        }
    }
}

/// A value of any wire type that has no `Default`: postcard decoding of zero bytes gives the first variant of every enum and empty or
/// zero fields everywhere. The richer samples below are written out; these stand in for payload types that are only wrapped.
fn zero<T: DeserializeOwned>() -> T {
    postcard::from_bytes(&[0u8; 4096]).unwrap_or_else(|e| {
        panic!(
            "cannot build a sample of {} from zero bytes ({e}): write the sample out by hand",
            std::any::type_name::<T>()
        )
    })
}

// ---- the compile-time guards: one exhaustive match per enum, no wildcard arms ----

fn tag_error_code(v: &ErrorCode) -> &'static str {
    match v {
        ErrorCode::AgentUnavailable => "AgentUnavailable",
        ErrorCode::NotFound => "NotFound",
        ErrorCode::AccessDenied => "AccessDenied",
        ErrorCode::Unsupported => "Unsupported",
        ErrorCode::BadRequest => "BadRequest",
        ErrorCode::Timeout => "Timeout",
        ErrorCode::Busy => "Busy",
        ErrorCode::Internal => "Internal",
        ErrorCode::NeedsConsent => "NeedsConsent",
        ErrorCode::Stale => "Stale",
    }
}

fn tag_live_detail(v: &LiveDetail) -> &'static str {
    match v {
        LiveDetail::Summary => "Summary",
        LiveDetail::Processes => "Processes",
        LiveDetail::Tray => "Tray",
    }
}

fn tag_message(v: &Message) -> &'static str {
    match v {
        Message::Request(_) => "Request",
        Message::Reply(_) => "Reply",
        Message::Live(_) => "Live",
        Message::Alert(_) => "Alert",
        Message::Activity(_) => "Activity",
        Message::Alarm(_) => "Alarm",
        Message::DiskScan(_) => "DiskScan",
        Message::DiskVolumes(_) => "DiskVolumes",
        Message::Job(_) => "Job",
        Message::WidgetLayoutChanged(_) => "WidgetLayoutChanged",
        Message::PowerChanged(_) => "PowerChanged",
        Message::WolChanged(_) => "WolChanged",
        Message::WakesChanged(_) => "WakesChanged",
    }
}

fn tag_job_spec(v: &JobSpec) -> &'static str {
    match v {
        JobSpec::FindFiles { .. } => "FindFiles",
        JobSpec::RescanIssues { .. } => "RescanIssues",
        JobSpec::RefreshBackup => "RefreshBackup",
        JobSpec::FileHash { .. } => "FileHash",
        JobSpec::SleepBlockers => "SleepBlockers",
        JobSpec::BootAnalysis => "BootAnalysis",
        JobSpec::AppUsage => "AppUsage",
        JobSpec::SoftwareInventory => "SoftwareInventory",
        JobSpec::Helper { .. } => "Helper",
        JobSpec::WolWake { .. } => "WolWake",
        JobSpec::UpdateList { .. } => "UpdateList",
        JobSpec::UpdateGeo => "UpdateGeo",
        JobSpec::RebuildIntel => "RebuildIntel",
        JobSpec::InstallTool { .. } => "InstallTool",
    }
}

fn tag_job_output(v: &JobOutput) -> &'static str {
    match v {
        JobOutput::Finder(_) => "Finder",
        JobOutput::Issues(_) => "Issues",
        JobOutput::Backup(_) => "Backup",
        JobOutput::FileHash(_) => "FileHash",
        JobOutput::Sleep(_) => "Sleep",
        JobOutput::Boot(_) => "Boot",
        JobOutput::AppUsage(_) => "AppUsage",
        JobOutput::Software(_) => "Software",
        JobOutput::Helper(_) => "Helper",
        JobOutput::Wol(_) => "Wol",
        JobOutput::Intel(_) => "Intel",
        JobOutput::Tool(_) => "Tool",
    }
}

fn tag_helper_request(v: &HelperRequest) -> &'static str {
    match v {
        HelperRequest::Info => "Info",
        HelperRequest::Traffic => "Traffic",
        HelperRequest::Audit(_) => "Audit",
        HelperRequest::Uninstall => "Uninstall",
        HelperRequest::Processes(_) => "Processes",
        HelperRequest::SocketOwners => "SocketOwners",
        HelperRequest::FileHolders { .. } => "FileHolders",
        HelperRequest::LoginItems => "LoginItems",
        HelperRequest::Counters(_) => "Counters",
        HelperRequest::MemoryModules => "MemoryModules",
        HelperRequest::Smart(_) => "Smart",
        HelperRequest::InstallTool { .. } => "InstallTool",
        HelperRequest::Firewall => "Firewall",
        HelperRequest::Extras(_) => "Extras",
        HelperRequest::LogTail(_) => "LogTail",
        HelperRequest::ScheduleWake { .. } => "ScheduleWake",
        HelperRequest::CancelWake { .. } => "CancelWake",
        HelperRequest::ListWakes => "ListWakes",
        HelperRequest::WolState { .. } => "WolState",
    }
}

fn tag_helper_reply(v: &HelperReply) -> &'static str {
    match v {
        HelperReply::About(..) => "About",
        HelperReply::Counters(_) => "Counters",
        HelperReply::Entries(_) => "Entries",
        HelperReply::Bye => "Bye",
        HelperReply::Firewall(_) => "Firewall",
        HelperReply::Wakes(_) => "Wakes",
        HelperReply::WakeCancelled(_) => "WakeCancelled",
        HelperReply::Wol(_) => "Wol",
        HelperReply::CounterValues(_) => "CounterValues",
        HelperReply::Memory(_) => "Memory",
        HelperReply::SocketOwners(_) => "SocketOwners",
        HelperReply::FileHolders(_) => "FileHolders",
        HelperReply::Extras(_) => "Extras",
        HelperReply::Processes(_) => "Processes",
        HelperReply::Smart(_) => "Smart",
        HelperReply::Installed(_) => "Installed",
        HelperReply::LoginItems(_) => "LoginItems",
        HelperReply::Log(_) => "Log",
    }
}

fn tag_helper_message(v: &HelperMessage) -> &'static str {
    match v {
        HelperMessage::Request(_) => "Request",
        HelperMessage::Reply(_) => "Reply",
    }
}

fn tag_request(v: &Request) -> &'static str {
    match v {
        Request::Subscribe { .. } => "Subscribe",
        Request::ProcessDetails { .. } => "ProcessDetails",
        Request::Lockers { .. } => "Lockers",
        Request::OpenLocation { .. } => "OpenLocation",
        Request::TakeSnapshot { .. } => "TakeSnapshot",
        Request::Snapshots => "Snapshots",
        Request::DiffSnapshots { .. } => "DiffSnapshots",
        Request::WhySlow => "WhySlow",
        Request::Anomalies => "Anomalies",
        Request::History { .. } => "History",
        Request::Activity { .. } => "Activity",
        Request::Health => "Health",
        Request::StartupEntries => "StartupEntries",
        Request::WeeklyReport { .. } => "WeeklyReport",
        Request::Alerts { .. } => "Alerts",
        Request::Settings => "Settings",
        Request::SaveSettings { .. } => "SaveSettings",
        Request::Plugins => "Plugins",
        Request::Shutdown => "Shutdown",
        Request::Inventory => "Inventory",
        Request::Alarms { .. } => "Alarms",
        Request::AlarmStats => "AlarmStats",
        Request::AcknowledgeAlarms { .. } => "AcknowledgeAlarms",
        Request::ShelveAlarm { .. } => "ShelveAlarm",
        Request::UnshelveAlarm { .. } => "UnshelveAlarm",
        Request::MuteRule { .. } => "MuteRule",
        Request::NoteAlarm { .. } => "NoteAlarm",
        Request::AlarmHistory { .. } => "AlarmHistory",
        Request::Capture => "Capture",
        Request::RecorderSnapshots => "RecorderSnapshots",
        Request::RecorderSnapshot { .. } => "RecorderSnapshot",
        Request::DiagnosticsPreview => "DiagnosticsPreview",
        Request::SaveDiagnostics { .. } => "SaveDiagnostics",
        Request::Network { .. } => "Network",
        Request::LookupIp { .. } => "LookupIp",
        Request::Benchmarks => "Benchmarks",
        Request::StartBenchmark { .. } => "StartBenchmark",
        Request::CancelBenchmark => "CancelBenchmark",
        Request::DeleteBenchmark { .. } => "DeleteBenchmark",
        Request::CompareBenchmarks { .. } => "CompareBenchmarks",
        Request::ResolveHosts { .. } => "ResolveHosts",
        Request::SaveReport { .. } => "SaveReport",
        Request::CounterObjects => "CounterObjects",
        Request::CounterObject { .. } => "CounterObject",
        Request::CounterSearch { .. } => "CounterSearch",
        Request::CounterValues { .. } => "CounterValues",
        Request::CounterHistory { .. } => "CounterHistory",
        Request::Throttling => "Throttling",
        Request::Lifespan => "Lifespan",
        Request::Baselines => "Baselines",
        Request::Issues { .. } => "Issues",
        Request::SaveIssues { .. } => "SaveIssues",
        Request::Unusual => "Unusual",
        Request::ForgetBaseline => "ForgetBaseline",
        Request::SearchSeed { .. } => "SearchSeed",
        Request::WebSearch { .. } => "WebSearch",
        Request::VirusTotalUrl { .. } => "VirusTotalUrl",
        Request::StartupOverview { .. } => "StartupOverview",
        Request::Changes { .. } => "Changes",
        Request::GpuProcesses => "GpuProcesses",
        Request::SaveInventoryReport { .. } => "SaveInventoryReport",
        Request::Maintenance => "Maintenance",
        Request::Backup => "Backup",
        Request::StartJob { .. } => "StartJob",
        Request::Jobs => "Jobs",
        Request::CancelJob { .. } => "CancelJob",
        Request::JobResult { .. } => "JobResult",
        Request::DiskVolumes => "DiskVolumes",
        Request::StartDiskScan { .. } => "StartDiskScan",
        Request::ForgetSavedDiskMap { .. } => "ForgetSavedDiskMap",
        Request::ForgetSavedDiskMaps => "ForgetSavedDiskMaps",
        Request::HelperStatus => "HelperStatus",
        Request::PowerState => "PowerState",
        Request::PowerWakes => "PowerWakes",
        Request::PowerSetWake { .. } => "PowerSetWake",
        Request::PowerCancelWake { .. } => "PowerCancelWake",
        Request::WolDevices => "WolDevices",
        Request::WolCandidates => "WolCandidates",
        Request::WolSave { .. } => "WolSave",
        Request::WolRemove { .. } => "WolRemove",
        Request::WolWake { .. } => "WolWake",
        Request::WolReadiness => "WolReadiness",
        Request::PowerKeepAwake { .. } => "PowerKeepAwake",
        Request::PowerStopKeepAwake => "PowerStopKeepAwake",
        Request::PowerIdleCancel => "PowerIdleCancel",
        Request::PowerIdleSnooze { .. } => "PowerIdleSnooze",
        Request::EnergyReport { .. } => "EnergyReport",
        Request::SaveBenchmarks { .. } => "SaveBenchmarks",
        Request::BenchGpuAdapters => "BenchGpuAdapters",
        Request::BenchEstimate { .. } => "BenchEstimate",
        Request::IntelStatus => "IntelStatus",
        Request::IntelEdit { .. } => "IntelEdit",
        Request::GeoSummary => "GeoSummary",
        Request::Firewall => "Firewall",
        Request::ToolOffer => "ToolOffer",
        Request::RemoveTool => "RemoveTool",
        Request::CancelDiskScan { .. } => "CancelDiskScan",
        Request::DiskScans => "DiskScans",
        Request::DiskChildren { .. } => "DiskChildren",
        Request::DiskPath { .. } => "DiskPath",
        Request::DiskRings { .. } => "DiskRings",
        Request::RetryDiskScan { .. } => "RetryDiskScan",
        Request::OpenFullDiskAccessSettings => "OpenFullDiskAccessSettings",
        Request::DiskReveal { .. } => "DiskReveal",
        Request::UpsertAlertRule { .. } => "UpsertAlertRule",
        Request::DeleteAlertRule { .. } => "DeleteAlertRule",
        Request::WidgetLayout { .. } => "WidgetLayout",
        Request::EditWidgetLayout { .. } => "EditWidgetLayout",
        Request::PopoverValues { .. } => "PopoverValues",
    }
}

fn tag_reply(v: &Reply) -> &'static str {
    match v {
        Reply::Ok => "Ok",
        Reply::Details(_) => "Details",
        Reply::Activity(_) => "Activity",
        Reply::Activities(_) => "Activities",
        Reply::Lockers(_) => "Lockers",
        Reply::Snapshot(_) => "Snapshot",
        Reply::Snapshots(_) => "Snapshots",
        Reply::Diff(_) => "Diff",
        Reply::Diagnosis(_) => "Diagnosis",
        Reply::Anomalies(_) => "Anomalies",
        Reply::History(_) => "History",
        Reply::Health(_) => "Health",
        Reply::Startup(_) => "Startup",
        Reply::Report(_) => "Report",
        Reply::Alerts(_) => "Alerts",
        Reply::Settings(_) => "Settings",
        Reply::Plugins(_) => "Plugins",
        Reply::Inventory(_) => "Inventory",
        Reply::Alarms(_) => "Alarms",
        Reply::AlarmStats(_) => "AlarmStats",
        Reply::AlarmChanges(_) => "AlarmChanges",
        Reply::Captured(_) => "Captured",
        Reply::RecorderSnapshots(_) => "RecorderSnapshots",
        Reply::RecorderSnapshot(_) => "RecorderSnapshot",
        Reply::DiagnosticsPreview(_) => "DiagnosticsPreview",
        Reply::DiagnosticsSaved(_) => "DiagnosticsSaved",
        Reply::Url(_) => "Url",
        Reply::Network(_) => "Network",
        Reply::Benchmarks(_) => "Benchmarks",
        Reply::BenchComparison(_) => "BenchComparison",
        Reply::HostNames(_) => "HostNames",
        Reply::ReportSaved(_) => "ReportSaved",
        Reply::CounterObjects(_) => "CounterObjects",
        Reply::CounterObject(_) => "CounterObject",
        Reply::CounterHits(_) => "CounterHits",
        Reply::CounterValues(_) => "CounterValues",
        Reply::CounterHistory(_) => "CounterHistory",
        Reply::Throttling(_) => "Throttling",
        Reply::Lifespan(_) => "Lifespan",
        Reply::Baselines(_) => "Baselines",
        Reply::Issues(_) => "Issues",
        Reply::IssuesSaved(_) => "IssuesSaved",
        Reply::Unusual(_) => "Unusual",
        Reply::SearchSeed(_) => "SearchSeed",
        Reply::StartupOverview(_) => "StartupOverview",
        Reply::Changes(_) => "Changes",
        Reply::Gpu(_) => "Gpu",
        Reply::InventorySaved(_) => "InventorySaved",
        Reply::Maintenance(_) => "Maintenance",
        Reply::Backup(_) => "Backup",
        Reply::DiskVolumes(_) => "DiskVolumes",
        Reply::DiskScan(_) => "DiskScan",
        Reply::DiskScans(_) => "DiskScans",
        Reply::DiskChildren(_) => "DiskChildren",
        Reply::DiskPath(_) => "DiskPath",
        Reply::DiskRings(_) => "DiskRings",
        Reply::Job(_) => "Job",
        Reply::Jobs(_) => "Jobs",
        Reply::JobCancelled(_) => "JobCancelled",
        Reply::JobOutput(_) => "JobOutput",
        Reply::WidgetLayout(_) => "WidgetLayout",
        Reply::PopoverValues(_) => "PopoverValues",
        Reply::HelperStatus(_) => "HelperStatus",
        Reply::Power(_) => "Power",
        Reply::PowerWakes(_) => "PowerWakes",
        Reply::Wol(_) => "Wol",
        Reply::WolCandidates(_) => "WolCandidates",
        Reply::WolReadiness(_) => "WolReadiness",
        Reply::Energy(_) => "Energy",
        Reply::GpuAdapters(_) => "GpuAdapters",
        Reply::BenchEstimate(_) => "BenchEstimate",
        Reply::Intel(_) => "Intel",
        Reply::IntelEdited(_) => "IntelEdited",
        Reply::Geo(_) => "Geo",
        Reply::Firewall(_) => "Firewall",
        Reply::ToolOffer(_) => "ToolOffer",
    }
}

// ---- the samples: one value per variant, with the payload filled where it can be ----

fn error_code_samples() -> Vec<ErrorCode> {
    use ErrorCode::*;
    vec![
        AgentUnavailable,
        NotFound,
        AccessDenied,
        Unsupported,
        BadRequest,
        Timeout,
        Busy,
        Internal,
        NeedsConsent,
        Stale,
    ]
}

fn live_detail_samples() -> Vec<LiveDetail> {
    vec![LiveDetail::Summary, LiveDetail::Processes, LiveDetail::Tray]
}

fn process_ref() -> ProcessRef {
    ProcessRef {
        pid: 42,
        started: Some(Timestamp(7)),
    }
}

fn job_spec_samples() -> Vec<JobSpec> {
    vec![
        JobSpec::FindFiles {
            query: FinderQuery {
                folder: "/x".into(),
                mode: Default::default(),
                min_bytes: 1,
            },
        },
        JobSpec::RescanIssues {
            query: IssuesQuery::default(),
        },
        JobSpec::RefreshBackup,
        JobSpec::FileHash {
            source: HashSource::Process(process_ref()),
        },
        JobSpec::SleepBlockers,
        JobSpec::BootAnalysis,
        JobSpec::AppUsage,
        JobSpec::SoftwareInventory,
        JobSpec::Helper {
            action: dyno_core::helper_wire::HelperAction::TurnOn,
        },
        JobSpec::UpdateList { id: "list-1".into() },
        JobSpec::UpdateGeo,
        JobSpec::WolWake { id: 1, test: true },
        JobSpec::RebuildIntel,
        JobSpec::InstallTool { tool: zero() },
    ]
}

fn job_output_samples() -> Vec<JobOutput> {
    vec![
        JobOutput::Finder(zero()),
        JobOutput::Issues(zero()),
        JobOutput::Backup(zero()),
        JobOutput::FileHash(zero()),
        JobOutput::Sleep(zero()),
        JobOutput::Boot(zero()),
        JobOutput::AppUsage(zero()),
        JobOutput::Software(zero()),
        JobOutput::Helper(Box::new(zero())),
        JobOutput::Wol(Box::new(zero())),
        JobOutput::Intel(Box::default()),
        JobOutput::Tool(Box::new(zero())),
    ]
}

fn helper_request_samples() -> Vec<HelperRequest> {
    vec![
        HelperRequest::Info,
        HelperRequest::Traffic,
        HelperRequest::Audit(25),
        HelperRequest::Uninstall,
        HelperRequest::Processes(zero()),
        HelperRequest::SocketOwners,
        HelperRequest::FileHolders { dev: 7, ino: 9 },
        HelperRequest::LoginItems,
        HelperRequest::Counters(zero()),
        HelperRequest::MemoryModules,
        HelperRequest::Smart(zero()),
        HelperRequest::InstallTool {
            tool: zero(),
            bytes: vec![1, 2, 3],
        },
        HelperRequest::Firewall,
        HelperRequest::Extras(zero()),
        HelperRequest::LogTail(50),
        HelperRequest::ScheduleWake {
            tag: dyno_core::wake::WakeTag::parse("nightly-1").unwrap(),
            at_utc: 1_700_000_000,
        },
        HelperRequest::CancelWake {
            tag: dyno_core::wake::WakeTag::parse("nightly-1").unwrap(),
        },
        HelperRequest::ListWakes,
        HelperRequest::WolState {
            ifname: dyno_core::wake::IfName::parse("eth0").unwrap(),
        },
    ]
}

fn helper_reply_samples() -> Vec<HelperReply> {
    vec![
        HelperReply::About(zero(), zero()),
        HelperReply::Counters(vec![]),
        HelperReply::Entries(vec![]),
        HelperReply::Bye,
        HelperReply::Firewall(zero()),
        HelperReply::Wakes(vec![]),
        HelperReply::WakeCancelled(true),
        HelperReply::Wol(zero()),
        HelperReply::CounterValues(vec![]),
        HelperReply::Memory(zero()),
        HelperReply::SocketOwners(zero()),
        HelperReply::FileHolders(zero()),
        HelperReply::Extras(zero()),
        HelperReply::Processes(zero()),
        HelperReply::Smart(zero()),
        HelperReply::Installed(zero()),
        HelperReply::LoginItems(zero()),
        HelperReply::Log(vec!["a line".into()]),
    ]
}

fn helper_message_samples() -> Vec<HelperMessage> {
    vec![
        HelperMessage::Request(HelperRequest::Audit(3)),
        HelperMessage::Reply(HelperReply::Bye),
    ]
}

fn request_samples() -> Vec<Request> {
    use dyno_core::{bench, disk::scan::DiskTarget, extras, report, widgets};
    let s = |t: &str| t.to_string();
    vec![
        Request::Subscribe {
            detail: LiveDetail::Processes,
        },
        Request::ProcessDetails { target: process_ref() },
        Request::Lockers { path: s("/x") },
        Request::OpenLocation { target: process_ref() },
        Request::TakeSnapshot { label: s("before") },
        Request::Snapshots,
        Request::DiffSnapshots {
            before: s("a"),
            after: s("b"),
        },
        Request::WhySlow,
        Request::Anomalies,
        Request::History {
            range: HistoryRange::Week,
            end: Some(Timestamp(9)),
        },
        Request::Activity { limit: 5 },
        Request::Health,
        Request::StartupEntries,
        Request::WeeklyReport { weeks_ago: 2 },
        Request::Alerts { limit: 5 },
        Request::Settings,
        Request::SaveSettings { settings: Box::default() },
        Request::Plugins,
        Request::Shutdown,
        Request::Inventory,
        Request::Alarms {
            filter: Default::default(),
        },
        Request::AlarmStats,
        Request::AcknowledgeAlarms { ids: vec![1, 2] },
        Request::ShelveAlarm {
            id: 1,
            until: dyno_core::alarms::ShelveFor::OneHour,
        },
        Request::UnshelveAlarm { id: 1 },
        Request::MuteRule {
            rule: s("cpu"),
            muted: true,
        },
        Request::NoteAlarm { id: 1, text: s("n") },
        Request::AlarmHistory {
            alarm_id: Some(1),
            limit: 10,
        },
        Request::Capture,
        Request::RecorderSnapshots,
        Request::RecorderSnapshot { id: 3 },
        Request::DiagnosticsPreview,
        Request::SaveDiagnostics { path: s("/d.zip") },
        Request::Network { query: Default::default() },
        Request::LookupIp {
            ip: s("192.0.2.1"),
            template: Some(s("t")),
        },
        Request::Benchmarks,
        Request::StartBenchmark {
            kinds: vec![bench::BenchKind::Cpu],
            label: s("l"),
            group: Some(s("g")),
            phase: Some(bench::BenchPhase::Before),
            disk_mib: Some(64),
            allow_battery: true,
            disk_volumes: vec![s("/")],
            allow_network_drives: true,
            disk_folders: vec![],
            gpu_adapter: Some(1),
            sustained_minutes: Some(5),
            sustained_write_gib: Some(2),
        },
        Request::SaveBenchmarks {
            path: s("/b.json"),
            format: bench::export::ExportFormat::Json,
            ids: vec![s("b1")],
        },
        Request::BenchGpuAdapters,
        Request::BenchEstimate {
            request: bench::estimate::EstimateRequest {
                kinds: vec![bench::BenchKind::Cpu],
                disk_mib: Some(64),
                drives: 1,
                sustained_minutes: None,
                sustained_write_gib: None,
            },
        },
        Request::CancelBenchmark,
        Request::DeleteBenchmark { id: s("b1") },
        Request::CompareBenchmarks {
            before: s("a"),
            after: s("b"),
        },
        Request::ResolveHosts {
            addresses: vec![s("192.0.2.1")],
        },
        Request::SaveReport {
            path: s("/r.md"),
            format: report::ReportFormat::Markdown,
            weeks_ago: 1,
        },
        Request::CounterObjects,
        Request::CounterObject { name: s("Memory") },
        Request::CounterSearch {
            query: s("pages"),
            limit: 20,
        },
        Request::CounterValues { paths: vec![s("\\a\\b")] },
        Request::CounterHistory {
            paths: vec![s("\\a\\b")],
            minutes: 5,
        },
        Request::Throttling,
        Request::Lifespan,
        Request::Baselines,
        Request::Issues {
            query: IssuesQuery::default(),
        },
        Request::SaveIssues {
            path: s("/i.jsonl"),
            days: 7,
        },
        Request::Unusual,
        Request::ForgetBaseline,
        Request::SearchSeed { text: s("x") },
        Request::WebSearch { query: s("q") },
        Request::VirusTotalUrl { sha256: "0".repeat(64) },
        Request::StartupOverview {
            filter: Default::default(),
        },
        Request::Changes { query: Default::default() },
        Request::GpuProcesses,
        Request::SaveInventoryReport {
            path: s("/i.md"),
            format: extras::software::InventoryFormat::Markdown,
        },
        Request::Maintenance,
        Request::Backup,
        Request::StartJob {
            spec: JobSpec::SoftwareInventory,
        },
        Request::Jobs,
        Request::CancelJob { id: 4 },
        Request::JobResult { id: 4 },
        Request::DiskVolumes,
        Request::StartDiskScan {
            target: DiskTarget::Folder { path: s("/x") },
            replaces: Some(1),
        },
        Request::ForgetSavedDiskMap { scan: 1 },
        Request::ForgetSavedDiskMaps,
        Request::HelperStatus,
        Request::PowerState,
        Request::PowerWakes,
        Request::PowerSetWake { entry: zero() },
        Request::PowerCancelWake { id: 2 },
        Request::WolDevices,
        Request::WolCandidates,
        Request::WolSave { device: zero() },
        Request::WolRemove { id: 1 },
        Request::WolWake { id: 1, test: false },
        Request::WolReadiness,
        Request::PowerKeepAwake { spec: zero() },
        Request::PowerStopKeepAwake,
        Request::PowerIdleCancel,
        Request::PowerIdleSnooze { minutes: 5 },
        Request::EnergyReport { days: 30 },
        Request::IntelStatus,
        Request::IntelEdit { edit: zero() },
        Request::GeoSummary,
        Request::Firewall,
        Request::ToolOffer,
        Request::RemoveTool,
        Request::CancelDiskScan { scan: 1 },
        Request::DiskScans,
        Request::DiskChildren {
            scan: 1,
            node: Some(2),
            offset: 0,
            limit: 48,
            min_fraction: 0.004,
        },
        Request::DiskPath { scan: 1, node: 2 },
        Request::DiskRings {
            scan: 1,
            node: Some(2),
            rings: 3,
            min_share: 0.01,
            max_nodes: 500,
        },
        Request::RetryDiskScan { scan: 1 },
        Request::OpenFullDiskAccessSettings,
        Request::DiskReveal { scan: 1, node: 2 },
        Request::UpsertAlertRule { rule: zero() },
        Request::DeleteAlertRule { id: s("r1") },
        Request::WidgetLayout {
            surface: widgets::DynoSurface::MenuBar,
        },
        Request::EditWidgetLayout {
            edit: widgets::SurfaceEdit::Popover(paddock_widgets::Edit::ResetToDefaults),
            base_revision: Some(3),
        },
        Request::PopoverValues {
            placements: vec![paddock_widgets::PlacementId::new("disk-space").unwrap()],
        },
    ]
}

fn disk_status() -> dyno_core::disk::scan::DiskScanStatus {
    use dyno_core::disk::scan::*;
    DiskScanStatus {
        scan: 1,
        seq: 1,
        name: "Macintosh HD".into(),
        target: DiskTarget::Folder { path: "/x".into() },
        volume: None,
        state: ScanState::Running,
        phase: ScanPhase::ReadingFolders,
        progress: ScanProgress::default(),
        truncated: false,
        unreadable_first: vec![],
        needs_full_disk_access: false,
        items_per_second: Some(10),
        eta_seconds: None,
        hidden_bytes: None,
        scanned_at: None,
        origin: ScanOrigin::Saved { saved_at: Timestamp(3) },
        changed_bytes: Some(-5),
        replaces: Some(1),
        not_saved: Some(NotSaved::Failed),
    }
}

fn wire_job() -> paddock_jobs::wire::WireSnapshot {
    paddock_jobs::wire::WireSnapshot {
        id: 1,
        kind: "find-files".into(),
        title: "Finding files".into(),
        subject: Some("/x".into()),
        state: paddock_jobs::wire::WireState::Queued,
        seq: 5,
        elapsed_ms: 1200,
        eta_ms: None,
        cancellable: true,
        not_cancellable_reason: None,
    }
}

fn reply_samples() -> Vec<Reply> {
    use dyno_core::{counters, diagnostics, extras, issues, recorder, report, widgets};
    let index = dyno_core::disk::index::DiskIndex::new("r", dyno_core::disk::index::ScanLimits::default());
    vec![
        Reply::Ok,
        Reply::Details(zero()),
        Reply::Activity(zero()),
        Reply::Activities(vec![]),
        Reply::Lockers(vec![]),
        Reply::Snapshot(zero()),
        Reply::Snapshots(vec![]),
        Reply::Diff(zero()),
        Reply::Diagnosis(zero()),
        Reply::Anomalies(vec![]),
        Reply::History(zero()),
        Reply::Health(zero()),
        Reply::Startup(vec![]),
        Reply::Report(zero()),
        Reply::Alerts(vec![]),
        Reply::Settings(Box::default()),
        Reply::Plugins(vec![]),
        Reply::Inventory(Box::new(dyno_core::inventory::SystemInventory::new(
            String::new(),
            None,
            zero(),
            dyno_core::inventory::MemoryLayout::unified(1, zero(), None),
            vec![],
        ))),
        Reply::Alarms(vec![]),
        Reply::AlarmStats(zero()),
        Reply::AlarmChanges(vec![]),
        Reply::Captured(zero()),
        Reply::RecorderSnapshots(vec![]),
        Reply::RecorderSnapshot(Box::new(recorder::RecorderSnapshot::new(
            Timestamp(0),
            recorder::CaptureReason::Manual,
            vec![],
        ))),
        Reply::DiagnosticsPreview(zero()),
        Reply::DiagnosticsSaved(diagnostics::SavedDiagnostics {
            path: "/x.zip".into(),
            bytes: 1,
            files: 1,
        }),
        Reply::Url("https://example.com/".into()),
        Reply::Network(zero()),
        Reply::Benchmarks(zero()),
        Reply::BenchComparison(zero()),
        Reply::HostNames(vec![]),
        Reply::ReportSaved(report::SavedReport {
            path: "/x.md".into(),
            bytes: 1,
        }),
        Reply::CounterObjects(zero()),
        Reply::CounterObject(Box::new(counters::CounterObject {
            name: "Memory".into(),
            counters: vec![],
            instances: vec![],
            instances_total: 0,
        })),
        Reply::CounterHits(vec![]),
        Reply::CounterValues(vec![]),
        Reply::CounterHistory(zero()),
        Reply::Throttling(zero()),
        Reply::Lifespan(zero()),
        Reply::Baselines(zero()),
        Reply::Issues(zero()),
        Reply::IssuesSaved(issues::SavedIssues {
            path: "/x.jsonl".into(),
            bytes: 1,
            count: 1,
        }),
        Reply::Unusual(zero()),
        Reply::SearchSeed(dyno_core::weblinks::SearchSeed {
            text: "t".into(),
            removed: vec!["1 IP address".into()],
        }),
        Reply::StartupOverview(zero()),
        Reply::Changes(zero()),
        Reply::Gpu(zero()),
        Reply::InventorySaved(SavedInventory {
            path: "/x.md".into(),
            bytes: 1,
        }),
        Reply::Maintenance(extras::maintenance::MaintenanceSettings::default().status(Timestamp(0), &dyno_core::time::FixedZone::UTC)),
        Reply::Backup(zero()),
        Reply::DiskVolumes(vec![]),
        Reply::DiskScan(Box::new(disk_status())),
        Reply::DiskScans(vec![disk_status()]),
        Reply::DiskChildren(Box::new(index.children(index.root(), 0, 10, 0.0).unwrap())),
        Reply::DiskPath("/x".into()),
        Reply::DiskRings(Box::new(index.rings(index.root(), 2, 0.0, 10).unwrap())),
        Reply::Job(Box::new(wire_job())),
        Reply::Jobs(vec![wire_job()]),
        Reply::JobCancelled(true),
        Reply::JobOutput(Box::new(JobOutput::Backup(zero()))),
        Reply::WidgetLayout(Box::new(widgets::layout_view(&Settings::default(), widgets::DynoSurface::Popover))),
        Reply::PopoverValues(widgets::JsonOnLink(vec![])),
        Reply::HelperStatus(Box::new(zero())),
        Reply::Power(Box::default()),
        Reply::PowerWakes(Box::new(zero())),
        Reply::Wol(Box::new(zero())),
        Reply::WolCandidates(vec![]),
        Reply::WolReadiness(Box::new(zero())),
        Reply::Energy(Box::new(zero())),
        Reply::GpuAdapters(vec![]),
        Reply::BenchEstimate(None),
        Reply::Intel(Box::default()),
        Reply::IntelEdited(Box::new(zero())),
        Reply::Geo(Box::default()),
        Reply::Firewall(Box::new(zero())),
        Reply::ToolOffer(Box::new(zero())),
    ]
}

fn message_samples() -> Vec<Message> {
    vec![
        Message::Request(Box::new(Request::Health)),
        Message::Reply(Box::new(Ok(Reply::Ok))),
        Message::Reply(Box::new(Err(AgentError::new(ErrorCode::Busy, "busy")))),
        Message::Live(Box::new(LiveUpdate {
            system: zero(),
            pressure: zero(),
            top_cpu: vec![],
            top_memory: vec![],
            overhead: Overhead::default(),
            processes: None,
            groups: Some(vec![]),
            other_users: None,
        })),
        Message::Alert(zero()),
        Message::Activity(zero()),
        Message::Alarm(zero()),
        Message::DiskScan(Box::new(disk_status())),
        Message::DiskVolumes(vec![]),
        Message::Job(Box::new(wire_job())),
        Message::WidgetLayoutChanged(dyno_core::widgets::DynoSurface::Popover),
        Message::PowerChanged(Box::default()),
        Message::WolChanged(Box::new(zero())),
        Message::WakesChanged(Box::new(zero())),
    ]
}

// ---- the tests ----

#[test]
fn every_sample_survives_postcard() {
    check("ErrorCode", &error_code_samples());
    check("LiveDetail", &live_detail_samples());
    check("JobSpec", &job_spec_samples());
    check("JobOutput", &job_output_samples());
    check("HelperRequest", &helper_request_samples());
    check("HelperReply", &helper_reply_samples());
    check("HelperMessage", &helper_message_samples());
    check("Request", &request_samples());
    check("Reply", &reply_samples());
    check("Message", &message_samples());
}

/// The variant names written in `source` for `pub enum <name>`: the lines at four spaces of indent that start a variant.
fn source_variants(source: &str, name: &str) -> BTreeSet<String> {
    let head = format!("pub enum {name} {{");
    let start = source
        .find(&head)
        .unwrap_or_else(|| panic!("`{head}` is not in the source any more: update this test"));
    let body = &source[start + head.len()..];
    let end = body.find("\n}\n").expect("the enum has no end");
    body[..end]
        .lines()
        .filter(|l| l.starts_with("    ") && !l.starts_with("     "))
        .map(|l| l.trim())
        .filter(|l| l.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
        .map(|l| l.split(|c: char| !c.is_alphanumeric()).next().unwrap().to_string())
        .collect()
}

fn assert_all_sampled(ty: &str, source: &str, tags: Vec<&'static str>) {
    let sampled: BTreeSet<String> = tags.iter().map(|t| t.to_string()).collect();
    let written = source_variants(source, ty);
    let missing: Vec<_> = written.difference(&sampled).collect();
    let stale: Vec<_> = sampled.difference(&written).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "wire type `{ty}`: variants without a sample in tests/wire_roundtrip.rs: {missing:?}; samples of variants that no longer exist: {stale:?}. \
         Add a sample for each new variant (and its arm in `tag_*`) so postcard is checked on it."
    );
}

#[test]
fn every_variant_has_a_sample() {
    let protocol = include_str!("../src/protocol.rs");
    let jobs = include_str!("../src/jobs.rs");
    let helper = include_str!("../src/helper_wire.rs");
    assert_all_sampled("ErrorCode", protocol, error_code_samples().iter().map(tag_error_code).collect());
    assert_all_sampled("LiveDetail", protocol, live_detail_samples().iter().map(tag_live_detail).collect());
    assert_all_sampled("Request", protocol, request_samples().iter().map(tag_request).collect());
    assert_all_sampled("Reply", protocol, reply_samples().iter().map(tag_reply).collect());
    assert_all_sampled("Message", protocol, message_samples().iter().map(tag_message).collect());
    assert_all_sampled("JobSpec", jobs, job_spec_samples().iter().map(tag_job_spec).collect());
    assert_all_sampled("JobOutput", jobs, job_output_samples().iter().map(tag_job_output).collect());
    assert_all_sampled(
        "HelperRequest",
        helper,
        helper_request_samples().iter().map(tag_helper_request).collect(),
    );
    assert_all_sampled("HelperReply", helper, helper_reply_samples().iter().map(tag_helper_reply).collect());
    assert_all_sampled(
        "HelperMessage",
        helper,
        helper_message_samples().iter().map(tag_helper_message).collect(),
    );
}

/// A wire type that postcard cannot carry, as someone would write it by mistake.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
enum InternallyTagged {
    Ping { n: u32 },
}

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Flattened {
    #[serde(flatten)]
    inner: std::collections::BTreeMap<String, u32>,
}

#[test]
fn the_guard_catches_what_postcard_cannot_carry() {
    let e = try_round(&InternallyTagged::Ping { n: 1 }).unwrap_err();
    assert!(e.contains("InternallyTagged") && e.contains("cannot decode"), "{e}");
    let e = try_round(&Flattened {
        inner: [("a".to_string(), 1)].into(),
    })
    .unwrap_err();
    assert!(e.contains("Flattened"), "{e}");
}

#[test]
fn the_source_scan_names_a_missing_variant() {
    let src = "pub enum Demo {\n    One,\n    Two {\n        a: u8,\n    },\n    Three(u8),\n}\n";
    let found: Vec<_> = source_variants(src, "Demo").into_iter().collect();
    assert_eq!(found, ["One", "Three", "Two"]);
}

/// Where `dyno-core` already uses an attribute postcard cannot carry, and why that is safe: the type is JSON-only (it reaches the UI
/// through `serde_json`, and the binary link uses a mirror type or `JsonOnLink`). A new use anywhere else, or one more in these files,
/// fails the test below: give the wire a separate binary type (`OwnerBinary` beside `OwnerJson`) or avoid the attribute.
const JSON_ONLY: &[(&str, usize)] = &[
    ("network.rs", 2),                    // OwnerJson (mirrored by OwnerBinary) and NetEvent
    ("bench.rs", 1),                      // StatusJson (mirrored by a binary form)
    ("bench/wire.rs", 1),                 // the JSON lines a dyno-bench child writes on stdout; never crosses the app-agent wire
    ("widgets/popover_values/mod.rs", 1), // PopoverSurfaceValues, carried by `JsonOnLink`
    ("intel/mod.rs", 2),                  // MatchedBy and Verdict: Serialize-only, never decoded; the verdict reaches the UI as JSON
];

fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn postcard_unfriendly_serde_attributes_only_appear_where_listed() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![];
    rust_files(&src, &mut files);
    let banned = ["tag = ", "untagged", "flatten", "skip_serializing_if", "content = "];
    let mut problems = vec![];
    for f in files {
        let rel = f.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
        let text = std::fs::read_to_string(&f).unwrap();
        let found = text
            .lines()
            .filter(|l| l.trim_start().starts_with("#[serde(") && banned.iter().any(|b| l.contains(b)))
            .count();
        let allowed = JSON_ONLY.iter().find(|(p, _)| *p == rel).map_or(0, |(_, n)| *n);
        if found != allowed {
            problems.push(format!("{rel}: {found} attribute(s) that postcard cannot carry, {allowed} listed"));
        }
    }
    assert!(
        problems.is_empty(),
        "serde attributes that postcard cannot decode (tag, untagged, flatten, skip_serializing_if, content) in dyno-core: {problems:?}. \
         The app and the agent talk postcard: use a plain enum for the wire, or list a JSON-only type in JSON_ONLY with the reason."
    );
}

#[test]
#[should_panic(expected = "InternallyTagged: postcard encodes it but cannot decode it")]
fn check_fails_loudly_and_names_the_type_for_an_internally_tagged_enum() {
    check("a wire type", &[InternallyTagged::Ping { n: 1 }]);
}

#[test]
#[should_panic(expected = "wire type `Demo`: variants without a sample in tests/wire_roundtrip.rs: [\"Two\"]")]
fn a_variant_without_a_sample_fails_naming_the_type_and_the_variant() {
    assert_all_sampled("Demo", "pub enum Demo {\n    One,\n    Two,\n}\n", vec!["One"]);
}
