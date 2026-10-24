//! The engine side of the settings page: settings, secrets and test sends as jobs.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use crate::common;
use common::*;
use paddock_channels::*;
use paddock_download::TransportError;
use paddock_secrets::{MemorySecrets, Secrets};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A poster that holds every request until the test lets it go.
struct Gate {
    entered: Mutex<Sender<()>>,
    release: Mutex<Receiver<()>>,
}

impl HttpPoster for Gate {
    fn post(&self, _r: &PostRequest) -> Result<PostResponse, TransportError> {
        self.entered.lock().unwrap().send(()).unwrap();
        let _ = self
            .release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(30));
        Ok(PostResponse {
            status: 200,
            retry_after: None,
        })
    }
}

struct Rig {
    control: Arc<ChannelsControl>,
    store: Arc<MemorySettings>,
    secrets: Arc<MemorySecrets>,
    events: Receiver<ChannelsEvent>,
}

fn rig_with(poster: Arc<dyn HttpPoster>, mailer: Arc<dyn MailSender>) -> Rig {
    let store = Arc::new(MemorySettings::default());
    let secrets = store_secrets();
    let (tx, events) = channel();
    let tx = Mutex::new(tx);
    let emit: Emit = Arc::new(move |e| tx.lock().unwrap().send(e.clone()).unwrap());
    let control = ChannelsControl::new(
        "Pitcrew",
        store.clone(),
        secrets.clone(),
        poster,
        mailer,
        Arc::new(RecordingClock::default()),
        emit,
    );
    Rig {
        control,
        store,
        secrets,
        events,
    }
}

fn store_secrets() -> Arc<MemorySecrets> {
    store()
}

fn rig() -> Rig {
    rig_with(Arc::new(FakePoster::new()), Arc::new(FakeMail::new()))
}

fn settings(channels: Vec<ChannelConfig>) -> Settings {
    Settings { channels }
}

/// The next event that is the outcome of a test (progress events are skipped).
fn next(r: &Rig) -> ChannelsEvent {
    loop {
        let e = r
            .events
            .recv_timeout(Duration::from_secs(30))
            .expect("the test reports");
        if matches!(e, ChannelsEvent::Test { .. }) {
            return e;
        }
    }
}

#[test]
fn an_empty_install_shows_no_channels() {
    let r = rig();
    let v = r.control.get().unwrap();
    assert!(v.settings.channels.is_empty() && v.secret_set.is_empty());
    assert_eq!(r.control.running(), 0);
}

#[test]
fn saving_validates_persists_and_reports_which_secrets_are_stored() {
    let r = rig();
    let mut bad = webhook("w", true);
    bad.transport = Transport::Webhook {
        url: "nonsense".into(),
        auth: true,
    };
    let e = r.control.save(settings(vec![bad])).unwrap_err();
    assert_eq!((e.code, e.retryable), (ErrorCode::Invalid, false));
    assert!(
        e.message.contains("must start with https://"),
        "{}",
        e.message
    );
    assert!(
        r.control.get().unwrap().settings.channels.is_empty(),
        "nothing was saved"
    );

    let (w, n, m) = (
        webhook("w", true),
        ntfy("n", false),
        email("m", Some("bot")),
    );
    r.control.set_secret("w", "tok").unwrap();
    let v = r.control.save(settings(vec![w, n, m])).unwrap();
    assert_eq!(v.secret_set.get("w"), Some(&true));
    assert_eq!(
        v.secret_set.get("m"),
        Some(&false),
        "needs a password, none stored yet"
    );
    assert!(
        !v.secret_set.contains_key("n"),
        "a channel without auth needs no secret"
    );
    assert_eq!(r.control.get().unwrap(), v);
    let json = serde_json::to_value(&v).unwrap();
    assert_eq!(json["secretSet"]["w"], true);
    assert!(json["settings"]["channels"][0]["minSeverity"].is_string());
}

#[test]
fn removing_a_channel_or_its_auth_deletes_its_secret_but_a_failed_save_keeps_it() {
    let r = rig();
    let (a, b) = (webhook("a", true), webhook("b", true));
    r.control
        .save(settings(vec![a.clone(), b.clone()]))
        .unwrap();
    r.control.set_secret("a", "ta").unwrap();
    r.control.set_secret("b", "tb").unwrap();
    *r.store.fail.lock().unwrap() = Some("disk full".into());
    let e = r.control.save(settings(vec![b.clone()])).unwrap_err();
    assert_eq!(
        (e.code, e.retryable, e.message.as_str()),
        (ErrorCode::StoreUnavailable, true, "disk full")
    );
    assert!(
        r.secrets.get(&a.secret_name().unwrap()).unwrap().is_some(),
        "the save failed, so nothing was removed"
    );
    *r.store.fail.lock().unwrap() = None;
    r.control.save(settings(vec![b.clone()])).unwrap();
    assert!(r.secrets.get(&a.secret_name().unwrap()).unwrap().is_none());
    assert!(r.secrets.get(&b.secret_name().unwrap()).unwrap().is_some());
    r.control.save(settings(vec![webhook("b", false)])).unwrap();
    assert!(
        r.secrets.get(&b.secret_name().unwrap()).unwrap().is_none(),
        "no longer needs one"
    );
}

#[test]
fn secrets_are_checked_and_the_store_being_locked_is_a_retryable_error() {
    let r = rig();
    for (id, value) in [("bad id/\u{7}", "x"), ("w", ""), ("w", "two\nlines")] {
        let e = r.control.set_secret(id, value).unwrap_err();
        assert_eq!(
            (e.code, e.retryable),
            (ErrorCode::Invalid, false),
            "{id:?} {value:?}"
        );
        assert!(!e.message.contains("two\nlines"));
    }
    assert_eq!(
        r.control.clear_secret(&"x".repeat(200)).unwrap_err().code,
        ErrorCode::Invalid
    );
    r.control.clear_secret("w").unwrap();
    r.control.save(settings(vec![webhook("w", true)])).unwrap();
    r.secrets.set_unavailable(true);
    for e in [
        r.control.get().unwrap_err(),
        r.control.set_secret("w", "t").unwrap_err(),
        r.control.clear_secret("w").unwrap_err(),
    ] {
        assert_eq!((e.code, e.retryable), (ErrorCode::StoreUnavailable, true));
        assert!(e.message.contains("Unlock"), "{}", e.message);
    }
    r.secrets.set_unavailable(false);
    assert!(r.control.get().is_ok());
}

#[test]
fn a_test_returns_at_once_and_reports_its_outcome_as_an_event() {
    let r = rig();
    let cfg = webhook("w", true);
    r.control.set_secret("w", "tok").unwrap();
    let id = r.control.test(cfg.clone()).unwrap();
    let ChannelsEvent::Test {
        test_id,
        channel_id,
        ok,
        attempts,
        message,
    } = next(&r);
    assert_eq!(
        (test_id, channel_id.as_str(), ok, attempts, message),
        (id, "w", true, 1, None)
    );
    assert_eq!(r.control.running(), 0);
    let json = serde_json::to_value(ChannelsEvent::Test {
        test_id: 1,
        channel_id: "w".into(),
        ok: false,
        attempts: 1,
        message: Some("x".into()),
    })
    .unwrap();
    assert_eq!(
        (
            json["kind"].as_str(),
            json["testId"].as_u64(),
            json["channelId"].as_str()
        ),
        (Some("test"), Some(1), Some("w"))
    );
}

#[test]
fn a_failed_test_says_what_to_do_and_a_missing_secret_is_named() {
    let poster = FakePoster::new().then_status(401);
    let r = rig_with(Arc::new(poster), Arc::new(FakeMail::new()));
    let cfg = webhook("w", true);
    r.control.set_secret("w", "tok").unwrap();
    r.control.test(cfg.clone()).unwrap();
    let ChannelsEvent::Test { ok, message, .. } = next(&r);
    assert!(!ok && message.unwrap().contains("HTTP 401"));
    r.control.clear_secret("w").unwrap();
    r.control.test(cfg).unwrap();
    let ChannelsEvent::Test { ok, message, .. } = next(&r);
    assert!(!ok && message.unwrap().contains("not stored"));
    let mut bad = webhook("w", false);
    bad.transport = Transport::Webhook {
        url: "nonsense".into(),
        auth: false,
    };
    r.control.test(bad).unwrap();
    let ChannelsEvent::Test {
        ok,
        attempts,
        message,
        ..
    } = next(&r);
    assert!(!ok && attempts == 0 && message.unwrap().contains("https://"));
}

#[test]
fn a_channel_is_tested_once_at_a_time_and_only_a_few_run_at_once() {
    let (entered_tx, entered) = channel();
    let (release_tx, release) = channel();
    let gate = Arc::new(Gate {
        entered: Mutex::new(entered_tx),
        release: Mutex::new(release),
    });
    let r = rig_with(gate, Arc::new(FakeMail::new()));
    r.control.test(webhook("a", false)).unwrap();
    entered.recv_timeout(Duration::from_secs(30)).unwrap();
    let e = r.control.test(webhook("a", false)).unwrap_err();
    assert_eq!((e.code, e.retryable), (ErrorCode::Busy, true));
    assert_eq!(r.control.running(), 1);
    release_tx.send(()).unwrap();
    assert!(matches!(next(&r), ChannelsEvent::Test { ok: true, .. }));
    // Fill the pool: MAX_TESTS gated tests, one more is refused.
    for i in 0..MAX_TESTS {
        r.control.test(webhook(&format!("c{i}"), false)).unwrap();
        entered.recv_timeout(Duration::from_secs(30)).unwrap();
    }
    assert_eq!(
        r.control.test(webhook("extra", false)).unwrap_err().code,
        ErrorCode::Busy
    );
    for _ in 0..MAX_TESTS {
        release_tx.send(()).unwrap();
        next(&r);
    }
    assert_eq!(r.control.running(), 0);
}

#[test]
fn cancel_at_exit_still_lets_every_test_report() {
    let (entered_tx, entered) = channel();
    let (release_tx, release) = channel();
    let gate = Arc::new(Gate {
        entered: Mutex::new(entered_tx),
        release: Mutex::new(release),
    });
    let r = rig_with(gate, Arc::new(FakeMail::new()));
    r.control.test(webhook("a", false)).unwrap();
    entered.recv_timeout(Duration::from_secs(30)).unwrap();
    r.control.cancel();
    release_tx.send(()).unwrap();
    assert!(matches!(next(&r), ChannelsEvent::Test { .. }));
    assert_eq!(r.control.running(), 0);
}

#[test]
fn the_app_sends_its_alerts_through_the_same_control() {
    let r = rig();
    r.control.save(settings(vec![webhook("w", false)])).unwrap();
    let out = r.control.dispatch(
        &message("diskLow", Severity::Error),
        &paddock_jobs::CancelToken::new(),
    );
    assert_eq!(out.len(), 1);
    assert!(out[0].result.is_ok());
    assert_eq!(r.control.dispatcher().settings().channels.len(), 1);
}

#[test]
fn file_settings_round_trip_and_a_damaged_file_gives_the_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("channels.json");
    let f = FileSettings::new(&path);
    assert!(f.load().channels.is_empty());
    f.save(&settings(vec![webhook("w", true)])).unwrap();
    assert_eq!(FileSettings::new(&path).load().channels[0].id, "w");
    std::fs::write(&path, b"{ not json").unwrap();
    assert!(
        FileSettings::new(&path).load().channels.is_empty(),
        "damaged: defaults, the file is kept aside"
    );
    let file = dir.path().join("a-file");
    std::fs::write(&file, b"x").unwrap();
    let e = FileSettings::new(file.join("c.json"))
        .save(&Settings::default())
        .unwrap_err();
    assert!(e.contains("could not be saved"), "{e}");
}

#[test]
fn a_running_test_reports_each_attempt_and_wait_and_the_numbers_only_go_up() {
    let poster = FakePoster::new().then_status(503).then_status(200);
    let r = rig_with(Arc::new(poster), Arc::new(FakeMail::new()));
    let id = r.control.test(webhook("w", false)).unwrap();
    let mut seen = Vec::new();
    let end = loop {
        match r.events.recv_timeout(Duration::from_secs(30)).unwrap() {
            ChannelsEvent::TestProgress {
                test_id,
                channel_id,
                attempt,
                of,
                phase,
                wait_secs,
            } => {
                assert_eq!((test_id, channel_id.as_str(), of), (id, "w", 3));
                seen.push((attempt, phase, wait_secs));
            }
            done @ ChannelsEvent::Test { .. } => break done,
        }
    };
    assert_eq!(
        seen,
        [
            (1, TestPhase::Sending, None),
            (1, TestPhase::Waiting, Some(2)),
            (2, TestPhase::Sending, None)
        ]
    );
    assert!(matches!(
        end,
        ChannelsEvent::Test {
            ok: true,
            attempts: 2,
            ..
        }
    ));
    let json = serde_json::to_value(ChannelsEvent::TestProgress {
        test_id: 1,
        channel_id: "w".into(),
        attempt: 2,
        of: 3,
        phase: TestPhase::Waiting,
        wait_secs: Some(4),
    })
    .unwrap();
    assert_eq!(
        (
            json["kind"].as_str(),
            json["phase"].as_str(),
            json["waitSecs"].as_u64(),
            json["of"].as_u64()
        ),
        (Some("testProgress"), Some("waiting"), Some(4), Some(3))
    );
}
