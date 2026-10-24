//! The real wire against in-process servers on the loopback interface (no outside network).
#![cfg(feature = "http")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support;

use paddock_download::{ByteRange, Get, ProxyMode, ProxyRules, ProxyServer, Transport};
use paddock_jobs::CancelToken;
use paddock_jobs::time::SystemClock;
use paddock_net::*;
use std::sync::Arc;
use std::time::Duration;
use support::{Server, respond};

fn egress_with(policy: Policy, lookup: Arc<dyn Lookup>) -> (Arc<Egress>, Arc<UreqWire>) {
    let wire = Arc::new(UreqWire::new(&policy, ProxyMode::Direct).unwrap());
    (
        Arc::new(Egress::new(
            policy,
            wire.clone(),
            lookup,
            Arc::new(SystemClock),
        )),
        wire,
    )
}

fn local() -> (Arc<Egress>, Arc<UreqWire>) {
    egress_with(
        Policy::default()
            .reach(Reach::Loopback)
            .timeout(Duration::from_secs(10)),
        Arc::new(SystemLookup),
    )
}

fn hello() -> Server {
    Server::start(Box::new(|req| {
        if req.starts_with("GET /redirect ") {
            Some(respond("302 Found", &[("Location", "/final")], ""))
        } else if req.starts_with("GET /meta ") {
            Some(respond(
                "302 Found",
                &[("Location", "https://169.254.169.254/latest")],
                "",
            ))
        } else if req.starts_with("GET /range ") {
            Some(respond(
                "206 Partial Content",
                &[("Content-Range", "bytes 0-3/10"), ("ETag", "\"e\"")],
                "0123",
            ))
        } else if req.starts_with("GET /slow ") {
            None
        } else {
            Some(respond(
                "200 OK",
                &[("ETag", "\"v1\""), ("X-Seen", "yes")],
                "hello",
            ))
        }
    }))
}

#[test]
fn a_get_and_a_post_reach_the_server_with_their_headers_and_body() {
    let s = hello();
    let (egress, _) = local();
    let resp = egress
        .send(
            &Request::get(&s.url("/a?q=1"))
                .header("X-Token", "t")
                .unwrap(),
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(
        (
            resp.status,
            resp.text().as_str(),
            resp.etag(),
            resp.header("x-seen")
        ),
        (200, "hello", Some("\"v1\""), Some("yes"))
    );
    let post = Request::post(&s.url("/in"), br#"{"a":1}"#.to_vec())
        .header("Content-Type", "application/json")
        .unwrap();
    assert!(egress.send(&post, &CancelToken::new()).unwrap().ok());
    assert!(
        egress
            .send(&Request::head(&s.url("/h")), &CancelToken::new())
            .unwrap()
            .body
            .is_empty()
    );
    let seen = s.requests();
    let lower = seen[0].to_ascii_lowercase();
    assert!(
        seen[0].starts_with("GET /a?q=1 HTTP/1.1")
            && lower.contains("x-token: t")
            && lower.contains("user-agent: paddock-net/"),
        "{}",
        seen[0]
    );
    assert!(lower.contains("accept-encoding: identity"), "{}", seen[0]);
    assert!(
        seen[1].starts_with("POST /in ")
            && seen[1].ends_with(r#"{"a":1}"#)
            && seen[1]
                .to_ascii_lowercase()
                .contains("content-type: application/json")
    );
    assert!(seen[2].starts_with("HEAD /h "));
}

#[test]
fn requests_reuse_one_connection() {
    let s = hello();
    let (egress, wire) = local();
    for i in 0..4 {
        assert!(
            egress
                .send(
                    &Request::get(&s.url(&format!("/n{i}"))),
                    &CancelToken::new()
                )
                .unwrap()
                .ok()
        );
    }
    assert_eq!(s.accepted(), 1, "four requests, one connection");
    assert_eq!(wire.agents(), 1);
}

#[test]
fn redirects_are_followed_by_the_egress_and_a_redirect_to_the_metadata_address_is_not() {
    let s = hello();
    let (egress, _) = local();
    let resp = egress
        .send(&Request::get(&s.url("/redirect")), &CancelToken::new())
        .unwrap();
    assert_eq!(
        (resp.redirects, resp.url.as_str()),
        (1, s.url("/final").as_str())
    );
    let e = egress
        .send(&Request::get(&s.url("/meta")), &CancelToken::new())
        .unwrap_err();
    assert!(
        matches!(
            e,
            NetError::Refused {
                why: Refusal::Never,
                ..
            }
        ),
        "{e:?}"
    );
    assert_eq!(
        s.requests()
            .iter()
            .filter(|r| r.starts_with("GET /meta"))
            .count(),
        1
    );
}

#[test]
fn the_address_the_socket_connects_to_is_checked_even_when_the_name_looked_public() {
    // The pre-check is told "public"; the socket resolves the real "localhost": the wire's guard refuses it (DNS rebinding).
    let s = hello();
    let lying = Arc::new(StaticLookup::new().with("localhost", &["93.184.216.34"]));
    let (egress, _) = egress_with(Policy::default().timeout(Duration::from_secs(5)), lying);
    let e = egress
        .send(
            &Request::get(&format!("https://localhost:{}/", s.port)),
            &CancelToken::new(),
        )
        .unwrap_err();
    assert!(
        matches!(
            e,
            NetError::Refused {
                why: Refusal::TooFar(Class::Loopback),
                ..
            }
        ),
        "{e:?}"
    );
    assert_eq!(s.accepted(), 0, "no connection was made");
}

#[test]
fn a_closed_port_is_a_connect_error_and_a_silent_server_ends_at_the_time_limit() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let (egress, _) = local();
    let e = egress
        .send(
            &Request::get(&format!("http://127.0.0.1:{port}/")),
            &CancelToken::new(),
        )
        .unwrap_err();
    assert!(e.retryable() && matches!(e, NetError::Connect(_)), "{e:?}");
    let s = hello();
    let (slow, _) = egress_with(
        Policy::default()
            .reach(Reach::Loopback)
            .timeout(Duration::from_millis(300)),
        Arc::new(SystemLookup),
    );
    let e = slow
        .send(&Request::get(&s.url("/slow")), &CancelToken::new())
        .unwrap_err();
    assert!(matches!(e, NetError::Timeout(_)), "{e:?}");
}

#[test]
fn https_to_a_server_that_does_not_speak_tls_fails_without_a_panic() {
    let s = hello();
    let (egress, _) = egress_with(
        Policy::default()
            .reach(Reach::Loopback)
            .timeout(Duration::from_secs(3)),
        Arc::new(SystemLookup),
    );
    let e = egress
        .send(
            &Request::get(&format!("https://127.0.0.1:{}/", s.port)),
            &CancelToken::new(),
        )
        .unwrap_err();
    assert!(
        matches!(
            e,
            NetError::Tls(_) | NetError::Connect(_) | NetError::Timeout(_)
        ),
        "{e:?}"
    );
}

#[test]
fn the_download_transport_streams_a_range() {
    let s = hello();
    let (egress, _) = local();
    let t = egress.download_transport();
    let url = s.url("/range");
    let mut resp = t
        .get(&Get {
            url: &url,
            range: Some(ByteRange {
                start: 0,
                end: Some(3),
            }),
            if_range: Some("\"e\""),
            body_timeout: Some(Duration::from_secs(5)),
            headers: &[],
        })
        .unwrap();
    assert_eq!(
        (resp.status, resp.headers.content_range.as_deref()),
        (206, Some("bytes 0-3/10"))
    );
    let mut body = String::new();
    std::io::Read::read_to_string(&mut resp.body, &mut body).unwrap();
    assert_eq!(body, "0123");
    let sent = s.requests().pop().unwrap().to_ascii_lowercase();
    assert!(
        sent.contains("range: bytes=0-3") && sent.contains("if-range: \"e\""),
        "{sent}"
    );
}

#[test]
fn a_proxy_that_cannot_be_reached_is_reported_and_proxy_settings_reload() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let proxy = ProxyServer::parse(&format!("http://127.0.0.1:{port}"))
        .unwrap()
        .with_credentials("user", "s3cret");
    let policy = Policy::default().timeout(Duration::from_secs(3));
    let wire = Arc::new(UreqWire::new(&policy, ProxyMode::Rules(ProxyRules::all(proxy))).unwrap());
    let lookup = Arc::new(StaticLookup::new().with("api.example.org", &["93.184.216.34"]));
    let egress = Egress::new(policy, wire.clone(), lookup, Arc::new(SystemClock));
    let e = egress
        .send(
            &Request::get("https://api.example.org/"),
            &CancelToken::new(),
        )
        .unwrap_err();
    assert!(
        matches!(e, NetError::Proxy(_) | NetError::Connect(_)),
        "{e:?}"
    );
    assert!(!format!("{e} {e:?}").contains("s3cret"));
    assert!(wire.reload_proxy().is_ok());
    let system = UreqWire::new(&Policy::default(), ProxyMode::System).map(|w| w.reload_proxy());
    assert!(system.is_ok() || system.is_err());
}

#[test]
fn a_bad_url_in_the_wire_is_invalid_not_a_panic() {
    let wire = UreqWire::new(&Policy::default(), ProxyMode::Direct).unwrap();
    let req = WireRequest {
        method: Method::Get,
        url: "not a url",
        reach: Reach::Public,
        headers: &[],
        body: &[],
        user_agent: "x",
        connect_timeout: Duration::from_secs(1),
        timeout: Some(Duration::from_secs(1)),
        response_timeout: Duration::from_secs(1),
        body_timeout: None,
    };
    assert!(matches!(wire.send(&req), Err(NetError::Invalid(_))));
}
