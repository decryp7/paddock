#![allow(clippy::unwrap_used, clippy::expect_used)]

//! The positional postcard bytes of every `HelperRequest` and `HelperReply` variant, pinned (the helper wire, `HELPER_API` 6 shipped
//! in dyno-v1.1.0). The helper is a separate, privileged program that an older or newer build of the agent talks to over postcard,
//! which is positional: a field added to, removed from or reordered in a payload type changes these bytes, and an older peer then
//! reads a wrong layout. So a failure here means: bump `HELPER_API` (and add its entry to the wire history in `helper_wire.rs`), then
//! update the expected bytes in the same change. A variant that is appended at the end (a minor wire change) adds a line here.
//!
//! The values are small but non-default, so a field that moves is seen. To print the bytes after a deliberate change, run this
//! test with `DYNO_PRINT_HELPER_GOLDEN=1` and `--nocapture`.

use dyno_core::btm::LoginItemsReply;
use dyno_core::firewall::{FirewallSource, RuleSource};
use dyno_core::helper::HelperHost;
use dyno_core::helper_counters::{CounterAnswer, CounterValue};
use dyno_core::helper_extras::ExtrasAnswer;
use dyno_core::helper_holders::{FileHolders, Holder, SocketOwner, SocketOwners};
use dyno_core::helper_policy::Visibility;
use dyno_core::helper_processes::{OthersTotal, OwnerKind, ProcState, ProcessEntry, ProcessPage};
use dyno_core::helper_requests::{CounterId, CounterQuery, ExtrasKind, ProcessQuery, RequestClass, SmartQuery, ToolKind};
use dyno_core::helper_wire::{
    AuditEntry, AuditVerdict, CallerView, HelperInfo, HelperMessage, HelperReply, HelperRequest, ToolStatus, HELPER_API,
};
use dyno_core::inventory::{MemoryLayout, Unavailable};
use dyno_core::network_traffic::{OthersTraffic, TrafficAnswer, TrafficCounter};
use dyno_core::read_via::ReadVia;
use dyno_core::smart::SmartReply;
use dyno_core::wake::{IfName, WakeEntry, WakeTag, WolMode, WolReply};

fn tag() -> WakeTag {
    WakeTag::parse("t").unwrap()
}

fn requests() -> Vec<(&'static str, HelperRequest)> {
    vec![
        ("Info", HelperRequest::Info),
        (
            "Processes",
            HelperRequest::Processes(ProcessQuery {
                after: Some(5),
                limit: 50,
                class: RequestClass::User,
            }),
        ),
        ("Traffic", HelperRequest::Traffic),
        ("SocketOwners", HelperRequest::SocketOwners),
        ("FileHolders", HelperRequest::FileHolders { dev: 2, ino: 3 }),
        ("LoginItems", HelperRequest::LoginItems),
        (
            "Counters",
            HelperRequest::Counters(CounterQuery {
                ids: vec![CounterId::Thermal],
                class: RequestClass::Background,
            }),
        ),
        ("MemoryModules", HelperRequest::MemoryModules),
        ("Smart", HelperRequest::Smart(SmartQuery { max_disks: 4 })),
        (
            "InstallTool",
            HelperRequest::InstallTool {
                tool: ToolKind::Smartmontools,
                bytes: vec![1, 2],
            },
        ),
        ("Firewall", HelperRequest::Firewall),
        ("Extras", HelperRequest::Extras(ExtrasKind::WakeRequests)),
        ("Audit", HelperRequest::Audit(5)),
        ("LogTail", HelperRequest::LogTail(7)),
        ("Uninstall", HelperRequest::Uninstall),
        ("ScheduleWake", HelperRequest::ScheduleWake { tag: tag(), at_utc: 9 }),
        ("CancelWake", HelperRequest::CancelWake { tag: tag() }),
        ("ListWakes", HelperRequest::ListWakes),
        (
            "WolState",
            HelperRequest::WolState {
                ifname: IfName::parse("en0").unwrap(),
            },
        ),
        ("Build", HelperRequest::Build),
    ]
}

fn replies() -> Vec<(&'static str, HelperReply)> {
    let entry = ProcessEntry {
        pid: 2,
        started: Some(3),
        parent: Some(1),
        name: "n".into(),
        path: Some("/p".into()),
        owner: OwnerKind::Service("s".into()),
        account: Some("a".into()),
        state: ProcState::Sleeping,
        threads: Some(4),
        cpu_time_ms: Some(5),
        memory_bytes: Some(6),
        disk_read_bytes: Some(7),
        disk_written_bytes: Some(8),
    };
    vec![
        (
            "About",
            HelperReply::About(
                HelperInfo {
                    version: "1.1.0".into(),
                    api: 6,
                    host: HelperHost::Linux,
                    agent_exe: "/x".into(),
                },
                CallerView {
                    scope: Visibility::Full,
                    tools: vec![],
                },
            ),
        ),
        (
            "Traffic",
            HelperReply::Traffic(TrafficAnswer {
                counters: vec![TrafficCounter {
                    pid: 4,
                    name: "x".into(),
                    bytes_in: 1,
                    bytes_out: 2,
                }],
                others: Some(OthersTraffic {
                    programs: 3,
                    bytes_in: 4,
                    bytes_out: 5,
                }),
            }),
        ),
        (
            "Entries",
            HelperReply::Entries(vec![AuditEntry {
                at: 1,
                peer: Some(2),
                what: "w".into(),
                verdict: AuditVerdict::Refused,
                detail: "d".into(),
            }]),
        ),
        (
            "Firewall",
            HelperReply::Firewall(FirewallSource {
                source: RuleSource::Nftables,
                states: vec![],
                switches: vec![],
                default_in: None,
                default_out: None,
                rule_count: 3,
                rules: vec![],
                truncated: 1,
                rules_hidden: true,
                note: Some("n".into()),
                via: ReadVia::Local,
            }),
        ),
        ("Wakes", HelperReply::Wakes(vec![WakeEntry { tag: tag(), at_utc: 9 }])),
        ("WakeCancelled", HelperReply::WakeCancelled(true)),
        (
            "Wol",
            HelperReply::Wol(WolReply {
                enabled: vec![WolMode::Magic],
                supported: vec![WolMode::Phy, WolMode::Magic],
            }),
        ),
        (
            "CounterValues",
            HelperReply::CounterValues(vec![CounterAnswer {
                id: CounterId::Fans,
                value: CounterValue::Unavailable("n".into()),
            }]),
        ),
        (
            "Memory",
            HelperReply::Memory(MemoryLayout::TotalOnly {
                total: 8,
                reason: Unavailable { reason: "r".into() },
            }),
        ),
        (
            "SocketOwners",
            HelperReply::SocketOwners(SocketOwners {
                owners: vec![SocketOwner {
                    inode: 5,
                    holder: Holder {
                        pid: 6,
                        started: Some(7),
                        name: "h".into(),
                        owner: OwnerKind::Caller,
                    },
                }],
                others: 1,
                truncated: true,
            }),
        ),
        ("FileHolders", HelperReply::FileHolders(FileHolders::default())),
        ("Extras", HelperReply::Extras(ExtrasAnswer::WakeRequests { count: 2, items: None })),
        (
            "Processes",
            HelperReply::Processes(ProcessPage {
                taken_at: 1,
                items: vec![entry],
                next: Some(2),
                others: Some(OthersTotal {
                    processes: 1,
                    memory_bytes: 2,
                    cpu_time_ms: 3,
                }),
                truncated: true,
            }),
        ),
        (
            "Smart",
            HelperReply::Smart(SmartReply {
                disks: vec![],
                skipped: vec![],
                not_visited: 1,
            }),
        ),
        (
            "Installed",
            HelperReply::Installed(ToolStatus {
                tool: ToolKind::Smartmontools,
                installed: Some("7".into()),
            }),
        ),
        (
            "LoginItems",
            HelperReply::LoginItems(LoginItemsReply {
                items: vec![],
                truncated: 1,
                other_accounts: 2,
            }),
        ),
        ("Log", HelperReply::Log(vec!["a".into()])),
        ("Bye", HelperReply::Bye),
        ("Build", HelperReply::Build("b".into())),
    ]
}

/// The postcard variant index of a request, by an exhaustive `match` with no wildcard arm: a variant added to `HelperRequest` does not
/// compile here until it has an arm, and the test below then needs a case (and a golden line) for that index. Bump
/// `REQUEST_VARIANTS` with the new arm.
fn request_index(r: &HelperRequest) -> usize {
    match r {
        HelperRequest::Info => 0,
        HelperRequest::Processes(_) => 1,
        HelperRequest::Traffic => 2,
        HelperRequest::SocketOwners => 3,
        HelperRequest::FileHolders { .. } => 4,
        HelperRequest::LoginItems => 5,
        HelperRequest::Counters(_) => 6,
        HelperRequest::MemoryModules => 7,
        HelperRequest::Smart(_) => 8,
        HelperRequest::InstallTool { .. } => 9,
        HelperRequest::Firewall => 10,
        HelperRequest::Extras(_) => 11,
        HelperRequest::Audit(_) => 12,
        HelperRequest::LogTail(_) => 13,
        HelperRequest::Uninstall => 14,
        HelperRequest::ScheduleWake { .. } => 15,
        HelperRequest::CancelWake { .. } => 16,
        HelperRequest::ListWakes => 17,
        HelperRequest::WolState { .. } => 18,
        HelperRequest::Build => 19,
    }
}
const REQUEST_VARIANTS: usize = 20;

/// The same for a reply. Bump `REPLY_VARIANTS` with a new arm.
fn reply_index(r: &HelperReply) -> usize {
    match r {
        HelperReply::About(..) => 0,
        HelperReply::Traffic(_) => 1,
        HelperReply::Entries(_) => 2,
        HelperReply::Firewall(_) => 3,
        HelperReply::Wakes(_) => 4,
        HelperReply::WakeCancelled(_) => 5,
        HelperReply::Wol(_) => 6,
        HelperReply::CounterValues(_) => 7,
        HelperReply::Memory(_) => 8,
        HelperReply::SocketOwners(_) => 9,
        HelperReply::FileHolders(_) => 10,
        HelperReply::Extras(_) => 11,
        HelperReply::Processes(_) => 12,
        HelperReply::Smart(_) => 13,
        HelperReply::Installed(_) => 14,
        HelperReply::LoginItems(_) => 15,
        HelperReply::Log(_) => 16,
        HelperReply::Bye => 17,
        HelperReply::Build(_) => 18,
    }
}
const REPLY_VARIANTS: usize = 19;

#[test]
fn every_variant_has_a_case_and_the_declared_index_is_the_postcard_tag() {
    let mut seen = vec![false; REQUEST_VARIANTS];
    for (name, r) in requests() {
        let i = request_index(&r);
        assert!(
            i < REQUEST_VARIANTS && !seen[i],
            "request {name}: index {i} is out of range or listed twice"
        );
        seen[i] = true;
        // The message tag (Request = 0) comes first, then the variant index: the declared order is the wire order.
        assert_eq!(
            postcard::to_stdvec(&HelperMessage::Request(r)).unwrap()[1] as usize,
            i,
            "request {name}: the wire tag moved"
        );
    }
    assert!(seen.iter().all(|s| *s), "a HelperRequest variant has no golden case: {seen:?}");
    let mut seen = vec![false; REPLY_VARIANTS];
    for (name, r) in replies() {
        let i = reply_index(&r);
        assert!(
            i < REPLY_VARIANTS && !seen[i],
            "reply {name}: index {i} is out of range or listed twice"
        );
        seen[i] = true;
        assert_eq!(
            postcard::to_stdvec(&HelperMessage::Reply(r)).unwrap()[1] as usize,
            i,
            "reply {name}: the wire tag moved"
        );
    }
    assert!(seen.iter().all(|s| *s), "a HelperReply variant has no golden case: {seen:?}");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The bytes of every message, labelled `Request/<variant>` or `Reply/<variant>`.
fn actual() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, r) in requests() {
        out.push((
            format!("Request/{name}"),
            hex(&postcard::to_stdvec(&HelperMessage::Request(r)).unwrap()),
        ));
    }
    for (name, r) in replies() {
        out.push((
            format!("Reply/{name}"),
            hex(&postcard::to_stdvec(&HelperMessage::Reply(r)).unwrap()),
        ));
    }
    out
}

/// Pinned from the build of dyno-v1.1.0's wire (5f7ea13fb on the day of writing; the bytes were equal there and on the branch).
const GOLDEN: &[(&str, &str)] = &[
    ("Request/Info", "0000"),
    ("Request/Processes", "000101053200"),
    ("Request/Traffic", "0002"),
    ("Request/SocketOwners", "0003"),
    ("Request/FileHolders", "00040203"),
    ("Request/LoginItems", "0005"),
    ("Request/Counters", "0006010001"),
    ("Request/MemoryModules", "0007"),
    ("Request/Smart", "000804"),
    ("Request/InstallTool", "000900020102"),
    ("Request/Firewall", "000a"),
    ("Request/Extras", "000b00"),
    ("Request/Audit", "000c05"),
    ("Request/LogTail", "000d07"),
    ("Request/Uninstall", "000e"),
    ("Request/ScheduleWake", "000f017412"),
    ("Request/CancelWake", "00100174"),
    ("Request/ListWakes", "0011"),
    ("Request/WolState", "001203656e30"),
    ("Request/Build", "0013"),
    ("Reply/About", "010005312e312e300602022f780000"),
    ("Reply/Traffic", "010101040178010201030405"),
    ("Reply/Entries", "0102010201020177010164"),
    ("Reply/Firewall", "010303000000000300010101016e01"),
    ("Reply/Wakes", "010401017412"),
    ("Reply/WakeCancelled", "010501"),
    ("Reply/Wol", "01060105020005"),
    ("Reply/CounterValues", "0107010203016e"),
    ("Reply/Memory", "010802080172"),
    ("Reply/SocketOwners", "0109010506010e0168000101"),
    ("Reply/FileHolders", "010a000000"),
    ("Reply/Extras", "010b000200"),
    (
        "Reply/Processes",
        "010c02010201060101016e01022f70020173010161010104010501060107010801020101020301",
    ),
    ("Reply/Smart", "010d000001"),
    ("Reply/Installed", "010e00010137"),
    ("Reply/LoginItems", "010f000102"),
    ("Reply/Log", "0110010161"),
    ("Reply/Bye", "0111"),
    ("Reply/Build", "01120162"),
];

#[test]
fn every_helper_message_has_these_postcard_bytes() {
    let now = actual();
    if std::env::var_os("DYNO_PRINT_HELPER_GOLDEN").is_some() {
        for (name, bytes) in &now {
            println!("    (\"{name}\", \"{bytes}\"),");
        }
    }
    let mut wrong = Vec::new();
    for (name, bytes) in &now {
        match GOLDEN.iter().find(|(n, _)| n == name) {
            Some((_, want)) if want == bytes => {}
            Some((_, want)) => wrong.push(format!("{name}: bytes are now {bytes}, were {want}")),
            None => wrong.push(format!(
                "{name}: no pinned bytes (a new variant is appended at the end and gets a line here)"
            )),
        }
    }
    assert!(
        wrong.is_empty(),
        "the helper wire changed. It is positional postcard, so a changed payload needs a HELPER_API bump (now {HELPER_API}) with its \
         entry in the wire history in helper_wire.rs, and these lines updated in the same change:\n{}",
        wrong.join("\n")
    );
    assert_eq!(GOLDEN.len(), now.len(), "a pinned line names a variant that no longer exists");
}
