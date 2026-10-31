//! The real adapters against in-process servers on the loopback interface: the HTTP poster and the SMTP sender.
#![cfg(feature = "smtp")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use crate::common;
use common::*;
use paddock_channels::*;
use paddock_download::{Header, ProxyMode, ProxyRules, ProxyServer, TransportErrorKind as K};
use paddock_jobs::time::SystemClock;
use paddock_net::{Egress, Policy, Reach, StaticLookup, SystemLookup, UreqWire};
use paddock_secrets::Secret;
use std::sync::Arc;
use std::time::Duration;

fn request(port: u16) -> PostRequest {
    PostRequest {
        url: format!("http://127.0.0.1:{port}/hook?key=1"),
        headers: vec![
            Header::new("Content-Type", "application/json").unwrap(),
            Header::new("Authorization", "Bearer abc").unwrap(),
        ],
        body: br#"{"title":"x"}"#.to_vec(),
        timeout: Duration::from_secs(10),
        lan: false,
    }
}

fn egress(mode: ProxyMode) -> Arc<Egress> {
    let policy = channel_policy(Reach::Loopback);
    let wire = Arc::new(UreqWire::new(&policy, mode).unwrap());
    Arc::new(Egress::new(
        policy,
        wire,
        Arc::new(SystemLookup),
        Arc::new(SystemClock),
    ))
}

/// The mail sender over the same loopback-reach egress as the poster.
fn sender() -> LettreSender {
    LettreSender::new(egress(ProxyMode::Direct))
}

fn poster() -> EgressPoster {
    EgressPoster::new(egress(ProxyMode::Direct))
}

#[test]
fn a_post_carries_the_headers_and_body_and_returns_the_status() {
    let (port, server) = http_server(Some("HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n"));
    let r = poster().post(&request(port)).unwrap();
    assert_eq!((r.status, r.retry_after), (202, None));
    let seen = server.join().unwrap();
    assert!(seen.starts_with("POST /hook?key=1 HTTP/1.1"), "{seen}");
    let lower = seen.to_ascii_lowercase();
    assert!(
        lower.contains("authorization: bearer abc")
            && lower.contains("content-type: application/json"),
        "{seen}"
    );
    assert!(lower.contains("user-agent: paddock-channels/"), "{seen}");
    assert!(seen.ends_with(r#"{"title":"x"}"#), "{seen}");
}

#[test]
fn an_error_status_is_an_answer_with_its_retry_after() {
    let (port, server) = http_server(Some(
        "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 7\r\nContent-Length: 0\r\n\r\n",
    ));
    let r = poster().post(&request(port)).unwrap();
    assert_eq!(
        (r.status, r.retry_after),
        (429, Some(Duration::from_secs(7)))
    );
    server.join().unwrap();
}

#[test]
fn a_redirect_is_not_followed() {
    let (port, server) = http_server(Some(
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/elsewhere\r\nContent-Length: 0\r\n\r\n",
    ));
    let r = poster().post(&request(port)).unwrap();
    assert_eq!(r.status, 302);
    server.join().unwrap();
}

#[test]
fn a_reset_connection_is_a_connect_error() {
    // A held port that resets every connection, not a port read off a dropped listener (ADR 0023).
    let held = paddock_testkit::net::HeldPort::reset();
    let port = held.port();
    let e = poster().post(&request(port)).unwrap_err();
    assert_eq!(e.kind, K::Connect, "{e}");
}

#[test]
fn a_server_that_never_answers_ends_at_the_time_limit() {
    let (port, server) = http_server(None);
    let mut req = request(port);
    req.timeout = Duration::from_millis(300);
    let e = poster().post(&req).unwrap_err();
    assert_eq!(e.kind, K::Timeout, "{e}");
    server.join().unwrap();
}

#[test]
fn a_bad_address_is_invalid_and_a_bad_proxy_is_reported_without_its_login() {
    let mut req = request(1);
    req.url = "not a url".into();
    assert_eq!(poster().post(&req).unwrap_err().kind, K::Invalid);
    let proxy = ProxyServer::parse("http://proxy.invalid:3128")
        .unwrap()
        .with_credentials("user", "s3cret");
    let p = EgressPoster::new(egress(ProxyMode::Rules(ProxyRules::all(proxy))));
    let e = p.post(&request(9)).unwrap_err();
    assert!(matches!(e.kind, K::Proxy | K::Connect), "{e}");
    assert!(!e.to_string().contains("s3cret"));
}

#[test]
fn the_outbound_policy_applies_https_only_and_offline() {
    let e = egress(ProxyMode::Direct);
    let p = EgressPoster::new(e.clone());
    let mut req = request(1);
    req.url = "http://hooks.example.org/in".into();
    let err = p.post(&req).unwrap_err();
    assert_eq!(err.kind, K::Invalid);
    assert!(err.message.contains("https"), "{err}");
    e.set_offline(true);
    let err = p.post(&request(1)).unwrap_err();
    assert!(err.message.contains("offline"), "{err}");
    let public = EgressPoster::new(Arc::new(Egress::new(
        channel_policy(Reach::Public),
        Arc::new(UreqWire::new(&channel_policy(Reach::Public), ProxyMode::Direct).unwrap()),
        Arc::new(SystemLookup),
        Arc::new(SystemClock),
    )));
    let err = public.post(&request(1)).unwrap_err();
    assert!(
        err.message.contains("https"),
        "plain http to this computer needs the loopback reach: {err}"
    );
}

type Check = fn(&SendError) -> bool;

fn mail(port: u16, login: Option<&str>) -> MailRequest {
    MailRequest {
        host: "127.0.0.1".into(),
        port,
        security: Security::None,
        login: login.map(|pw| Login {
            username: "bot".into(),
            password: Secret::new(pw).unwrap(),
        }),
        from: "bot@example.com".into(),
        to: vec!["a@example.org".into(), "b@example.org".into()],
        subject: "Disk low".into(),
        body: "C: has 2 GB left".into(),
        step_timeout: Duration::from_secs(5),
        lan: false,
    }
}

#[test]
fn smtp_authenticates_and_delivers_to_every_recipient() {
    let (port, server) = smtp_server(SmtpScript::OK);
    assert_eq!(sender().send(&mail(port, Some("pw-value"))), Ok(()));
    let seen = server.join().unwrap();
    let all = seen.join("\n");
    assert!(seen[0].to_uppercase().starts_with("EHLO"), "{all}");
    assert!(
        seen.iter().any(|l| l.to_uppercase().starts_with("AUTH")),
        "{all}"
    );
    assert!(all.contains("MAIL FROM:<bot@example.com>"), "{all}");
    assert!(
        all.contains("RCPT TO:<a@example.org>") && all.contains("RCPT TO:<b@example.org>"),
        "{all}"
    );
    assert!(
        all.contains("Subject: Disk low") && all.contains("C: has 2 GB left"),
        "{all}"
    );
}

#[test]
fn smtp_without_a_login_does_not_authenticate() {
    let (port, server) = smtp_server(SmtpScript::OK);
    assert_eq!(sender().send(&mail(port, None)), Ok(()));
    assert!(
        !server
            .join()
            .unwrap()
            .iter()
            .any(|l| l.to_uppercase().starts_with("AUTH"))
    );
}

#[test]
fn smtp_replies_become_the_right_error() {
    let cases: [(SmtpScript, Check); 4] = [
        (
            SmtpScript {
                auth: b"535 bad credentials\r\n",
                ..SmtpScript::OK
            },
            |e| matches!(e, SendError::Auth(h) if h == "127.0.0.1"),
        ),
        (
            SmtpScript {
                rcpt: b"550 no such user\r\n",
                ..SmtpScript::OK
            },
            |e| matches!(e, SendError::MailRefused(m) if m.contains("550")),
        ),
        (
            SmtpScript {
                rcpt: b"451 try later\r\n",
                ..SmtpScript::OK
            },
            |e| matches!(e, SendError::MailBusy(m) if m.contains("451")),
        ),
        (
            SmtpScript {
                rcpt: b"250 ok\r\n",
                auth: b"454 temporary auth failure\r\n",
                ..SmtpScript::OK
            },
            |e| matches!(e, SendError::Auth(_)),
        ),
    ];
    for (script, check) in cases {
        let (port, server) = smtp_server(script);
        let e = sender().send(&mail(port, Some("pw"))).unwrap_err();
        assert!(check(&e), "{e:?}");
        assert!(!format!("{e:?} {e}").contains("pw-value"));
        let _ = server.join();
    }
}

#[test]
fn smtp_reset_by_the_server_is_a_connect_error() {
    // A held port that resets every connection, not a port read off a dropped listener (ADR 0023).
    let held = paddock_testkit::net::HeldPort::reset();
    let port = held.port();
    let e = sender().send(&mail(port, None)).unwrap_err();
    assert!(e.retryable() && matches!(e, SendError::Connect(_)), "{e:?}");
}

#[test]
fn smtp_security_against_a_server_without_tls_fails_cleanly() {
    for security in [Security::StartTls, Security::Implicit] {
        let (port, server) = smtp_server(SmtpScript::OK);
        let mut m = mail(port, Some("pw"));
        m.security = security;
        m.step_timeout = Duration::from_secs(2);
        let e = sender().send(&m).unwrap_err();
        assert!(!matches!(e, SendError::Panicked), "{e:?}");
        assert!(!e.to_string().contains("pw"));
        drop(server);
    }
}

#[test]
fn bad_addresses_are_refused_before_connecting() {
    let mut m = mail(1, None);
    m.from = "nobody".into();
    assert!(matches!(sender().send(&m), Err(SendError::Invalid(t)) if t.contains("sender")));
    let mut m = mail(1, None);
    m.to = vec!["a b".into()];
    assert!(matches!(sender().send(&m), Err(SendError::Invalid(t)) if t.contains("recipient")));
    let mut m = mail(1, None);
    m.to.clear();
    assert!(matches!(sender().send(&m), Err(SendError::Invalid(t)) if t.contains("recipient")));
    let mut m = mail(1, None);
    m.security = Security::StartTls;
    m.host = "a b".into();
    assert!(matches!(sender().send(&m), Err(SendError::Invalid(_))));
}

#[test]
fn the_whole_stack_delivers_a_webhook_and_an_email_through_the_real_adapters() {
    let (hport, hserver) = http_server(Some("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"));
    let (sport, sserver) = smtp_server(SmtpScript::OK);
    let mut w = webhook("w", true);
    w.transport = Transport::Webhook {
        url: format!("http://127.0.0.1:{hport}/in"),
        auth: true,
    };
    let mut m = email("m", Some("bot"));
    m.transport = Transport::Email {
        host: "127.0.0.1".into(),
        port: sport,
        security: Security::None,
        username: Some("bot".into()),
        from: "bot@example.com".into(),
        to: vec!["a@example.org".into()],
    };
    let secrets = store();
    put(&secrets, &w, "tok");
    put(&secrets, &m, "pw");
    let d = Dispatcher::new(
        Settings {
            channels: vec![w, m],
        },
        secrets,
        std::sync::Arc::new(poster()),
        std::sync::Arc::new(sender()),
        std::sync::Arc::new(paddock_jobs::time::SystemClock),
    );
    let out = d.dispatch(
        &message("diskLow", Severity::Error),
        &paddock_jobs::CancelToken::new(),
    );
    assert!(out.iter().all(|x| x.result.is_ok()), "{out:?}");
    assert!(
        hserver
            .join()
            .unwrap()
            .to_ascii_lowercase()
            .contains("authorization: bearer tok")
    );
    assert!(
        sserver
            .join()
            .unwrap()
            .join("\n")
            .contains("RCPT TO:<a@example.org>")
    );
}

#[test]
fn smtp_edge_cases_are_classified() {
    // No AUTH offered although a login is set: the library has no mechanism to use.
    let (port, server) = smtp_server(SmtpScript {
        ehlo: b"250 fake\r\n",
        ..SmtpScript::OK
    });
    let e = sender().send(&mail(port, Some("pw"))).unwrap_err();
    assert!(matches!(e, SendError::Auth(_)), "{e:?}");
    let _ = server.join();
    // A greeting that is not SMTP.
    let (port, server) = smtp_server(SmtpScript {
        greeting: b"HELLO there\r\n",
        ..SmtpScript::OK
    });
    let e = sender().send(&mail(port, None)).unwrap_err();
    assert!(
        matches!(e, SendError::Failed(_) | SendError::Connect(_)),
        "{e:?}"
    );
    let _ = server.join();
    // A server that accepts and never speaks ends at the step limit.
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut m = mail(silent.local_addr().unwrap().port(), None);
    m.step_timeout = Duration::from_millis(300);
    let e = sender().send(&m).unwrap_err();
    assert!(
        matches!(e, SendError::Timeout(_) | SendError::Connect(_)),
        "{e:?}"
    );
    drop(silent);
    // A name that cannot be a TLS server name.
    let mut m = mail(1, None);
    m.security = Security::StartTls;
    m.host = "a..b".into();
    let e = sender().send(&m).unwrap_err();
    assert!(
        matches!(
            e,
            SendError::Invalid(_) | SendError::Connect(_) | SendError::Tls(_)
        ),
        "{e:?}"
    );
}

#[test]
fn the_poster_maps_every_network_failure_to_a_transport_kind() {
    use paddock_net::{FakeWire, NetError, StaticLookup};
    for (error, kind) in [
        (NetError::Tls("h".into()), K::Tls),
        (NetError::Proxy("p".into()), K::Proxy),
        (NetError::Connect("h".into()), K::Connect),
        (NetError::Timeout("h".into()), K::Timeout),
        (NetError::Offline, K::Invalid),
    ] {
        let wire = Arc::new(FakeWire::new().then_error(error));
        let lookup = Arc::new(StaticLookup::new().with("hooks.example.org", &["93.184.216.34"]));
        let egress = Arc::new(Egress::new(
            channel_policy(Reach::Public),
            wire,
            lookup,
            Arc::new(SystemClock),
        ));
        let mut req = request(1);
        req.url = "https://hooks.example.org/in".into();
        assert_eq!(EgressPoster::new(egress).post(&req).unwrap_err().kind, kind);
    }
}

fn sender_with(policy: Policy, lookup: StaticLookup) -> (LettreSender, Arc<Egress>) {
    let e = Arc::new(Egress::new(
        policy.clone(),
        Arc::new(UreqWire::new(&policy, ProxyMode::Direct).unwrap()),
        Arc::new(lookup),
        Arc::new(SystemClock),
    ));
    (LettreSender::new(e.clone()), e)
}

/// Whether anything connected to `listener` (it is non-blocking).
fn connected(listener: &std::net::TcpListener) -> bool {
    listener.set_nonblocking(true).unwrap();
    listener.accept().is_ok()
}

#[test]
fn offline_mode_stops_mail_before_anything_is_resolved_or_connected() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let lookup = StaticLookup::new();
    let (s, e) = sender_with(channel_policy(Reach::Loopback), lookup);
    e.set_offline(true);
    let err = s.send(&mail(port, None)).unwrap_err();
    assert!(
        matches!(&err, SendError::Invalid(t) if t.contains("offline")),
        "{err}"
    );
    assert!(!err.retryable());
    assert!(!connected(&listener));
}

#[test]
fn a_mail_server_on_a_private_or_this_computers_address_needs_the_policy_and_the_switch() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // Public reach: this computer is refused, with or without the switch, and nothing connects.
    let (public, _) = sender_with(channel_policy(Reach::Public), StaticLookup::new());
    for lan in [false, true] {
        let mut m = mail(port, None);
        m.lan = lan;
        let err = public.send(&m).unwrap_err();
        assert!(matches!(err, SendError::Invalid(_)), "{err}");
    }
    // A private address is refused for a channel without the switch even under a wide policy.
    let (wide, _) = sender_with(
        channel_policy(Reach::Lan),
        StaticLookup::new().with("mail.example.org", &["192.168.1.20"]),
    );
    let mut m = mail(port, None);
    m.host = "mail.example.org".into();
    let err = wide.send(&m).unwrap_err();
    assert!(
        matches!(&err, SendError::Invalid(t) if t.contains("private network")),
        "{err}"
    );
    // The address in the clear is refused the same way.
    m.host = "192.168.1.20".into();
    assert!(matches!(wide.send(&m), Err(SendError::Invalid(_))));
    assert!(!connected(&listener));
}

#[test]
fn mail_connects_to_the_checked_address_not_to_the_name() {
    // `mail.test` exists only in the injected lookup: the system resolver would not find it, so a delivery proves the socket
    // went to the address that was checked.
    let (port, server) = smtp_server(SmtpScript::OK);
    let (s, _) = sender_with(
        channel_policy(Reach::Loopback),
        StaticLookup::new().with("mail.test", &["127.0.0.1"]),
    );
    let mut m = mail(port, None);
    m.host = "mail.test".into();
    assert_eq!(s.send(&m), Ok(()));
    assert!(
        server
            .join()
            .unwrap()
            .iter()
            .any(|l| l.starts_with("MAIL FROM"))
    );
}

#[test]
fn a_name_with_no_address_is_a_connect_failure() {
    let (s, _) = sender_with(channel_policy(Reach::Loopback), StaticLookup::new());
    let mut m = mail(25, None);
    m.host = "nowhere.example.org".into();
    assert!(matches!(s.send(&m), Err(SendError::Connect(_))));
}

#[test]
fn the_sender_has_a_debug_form_that_shows_nothing_of_the_policy() {
    assert_eq!(format!("{:?}", sender()), "LettreSender");
}

#[test]
fn a_refused_connection_to_a_checked_address_is_a_connect_failure() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let err = sender().send(&mail(port, None)).unwrap_err();
    assert!(matches!(err, SendError::Connect(_)), "{err}");
    assert!(err.retryable());
}

#[test]
fn the_next_checked_address_is_tried_after_a_refused_connect() {
    // The first address of the name has nothing listening (::1, or no IPv6 at all: either way refused at once); the second has
    // the server.
    let (port, server) = smtp_server(SmtpScript::OK);
    let (s, _) = sender_with(
        channel_policy(Reach::Loopback),
        StaticLookup::new().with("mail.test", &["::1", "127.0.0.1"]),
    );
    let mut m = mail(port, None);
    m.host = "mail.test".into();
    assert_eq!(s.send(&m), Ok(()));
    assert!(
        server
            .join()
            .unwrap()
            .iter()
            .any(|l| l.starts_with("MAIL FROM"))
    );
}
