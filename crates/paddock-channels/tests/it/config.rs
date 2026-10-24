//! Settings: defaults, validation, secret names, subscriptions.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use crate::common;
use common::*;
use paddock_channels::*;
use paddock_secrets::Secrets;

fn fails(s: &Settings) -> String {
    s.validate().unwrap_err().to_string()
}

fn with(cfg: ChannelConfig) -> Settings {
    Settings {
        channels: vec![cfg],
    }
}

#[test]
fn nothing_is_configured_by_default_and_a_valid_set_passes() {
    assert!(Settings::default().channels.is_empty());
    assert_eq!(Settings::default().validate(), Ok(()));
    let all = Settings {
        channels: vec![
            webhook("w", true),
            ntfy("n", false),
            email("m", Some("bot")),
        ],
    };
    assert_eq!(all.validate(), Ok(()));
    assert_eq!(all.channel("n").unwrap().id, "n");
    assert!(all.channel("zz").is_none());
}

#[test]
fn json_round_trips_and_missing_keys_take_the_safe_defaults() {
    let json = r#"{"channels":[{"id":"w","transport":{"type":"webhook","url":"https://a.example.org/x"}},
        {"id":"n","transport":{"type":"ntfy","topic":"t"}},
        {"id":"m","transport":{"type":"email","host":"h.example.org","port":25,"from":"a@b.org","to":["c@d.org"]}}]}"#;
    let s: Settings = serde_json::from_str(json).unwrap();
    assert!(
        s.channels
            .iter()
            .all(|c| !c.enabled && c.events.is_empty() && c.min_severity == Severity::Info)
    );
    assert!(matches!(
        &s.channels[0].transport,
        Transport::Webhook { auth: false, .. }
    ));
    assert!(
        matches!(&s.channels[1].transport, Transport::Ntfy { server, auth: false, .. } if server == "https://ntfy.sh")
    );
    assert!(matches!(
        &s.channels[2].transport,
        Transport::Email {
            security: Security::StartTls,
            username: None,
            ..
        }
    ));
    let again: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(again, s);
    assert!(
        serde_json::from_str::<Settings>(
            r#"{"channels":[{"id":"x","transport":{"type":"carrier-pigeon"}}]}"#
        )
        .is_err()
    );
}

#[test]
fn a_channel_that_would_never_get_a_message_is_still_valid_but_wants_nothing() {
    let mut c = webhook("w", false);
    c.enabled = false;
    assert!(!c.wants(&message("diskLow", Severity::Error)));
    c.enabled = true;
    assert!(c.wants(&message("diskLow", Severity::Info)));
    assert!(
        !c.wants(&message("other", Severity::Error)),
        "not subscribed"
    );
    c.min_severity = Severity::Warning;
    assert!(
        !c.wants(&message("diskLow", Severity::Info)),
        "below the minimum"
    );
    assert!(c.wants(&message("diskLow", Severity::Warning)));
    c.events.clear();
    assert!(
        !c.wants(&message("diskLow", Severity::Error)),
        "no events means none"
    );
}

#[test]
fn bad_settings_name_what_to_fix() {
    let mut many = Settings::default();
    for i in 0..=MAX_CHANNELS {
        many.channels.push(webhook(&format!("c{i}"), false));
    }
    assert!(fails(&many).contains("At most 8"));
    assert!(
        fails(&Settings {
            channels: vec![webhook("a", false), webhook("a", false)]
        })
        .contains("used twice")
    );
    for id in ["", "a b", "a:b", &"x".repeat(65), "é"] {
        assert!(
            fails(&with(webhook(id, false))).contains("Channel id"),
            "{id}"
        );
    }
    let mut c = webhook("w", false);
    c.name = "Ops\nchannel".into();
    assert!(fails(&with(c.clone())).contains("Channel name contains control characters"));
    c.name = "Ops".into();
    c.events = vec!["e".into(); MAX_EVENTS + 1];
    assert!(fails(&with(c.clone())).starts_with("Ops: A channel can subscribe"));
    c.events = vec!["bad\u{7}".into()];
    assert!(fails(&with(c)).contains("control characters"));

    let url = |u: &str| {
        let mut c = webhook("w", false);
        c.transport = Transport::Webhook {
            url: u.into(),
            auth: false,
        };
        fails(&with(c))
    };
    assert!(url("ftp://a.example.org").contains("must start with https://"));
    assert!(url("http://hooks.example.org/x").contains("plain http is only for this computer"));
    assert!(url("https://").contains("valid host"));
    assert!(url("https://:80/x").contains("valid host"));
    assert!(url("https://a b/x").contains("valid host"));
    assert!(url("https://user:pw@a.example.org/").contains("must not contain a login"));
    assert!(url(&format!("https://a.example.org/{}", "x".repeat(3000))).contains("too long"));

    let topic = |t: &str, server: &str| {
        let mut c = ntfy("n", false);
        c.transport = Transport::Ntfy {
            server: server.into(),
            topic: t.into(),
            auth: false,
        };
        fails(&with(c))
    };
    for t in ["", "a/b", "a b", &"t".repeat(65)] {
        assert!(topic(t, "https://ntfy.sh").contains("ntfy topic"), "{t}");
    }
    assert!(topic("ok", "ntfy.sh").contains("ntfy server must start"));
}

struct Parts<'a> {
    host: &'a mut String,
    port: &'a mut u16,
    username: &'a mut Option<String>,
    from: &'a mut String,
    to: &'a mut Vec<String>,
}

fn parts(t: &mut Transport) -> Parts<'_> {
    match t {
        Transport::Email {
            host,
            port,
            username,
            from,
            to,
            ..
        } => Parts {
            host,
            port,
            username,
            from,
            to,
        },
        _ => unreachable!(),
    }
}

#[test]
fn plain_http_is_valid_only_for_this_computer() {
    for url in [
        "http://localhost:8080/hook",
        "http://127.0.0.1/x",
        "http://[::1]:9/x",
        "http://app.localhost/x",
        "https://hooks.example.org/x",
    ] {
        let mut c = webhook("w", false);
        c.transport = Transport::Webhook {
            url: url.into(),
            auth: false,
        };
        assert_eq!(with(c).validate(), Ok(()), "{url}");
    }
    let mut c = ntfy("n", false);
    c.transport = Transport::Ntfy {
        server: "http://ntfy.example.org".into(),
        topic: "t".into(),
        auth: false,
    };
    assert!(fails(&with(c)).contains("plain http"));
}

#[test]
fn bad_mail_settings_name_what_to_fix() {
    let mail = |f: &dyn Fn(&mut Transport)| {
        let mut c = email("m", Some("bot"));
        f(&mut c.transport);
        fails(&with(c))
    };
    assert!(mail(&|t| *parts(t).host = String::new()).contains("SMTP host"));
    assert!(mail(&|t| *parts(t).host = "a b".into()).contains("SMTP host"));
    assert!(mail(&|t| *parts(t).port = 0).contains("port"));
    assert!(
        mail(&|t| *parts(t).username = Some("  ".into())).contains("username must not be empty")
    );
    assert!(mail(&|t| *parts(t).username = Some("x".repeat(201))).contains("username is too long"));
    assert!(mail(&|t| *parts(t).from = "nobody".into()).contains("Sender"));
    assert!(mail(&|t| parts(t).to.clear()).contains("at least one recipient"));
    assert!(
        mail(&|t| *parts(t).to = vec!["a@b.org".into(); MAX_RECIPIENTS + 1])
            .contains("At most 10 recipients")
    );
    for bad in [
        "a@b",
        "@b.org",
        "a@.org",
        "a@b.org.",
        "a b@c.org",
        "a@b@c.org",
        "a@b.org\n",
    ] {
        assert!(
            mail(&|t| *parts(t).to = vec![bad.into()]).contains("Recipient"),
            "{bad}"
        );
    }
}

#[test]
fn secrets_are_named_by_channel_and_need_no_entry_when_unused() {
    let n = webhook("w1", true).secret_name().unwrap();
    assert_eq!(
        (n.scope(), n.key(), n.account().as_str()),
        ("channel:w1", "secret", "channel:w1:secret")
    );
    assert_eq!(ntfy("n", true).secret_name().unwrap().key(), "secret");
    assert_eq!(email("m", Some("bot")).secret_name(), secret_name_of("m"));
    assert!(webhook("w", false).secret_name().is_none());
    assert!(
        email("m", None).secret_name().is_none(),
        "a relay without a login has no password"
    );
    assert!(
        webhook(&"x".repeat(200), true).secret_name().is_none(),
        "an unusable id is reported by validate, never a panic"
    );
}

#[test]
fn forgetting_a_secret_removes_only_that_channels_entry() {
    let secrets = store();
    let (a, b) = (webhook("a", true), webhook("b", true));
    put(&secrets, &a, "ta");
    put(&secrets, &b, "tb");
    a.forget_secret(secrets.as_ref()).unwrap();
    a.forget_secret(secrets.as_ref()).unwrap();
    webhook("c", false).forget_secret(secrets.as_ref()).unwrap();
    assert!(secrets.get(&a.secret_name().unwrap()).unwrap().is_none());
    assert!(secrets.get(&b.secret_name().unwrap()).unwrap().is_some());
    secrets.set_unavailable(true);
    assert!(b.forget_secret(secrets.as_ref()).is_err());
}

#[test]
fn labels_prefer_the_name() {
    let mut c = webhook("w", false);
    assert_eq!(c.label(), "w");
    c.name = "Ops".into();
    assert_eq!(c.label(), "Ops");
}
