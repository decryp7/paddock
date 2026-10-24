//! The real adapters against in-process servers on the loopback interface: the HTTP poster and the SMTP sender.
#![cfg(feature = "smtp")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use crate::common;
use common::*;
use paddock_channels::*;
use paddock_download::{Header, ProxyMode, ProxyRules, ProxyServer, TransportErrorKind as K};
use paddock_jobs::time::SystemClock;
use paddock_net::{Egress, Reach, SystemLookup, UreqWire};
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
fn an_unreachable_host_is_a_connect_error() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
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
    }
}

#[test]
fn smtp_authenticates_and_delivers_to_every_recipient() {
    let (port, server) = smtp_server(SmtpScript::OK);
    assert_eq!(LettreSender.send(&mail(port, Some("pw-value"))), Ok(()));
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
    assert_eq!(LettreSender.send(&mail(port, None)), Ok(()));
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
        let e = LettreSender.send(&mail(port, Some("pw"))).unwrap_err();
        assert!(check(&e), "{e:?}");
        assert!(!format!("{e:?} {e}").contains("pw-value"));
        let _ = server.join();
    }
}

#[test]
fn smtp_to_a_closed_port_is_a_connect_error() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let e = LettreSender.send(&mail(port, None)).unwrap_err();
    assert!(e.retryable() && matches!(e, SendError::Connect(_)), "{e:?}");
}

#[test]
fn smtp_security_against_a_server_without_tls_fails_cleanly() {
    for security in [Security::StartTls, Security::Implicit] {
        let (port, server) = smtp_server(SmtpScript::OK);
        let mut m = mail(port, Some("pw"));
        m.security = security;
        m.step_timeout = Duration::from_secs(2);
        let e = LettreSender.send(&m).unwrap_err();
        assert!(!matches!(e, SendError::Panicked), "{e:?}");
        assert!(!e.to_string().contains("pw"));
        drop(server);
    }
}

#[test]
fn bad_addresses_are_refused_before_connecting() {
    let mut m = mail(1, None);
    m.from = "nobody".into();
    assert!(matches!(LettreSender.send(&m), Err(SendError::Invalid(t)) if t.contains("sender")));
    let mut m = mail(1, None);
    m.to = vec!["a b".into()];
    assert!(matches!(LettreSender.send(&m), Err(SendError::Invalid(t)) if t.contains("recipient")));
    let mut m = mail(1, None);
    m.to.clear();
    assert!(matches!(LettreSender.send(&m), Err(SendError::Invalid(t)) if t.contains("recipient")));
    let mut m = mail(1, None);
    m.security = Security::StartTls;
    m.host = "a b".into();
    assert!(matches!(LettreSender.send(&m), Err(SendError::Invalid(_))));
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
        std::sync::Arc::new(LettreSender),
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
    let e = LettreSender.send(&mail(port, Some("pw"))).unwrap_err();
    assert!(matches!(e, SendError::Auth(_)), "{e:?}");
    let _ = server.join();
    // A greeting that is not SMTP.
    let (port, server) = smtp_server(SmtpScript {
        greeting: b"HELLO there\r\n",
        ..SmtpScript::OK
    });
    let e = LettreSender.send(&mail(port, None)).unwrap_err();
    assert!(
        matches!(e, SendError::Failed(_) | SendError::Connect(_)),
        "{e:?}"
    );
    let _ = server.join();
    // A server that accepts and never speaks ends at the step limit.
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut m = mail(silent.local_addr().unwrap().port(), None);
    m.step_timeout = Duration::from_millis(300);
    let e = LettreSender.send(&m).unwrap_err();
    assert!(
        matches!(e, SendError::Timeout(_) | SendError::Connect(_)),
        "{e:?}"
    );
    drop(silent);
    // A name that cannot be a TLS server name.
    let mut m = mail(1, None);
    m.security = Security::StartTls;
    m.host = "a..b".into();
    let e = LettreSender.send(&m).unwrap_err();
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
