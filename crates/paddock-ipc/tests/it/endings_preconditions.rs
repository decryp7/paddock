//! Scenario preconditions (ADR 0122 WP-1): a scenario declares the state it depends on, the rig reports whether it held, and
//! a precondition that was missed or not measured is the rig's failure, never a finding about the endpoint (no ratchet entry
//! is new or stale for it). The verdict is report-only for now (`test:endings-preconditions`): a miss is then a warning and
//! the scenario is judged and ratcheted as before.

use paddock_ipc::endpoint::Stream;
use paddock_ipc::frame::{self, Frame, FrameLimit, ProtocolVersion};
use paddock_ipc::testing::endings::applicability::Applicability;
use paddock_ipc::testing::endings::ratchet::Ratchet;
use paddock_ipc::testing::endings::raw::{READ_GUARD, read_to_end, write_junk};
use paddock_ipc::testing::endings::{
    self, Evidence, Expect, Fault, Observed, Outcome, Precondition, PreconditionVerdict,
    RigFailure, Scenario, conclude, ipc, judge, log,
};
use paddock_ipc::{Host, Token, codes};
use std::time::Duration;

const GUARD: Duration = Duration::from_secs(30);

fn raw_end(kind: endings::raw::RawEndKind) -> Option<endings::raw::RawEnd> {
    Some(endings::raw::RawEnd {
        bytes: Vec::new(),
        kind,
    })
}

/// A client that ended with a bare end and no reason: the judge finds `no-reason`.
fn bare_end() -> Observed {
    Observed {
        raw: raw_end(endings::raw::RawEndKind::Eof),
        ..Observed::default()
    }
}

fn no_end_line(expect: Expect) -> Expect {
    expect.without_end_line()
}

/// Test 1's rig (the positive control of "input queued when refused"): against `paddock-ipc`'s own host, a refused client
/// writes its hello alone and its junk only after the endpoint's end line, so nothing is unread when the host closes. The
/// rig sees its own order and records the precondition as missed.
fn junk_after_the_end_line() -> Result<(Observed, Expect), String> {
    ipc::logged(|| -> Result<(Observed, Expect), String> {
        let capture = log::process_capture()?;
        let from = capture.len();
        let limit = FrameLimit::default();
        let mut host = Host::listen(limit, ProtocolVersion::UNVERSIONED)
            .map_err(|e| format!("no endpoint: {e}"))?;
        let wrong = Token::from_bytes([0x11; paddock_ipc::token::TOKEN_BYTES]);
        let hello = frame::encode(
            &Frame::Hello {
                version: frame::VERSION,
                protocol: host.protocol(),
                token: *wrong.as_bytes(),
                pid: std::process::id(),
            },
            limit,
        )
        .map_err(|e| format!("encode: {e}"))?;
        let name = host.endpoint().to_string();
        let server = std::thread::spawn(move || drop(host.accept(GUARD)));
        let mut c = Stream::connect(&name, GUARD).map_err(|e| format!("connect: {e}"))?;
        std::io::Write::write_all(&mut c, &hello).map_err(|e| format!("write: {e}"))?;
        let ended = capture.wait_for_end_line_since(from, GUARD);
        write_junk(&mut c, endings::KEEP_WRITING_BYTES);
        let raw = read_to_end(&mut c, READ_GUARD);
        drop(server.join());
        let decoded = ipc::decode_frames(&raw.bytes, limit);
        let evidence = Evidence::Missed(format!(
            "the client wrote its junk only after the endpoint's end line (end line seen: {ended})"
        ));
        Ok((
            Observed {
                raw: Some(raw),
                decoded,
                evidence: vec![(Precondition::InputQueuedWhenRefused, evidence)],
                ..Observed::default()
            },
            Expect::reason(&[codes::AUTH]),
        ))
    })
}

/// Test 1. Run 9001: the junk went out after the host had closed, the stream ended in order, and the ratchet called I10
/// stale; the finding was the rig's. Once the verdict blocks, such a run is `RIG FAILURE (precondition ... not met: ...)`
/// and the ratchet says nothing new or stale about the scenario; while it is report-only, it is a warning naming the
/// precondition.
#[test]
fn a_rig_that_misses_input_queued_is_a_rig_failure_and_the_ratchet_names_nothing_stale_or_new() {
    let applicability =
        Applicability::load_from(env!("CARGO_MANIFEST_DIR")).expect("the register file");
    let ran = junk_after_the_end_line();
    let scenario = Scenario::ClientKeepsWriting;

    let report_only = conclude(
        "ipc-host",
        scenario,
        &applicability,
        PreconditionVerdict::ReportOnly,
        ran.clone(),
    );
    assert!(
        matches!(report_only.outcome, Outcome::Ran(_)),
        "{}",
        report_only.summary()
    );
    assert_eq!(report_only.warnings.len(), 1, "{:?}", report_only.warnings);
    assert!(
        report_only.warnings[0].contains("precondition input-queued-when-refused not met")
            && report_only.warnings[0].contains("test:endings-preconditions"),
        "{}",
        report_only.warnings[0]
    );

    let blocking = conclude(
        "ipc-host",
        scenario,
        &applicability,
        PreconditionVerdict::Blocking,
        ran,
    );
    assert!(
        matches!(
            &blocking.outcome,
            Outcome::RigFailure(RigFailure::Precondition {
                which: Precondition::InputQueuedWhenRefused,
                evidence: Evidence::Missed(_),
            })
        ),
        "{}",
        blocking.summary()
    );
    assert!(
        blocking
            .summary()
            .starts_with("ipc-host client-keeps-writing: RIG FAILURE (precondition input-queued-when-refused not met: "),
        "{}",
        blocking.summary()
    );
    // The scenario's own ratchet entries (I10 on Linux, the deferred drain everywhere) are neither stale nor matched.
    let mut ratchet = Ratchet::load_from(env!("CARGO_MANIFEST_DIR")).expect("the ratchet file");
    ratchet
        .entries
        .retain(|e| e.endpoint == "ipc-host" && e.scenario == scenario.name());
    assert!(
        !ratchet.entries.is_empty(),
        "the control needs the scenario's ratchet entries"
    );
    let verdict = ratchet
        .check("ipc-host", std::slice::from_ref(&blocking))
        .expect_err("a rig failure is red");
    assert!(
        verdict.contains("precondition input-queued-when-refused not met"),
        "{verdict}"
    );
    assert!(verdict.contains("no entry is new or stale"), "{verdict}");
    assert!(!verdict.contains("stale ratchet entry"), "{verdict}");
    assert!(!verdict.contains("new failure"), "{verdict}");
}

/// Test 5 (ER-33): a precondition the rig did not measure never counts as held: a warning while the verdict is report-only,
/// a rig failure once it blocks. A rig that recorded nothing for a declared precondition is the same as one whose probe
/// failed.
#[test]
fn a_precondition_that_was_not_measured_never_passes() {
    let none = Applicability::none();
    let scenario = Scenario::ClientKeepsWriting;
    let silent = Ok::<_, String>((bare_end(), no_end_line(Expect::reason(&["auth"]))));
    let mut probe_failed = bare_end();
    probe_failed.evidence = vec![(
        Precondition::InputQueuedWhenRefused,
        Evidence::NotMeasured("the queue could not be read".into()),
    )];
    let failed = Ok::<_, String>((probe_failed, no_end_line(Expect::reason(&["auth"]))));
    for ran in [silent, failed] {
        let warned = conclude(
            "rigged",
            scenario,
            &none,
            PreconditionVerdict::ReportOnly,
            ran.clone(),
        );
        assert!(
            matches!(warned.outcome, Outcome::Ran(_)),
            "{}",
            warned.summary()
        );
        assert!(
            warned.warnings.len() == 1
                && warned.warnings[0]
                    .contains("precondition input-queued-when-refused not measured"),
            "{:?}",
            warned.warnings
        );
        let red = conclude(
            "rigged",
            scenario,
            &none,
            PreconditionVerdict::Blocking,
            ran,
        );
        assert!(
            matches!(
                &red.outcome,
                Outcome::RigFailure(RigFailure::Precondition {
                    evidence: Evidence::NotMeasured(_),
                    ..
                })
            ),
            "{}",
            red.summary()
        );
        assert!(
            red.summary()
                .contains("RIG FAILURE (precondition input-queued-when-refused not measured: "),
            "{}",
            red.summary()
        );
        assert!(
            Ratchet::default()
                .check("rigged", std::slice::from_ref(&red))
                .is_err(),
            "not measured never passes"
        );
    }
    // The control: the same scenario with the precondition held runs, warns about nothing and is judged.
    let mut held = bare_end();
    held.evidence = vec![(
        Precondition::InputQueuedWhenRefused,
        Evidence::Held("os_queue=1030".into()),
    )];
    let ok = conclude(
        "rigged",
        scenario,
        &none,
        PreconditionVerdict::Blocking,
        Ok::<_, String>((held, no_end_line(Expect::reason(&["auth"])))),
    );
    assert!(ok.warnings.is_empty(), "{:?}", ok.warnings);
    assert_eq!(
        ok.faults().iter().map(Fault::kind).collect::<Vec<_>>(),
        vec!["reset"],
        "held input-queued folds the missing reason into the reset: {}",
        ok.summary()
    );
}

#[test]
fn each_scenario_declares_its_default_preconditions() {
    use Precondition::*;
    for s in Scenario::ALL {
        let want: &[Precondition] = match s {
            Scenario::WrongAccount | Scenario::Busy | Scenario::ClientKeepsWriting => {
                &[InputQueuedWhenRefused]
            }
            Scenario::OwnerCloseWhilePeerNotReading => &[PeerNotReadingAtClose],
            _ => &[],
        };
        assert_eq!(s.default_preconditions(), want, "{s}");
    }
    let names: Vec<&str> = Precondition::ALL.iter().map(|p| p.name()).collect();
    assert_eq!(
        names,
        [
            "input-queued-when-refused",
            "peer-not-reading-at-close",
            "peer-reads-before-reason-deadline"
        ]
    );
    for p in Precondition::ALL {
        assert_eq!(Precondition::from_name(p.name()), Some(p));
    }
    assert_eq!(Precondition::from_name("nonsense"), None);
}

/// The judge folds a missing reason into the reset only where input-queued is declared; an exempt scenario (dyno-helper's
/// busy client writes nothing) keeps its `no-reason`, and a scenario with no precondition is not warned about.
#[test]
fn an_exemption_narrows_the_default_and_keeps_the_missing_reason() {
    let declared = judge(
        &bare_end(),
        &no_end_line(Expect::reason(&["busy"]))
            .with_precondition(Precondition::InputQueuedWhenRefused),
    );
    assert_eq!(
        declared.iter().map(Fault::kind).collect::<Vec<_>>(),
        vec!["reset"]
    );
    let undeclared = judge(&bare_end(), &no_end_line(Expect::reason(&["busy"])));
    assert_eq!(
        undeclared.iter().map(Fault::kind).collect::<Vec<_>>(),
        vec!["no-reason"]
    );

    let exempt = Applicability::none().with_exempt(
        "rigged",
        Scenario::Busy,
        Precondition::InputQueuedWhenRefused,
        "writes nothing",
    );
    let report = conclude(
        "rigged",
        Scenario::Busy,
        &exempt,
        PreconditionVerdict::Blocking,
        Ok::<_, String>((bare_end(), no_end_line(Expect::reason(&["busy"])))),
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(
        report.faults().iter().map(Fault::kind).collect::<Vec<_>>(),
        vec!["no-reason"],
        "{}",
        report.summary()
    );
    // Not exempt: declared by default, so the same run is the rig's failure once the verdict blocks.
    let other = conclude(
        "other",
        Scenario::Busy,
        &exempt,
        PreconditionVerdict::Blocking,
        Ok::<_, String>((bare_end(), no_end_line(Expect::reason(&["busy"])))),
    );
    assert!(
        matches!(
            other.outcome,
            Outcome::RigFailure(RigFailure::Precondition { .. })
        ),
        "{}",
        other.summary()
    );
    let plain = conclude(
        "rigged",
        Scenario::WrongToken,
        &exempt,
        PreconditionVerdict::Blocking,
        Ok::<_, String>((bare_end(), no_end_line(Expect::reason(&["auth"])))),
    );
    assert!(
        plain.warnings.is_empty() && matches!(plain.outcome, Outcome::Ran(_)),
        "{}",
        plain.summary()
    );
}

#[test]
fn the_register_reads_precondition_exemptions_and_refuses_bad_ones() {
    let ok = Applicability::parse(
        "[[precondition_exempt]]\nendpoint = \"e\"\nscenario = \"busy\"\nprecondition = \"input-queued-when-refused\"\nreason = \"r\"\n",
    )
    .expect("parses");
    assert_eq!(
        ok.exempt_reason("e", Scenario::Busy, Precondition::InputQueuedWhenRefused),
        Some("r")
    );
    assert_eq!(
        ok.exempt_reason(
            "e",
            Scenario::ClientKeepsWriting,
            Precondition::InputQueuedWhenRefused
        ),
        None
    );
    for (bad, why) in [
        (
            "scenario = \"busy\"\nprecondition = \"nonsense\"\nreason = \"r\"",
            "unknown precondition",
        ),
        (
            "scenario = \"nonsense\"\nprecondition = \"input-queued-when-refused\"\nreason = \"r\"",
            "unknown scenario",
        ),
        (
            "scenario = \"stall\"\nprecondition = \"input-queued-when-refused\"\nreason = \"r\"",
            "not a precondition of",
        ),
        (
            "scenario = \"busy\"\nprecondition = \"input-queued-when-refused\"",
            "no `reason`",
        ),
    ] {
        let text = format!("[[precondition_exempt]]\nendpoint = \"e\"\n{bad}\n");
        let err = Applicability::parse(&text).expect_err(why);
        assert!(err.contains(why), "{why}: {err}");
    }
}

/// The register's exemptions at the start (ADR 0122 §1): dyno-helper busy and pitcrew-service busy, nothing else.
#[test]
fn the_repository_register_exempts_exactly_the_two_busy_scenarios() {
    let applicability =
        Applicability::load_from(env!("CARGO_MANIFEST_DIR")).expect("the register file");
    let mut got: Vec<(String, Scenario, Precondition)> = applicability
        .exempt
        .iter()
        .map(|e| (e.endpoint.clone(), e.scenario, e.precondition))
        .collect();
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        got,
        vec![
            (
                "dyno-helper".to_string(),
                Scenario::Busy,
                Precondition::InputQueuedWhenRefused
            ),
            (
                "pitcrew-service".to_string(),
                Scenario::Busy,
                Precondition::InputQueuedWhenRefused
            ),
        ]
    );
}
