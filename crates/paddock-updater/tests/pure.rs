//! The pure parts: errors, release information, the signature check, the phase machine, the record, release notes and
//! About.

mod support;

use base64::Engine;
use paddock_download::{DownloadError, TransportError, TransportErrorKind};
use paddock_jobs::{CancelReason, JobError};
use paddock_updater::manifest::{self, MAX_MANIFEST_BYTES, MAX_NOTES_CHARS};
use paddock_updater::notes::{self, MAX_CHANGELOG_CHARS};
use paddock_updater::record::{self, FailedInstall, Record, RecordStore};
use paddock_updater::signature::{self, VerifyError};
use paddock_updater::state::{AUTO_CHECK_DELAY_SECS, AUTO_CHECK_INTERVAL_SECS, auto_check_due};
use paddock_updater::*;
use serde_json::{Value, json};
use std::sync::Arc;
use support::*;

fn b64(s: impl AsRef<[u8]>) -> String {
    base64::engine::general_purpose::STANDARD.encode(s)
}

fn info() -> UpdateInfo {
    UpdateInfo {
        version: "9.9.9".into(),
        current_version: "1.0.0".into(),
        notes: "n".into(),
        pub_date: None,
        size: Some(10),
    }
}

// ---- errors ----

#[test]
fn every_error_has_a_kind_a_message_and_a_next_step() {
    let d = || "why".to_string();
    let all = [
        UpdateError::NotAvailable,
        UpdateError::Offline,
        UpdateError::Unreachable { detail: d() },
        UpdateError::NoRelease,
        UpdateError::NoPlatform { detail: d() },
        UpdateError::InvalidManifest { detail: d() },
        UpdateError::SignatureInvalid { detail: d() },
        UpdateError::TooLarge { limit: 512_000_000 },
        UpdateError::DiskFull { detail: d() },
        UpdateError::Storage { detail: d() },
        UpdateError::InstallFailed { detail: d() },
        UpdateError::Busy {
            detail: "Wait.".into(),
        },
        UpdateError::Invalid {
            detail: "Bad.".into(),
        },
    ];
    let mut kinds = std::collections::BTreeSet::new();
    for e in &all {
        let dto = e.to_dto();
        assert!(
            !dto.message.is_empty() && dto.next_step.ends_with('.'),
            "{e:?}"
        );
        assert_eq!(
            serde_json::to_value(e).unwrap(),
            serde_json::to_value(&dto).unwrap()
        );
        kinds.insert(
            serde_json::to_value(dto.kind)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string(),
        );
        match e.detail() {
            Some(detail) => assert_eq!(e.to_string(), format!("{} ({detail})", e.message())),
            None => assert_eq!(e.to_string(), e.message()),
        }
        let _: &dyn std::error::Error = e;
    }
    assert_eq!(kinds.len(), all.len(), "one wire kind per variant");
    assert_eq!(
        UpdateError::TooLarge { limit: 512_000_000 }.message(),
        "The update is larger than 512 MB, so it was not downloaded."
    );
    assert_eq!(
        UpdateError::NotAvailable.message(),
        "Updates are not available in this build."
    );
    assert_eq!(
        UpdateError::Busy {
            detail: "Wait.".into()
        }
        .message(),
        "Wait."
    );
    assert!(!UpdateError::SignatureInvalid { detail: d() }.retry_download());
    assert!(!UpdateError::TooLarge { limit: 1 }.retry_download());
    assert!(!UpdateError::NotAvailable.retry_download());
    assert!(UpdateError::Unreachable { detail: d() }.retry_download());
}

#[test]
fn network_and_download_errors_are_classified_once() {
    let offline = TransportError::new(
        TransportErrorKind::Connect,
        "dns error: failed to lookup address",
    );
    assert_eq!(UpdateError::from_transport(&offline), UpdateError::Offline);
    let refused = TransportError::new(TransportErrorKind::Connect, "connection refused");
    assert!(matches!(
        UpdateError::from_transport(&refused),
        UpdateError::Unreachable { .. }
    ));
    let tls = TransportError::new(TransportErrorKind::Tls, "no route to host");
    assert!(
        matches!(
            UpdateError::from_transport(&tls),
            UpdateError::Unreachable { .. }
        ),
        "only a connect failure is offline"
    );

    let h = || "updates.test".to_string();
    let cases: Vec<(DownloadError, UpdateErrorKind)> = vec![
        (
            DownloadError::Network {
                host: h(),
                detail: "Network is unreachable".into(),
            },
            UpdateErrorKind::Offline,
        ),
        (
            DownloadError::Network {
                host: h(),
                detail: "reset".into(),
            },
            UpdateErrorKind::Unreachable,
        ),
        (
            DownloadError::TooLarge { size: 9, limit: 5 },
            UpdateErrorKind::TooLarge,
        ),
        (DownloadError::DiskFull, UpdateErrorKind::DiskFull),
        (
            DownloadError::NotEnoughSpace { needed: 9, free: 1 },
            UpdateErrorKind::DiskFull,
        ),
        (
            DownloadError::Io(std::io::Error::other("denied")),
            UpdateErrorKind::Storage,
        ),
        (
            DownloadError::InvalidRequest("bad".into()),
            UpdateErrorKind::InvalidManifest,
        ),
        (
            DownloadError::Cancelled(CancelReason::User),
            UpdateErrorKind::Busy,
        ),
        (
            DownloadError::Http {
                host: h(),
                status: 404,
            },
            UpdateErrorKind::Unreachable,
        ),
        (
            DownloadError::Stalled { host: h(), secs: 1 },
            UpdateErrorKind::Unreachable,
        ),
    ];
    for (e, kind) in cases {
        assert_eq!(UpdateError::from_download(&e).kind(), kind, "{e:?}");
    }
    assert_eq!(
        UpdateError::from_download(&DownloadError::TooLarge { size: 9, limit: 5 }),
        UpdateError::TooLarge { limit: 5 }
    );
    assert!(error::looks_offline("Temporary failure in name resolution"));
    assert!(!error::looks_offline("HTTP 500"));
}

// ---- latest.json ----

fn manifest_json() -> Value {
    json!({
        "version": "1.2.0",
        "notes": "## Fixes\n- faster <b>grid</b>\u{0007}",
        "pub_date": "2026-10-17T12:00:00Z",
        "platforms": {
            "windows-x86_64-nsis": { "url": "https://example.net/r/App_1.2.0_x64-setup.exe", "signature": signature("update.bin.sig"), "size": 41_000_000 },
            "windows-x86_64": { "url": "https://example.net/r/App_1.2.0_x64.msi", "signature": signature("update.bin.sig") },
            "darwin-aarch64": { "url": "https://example.net/r/App.app.tar.gz", "signature": signature("update.bin.sig") }
        }
    })
}

#[test]
fn release_information_parses_and_is_sanitized() {
    let m = manifest::parse(&manifest_json().to_string()).unwrap();
    assert_eq!(m.version, semver::Version::new(1, 2, 0));
    // Plain text kept as is (markup is shown, not rendered); the bell is removed.
    assert_eq!(m.notes, "## Fixes\n- faster <b>grid</b>");
    assert_eq!(m.pub_date.as_deref(), Some("2026-10-17T12:00:00Z"));
    assert_eq!(m.platforms.len(), 3);
    assert_eq!(m.raw, manifest_json());
    // A "v" prefix, missing notes and date are fine.
    let mut v = manifest_json();
    v["version"] = json!("v2.0.0-rc.1");
    v.as_object_mut().unwrap().remove("notes");
    v["pub_date"] = Value::Null;
    let m = manifest::from_value(v).unwrap();
    assert_eq!(
        (m.version.to_string(), m.notes, m.pub_date),
        ("2.0.0-rc.1".into(), String::new(), None)
    );
}

#[test]
fn the_package_for_this_system_is_chosen_most_specific_first() {
    let m = manifest::parse(&manifest_json().to_string()).unwrap();
    let win = |installer: Option<&str>| Target {
        os: "windows".into(),
        arch: "x86_64".into(),
        installer: installer.map(String::from),
    };
    assert_eq!(
        m.select(&win(Some("nsis"))).unwrap().0,
        "windows-x86_64-nsis"
    );
    assert_eq!(
        m.select(&win(Some("nsis"))).unwrap().1.size,
        Some(41_000_000)
    );
    assert_eq!(
        m.select(&win(Some("msi"))).unwrap().0,
        "windows-x86_64",
        "falls back to the plain key"
    );
    assert_eq!(m.select(&win(None)).unwrap().0, "windows-x86_64");
    let linux = Target {
        os: "linux".into(),
        arch: "x86_64".into(),
        installer: Some("appimage".into()),
    };
    assert_eq!(
        m.select(&linux).unwrap_err(),
        "the release has no package for linux-x86_64-appimage or linux-x86_64"
    );
    let here = Target::current(Some("NSIS"));
    assert_eq!(here.installer.as_deref(), Some("nsis"));
    assert!(["linux", "darwin", "windows"].contains(&here.os.as_str()) || !here.os.is_empty());
    assert_eq!(here.keys().len(), 2);
    assert!(here.keys()[0].starts_with(&here.keys()[1]));
}

#[test]
fn malformed_release_information_is_refused_with_a_reason() {
    let base = manifest_json;
    let with = |f: &dyn Fn(&mut Value)| {
        let mut v = base();
        f(&mut v);
        manifest::from_value(v).unwrap_err()
    };
    let platform = |f: &dyn Fn(&mut Value)| {
        with(&|v: &mut Value| {
            let p = &mut v["platforms"]["darwin-aarch64"];
            f(p);
        })
    };
    assert!(
        manifest::parse("not json")
            .unwrap_err()
            .starts_with("not JSON")
    );
    assert!(
        manifest::parse(&" ".repeat(MAX_MANIFEST_BYTES + 1))
            .unwrap_err()
            .contains("larger than")
    );
    assert_eq!(
        manifest::from_value(json!([])).unwrap_err(),
        "expected a JSON object"
    );
    assert_eq!(
        with(&|v| {
            v.as_object_mut().unwrap().remove("version");
        }),
        "missing \"version\""
    );
    assert!(with(&|v| v["version"] = json!("one")).contains("not a semantic version"));
    assert_eq!(with(&|v| v["notes"] = json!(3)), "\"notes\" must be text");
    assert_eq!(
        with(&|v| v["pub_date"] = json!("yesterday")),
        "\"pub_date\" is not an RFC 3339 date"
    );
    assert_eq!(
        with(&|v| v["pub_date"] = json!(1)),
        "\"pub_date\" must be text"
    );
    assert_eq!(
        with(&|v| {
            v.as_object_mut().unwrap().remove("platforms");
        }),
        "missing \"platforms\""
    );
    assert_eq!(
        with(&|v| v["platforms"] = json!({})),
        "\"platforms\" is empty"
    );
    let many: serde_json::Map<String, Value> =
        (0..65).map(|i| (format!("p{i}"), json!({}))).collect();
    assert_eq!(
        with(&|v| v["platforms"] = Value::Object(many.clone())),
        "more than 64 platforms"
    );
    assert!(
        with(&|v| v["platforms"] = json!({ "Bad Key": {} })).ends_with("invalid platform name")
    );
    assert!(with(&|v| v["platforms"] = json!({ "": {} })).ends_with("invalid platform name"));
    assert!(platform(&|p| *p = json!(1)).ends_with("expected an object"));
    assert!(
        platform(&|p| {
            p.as_object_mut().unwrap().remove("url");
        })
        .ends_with("missing \"url\"")
    );
    assert!(
        platform(&|p| p["url"] = json!(format!("https://x/{}", "a".repeat(3000))))
            .ends_with("\"url\" is too long")
    );
    assert!(
        platform(&|p| p["url"] = json!("http://example.net/a")).ends_with("\"url\" must use https")
    );
    assert!(platform(&|p| p["url"] = json!("https://")).contains("\"url\": cannot download"));
    assert!(
        platform(&|p| {
            p.as_object_mut().unwrap().remove("signature");
        })
        .ends_with("missing \"signature\"")
    );
    assert!(platform(&|p| p["signature"] = json!(" ")).ends_with("\"signature\" is empty"));
    assert!(
        platform(&|p| p["signature"] = json!("a".repeat(9000)))
            .ends_with("\"signature\" is too long")
    );
    assert!(platform(&|p| p["signature"] = json!("***")).ends_with("\"signature\" is not base64"));
    assert!(
        platform(&|p| p["signature"] = json!(b64([0xff, 0xfe])))
            .ends_with("\"signature\" is not a minisign signature")
    );
    assert!(
        platform(&|p| p["signature"] = json!(b64("hello")))
            .ends_with("\"signature\" is not a minisign signature")
    );
    assert!(
        platform(&|p| p["size"] = json!(-1)).ends_with("\"size\" must be a whole number of bytes")
    );
    assert!(
        manifest::from_value({
            let mut v = base();
            v["platforms"]["darwin-aarch64"]["size"] = Value::Null;
            v
        })
        .is_ok()
    );
}

#[test]
fn dates_and_notes_are_checked_and_bounded() {
    for ok in [
        "2026-10-17T12:00:00Z",
        "2026-10-17t12:00:00z",
        "2026-10-17 12:00:00.123+02:00",
        "2026-10-17T12:00:00-05:30",
    ] {
        let mut v = manifest_json();
        v["pub_date"] = json!(ok);
        assert!(manifest::from_value(v).is_ok(), "{ok}");
    }
    for bad in [
        "2026-10-17",
        "2026-10-17T12:00:00",
        "2026-10-17T12:00:00.Z",
        "2026-10-17T12:00:00+0200",
        "2026-10-17T12:00:00+aa:bb",
        "2026-10-17X12:00:00Z",
        "2026-10-17T12:00:00.000000000000000000000000Z",
    ] {
        let mut v = manifest_json();
        v["pub_date"] = json!(bad);
        assert!(manifest::from_value(v).is_err(), "{bad}");
    }
    let long = "a".repeat(MAX_NOTES_CHARS + 50);
    assert_eq!(
        manifest::sanitize_notes(&long).chars().count(),
        MAX_NOTES_CHARS + 1
    );
    assert_eq!(
        manifest::sanitize_notes("  a\tb\nc\u{1b}[0m  "),
        "a\tb\nc[0m"
    );
}

// ---- the signature ----

#[test]
fn a_package_signed_for_the_announced_version_passes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("p");
    std::fs::write(&file, package()).unwrap();
    let v = semver::Version::new(9, 9, 9);
    assert_eq!(
        signature::verify_file(&key(), &file, &signature("update.bin.sig"), &v, None),
        Ok(())
    );
    // With a job: the bytes are counted, and a cancel stops the check.
    let (reg, _) = jobs();
    let ctx = job(&reg);
    ctx.phase(paddock_jobs::Phase::bytes(
        SIGNATURE_PHASE,
        "Checking the signature",
        Some(package().len() as u64),
    ));
    assert_eq!(
        signature::verify_file(&key(), &file, &signature("update.bin.sig"), &v, Some(&ctx)),
        Ok(())
    );
    assert_eq!(
        ctx.reporter().snapshot().state,
        paddock_jobs::JobState::Running {
            phase: Some(paddock_jobs::PhaseProgress {
                phase: paddock_jobs::Phase::bytes(
                    SIGNATURE_PHASE,
                    "Checking the signature",
                    Some(package().len() as u64)
                ),
                done: package().len() as u64,
            })
        }
    );
    reg.cancel(ctx.id());
    assert_eq!(
        signature::verify_file(&key(), &file, &signature("update.bin.sig"), &v, Some(&ctx)),
        Err(VerifyError::Cancelled)
    );
}

fn refusal(file_bytes: &[u8], sig: &str, version: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("p");
    std::fs::write(&file, file_bytes).unwrap();
    match signature::verify_file(
        &key(),
        &file,
        sig,
        &semver::Version::parse(version).unwrap(),
        None,
    ) {
        Err(VerifyError::Invalid(UpdateError::SignatureInvalid { detail })) => detail,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_bad_signature_is_refused() {
    let mut tampered = package();
    tampered[100] ^= 1;
    assert_eq!(
        refusal(&tampered, &signature("update.bin.sig"), "9.9.9"),
        "the file does not match its signature"
    );
    assert_eq!(
        refusal(&package(), &signature("other-key.sig"), "9.9.9"),
        "the update was signed with another key than this app's"
    );
    assert_eq!(
        refusal(&package(), &signature("other-version.sig"), "9.9.9"),
        "the signature was made for version 9.9.8, but the release information announces 9.9.9"
    );
    assert_eq!(
        refusal(&package(), &signature("no-version.sig"), "9.9.9"),
        "the signature does not name the version it was made for"
    );
    // A legacy (not pre-hashed) signature: the same signature with its algorithm changed to "Ed".
    let text = String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(signature("update.bin.sig"))
            .unwrap(),
    )
    .unwrap();
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let mut raw = base64::engine::general_purpose::STANDARD
        .decode(&lines[1])
        .unwrap();
    raw[1] = b'd';
    lines[1] = b64(raw);
    let legacy = b64(lines.join("\n") + "\n");
    assert_eq!(
        refusal(&package(), &legacy, "9.9.9"),
        "legacy (not pre-hashed) signatures are not accepted"
    );
    // Not a signature at all.
    assert_eq!(
        refusal(&package(), "", "9.9.9"),
        "the signature is empty or too long"
    );
    assert_eq!(
        refusal(&package(), "***", "9.9.9"),
        "the signature is not base64"
    );
    assert_eq!(
        refusal(&package(), &b64([0xffu8, 0xfe]), "9.9.9"),
        "the signature is not text"
    );
    assert!(
        refusal(&package(), &b64("hello"), "9.9.9").starts_with("the signature cannot be read")
    );
    // An unreadable file.
    let missing = signature::verify_file(
        &key(),
        std::path::Path::new("/nonexistent/p"),
        &signature("update.bin.sig"),
        &semver::Version::new(9, 9, 9),
        None,
    );
    assert!(matches!(missing.clone(), Err(VerifyError::Io(_))));
    assert!(matches!(
        missing.unwrap_err().into_update_error(),
        UpdateError::Storage { .. }
    ));
    assert!(matches!(
        VerifyError::Cancelled.into_update_error(),
        UpdateError::Busy { .. }
    ));
    assert!(matches!(
        VerifyError::Invalid(UpdateError::SignatureInvalid { detail: "x".into() })
            .into_update_error(),
        UpdateError::SignatureInvalid { .. }
    ));
}

#[test]
fn the_signed_version_is_compared_as_a_semantic_version() {
    let v = semver::Version::new(1, 2, 3);
    assert!(signature::check_signed_version("timestamp:1\tfile:a\tversion:v1.2.3", &v).is_ok());
    assert!(signature::check_signed_version("timestamp:1\tversion:1.2.4", &v).is_err());
    assert!(
        signature::check_signed_version("version:latest", &v)
            .unwrap_err()
            .contains("made for version latest")
    );
    assert!(signature::check_signed_version("timestamp:1\tfile:app-version:2.zip", &v).is_err());
}

#[test]
fn the_update_key_is_parsed_once_and_resolved_from_the_first_given() {
    assert!(UpdateKey::parse(&key_text()).is_ok());
    assert_eq!(format!("{:?}", key()), "UpdateKey(..)");
    assert!(UpdateKey::parse("***").unwrap_err().contains("not base64"));
    assert!(
        UpdateKey::parse(&b64("untrusted comment: x\nnot-a-key\n"))
            .unwrap_err()
            .contains("not a minisign public key")
    );
    assert!(UpdateKey::resolve([None, Some("  "), Some(key_text().as_str())]).is_some());
    assert!(
        UpdateKey::resolve([Some("garbage")]).is_none(),
        "an invalid key means no updates, not another key"
    );
    assert!(UpdateKey::resolve([None, None]).is_none());
}

// ---- the phase machine ----

#[test]
fn the_happy_path_goes_through_every_phase() {
    use UpdatePhase as P;
    let p = P::Idle.next(Event::Check).unwrap();
    assert_eq!(p, P::Checking { job: None });
    let p = p.next(Event::Job(4)).unwrap();
    assert_eq!(p, P::Checking { job: Some(4) });
    let p = p.next(Event::Checked(Ok(Some(info())))).unwrap();
    assert_eq!(p, P::Available { info: info() });
    let p = p.next(Event::Download).unwrap();
    assert_eq!(
        p.next(Event::Job(5)).unwrap(),
        P::Downloading {
            info: info(),
            job: Some(5)
        }
    );
    let p = p.next(Event::Downloaded(Ok(()))).unwrap();
    assert_eq!(
        p,
        P::Ready {
            info: info(),
            install_on_quit: false
        }
    );
    let p = p.next(Event::InstallOnQuit(true)).unwrap();
    assert_eq!(
        p,
        P::Ready {
            info: info(),
            install_on_quit: true
        }
    );
    let p = p.next(Event::Install).unwrap();
    assert_eq!(p, P::Installing { info: info() });
    let e = UpdateError::InstallFailed { detail: "x".into() };
    assert_eq!(
        p.next(Event::InstallFailed(e.clone())).unwrap(),
        P::Failed {
            error: e,
            info: Some(info())
        }
    );
    assert_eq!(
        P::Checking { job: None }
            .next(Event::Checked(Ok(None)))
            .unwrap(),
        P::UpToDate
    );
    assert_eq!(
        P::Checking { job: None }
            .next(Event::CheckCancelled)
            .unwrap(),
        P::Idle
    );
    assert!(matches!(
        P::Checking { job: None }
            .next(Event::Checked(Err(UpdateError::Offline)))
            .unwrap(),
        P::Failed { info: None, .. }
    ));
}

#[test]
fn steps_that_do_not_fit_are_refused_and_change_nothing() {
    use UpdatePhase as P;
    let downloading = P::Downloading {
        info: info(),
        job: None,
    };
    let ready = P::Ready {
        info: info(),
        install_on_quit: false,
    };
    let failed = |e: UpdateError| P::Failed {
        error: e,
        info: Some(info()),
    };
    assert_eq!(
        P::Unavailable.next(Event::Check),
        Err(UpdateError::NotAvailable)
    );
    for (phase, event) in [
        (P::Checking { job: None }, Event::Check),
        (downloading.clone(), Event::Check),
        (ready.clone(), Event::Check),
        (P::Idle, Event::Download),
        (P::Idle, Event::Cancel),
        (P::Idle, Event::Install),
        (P::Available { info: info() }, Event::InstallOnQuit(true)),
        (P::Idle, Event::Job(1)),
        (P::Idle, Event::Checked(Ok(None))),
        (P::Idle, Event::CheckCancelled),
        (P::Available { info: info() }, Event::Downloaded(Ok(()))),
        (P::Idle, Event::InstallFailed(UpdateError::Offline)),
        (downloading.clone(), Event::Reset),
        (
            failed(UpdateError::SignatureInvalid { detail: "x".into() }),
            Event::Download,
        ),
        (failed(UpdateError::TooLarge { limit: 1 }), Event::Download),
    ] {
        assert!(
            matches!(phase.next(event.clone()), Err(UpdateError::Busy { .. })),
            "{phase:?} + {event:?}"
        );
    }
    // A failure worth retrying downloads again; cancelling a download or a verified update keeps it available.
    assert!(matches!(
        failed(UpdateError::Offline).next(Event::Download),
        Ok(P::Downloading { .. })
    ));
    assert_eq!(
        downloading.next(Event::Cancel).unwrap(),
        P::Available { info: info() }
    );
    assert_eq!(
        ready.next(Event::Cancel).unwrap(),
        P::Available { info: info() }
    );
    assert!(matches!(
        downloading.next(Event::Downloaded(Err(UpdateError::Offline))),
        Ok(P::Failed { info: Some(_), .. })
    ));
    for phase in [
        P::Idle,
        P::UpToDate,
        P::Available { info: info() },
        failed(UpdateError::Offline),
    ] {
        assert!(phase.can_check());
        assert_eq!(phase.next(Event::Reset).unwrap(), P::Idle);
        assert!(matches!(phase.next(Event::Check), Ok(P::Checking { .. })));
    }
    assert!(!ready.can_check() && !P::Unavailable.can_check());
    assert_eq!(ready.info(), Some(&info()));
    assert_eq!(P::Installing { info: info() }.info(), Some(&info()));
    assert_eq!(failed(UpdateError::Offline).info(), Some(&info()));
    assert_eq!(P::Idle.info(), None);
    assert_eq!(P::Available { info: info() }.info(), Some(&info()));
}

#[test]
fn the_automatic_check_waits_for_start_up_and_runs_daily() {
    let day = AUTO_CHECK_INTERVAL_SECS;
    let up = AUTO_CHECK_DELAY_SECS;
    assert!(auto_check_due(true, &UpdatePhase::Idle, up, None));
    assert!(auto_check_due(true, &UpdatePhase::UpToDate, up, Some(day)));
    assert!(
        !auto_check_due(false, &UpdatePhase::Idle, up, None),
        "off by default"
    );
    assert!(
        !auto_check_due(true, &UpdatePhase::Idle, up - 1, None),
        "not during start-up"
    );
    assert!(!auto_check_due(true, &UpdatePhase::Idle, up, Some(day - 1)));
    assert!(
        !auto_check_due(true, &UpdatePhase::Available { info: info() }, up, None),
        "an update already waits"
    );
    assert!(!auto_check_due(
        true,
        &UpdatePhase::Checking { job: None },
        up,
        None
    ));
}

#[test]
fn the_wire_form_of_a_phase_is_tagged() {
    let json = serde_json::to_value(UpdatePhase::Ready {
        info: info(),
        install_on_quit: true,
    })
    .unwrap();
    assert_eq!(json["kind"], "ready");
    assert_eq!(json["installOnQuit"], true);
    assert_eq!(json["info"]["currentVersion"], "1.0.0");
    let failed = serde_json::to_value(UpdatePhase::Failed {
        error: UpdateError::Offline,
        info: None,
    })
    .unwrap();
    assert_eq!(failed["error"]["kind"], "offline");
    assert_eq!(
        failed["error"]["nextStep"],
        UpdateError::Offline.next_step()
    );
}

// ---- the record ----

#[test]
fn the_record_survives_restarts_and_falls_back_to_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let store = RecordStore::new(Some(dir.path()));
    assert_eq!(store.load(), Record::default());
    let ((), saved) = store.update(|r| {
        r.whats_new_seen = Some("1.0.0".into());
        r.last_checked = Some(7);
    });
    assert!(saved.is_ok());
    let again = RecordStore::new(Some(dir.path())).load();
    assert_eq!(
        (
            again.whats_new_seen.as_deref(),
            again.last_checked,
            again.version
        ),
        (Some("1.0.0"), Some(7), record::VERSION)
    );
    let path = dir.path().join(record::FILE);
    for bad in ["{", "[]", r#"{"version": 99, "lastChecked": 1}"#] {
        std::fs::write(&path, bad).unwrap();
        assert_eq!(store.load(), Record::default(), "{bad}");
    }
    std::fs::write(&path, vec![b' '; 70 * 1024]).unwrap();
    assert_eq!(store.load(), Record::default(), "oversized");
    // Unknown and missing fields are fine (forward compatible within a version).
    std::fs::write(
        &path,
        r#"{"version": 1, "whatsNewSeen": "1.0.0", "later": 1}"#,
    )
    .unwrap();
    assert_eq!(store.load().whats_new_seen.as_deref(), Some("1.0.0"));
    // A folder where the file should be, and a parent that is a file: defaults, and the write error is returned.
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(store.load(), Record::default());
    let file_parent = dir.path().join("plain");
    std::fs::write(&file_parent, "x").unwrap();
    assert!(
        RecordStore::new(Some(&file_parent))
            .update(|_| ())
            .1
            .is_err()
    );
    // Without a folder nothing persists and nothing fails.
    let memory = RecordStore::new(None);
    assert!(memory.update(|r| r.last_checked = Some(1)).1.is_ok());
    assert!(memory.load().last_checked.is_none());
}

#[test]
fn a_failed_install_is_kept_short_and_shown_as_an_install_failure() {
    let long = "x".repeat(5000);
    let f = FailedInstall::of("9.9.9", &UpdateError::InstallFailed { detail: long });
    assert_eq!(f.detail.chars().count(), 2048);
    assert!(
        matches!(f.error(), UpdateError::InstallFailed { detail } if detail.starts_with("version 9.9.9: xxx"))
    );
    let f = FailedInstall::of("9.9.9", &UpdateError::Offline);
    assert_eq!(f.detail, UpdateError::Offline.message());
}

// ---- release notes ----

const CHANGELOG: &str = "# Changelog

Intro text.

## Unreleased

### Added
- not released yet

## [1.2.0] - 2026-10-24

### Added
- **Updates** in the app: check,
  download and verify.
- Second item
* Third item

### Empty

## 1.1.0 (2026-10-20)

### Fixed
- one fix

## v1.0.0

### Changed
- first

## not-a-version

### Added
- ignored
";

#[test]
fn the_changelog_is_parsed_in_its_usual_forms() {
    let all = notes::parse(CHANGELOG);
    let versions: Vec<_> = all
        .iter()
        .map(|r| (r.version.as_str(), r.date.as_deref()))
        .collect();
    assert_eq!(
        versions,
        [
            ("1.2.0", Some("2026-10-24")),
            ("1.1.0", Some("2026-10-20")),
            ("1.0.0", None)
        ]
    );
    assert_eq!(all[0].groups.len(), 1, "an empty group is dropped");
    assert_eq!(
        all[0].groups[0],
        NotesGroup {
            title: "Added".into(),
            items: vec![
                "Updates in the app: check, download and verify.".into(),
                "Second item".into(),
                "Third item".into()
            ],
        }
    );
    assert!(notes::parse("").is_empty());
    assert!(
        notes::parse("## [1.0.0\n### A\n- x").is_empty(),
        "an unclosed bracket is no release"
    );
    assert_eq!(notes::parse("## 1.0.0 - someday\n### A\n- x")[0].date, None);
}

#[test]
fn what_is_new_is_the_running_version_only_never_the_newest() {
    assert_eq!(
        notes::release_for(CHANGELOG, "1.1.0").unwrap().groups[0].items,
        ["one fix"]
    );
    assert_eq!(
        notes::release_for(CHANGELOG, "v1.2.0").unwrap().version,
        "1.2.0"
    );
    // A build between releases (or a version the changelog lacks) has no notes: not 1.2.0's.
    assert_eq!(notes::release_for(CHANGELOG, "1.3.0"), None);
    assert_eq!(notes::release_for(CHANGELOG, "1.2.0-dev.1"), None);
    assert_eq!(notes::release_for(CHANGELOG, "garbage"), None);
}

#[test]
fn what_is_new_opens_once_after_an_update_to_a_version_with_notes() {
    use WhatsNewDecision as D;
    assert_eq!(
        notes::decide(None, "1.2.0", true),
        D::Remember,
        "a first run has nothing new"
    );
    assert_eq!(notes::decide(Some("1.1.0"), "1.2.0", true), D::Show);
    assert_eq!(notes::decide(Some("1.1.0"), "1.2.0", false), D::Remember);
    assert_eq!(notes::decide(Some("1.2.0"), "1.2.0", true), D::Nothing);
}

#[test]
fn the_changelog_is_bounded() {
    let mut big = String::from("## 1.0.0\n### Added\n");
    for i in 0..600 {
        big.push_str(&format!("- item {i}\n"));
    }
    big.push_str(&format!("- {}\n  {}\n", "a".repeat(5000), "b".repeat(10)));
    for i in 0..40 {
        big.push_str(&format!("### G{i}\n- x\n"));
    }
    let r = &notes::parse(&big)[0];
    assert_eq!(r.groups[0].items.len(), 500);
    assert!(r.groups.len() <= 32);
    let mut long = String::from("## 1.0.0\n### Added\n- start\n");
    long.push_str(&format!("  {}\n", "w".repeat(5000)));
    let item = &notes::parse(&long)[0].groups[0].items[0];
    assert_eq!(item.chars().count(), 4001, "cut with an ellipsis");
    assert!(item.ends_with('…'));
    let huge = format!("## 1.0.0\n### A\n- x\n{}", "y".repeat(MAX_CHANGELOG_CHARS));
    assert_eq!(notes::parse(&huge).len(), 1);
    assert!(
        notes::parse("## 1.0.0\n- before any group\n  wrapped\n")[0]
            .groups
            .is_empty()
    );
}

// ---- About ----

#[test]
fn about_has_the_running_version_build_facts_and_legal_texts() {
    let records = Arc::new(RecordStore::new(None));
    let about = About::new("Demo", "1.2.0", records)
        .build(Some(" abc123 "), Some(""))
        .homepage("https://example.net")
        .releases_page("https://example.net/releases")
        .updates_available(true)
        .legal("notices", "Third-party notices", "# Notices")
        .unwrap()
        .legal("mit", "MIT licence", String::from("MIT License"))
        .unwrap()
        .legal("notices", "Notices", "# Notices v2")
        .unwrap()
        .changelog(CHANGELOG);
    let info = about.info();
    assert_eq!(
        (info.app_name.as_str(), info.version.as_str()),
        ("Demo", "1.2.0")
    );
    assert_eq!((info.commit.as_deref(), info.built), (Some("abc123"), None));
    assert_eq!(info.os, std::env::consts::OS);
    assert_eq!(info.arch, std::env::consts::ARCH);
    assert!(info.updates_available);
    assert_eq!(
        (info.homepage.as_deref(), info.releases_page.as_deref()),
        (
            Some("https://example.net"),
            Some("https://example.net/releases")
        )
    );
    let ids: Vec<_> = info
        .legal
        .iter()
        .map(|d| (d.id.as_str(), d.title.as_str()))
        .collect();
    assert_eq!(
        ids,
        [("mit", "MIT licence"), ("notices", "Notices")],
        "a second document with the same id replaces the first"
    );
    assert_eq!(about.legal_text("notices").unwrap(), "# Notices v2");
    let e = about.legal_text(&"x".repeat(500)).unwrap_err();
    assert_eq!(
        e.to_string(),
        format!("there is no legal document '{}'", "x".repeat(32))
    );
    let _: &dyn std::error::Error = &e;
    for bad in ["", "Notices", "a b", &"a".repeat(33)] {
        assert!(
            About::new("D", "1.0.0", Arc::new(RecordStore::new(None)))
                .legal(bad, "t", "x")
                .is_err(),
            "{bad:?}"
        );
    }
    assert_eq!(about.whats_new().release.unwrap().version, "1.2.0");
}

#[test]
fn whats_new_at_start_is_decided_once_and_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let records = Arc::new(RecordStore::new(Some(dir.path())));
    let at = |version: &str| About::new("Demo", version, records.clone()).changelog(CHANGELOG);
    assert_eq!(
        at("1.1.0").whats_new_on_start(),
        WhatsNewDecision::Remember,
        "first run"
    );
    assert_eq!(at("1.1.0").whats_new_on_start(), WhatsNewDecision::Nothing);
    assert_eq!(
        at("1.2.0").whats_new_on_start(),
        WhatsNewDecision::Show,
        "updated to a version with notes"
    );
    assert_eq!(
        at("1.2.0").whats_new_on_start(),
        WhatsNewDecision::Nothing,
        "only once"
    );
    assert_eq!(
        at("1.3.0").whats_new_on_start(),
        WhatsNewDecision::Remember,
        "no notes for this version: nothing shown"
    );
    let w = at("1.3.0").whats_new();
    assert_eq!((w.version.as_str(), w.release), ("1.3.0", None));
    let _ = JobError::Failed(String::new());
}
