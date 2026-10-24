//! The dispatcher against the in-process fakes: routing, request shapes, secrets, retries, cancel, contained failures, test send.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use crate::common;
use common::*;
use paddock_channels::*;
use paddock_download::{TransportError, TransportErrorKind as K};
use paddock_jobs::CancelToken;
use paddock_jobs::time::{Clock, FakeClock};
use paddock_secrets::{Secret, Secrets};
use std::sync::Arc;

fn header<'a>(r: &'a PostRequest, name: &str) -> Option<&'a str> {
    r.headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

fn ok(d: &Delivery) -> bool {
    d.result.is_ok()
}

#[test]
fn only_channels_that_want_the_message_get_it_and_results_keep_the_settings_order() {
    let mut off = webhook("off", false);
    off.enabled = false;
    let mut other = webhook("other", false);
    other.events = vec!["other".into()];
    let r = rig(
        vec![
            webhook("a", false),
            off,
            other,
            ntfy("b", false),
            email("c", None),
        ],
        FakePoster::new(),
        FakeMail::new(),
        store(),
    );
    let out = r
        .dispatcher
        .dispatch(&message("diskLow", Severity::Error), &CancelToken::new());
    assert_eq!(
        out.iter().map(|d| d.channel.as_str()).collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    assert!(out.iter().all(|d| ok(d) && d.attempts == 1));
    assert_eq!(r.poster.requests().len(), 2);
    assert_eq!(r.mailer.mails().len(), 1);
    assert!(
        r.dispatcher
            .dispatch(&message("unknown", Severity::Error), &CancelToken::new())
            .is_empty()
    );
    assert!(Dispatcher::settings(&r.dispatcher).channel("a").is_some());
}

#[test]
fn a_webhook_posts_json_with_the_bearer_token() {
    let secrets = store();
    let cfg = webhook("w", true);
    put(&secrets, &cfg, "tok-123");
    let r = rig(vec![cfg], FakePoster::new(), FakeMail::new(), secrets);
    let out = r
        .dispatcher
        .dispatch(&message("diskLow", Severity::Warning), &CancelToken::new());
    assert!(ok(&out[0]));
    let req = &r.poster.requests()[0];
    assert_eq!(req.url, "https://hooks.example.org/in?key=abc");
    assert_eq!(header(req, "content-type"), Some("application/json"));
    assert_eq!(header(req, "authorization"), Some("Bearer tok-123"));
    let sent: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(sent["title"], "Disk low");
    assert_eq!(sent["severity"], "warning");
    assert_eq!(sent["kind"], "diskLow");
    assert_eq!(sent["source"], "runner-1");
    assert!(
        !format!("{req:?}").contains("tok-123"),
        "Debug hides header values"
    );
}

#[test]
fn a_webhook_without_auth_sends_no_authorization_header() {
    let r = rig(
        vec![webhook("w", false)],
        FakePoster::new(),
        FakeMail::new(),
        store(),
    );
    r.dispatcher
        .dispatch(&message("diskLow", Severity::Info), &CancelToken::new());
    assert_eq!(header(&r.poster.requests()[0], "authorization"), None);
}

#[test]
fn ntfy_posts_the_body_with_title_priority_tags_and_token() {
    let secrets = store();
    let cfg = ntfy("n", true);
    put(&secrets, &cfg, "nt");
    let r = rig(vec![cfg], FakePoster::new(), FakeMail::new(), secrets);
    let mut m = message("diskLow", Severity::Error);
    m.title = "Disk\r\nlow".into();
    r.dispatcher.dispatch(&m, &CancelToken::new());
    let req = &r.poster.requests()[0];
    assert_eq!(
        req.url, "https://ntfy.example.org/alerts",
        "the trailing slash of the server is not doubled"
    );
    assert_eq!(req.body, b"C: has 2 GB left");
    assert_eq!(header(req, "title"), Some("Disk  low"));
    assert_eq!(header(req, "priority"), Some("5"));
    assert_eq!(header(req, "tags"), Some("rotating_light"));
    assert_eq!(header(req, "authorization"), Some("Bearer nt"));
    for (sev, prio, tag) in [
        (Severity::Info, "3", "information_source"),
        (Severity::Warning, "4", "warning"),
    ] {
        r.dispatcher
            .dispatch(&message("diskLow", sev), &CancelToken::new());
        let req = r.poster.requests().pop().unwrap();
        assert_eq!(
            (header(&req, "priority"), header(&req, "tags")),
            (Some(prio), Some(tag))
        );
    }
}

#[test]
fn e_mail_goes_to_every_recipient_with_the_login_from_the_store() {
    let secrets = store();
    let cfg = email("m", Some("bot"));
    put(&secrets, &cfg, "pw-1");
    let r = rig(vec![cfg], FakePoster::new(), FakeMail::new(), secrets);
    let mut m = message("diskLow", Severity::Error);
    m.title = "Disk\nlow".into();
    assert!(ok(&r.dispatcher.dispatch(&m, &CancelToken::new())[0]));
    let mail = &r.mailer.mails()[0];
    assert_eq!(
        (mail.host.as_str(), mail.port, mail.security),
        ("mail.example.org", 587, Security::StartTls)
    );
    assert_eq!(mail.to, ["a@example.org", "b@example.org"]);
    assert_eq!(
        (mail.subject.as_str(), mail.body.as_str()),
        ("Disk low", "C: has 2 GB left")
    );
    let login = mail.login.as_ref().unwrap();
    assert_eq!(
        (login.username.as_str(), login.password.expose()),
        ("bot", "pw-1")
    );
    assert!(
        !format!("{mail:?}").contains("pw-1"),
        "Debug hides the password"
    );
}

#[test]
fn a_relay_without_a_login_sends_without_credentials() {
    let r = rig(
        vec![email("m", None)],
        FakePoster::new(),
        FakeMail::new(),
        store(),
    );
    assert!(ok(&r.dispatcher.dispatch(
        &message("diskLow", Severity::Info),
        &CancelToken::new()
    )[0]));
    assert!(r.mailer.mails()[0].login.is_none());
}

#[test]
fn payloads_are_bounded_before_they_leave() {
    let r = rig(
        vec![webhook("w", false), email("m", None)],
        FakePoster::new(),
        FakeMail::new(),
        store(),
    );
    let mut m = message("diskLow", Severity::Info);
    m.title = "t".repeat(5000);
    m.body = "€".repeat(100_000);
    r.dispatcher.dispatch(&m, &CancelToken::new());
    let sent: serde_json::Value = serde_json::from_slice(&r.poster.requests()[0].body).unwrap();
    assert_eq!(
        sent["title"].as_str().unwrap().chars().count(),
        MAX_TITLE_CHARS
    );
    assert!(sent["body"].as_str().unwrap().len() <= MAX_BODY_BYTES);
    assert!(sent["body"].as_str().unwrap().ends_with("[cut]"));
    assert!(r.mailer.mails()[0].body.len() <= MAX_BODY_BYTES);
}

#[test]
fn a_missing_or_unreadable_secret_fails_without_a_request_and_is_not_retried() {
    let r = rig(
        vec![webhook("w", true)],
        FakePoster::new(),
        FakeMail::new(),
        store(),
    );
    let d = &r
        .dispatcher
        .dispatch(&message("diskLow", Severity::Info), &CancelToken::new())[0];
    assert!(matches!(&d.result, Err(SendError::MissingSecret { channel }) if channel == "w"));
    assert_eq!(d.attempts, 1);
    assert!(r.poster.requests().is_empty());
    r.secrets.set_unavailable(true);
    let d = &r
        .dispatcher
        .dispatch(&message("diskLow", Severity::Info), &CancelToken::new())[0];
    assert!(matches!(&d.result, Err(SendError::SecretStore(_))), "{d:?}");
    assert!(
        d.result
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("Unlock")
    );
}

#[test]
fn a_token_that_cannot_be_a_header_fails_without_repeating_it() {
    let secrets = store();
    let cfg = webhook("w", true);
    let long = "x".repeat(MAX_HEADER_VALUE);
    secrets
        .set(
            &cfg.secret_name().unwrap(),
            &Secret::new(long.clone()).unwrap(),
        )
        .unwrap();
    let r = rig(vec![cfg], FakePoster::new(), FakeMail::new(), secrets);
    let e = r
        .dispatcher
        .dispatch(&message("diskLow", Severity::Info), &CancelToken::new())[0]
        .result
        .clone()
        .unwrap_err();
    assert!(matches!(e, SendError::Invalid(_)));
    assert!(!e.to_string().contains(&long[..20]));
    assert!(r.poster.requests().is_empty());
}

#[test]
fn transient_failures_are_retried_with_the_familys_back_off() {
    let poster = FakePoster::new()
        .then_status(503)
        .then(Err(TransportError::new(K::Timeout, "slow")))
        .then_status(200);
    let r = rig(vec![webhook("w", false)], poster, FakeMail::new(), store());
    let d = &r
        .dispatcher
        .dispatch(&message("diskLow", Severity::Info), &CancelToken::new())[0];
    assert!(ok(d));
    assert_eq!(d.attempts, 3);
    assert_eq!(r.clock.waits(), [secs(2), secs(4)], "2 s, doubling");
}

#[test]
fn the_last_error_is_reported_when_every_attempt_fails() {
    let poster = FakePoster::new()
        .then(Err(TransportError::new(K::Connect, "refused")))
        .then(Err(TransportError::new(K::Connect, "refused")))
        .then(Err(TransportError::new(K::Connect, "refused")));
    let r = rig(vec![webhook("w", false)], poster, FakeMail::new(), store());
    let d = &r
        .dispatcher
        .dispatch(&message("diskLow", Severity::Info), &CancelToken::new())[0];
    assert_eq!(d.attempts, 3);
    assert_eq!(
        d.result,
        Err(SendError::Connect("hooks.example.org".into())),
        "the host only: no path, no query key"
    );
    assert!(
        d.result
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("could not be reached")
    );
}

#[test]
fn the_attempts_policy_is_honoured_and_capped() {
    let many = || {
        let mut p = FakePoster::new();
        for _ in 0..20 {
            p = p.then_status(500);
        }
        p
    };
    for (asked, expect) in [(1u32, 1u32), (2, 2), (0, 1), (99, 5)] {
        let r = rig(vec![webhook("w", false)], many(), FakeMail::new(), store());
        let d = r.dispatcher.clone().with_policy(RetryPolicy {
            attempts: asked,
            ..RetryPolicy::default()
        });
        assert_eq!(
            d.dispatch(&message("diskLow", Severity::Info), &CancelToken::new())[0].attempts,
            expect,
            "asked {asked}"
        );
        assert_eq!(r.poster.requests().len() as u32, expect);
    }
}

#[test]
fn a_server_that_says_wait_is_obeyed_but_capped() {
    let busy = |secs| {
        Ok(PostResponse {
            status: 429,
            retry_after: Some(secs),
        })
    };
    let poster = FakePoster::new()
        .then(busy(secs(10)))
        .then(busy(secs(3600)))
        .then_status(200);
    let r = rig(vec![webhook("w", false)], poster, FakeMail::new(), store());
    let d = r.dispatcher.clone().with_policy(RetryPolicy {
        attempts: 3,
        ..RetryPolicy::default()
    });
    assert!(ok(&d.dispatch(
        &message("diskLow", Severity::Info),
        &CancelToken::new()
    )[0]));
    assert_eq!(
        r.clock.waits(),
        [secs(10), secs(30)],
        "10 s as asked, an hour capped at the policy's longest wait"
    );
}

#[test]
fn failures_that_cannot_change_are_not_retried() {
    for status in [400, 401, 403, 404, 301, 302] {
        let r = rig(
            vec![webhook("w", false)],
            FakePoster::new().then_status(status),
            FakeMail::new(),
            store(),
        );
        let d = &r
            .dispatcher
            .dispatch(&message("diskLow", Severity::Info), &CancelToken::new())[0];
        assert_eq!(d.attempts, 1, "{status}");
        assert!(matches!(d.result, Err(SendError::Rejected { status: s, .. }) if s == status));
        assert!(r.clock.waits().is_empty());
    }
    for (kind, want) in [
        (K::Tls, SendError::Tls("hooks.example.org".into())),
        (
            K::Invalid,
            SendError::Invalid("The address hooks.example.org cannot be used.".into()),
        ),
        (K::Proxy, SendError::Proxy("bad proxy".into())),
    ] {
        let poster = FakePoster::new().then(Err(TransportError::new(kind, "bad proxy")));
        let r = rig(vec![webhook("w", false)], poster, FakeMail::new(), store());
        let d = &r
            .dispatcher
            .dispatch(&message("diskLow", Severity::Info), &CancelToken::new())[0];
        assert_eq!((d.attempts, d.result.clone()), (1, Err(want)));
    }
}

#[test]
fn mail_failures_are_classified_for_retry() {
    for (err, retry) in [
        (SendError::MailBusy("code 451".into()), true),
        (SendError::Timeout("h".into()), true),
        (SendError::Connect("h".into()), true),
        (SendError::MailRefused("code 550".into()), false),
        (SendError::Auth("h".into()), false),
        (SendError::Tls("h".into()), false),
        (SendError::Failed("x".into()), false),
        (SendError::Cancelled, false),
        (SendError::Panicked, false),
    ] {
        assert_eq!(err.retryable(), retry, "{err:?}");
        assert!(!err.to_string().is_empty());
        assert_eq!(err.retry_after(), None);
    }
    let r = rig(
        vec![email("m", None)],
        FakePoster::new(),
        FakeMail::new().then_fail(SendError::MailBusy("code 451".into())),
        store(),
    );
    let d = &r
        .dispatcher
        .dispatch(&message("diskLow", Severity::Info), &CancelToken::new())[0];
    assert!(ok(d) && d.attempts == 2);
    for status in [408, 425, 429, 500, 502, 599] {
        assert!(
            SendError::Rejected {
                status,
                retry_after: None
            }
            .retryable(),
            "{status}"
        );
    }
}

#[test]
fn cancel_before_the_start_sends_nothing() {
    let r = rig(
        vec![webhook("w", false)],
        FakePoster::new(),
        FakeMail::new(),
        store(),
    );
    let c = CancelToken::new();
    c.cancel();
    let d = &r
        .dispatcher
        .dispatch(&message("diskLow", Severity::Info), &c)[0];
    assert_eq!(
        (d.attempts, d.result.clone()),
        (0, Err(SendError::Cancelled))
    );
    assert!(r.poster.requests().is_empty());
}

#[test]
fn cancel_during_the_back_off_wait_stops_the_delivery() {
    let clock = FakeClock::new();
    let poster = Arc::new(FakePoster::new().then_status(503).then_status(503));
    let dispatcher = Dispatcher::new(
        Settings {
            channels: vec![webhook("w", false)],
        },
        store(),
        poster.clone(),
        Arc::new(FakeMail::new()),
        Arc::new(clock.clone()),
    );
    let cancel = CancelToken::new();
    let out = std::thread::scope(|s| {
        let h = s.spawn(|| dispatcher.dispatch(&message("diskLow", Severity::Info), &cancel));
        assert!(
            clock.wait_for_waiters(1, secs(30)),
            "the delivery is waiting on the clock"
        );
        cancel.cancel();
        h.join().unwrap()
    });
    assert_eq!(
        (out[0].attempts, out[0].result.clone()),
        (1, Err(SendError::Cancelled))
    );
    assert_eq!(poster.requests().len(), 1);
    assert_eq!(clock.waiters(), 0);
}

#[test]
fn a_back_off_wait_ends_when_the_clock_reaches_it() {
    let clock = FakeClock::new();
    let poster = Arc::new(FakePoster::new().then_status(503));
    let dispatcher = Dispatcher::new(
        Settings {
            channels: vec![webhook("w", false)],
        },
        store(),
        poster.clone(),
        Arc::new(FakeMail::new()),
        Arc::new(clock.clone()),
    );
    let out = std::thread::scope(|s| {
        let h = s.spawn(|| {
            dispatcher.dispatch(&message("diskLow", Severity::Info), &CancelToken::new())
        });
        assert!(clock.wait_for_waiters(1, secs(30)));
        clock.advance(secs(2));
        h.join().unwrap()
    });
    assert!(ok(&out[0]) && out[0].attempts == 2);
    assert_eq!(clock.monotonic(), secs(2));
}

#[test]
fn a_panicking_channel_is_contained_and_the_others_still_deliver() {
    let r = rig(
        vec![webhook("w", false), email("m", None), ntfy("n", false)],
        // The webhook's requests panic, not whichever HTTP channel happens to post first (they post in parallel).
        FakePoster::new().panic_on("hooks.example.org"),
        FakeMail::new().then_panic(),
        store(),
    );
    let out = r
        .dispatcher
        .dispatch(&message("diskLow", Severity::Info), &CancelToken::new());
    assert_eq!(out[0].result, Err(SendError::Panicked));
    assert_eq!(out[1].result, Err(SendError::Panicked));
    assert!(ok(&out[2]), "{:?}", out[2]);
    assert!(SendError::Panicked.to_string().contains("keeps running"));
}

#[test]
fn the_test_send_ignores_enabled_and_events_and_does_not_retry() {
    let mut cfg = webhook("w", false);
    cfg.enabled = false;
    cfg.events.clear();
    cfg.name = "Ops".into();
    let r = rig(
        vec![cfg],
        FakePoster::new().then_status(503),
        FakeMail::new(),
        store(),
    );
    let d = r
        .dispatcher
        .test("w", "Pitcrew", &CancelToken::new())
        .unwrap();
    assert_eq!(
        (d.attempts, d.result.clone()),
        (
            1,
            Err(SendError::Rejected {
                status: 503,
                retry_after: None
            })
        )
    );
    assert!(
        r.clock.waits().is_empty(),
        "the user is waiting: one attempt"
    );
    assert!(ok(&r
        .dispatcher
        .test("w", "Pitcrew", &CancelToken::new())
        .unwrap()));
    let sent: serde_json::Value = serde_json::from_slice(&r.poster.requests()[1].body).unwrap();
    assert_eq!(sent["title"], "Pitcrew test message");
    assert_eq!(sent["kind"], TEST_KIND);
    assert!(
        sent["body"]
            .as_str()
            .unwrap()
            .contains("notifications through Ops work")
    );
    let e = r
        .dispatcher
        .test("nope", "Pitcrew", &CancelToken::new())
        .unwrap_err();
    assert!(matches!(e, SendError::Invalid(m) if m.contains("'nope'")));
}

#[test]
fn a_channel_not_saved_yet_can_be_tested_with_the_typed_secret() {
    let r = rig(vec![], FakePoster::new(), FakeMail::new(), store());
    let cfg = webhook("draft", true);
    let typed = Secret::new("typed-token").unwrap();
    let d = r
        .dispatcher
        .test_with(&cfg, Some(&typed), "Spotter", &CancelToken::new());
    assert!(ok(&d));
    assert_eq!(
        header(&r.poster.requests()[0], "authorization"),
        Some("Bearer typed-token")
    );
    let d = r
        .dispatcher
        .test_with(&cfg, None, "Spotter", &CancelToken::new());
    assert!(
        matches!(d.result, Err(SendError::MissingSecret { .. })),
        "nothing stored, nothing typed"
    );
    put(&r.secrets, &cfg, "stored");
    assert!(ok(&r.dispatcher.test_with(
        &cfg,
        None,
        "Spotter",
        &CancelToken::new()
    )));
    assert_eq!(
        header(&r.poster.requests()[1], "authorization"),
        Some("Bearer stored")
    );
}

#[test]
fn a_test_of_an_invalid_channel_says_what_to_fix_without_sending() {
    let r = rig(vec![], FakePoster::new(), FakeMail::new(), store());
    let mut cfg = webhook("draft", false);
    cfg.transport = Transport::Webhook {
        url: "nonsense".into(),
        auth: false,
    };
    let d = r
        .dispatcher
        .test_with(&cfg, None, "Spotter", &CancelToken::new());
    assert_eq!(d.attempts, 0);
    assert!(
        matches!(d.result, Err(SendError::Invalid(m)) if m.contains("must start with https://"))
    );
    assert!(r.poster.requests().is_empty());
}

#[test]
fn messages_in_errors_never_contain_secrets() {
    let secrets = store();
    let cfg = email("m", Some("bot"));
    put(&secrets, &cfg, "hunter2");
    let r = rig(
        vec![cfg],
        FakePoster::new(),
        FakeMail::new().then_fail(SendError::Auth("mail.example.org".into())),
        secrets,
    );
    let d = &r
        .dispatcher
        .dispatch(&message("diskLow", Severity::Info), &CancelToken::new())[0];
    let text = format!("{d:?} {}", d.result.as_ref().unwrap_err());
    assert!(!text.contains("hunter2"));
    assert!(text.contains("app password"));
}

#[test]
fn every_error_reads_as_advice() {
    let all = [
        SendError::MissingSecret {
            channel: "Ops".into(),
        },
        SendError::SecretStore("locked".into()),
        SendError::Invalid("why".into()),
        SendError::Connect("h".into()),
        SendError::Tls("h".into()),
        SendError::Proxy("p".into()),
        SendError::Auth("h".into()),
        SendError::Timeout("h".into()),
        SendError::Rejected {
            status: 404,
            retry_after: None,
        },
        SendError::MailRefused("c".into()),
        SendError::MailBusy("c".into()),
        SendError::Failed("c".into()),
        SendError::Cancelled,
        SendError::Panicked,
    ];
    for e in all {
        assert!(!e.to_string().is_empty(), "{e:?}");
    }
    assert!(
        SendError::Rejected {
            status: 404,
            retry_after: Some(secs(1))
        }
        .retry_after()
        .is_some()
    );
}
