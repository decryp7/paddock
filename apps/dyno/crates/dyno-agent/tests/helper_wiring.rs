#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Every reading the helper advertises is wired in the agent (ADR 0052 section 1.1, C83 for Dyno until `./dev check` takes it
//! over; Dyno plan 2026-11-04 section 7.2).
//!
//! The registry is the seam itself: [`HelperReading::KINDS`] is required of every reading (the compiler refuses one that does not
//! say what it asks), and `HelperFirst::new` records it where the composition builds the reading. The composition test runs the
//! real agent over the fakes for each host, with [`wired_kinds_while`] around the build, and compares what was wired with the
//! readings the card advertises for that host, in both directions. A reading wired without being advertised needs a reason in
//! [`unlisted`]; a reading advertised and not wired fails with the next step. There is no source scan: a call shape a scan does not
//! know cannot hide a reading, and a reading cannot be wired without being registered. What this does not cover: a request kind sent
//! outside a [`HelperReading`] (`Info`, `Audit`, tool install, wakes, `Build`); the helper's run-time `served:` counts (ADR 0052
//! section 1.5) and `apps/dyno/wiring.toml` (C83) stay the check for those.
//!
//! The user guide lists the advertised readings between `<!-- helper-readings:start -->` and `<!-- helper-readings:end -->`.

use dyno_agent::helper_first::{wired_kinds_while, HelperReading};
use dyno_agent::paths::Dirs;
use dyno_agent::Running;
use dyno_core::helper::HelperHost;
use dyno_core::helper_policy::RequestKind;
use dyno_core::helper_wire::{readings, Advertised};
use dyno_platform::fake::Fake;
use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

#[path = "common/helper_rig.rs"]
mod helper_rig;
use helper_rig::{dispatcher, wiring, Caller, DispatcherTransport, Readers};

/// `apps/dyno`: two folders above this crate. The run-time `CARGO_MANIFEST_DIR` wins over the one built in, so a test program
/// built here can run on another machine (the Windows test VM) over a copy of `wiring.toml` and the user guide.
fn dyno_root() -> PathBuf {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
    manifest.join("../..").canonicalize().unwrap()
}

// ---------------------------------------------------------------- the registry

/// What `HelperFirst::new` recorded while the real agent was built for `host` over the fakes (no machine is touched: the data
/// folder is a temp folder, the platform and the helper are the rig's).
fn wired_on(host: HelperHost) -> HashSet<RequestKind> {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::at(tmp.path().join("data"));
    let fake = Fake::new();
    let transport = DispatcherTransport::new(dispatcher(host, Readers::default(), tmp.path()), Caller::Owner);
    let (running, wired) = wired_kinds_while(|| {
        Running::start_with_helper(
            dirs,
            fake.platform(),
            Box::new(fake.clone()),
            Box::new(|_| Ok(())),
            wiring(transport, host),
        )
        .unwrap()
    });
    running.stop();
    wired
}

/// Kinds that are wired on some host without a line on the card for that host, and why. Anything else wired and not advertised
/// fails the check, and an entry that is wired nowhere unadvertised is stale.
const UNLISTED: &[(RequestKind, &str)] = &[
    (
        RequestKind::Smart,
        "Linux: the agent's wear forecast asks the helper for SMART (the helper applies on every system but macOS), while the card lists disk health for Windows only",
    ),
    (
        RequestKind::LogTail,
        "the problem report's copy of the helper's own log, not a reading on the card (diagnostics::HelperLog)",
    ),
    (
        RequestKind::WolState,
        "Linux Wake-on-LAN readiness (plan 2026-11-04, D4), not a reading on the card",
    ),
];

/// Request kinds that are not readings, so no `HelperReading` registers them: the helper's own state and actions, each sent by the
/// code that owns the action. Every other kind is advertised or wired, or declared in `apps/dyno/wiring.toml` with an end.
const NOT_A_READING: &[(RequestKind, &str)] = &[
    (RequestKind::Info, "the handshake: the helper's version, API and host (helper_link)"),
    (
        RequestKind::Audit,
        "the card's audit log of the helper's changes, read on request (helper_centre)",
    ),
    (
        RequestKind::InstallTool,
        "installs the pinned disk-health tool when the person asks (a change, not a reading)",
    ),
    (
        RequestKind::Uninstall,
        "the helper removes itself when the person turns it off (a change, not a reading)",
    ),
    (RequestKind::ScheduleWake, "sets a scheduled wake (an action of the wake centre)"),
    (RequestKind::CancelWake, "cancels a scheduled wake (an action of the wake centre)"),
    (
        RequestKind::ListWakes,
        "lists the wakes the helper set (the wake centre's read of its own schedule)",
    ),
    (
        RequestKind::Build,
        "the helper's build id, to tell a stale helper from this build (helper_centre)",
    ),
];

fn advertised_on(host: HelperHost) -> HashSet<RequestKind> {
    readings(host).iter().flat_map(|a| a.kinds.iter().copied()).collect()
}

/// The problems between what was wired and what is advertised for one host; empty means they agree.
fn problems(host: HelperHost, wired: &HashSet<RequestKind>, allowed: &[(RequestKind, &str)]) -> Vec<String> {
    let advertised = advertised_on(host);
    let mut out = Vec::new();
    for kind in &advertised {
        if !wired.contains(kind) {
            out.push(format!(
                "{host:?}: {kind:?} is advertised by readings() but no reading that declares it is built by the agent; wire a HelperReading with KINDS \
                 containing {kind:?} through HelperFirst::new, or stop advertising it (and say so in the changelog)"
            ));
        }
    }
    for kind in wired {
        if !advertised.contains(kind) && !allowed.iter().any(|(k, _)| k == kind) {
            out.push(format!(
                "{host:?}: {kind:?} is wired but not advertised; add it to readings() or give it a reason in UNLISTED (the card and the guide must tell the truth)"
            ));
        }
    }
    out
}

#[test]
fn what_the_agent_wires_is_what_readings_advertises_on_every_host() {
    let mut all = Vec::new();
    let mut used: HashSet<RequestKind> = HashSet::new();
    for host in [HelperHost::Windows, HelperHost::Macos, HelperHost::Linux] {
        let wired = wired_on(host);
        used.extend(wired.difference(&advertised_on(host)).copied());
        all.extend(problems(host, &wired, UNLISTED));
    }
    for (kind, _) in UNLISTED {
        if !used.contains(kind) {
            all.push(format!(
                "UNLISTED names {kind:?}, which no host wires without advertising it; delete the entry"
            ));
        }
    }
    assert!(all.is_empty(), "{} wiring problem(s):\n- {}", all.len(), all.join("\n- "));
}

/// How many places claim `kind`: wired or advertised on some host, [`NOT_A_READING`], and a `wiring.toml` entry. Exactly one is right.
fn kind_problems(wired_any: &HashSet<RequestKind>, entries: &[Unwired], today: i64) -> Vec<String> {
    let advertised_any: HashSet<RequestKind> = [HelperHost::Windows, HelperHost::Macos, HelperHost::Linux]
        .iter()
        .flat_map(|h| advertised_on(*h))
        .collect();
    let mut out = Vec::new();
    for kind in RequestKind::ALL {
        let name = format!("{kind:?}");
        let reading = wired_any.contains(&kind) || advertised_any.contains(&kind);
        let not_a_reading = NOT_A_READING.iter().any(|(k, _)| *k == kind);
        let declared = entries.iter().any(|e| e.kind == name);
        match (reading, not_a_reading, declared) {
            (true, false, false) | (false, true, false) | (false, false, true) => {}
            (false, false, false) => out.push(format!(
                "{name}: no reading wires it and it is not advertised, it is not in NOT_A_READING, and wiring.toml does not declare it; wire it, \
                 or add an [[unwired]] entry (wire = \"agent-helper\", kind, why, until) to apps/dyno/wiring.toml"
            )),
            (true, false, true) => out.push(format!(
                "{name}: wired or advertised, but apps/dyno/wiring.toml still declares it unwired; delete the [[unwired]] entry"
            )),
            _ => out.push(format!(
                "{name}: claimed in more than one place (wired or advertised, NOT_A_READING, wiring.toml); keep exactly one"
            )),
        }
    }
    for e in entries {
        if let Some(p) = until_problem(&e.until, today) {
            out.push(format!(
                "wiring.toml: {}: {p}; wire it or move the date with a Wiring-date trailer",
                e.kind
            ));
        }
    }
    out
}

fn today() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    (secs / 86_400) as i64
}

#[test]
fn every_request_kind_is_wired_or_not_a_reading_or_declared_in_wiring_toml() {
    let mut wired = HashSet::new();
    for host in [HelperHost::Windows, HelperHost::Macos, HelperHost::Linux] {
        wired.extend(wired_on(host));
    }
    let text = std::fs::read_to_string(dyno_root().join("wiring.toml")).expect("apps/dyno/wiring.toml");
    let problems = kind_problems(&wired, &parse_wiring(&text).unwrap(), today());
    assert!(
        problems.is_empty(),
        "{} request kind(s) are claimed wrongly:\n- {}",
        problems.len(),
        problems.join("\n- ")
    );
}

#[test]
fn the_kind_rules_name_each_way_a_kind_can_be_wrong() {
    let today = days_from_civil(2026, 11, 4);
    let unwired = |kind: &str, until: &str| Unwired {
        wire: "agent-helper".into(),
        kind: kind.into(),
        why: "later".into(),
        until: until.into(),
    };
    let all_but = |skip: RequestKind| -> HashSet<RequestKind> {
        RequestKind::ALL
            .iter()
            .copied()
            .filter(|k| *k != skip && !NOT_A_READING.iter().any(|(n, _)| n == k))
            .collect()
    };
    // Everything wired or not a reading: fine.
    assert!(kind_problems(
        &all_but(RequestKind::LogTail).into_iter().chain([RequestKind::LogTail]).collect(),
        &[],
        today
    )
    .is_empty());
    // A kind nobody claims needs wiring.toml.
    let p = kind_problems(&all_but(RequestKind::LogTail), &[], today);
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(p[0].starts_with("LogTail:") && p[0].contains("[[unwired]]"), "{p:?}");
    // Declared with an end: fine. Expired: a problem.
    assert!(kind_problems(&all_but(RequestKind::LogTail), &[unwired("LogTail", "2026-12-01")], today).is_empty());
    let p = kind_problems(&all_but(RequestKind::LogTail), &[unwired("LogTail", "2026-11-03")], today);
    assert!(p.iter().any(|m| m.contains("ended")), "{p:?}");
    // A kind that is wired may not stay declared.
    let wired_all: HashSet<RequestKind> = all_but(RequestKind::Info);
    let p = kind_problems(&wired_all, &[unwired("LogTail", "2026-12-01")], today);
    assert!(p.iter().any(|m| m.starts_with("LogTail: wired or advertised, but")), "{p:?}");
    // A NOT_A_READING kind that is also declared is claimed twice.
    let p = kind_problems(&wired_all, &[unwired("Info", "2026-12-01")], today);
    assert!(p.iter().any(|m| m.starts_with("Info: claimed in more than one place")), "{p:?}");
}

#[test]
fn the_check_names_each_way_the_two_sides_can_differ() {
    let host = HelperHost::Linux;
    let advertised = advertised_on(host);
    assert!(
        problems(host, &advertised, &[]).is_empty(),
        "the advertised set is its own fixture of agreement"
    );
    let mut missing = advertised.clone();
    missing.remove(&RequestKind::Firewall);
    let p = problems(host, &missing, &[]);
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(
        p[0].contains("Firewall is advertised") && p[0].contains("HelperFirst::new"),
        "{p:?}"
    );
    let mut extra = advertised;
    extra.insert(RequestKind::Smart);
    let p = problems(host, &extra, &[]);
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(p[0].contains("Smart is wired but not advertised"), "{p:?}");
    assert!(
        problems(host, &extra, &[(RequestKind::Smart, "x")]).is_empty(),
        "a reason lets it through"
    );
}

#[test]
fn a_reading_declares_its_kinds_at_compile_time() {
    // The trait requires KINDS: this does not compile for a reading without it, which is the point of the registry.
    fn kinds<R: HelperReading>() -> &'static [RequestKind] {
        R::KINDS
    }
    assert_eq!(kinds::<dyno_agent::diagnostics::HelperLog>(), &[RequestKind::LogTail]);
}

// ---------------------------------------------------------------- wiring.toml

#[derive(Debug, Clone, PartialEq, Eq)]
struct Unwired {
    wire: String,
    kind: String,
    why: String,
    until: String,
}

/// Parses `wiring.toml` (ADR 0052 section 1.1); a malformed entry is a problem.
fn parse_wiring(text: &str) -> Result<Vec<Unwired>, String> {
    let v: toml::Table = text.parse().map_err(|e| format!("wiring.toml does not parse: {e}"))?;
    let mut out = Vec::new();
    for e in v.get("unwired").and_then(|u| u.as_array()).map_or(&[][..], |a| a.as_slice()) {
        let field = |k: &str| {
            e.get(k)
                .and_then(|x| x.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
                .ok_or_else(|| format!("wiring.toml: an [[unwired]] entry has no `{k}`"))
        };
        out.push(Unwired {
            wire: field("wire")?,
            kind: field("kind")?,
            why: field("why")?,
            until: field("until")?,
        });
    }
    Ok(out)
}

/// Days since 1970-01-01 of a civil date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `until` is `dyno-vX.Y.Z` or `YYYY-MM-DD`; a date before `today` (days since 1970) is expired.
fn until_problem(until: &str, today: i64) -> Option<String> {
    if let Some(v) = until.strip_prefix("dyno-v") {
        let ok = v.split('.').count() == 3 && v.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        return (!ok).then(|| format!("`until` \"{until}\" is not a release tag like dyno-v1.2.0 or a date"));
    }
    let parts: Vec<&str> = until.split('-').collect();
    let nums: Option<Vec<i64>> = parts.iter().map(|p| p.parse().ok()).collect();
    match (parts.len(), nums) {
        (3, Some(n)) if (1..=12).contains(&n[1]) && (1..=31).contains(&n[2]) => {
            let day = days_from_civil(n[0], n[1], n[2]);
            if day < today {
                Some(format!("the waiver ended on {until}"))
            } else if day > today + 60 {
                Some(format!(
                    "the waiver runs more than 60 days ahead ({until}); a waiver is at most 60 days or a release tag"
                ))
            } else {
                None
            }
        }
        _ => Some(format!("`until` \"{until}\" is not a release tag like dyno-v1.2.0 or a date")),
    }
}

fn advertised_kinds() -> BTreeSet<String> {
    [HelperHost::Windows, HelperHost::Macos, HelperHost::Linux]
        .iter()
        .flat_map(|h| readings(*h).iter().flat_map(|a| a.kinds.iter().map(|k| format!("{k:?}"))))
        .collect()
}

// ---------------------------------------------------------------- doc claims

/// The lines `- <text>` between `<!-- helper-readings:start -->` and `<!-- helper-readings:end -->`.
fn claimed(doc: &str) -> Result<BTreeSet<String>, String> {
    let (_, rest) = doc
        .split_once("<!-- helper-readings:start -->")
        .ok_or("no `<!-- helper-readings:start -->` marker")?;
    let (block, _) = rest
        .split_once("<!-- helper-readings:end -->")
        .ok_or("no `<!-- helper-readings:end -->` marker")?;
    Ok(block
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- "))
        .map(|l| l.trim().to_string())
        .collect())
}

/// Problems between a document's claimed readings and the advertised ones.
fn claim_problems(name: &str, doc: &str, all: &[Advertised]) -> Vec<String> {
    let claimed = match claimed(doc) {
        Ok(c) => c,
        Err(e) => {
            return vec![format!(
                "{name}: {e}; list the advertised readings between the markers, one `- text` per line"
            )]
        }
    };
    let wanted: BTreeSet<String> = all.iter().map(|a| a.text.to_string()).collect();
    let mut out: Vec<String> = wanted
        .difference(&claimed)
        .map(|t| format!("{name}: does not mention \"{t}\""))
        .collect();
    out.extend(
        claimed
            .difference(&wanted)
            .map(|t| format!("{name}: claims \"{t}\", which readings() does not advertise")),
    );
    out
}

fn all_advertised() -> Vec<Advertised> {
    [HelperHost::Windows, HelperHost::Macos, HelperHost::Linux]
        .iter()
        .flat_map(|h| readings(*h).iter().copied())
        .collect()
}

#[test]
fn the_user_guide_claims_what_readings_advertises() {
    let doc = std::fs::read_to_string(dyno_root().join("docs/user-guide.md")).unwrap();
    let problems = claim_problems("docs/user-guide.md", &doc, &all_advertised());
    assert!(
        problems.is_empty(),
        "the guide differs from readings():\n- {}",
        problems.join("\n- ")
    );
}

#[test]
fn waivers_are_bounded() {
    let today = days_from_civil(2026, 11, 4);
    assert_eq!(until_problem("dyno-v1.2.0", today), None);
    assert_eq!(until_problem("2026-12-01", today), None);
    assert!(until_problem("2026-11-03", today).unwrap().contains("ended"));
    assert!(until_problem("2027-01-04", today).unwrap().contains("60 days"));
    assert_eq!(until_problem("2027-01-03", today), None);
    assert!(until_problem("someday", today).is_some());
    assert!(until_problem("dyno-v1.2", today).is_some());
    assert!(parse_wiring("[[unwired]]\nwire = \"agent-helper\"\nkind = \"LogTail\"\n")
        .unwrap_err()
        .contains("`why`"));
}

#[test]
fn wiring_toml_of_the_tree_parses_and_names_real_kinds() {
    let text = std::fs::read_to_string(dyno_root().join("wiring.toml")).expect("apps/dyno/wiring.toml");
    let entries = parse_wiring(&text).unwrap();
    // No entry is a valid file: every kind is advertised or wired.
    for e in &entries {
        assert!(RequestKind::ALL.iter().any(|k| format!("{k:?}") == e.kind), "{e:?}");
        assert!(
            !advertised_kinds().contains(&e.kind),
            "{}: an advertised kind may not be declared unwired",
            e.kind
        );
    }
}

#[test]
fn the_claims_check_names_what_differs() {
    let list = all_advertised();
    let mut doc = String::from("x\n<!-- helper-readings:start -->\n");
    let texts: BTreeSet<&str> = list.iter().map(|a| a.text).collect();
    for t in &texts {
        doc.push_str(&format!("- {t}\n"));
    }
    doc.push_str("<!-- helper-readings:end -->\n");
    assert!(claim_problems("doc", &doc, &list).is_empty());
    let missing = doc.replace("- Memory modules\n", "");
    assert_eq!(
        claim_problems("doc", &missing, &list),
        vec!["doc: does not mention \"Memory modules\"".to_string()]
    );
    let extra = doc.replace("- Memory modules\n", "- Memory modules\n- Fan curves\n");
    assert!(claim_problems("doc", &extra, &list)[0].contains("claims \"Fan curves\""));
    assert!(claim_problems("doc", "no markers", &list)[0].contains("marker"));
}
