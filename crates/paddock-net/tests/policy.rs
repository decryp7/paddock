//! The policy on the in-process fakes: nothing here opens a socket.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_download::{Get, Transport};
use paddock_jobs::CancelToken;
use paddock_jobs::time::{Clock, FakeClock};
use paddock_net::*;
use proptest::prelude::*;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

struct Rig {
    egress: Arc<Egress>,
    wire: Arc<FakeWire>,
    lookup: Arc<StaticLookup>,
    clock: FakeClock,
}

fn rig(policy: Policy, wire: FakeWire) -> Rig {
    let wire = Arc::new(wire);
    let lookup = Arc::new(
        StaticLookup::new()
            .with("api.example.org", &["93.184.216.34"])
            .with("cdn.example.org", &["93.184.216.35", "2606:2800:220:1::1"])
            .with("other.example.net", &["198.51.100.7"])
            .with("nas.home", &["192.168.1.20"])
            .with("rebind.example.org", &["93.184.216.34", "127.0.0.1"])
            .with("meta.example.org", &["169.254.169.254"])
            .with("void.example.org", &[])
            .with("localhost", &["127.0.0.1", "::1"]),
    );
    let clock = FakeClock::new();
    let egress = Arc::new(Egress::new(
        policy,
        wire.clone(),
        lookup.clone(),
        Arc::new(clock.clone()),
    ));
    Rig {
        egress,
        wire,
        lookup,
        clock,
    }
}

fn get(r: &Rig, url: &str) -> Result<Response, NetError> {
    r.egress.send(&Request::get(url), &CancelToken::new())
}

#[test]
fn an_https_request_to_a_public_host_goes_out_as_asked() {
    let r = rig(
        Policy::default(),
        FakeWire::new().then(Reply::ok(b"hello").header("ETag", "\"v1\"")),
    );
    let req = Request::get("https://api.example.org/v1/items?page=2")
        .header("Accept", "application/json")
        .unwrap();
    let resp = r.egress.send(&req, &CancelToken::new()).unwrap();
    assert!(
        resp.ok() && resp.text() == "hello" && resp.etag() == Some("\"v1\"") && resp.redirects == 0
    );
    assert_eq!(resp.url, "https://api.example.org/v1/items");
    let seen = &r.wire.seen()[0];
    assert_eq!(
        (seen.method, seen.url.as_str()),
        (Method::Get, "https://api.example.org/v1/items?page=2")
    );
    assert_eq!(
        seen.headers,
        [("Accept".to_string(), "application/json".to_string())]
    );
    assert_eq!(r.lookup.asked(), ["api.example.org"]);
    assert!(
        !format!("{req:?}").contains("application/json") && format!("{req:?}").contains("Accept")
    );
}

#[test]
fn schemes_logins_and_addresses_are_checked_before_anything_is_resolved() {
    let r = rig(Policy::default(), FakeWire::new());
    for (url, want) in [
        ("http://api.example.org/", "Scheme"),
        ("ftp://api.example.org/", "Scheme"),
        ("file:///etc/passwd", "Invalid"),
        ("https://user:pw@api.example.org/", "Invalid"),
        ("not a url", "Invalid"),
        ("https://", "Invalid"),
    ] {
        let e = get(&r, url).unwrap_err();
        assert!(format!("{e:?}").starts_with(want), "{url}: {e:?}");
        assert!(!e.to_string().contains("pw"), "{e}");
    }
    assert!(r.lookup.asked().is_empty() && r.wire.sends() == 0);
}

#[test]
fn plain_http_is_only_for_this_computer_and_only_when_the_reach_allows_it() {
    let local = rig(Policy::default().reach(Reach::Loopback), FakeWire::new());
    for url in [
        "http://127.0.0.1:8080/x",
        "http://[::1]:8080/x",
        "http://localhost:9/x",
        "http://app.localhost/x",
    ] {
        let r = get(&local, url);
        assert!(
            r.is_ok() || matches!(r, Err(NetError::Connect(_))),
            "{url}: {r:?}"
        );
    }
    assert!(
        matches!(
            get(&local, "http://api.example.org/"),
            Err(NetError::Scheme(_))
        ),
        "a public host needs https"
    );
    assert!(
        matches!(get(&local, "http://nas.home/"), Err(NetError::Scheme(_))),
        "so does the local network"
    );
    let lan = rig(Policy::default().reach(Reach::Lan), FakeWire::new());
    assert!(matches!(
        get(&lan, "http://127.0.0.1/"),
        Err(NetError::Scheme(_))
    ));
    assert!(get(&lan, "https://nas.home/").is_ok());
}

#[test]
fn every_address_of_a_name_must_be_within_reach() {
    let r = rig(Policy::default(), FakeWire::new());
    let refused = |e: NetError| match e {
        NetError::Refused { why, .. } => why,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        refused(get(&r, "https://rebind.example.org/").unwrap_err()),
        Refusal::TooFar(Class::Loopback),
        "one bad address is enough"
    );
    assert_eq!(
        refused(get(&r, "https://nas.home/").unwrap_err()),
        Refusal::TooFar(Class::Lan)
    );
    assert_eq!(
        refused(get(&r, "https://meta.example.org/").unwrap_err()),
        Refusal::Never
    );
    assert_eq!(
        refused(get(&r, "https://void.example.org/").unwrap_err()),
        Refusal::NoAddress
    );
    assert!(matches!(
        get(&r, "https://unknown.example.org/"),
        Err(NetError::Connect(_))
    ));
    assert!(
        get(&r, "https://cdn.example.org/").is_ok(),
        "v4 and v6 public addresses"
    );
    assert_eq!(r.wire.sends(), 1, "nothing was sent to a refused host");
    let e = get(&r, "https://meta.example.org/?token=secret").unwrap_err();
    assert!(!e.to_string().contains("secret"));
}

#[test]
fn ip_literals_are_classified_without_a_lookup() {
    let public = rig(Policy::default(), FakeWire::new());
    let loopback = rig(Policy::default().reach(Reach::Loopback), FakeWire::new());
    for ip in [
        "169.254.169.254",
        "169.254.1.1",
        "0.0.0.0",
        "255.255.255.255",
        "224.0.0.1",
        "[fe80::1]",
        "[::]",
        "[ff02::1]",
        "[::ffff:169.254.169.254]",
    ] {
        for r in [&public, &loopback] {
            let e = get(r, &format!("https://{ip}/")).unwrap_err();
            assert!(
                matches!(
                    e,
                    NetError::Refused {
                        why: Refusal::Never,
                        ..
                    }
                ),
                "{ip}: {e:?}"
            );
        }
    }
    for ip in [
        "10.0.0.1",
        "172.16.5.4",
        "192.168.0.9",
        "100.64.0.1",
        "[fd00::1]",
        "[::ffff:10.0.0.1]",
    ] {
        assert!(
            matches!(
                get(&public, &format!("https://{ip}/")),
                Err(NetError::Refused {
                    why: Refusal::TooFar(Class::Lan),
                    ..
                })
            ),
            "{ip}"
        );
        assert!(get(&loopback, &format!("https://{ip}/")).is_ok(), "{ip}");
    }
    for ip in ["127.0.0.1", "127.9.9.9", "[::1]", "[::ffff:127.0.0.1]"] {
        assert!(
            matches!(
                get(&public, &format!("https://{ip}/")),
                Err(NetError::Refused {
                    why: Refusal::TooFar(Class::Loopback),
                    ..
                })
            ),
            "{ip}"
        );
        assert!(get(&loopback, &format!("https://{ip}/")).is_ok(), "{ip}");
    }
    assert!(
        get(&public, "https://93.184.216.34/").is_ok()
            && get(&public, "https://[2606:2800:220:1::1]/").is_ok()
    );
    assert!(
        public.lookup.asked().is_empty(),
        "literals are not looked up"
    );
}

#[test]
fn the_allow_list_is_checked_before_the_lookup() {
    let r = rig(
        Policy::default().allow_hosts(&["api.example.org", "*.example.net", "bad pattern/", ""]),
        FakeWire::new(),
    );
    assert!(get(&r, "https://api.example.org/").is_ok());
    assert!(get(&r, "https://other.example.net/").is_ok());
    assert!(
        matches!(get(&r, "https://cdn.example.org/"), Err(NetError::NotAllowed(h)) if h == "cdn.example.org")
    );
    assert!(
        matches!(
            get(&r, "https://example.net/"),
            Err(NetError::NotAllowed(_))
        ),
        "a wildcard is not the apex"
    );
    assert_eq!(
        r.lookup.asked(),
        ["api.example.org", "other.example.net"],
        "refused hosts are never looked up"
    );
    let none = rig(Policy::default().allow_hosts(&[]), FakeWire::new());
    assert!(matches!(
        get(&none, "https://api.example.org/"),
        Err(NetError::NotAllowed(_))
    ));
    assert!(none.lookup.asked().is_empty());
}

#[test]
fn host_patterns_parse_strictly() {
    for bad in [
        "",
        " ",
        "*",
        "*.",
        "a*.com",
        "http://a.com",
        "a.com:80",
        "a.com/x",
        ".a.com",
        "a.com.",
        "a b.com",
        "user@a.com",
    ] {
        assert!(HostPattern::parse(bad).is_none(), "{bad:?}");
    }
    let p = HostPattern::parse(" *.Example.COM ").unwrap();
    assert!(
        p.matches("a.example.com")
            && p.matches("A.B.example.com.")
            && !p.matches("example.com")
            && !p.matches("badexample.com")
    );
    assert!(HostPattern::parse("a.com").unwrap().matches("A.com"));
    assert!(Policy::default().host_allowed("anything"));
}

#[test]
fn redirects_are_followed_by_hand_and_every_hop_is_checked() {
    let wire = FakeWire::new()
        .then(Reply::redirect(302, "/next?x=1"))
        .then(Reply::redirect(301, "https://other.example.net/final"))
        .then(Reply::ok(b"done"));
    let r = rig(Policy::default(), wire);
    let resp = get(&r, "https://api.example.org/start").unwrap();
    assert_eq!(
        (resp.text().as_str(), resp.redirects, resp.url.as_str()),
        ("done", 2, "https://other.example.net/final")
    );
    let urls: Vec<String> = r.wire.seen().into_iter().map(|s| s.url).collect();
    assert_eq!(
        urls,
        [
            "https://api.example.org/start",
            "https://api.example.org/next?x=1",
            "https://other.example.net/final"
        ]
    );
}

#[test]
fn a_redirect_to_the_metadata_address_or_another_unsafe_place_is_refused() {
    for target in [
        "http://169.254.169.254/latest/meta-data",
        "https://169.254.169.254/",
        "https://meta.example.org/",
        "https://10.0.0.5/admin",
        "https://127.0.0.1:9/",
        "https://rebind.example.org/",
    ] {
        let r = rig(
            Policy::default(),
            FakeWire::new().then(Reply::redirect(302, target)),
        );
        let e = get(&r, "https://api.example.org/").unwrap_err();
        assert!(
            matches!(e, NetError::Refused { .. } | NetError::BadRedirect(_)),
            "{target}: {e:?}"
        );
        assert_eq!(
            r.wire.sends(),
            1,
            "{target}: the second hop never reached the wire"
        );
    }
}

#[test]
fn a_redirect_never_goes_farther_than_where_the_request_started() {
    // Started public: a private network and this computer are out even though the policy allows them.
    let r = rig(
        Policy::default().reach(Reach::Loopback),
        FakeWire::new().then(Reply::redirect(302, "https://nas.home/")),
    );
    assert!(matches!(
        get(&r, "https://api.example.org/"),
        Err(NetError::Refused {
            why: Refusal::TooFar(Class::Lan),
            ..
        })
    ));
    let r = rig(
        Policy::default().reach(Reach::Loopback),
        FakeWire::new().then(Reply::redirect(302, "https://127.0.0.1/")),
    );
    assert!(
        matches!(
            get(&r, "https://nas.home/"),
            Err(NetError::Refused {
                why: Refusal::TooFar(Class::Loopback),
                ..
            })
        ),
        "from the LAN, not to this computer"
    );
    // Started on this computer: free to go anywhere the policy allows.
    let r = rig(
        Policy::default().reach(Reach::Loopback),
        FakeWire::new()
            .then(Reply::redirect(302, "https://nas.home/"))
            .then(Reply::ok(b"x")),
    );
    assert_eq!(get(&r, "https://127.0.0.1/").unwrap().redirects, 1);
}

#[test]
fn a_redirect_to_plain_http_too_many_redirects_and_bad_locations_are_refused() {
    let r = rig(
        Policy::default().reach(Reach::Loopback),
        FakeWire::new().then(Reply::redirect(302, "http://127.0.0.1/")),
    );
    assert!(
        matches!(get(&r, "https://127.0.0.1/"), Err(NetError::BadRedirect(m)) if m.contains("plain http"))
    );
    let mut loop_wire = FakeWire::new();
    for _ in 0..12 {
        loop_wire = loop_wire.then(Reply::redirect(302, "/again"));
    }
    let r = rig(Policy::default().max_redirects(3), loop_wire);
    assert_eq!(
        get(&r, "https://api.example.org/").unwrap_err(),
        NetError::TooManyRedirects
    );
    assert_eq!(r.wire.sends(), 4);
    let r = rig(
        Policy::default().max_redirects(0),
        FakeWire::new().then(Reply::redirect(302, "/x")),
    );
    let resp = get(&r, "https://api.example.org/").unwrap();
    assert_eq!(
        (resp.status, resp.header("location"), r.wire.sends()),
        (302, Some("/x"), 1),
        "no redirects allowed: the 302 is the answer"
    );
    assert_eq!(
        Policy::default().max_redirects(99).max_redirects,
        MAX_REDIRECTS
    );
    let r = rig(
        Policy::default(),
        FakeWire::new().then(Reply::redirect(302, "http://[bad")),
    );
    assert!(matches!(
        get(&r, "https://api.example.org/"),
        Err(NetError::BadRedirect(_))
    ));
    // A redirect status without a Location is just the answer.
    let r = rig(Policy::default(), FakeWire::new().then(Reply::status(302)));
    assert_eq!(get(&r, "https://api.example.org/").unwrap().status, 302);
    // The allow-list applies to the target.
    let r = rig(
        Policy::default().allow_hosts(&["api.example.org"]),
        FakeWire::new().then(Reply::redirect(302, "https://other.example.net/")),
    );
    assert!(matches!(
        get(&r, "https://api.example.org/"),
        Err(NetError::NotAllowed(_))
    ));
}

#[test]
fn credentials_do_not_follow_a_redirect_to_another_host_and_a_303_becomes_a_get() {
    let wire = FakeWire::new()
        .then(Reply::redirect(302, "/same"))
        .then(Reply::redirect(303, "https://other.example.net/there"))
        .then(Reply::ok(b""));
    let r = rig(Policy::default(), wire);
    let req = Request::post("https://api.example.org/in", b"payload".to_vec())
        .header("Authorization", "Bearer t")
        .unwrap()
        .header("Cookie", "a=b")
        .unwrap()
        .header("Content-Type", "application/json")
        .unwrap()
        .header("X-Trace", "1")
        .unwrap();
    r.egress.send(&req, &CancelToken::new()).unwrap();
    let seen = r.wire.seen();
    let names = |i: usize| {
        seen[i]
            .headers
            .iter()
            .map(|h| h.0.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        (seen[0].method, seen[0].body.as_slice()),
        (Method::Post, &b"payload"[..])
    );
    assert_eq!(
        (seen[1].method, seen[1].body.len()),
        (Method::Get, 0),
        "302 after a POST is a GET"
    );
    assert!(
        names(1).contains(&"Authorization"),
        "same host keeps the login"
    );
    assert_eq!(
        (seen[2].method, names(2)),
        (Method::Get, vec!["X-Trace"]),
        "another host gets no login, cookie or content type"
    );
}

#[test]
fn a_307_keeps_the_method_and_body() {
    let r = rig(
        Policy::default(),
        FakeWire::new()
            .then(Reply::redirect(307, "/again"))
            .then(Reply::ok(b"")),
    );
    r.egress
        .send(
            &Request::post("https://api.example.org/in", b"p".to_vec()),
            &CancelToken::new(),
        )
        .unwrap();
    let seen = r.wire.seen();
    assert_eq!(
        (seen[1].method, seen[1].body.as_slice()),
        (Method::Post, &b"p"[..])
    );
}

#[test]
fn offline_refuses_before_a_name_is_resolved_or_a_socket_opened() {
    let r = rig(Policy::default(), FakeWire::new());
    r.egress.set_offline(true);
    assert!(r.egress.is_offline());
    assert_eq!(
        get(&r, "https://api.example.org/").unwrap_err(),
        NetError::Offline
    );
    assert!(
        r.egress
            .stream(
                &Request::get("https://api.example.org/"),
                &CancelToken::new(),
                None
            )
            .is_err()
    );
    assert!(r.lookup.asked().is_empty() && r.wire.sends() == 0);
    assert!(!NetError::Offline.retryable() && NetError::Offline.to_string().contains("offline"));
    r.egress.set_offline(false);
    assert!(get(&r, "https://api.example.org/").is_ok());
}

#[test]
fn bodies_are_capped_by_header_and_by_what_actually_arrives() {
    let big = vec![b'x'; 101];
    let r = rig(
        Policy::default().max_body(100),
        FakeWire::new()
            .then(Reply::ok(&big).header("Content-Length", "101"))
            .then(Reply::ok(&big))
            .then(Reply::ok(&big[..100])),
    );
    assert_eq!(
        get(&r, "https://api.example.org/").unwrap_err(),
        NetError::TooLarge(100),
        "announced"
    );
    assert_eq!(
        get(&r, "https://api.example.org/").unwrap_err(),
        NetError::TooLarge(100),
        "unannounced"
    );
    assert_eq!(
        get(&r, "https://api.example.org/").unwrap().body.len(),
        100,
        "exactly the limit is fine"
    );
    let e = r.egress.send(
        &Request::post(
            "https://api.example.org/",
            vec![0; (Policy::default().max_request_body + 1) as usize],
        ),
        &CancelToken::new(),
    );
    assert!(matches!(e, Err(NetError::Invalid(m)) if m.contains("request body")));
    assert!(NetError::TooLarge(5).to_string().contains("5 byte"));
}

#[test]
fn head_and_not_modified_have_no_body_and_conditional_requests_carry_the_etag() {
    let r = rig(
        Policy::default(),
        FakeWire::new().then(Reply::ok(b"xxxx")).then(
            Reply::status(304)
                .header("ETag", "\"v2\"")
                .header("Retry-After", "7"),
        ),
    );
    let head = r
        .egress
        .send(
            &Request::head("https://api.example.org/"),
            &CancelToken::new(),
        )
        .unwrap();
    assert!(head.body.is_empty());
    let req = Request::get("https://api.example.org/")
        .if_none_match("\"v2\"")
        .unwrap();
    let resp = r.egress.send(&req, &CancelToken::new()).unwrap();
    assert!(
        resp.not_modified() && !resp.ok() && resp.body.is_empty() && resp.etag() == Some("\"v2\"")
    );
    assert_eq!(
        r.wire.seen()[1].headers,
        [("If-None-Match".to_string(), "\"v2\"".to_string())]
    );
    assert_eq!(
        resp.retry_after(SystemTime::UNIX_EPOCH),
        Some(Duration::from_secs(7))
    );
    assert_eq!(resp.header("RETRY-after"), Some("7"));
    assert_eq!(Method::Head.as_str(), "HEAD");
}

#[test]
fn headers_are_validated() {
    let base = Request::get("https://api.example.org/");
    for (n, v) in [
        ("", "x"),
        ("bad name", "x"),
        ("Host", "x"),
        ("content-length", "1"),
        ("Accept-Encoding", "gzip"),
        ("Proxy-Authorization", "x"),
        ("X-A", "a\nb"),
        ("X-A", &"v".repeat(9000)),
    ] {
        assert!(
            matches!(base.clone().header(n, v), Err(NetError::Invalid(_))),
            "{n:?}"
        );
    }
    let mut r = base.clone();
    for i in 0..32 {
        r = r.header(&format!("X-{i}"), "v").unwrap();
    }
    assert!(
        matches!(r.header("X-Last", "v"), Err(NetError::Invalid(m)) if m.contains("at most 32"))
    );
    assert!(
        base.header("Range", "bytes=0-1").is_ok(),
        "a range is the caller's to ask"
    );
}

#[test]
fn requests_to_one_host_keep_the_least_gap_on_the_clock() {
    let r = rig(
        Policy::default().min_gap(Duration::from_secs(2)),
        FakeWire::new(),
    );
    assert!(
        get(&r, "https://api.example.org/a").is_ok(),
        "the first is not delayed"
    );
    assert!(
        get(&r, "https://other.example.net/a").is_ok(),
        "another host is not delayed"
    );
    let egress = r.egress.clone();
    std::thread::scope(|s| {
        let h = s.spawn(|| {
            egress.send(
                &Request::get("https://api.example.org/b"),
                &CancelToken::new(),
            )
        });
        assert!(
            r.clock.wait_for_waiters(1, Duration::from_secs(30)),
            "the second request waits on the clock"
        );
        assert_eq!(r.wire.sends(), 2);
        r.clock.advance(Duration::from_secs(2));
        assert!(h.join().unwrap().is_ok());
    });
    assert_eq!(r.wire.sends(), 3);
    assert_eq!(r.clock.monotonic(), Duration::from_secs(2));
}

#[test]
fn a_wait_for_the_gap_can_be_cancelled_and_cancel_before_the_start_sends_nothing() {
    let r = rig(
        Policy::default().min_gap(Duration::from_secs(2)),
        FakeWire::new(),
    );
    get(&r, "https://api.example.org/a").unwrap();
    let cancel = CancelToken::new();
    let egress = r.egress.clone();
    let out = std::thread::scope(|s| {
        let h = s.spawn(|| egress.send(&Request::get("https://api.example.org/b"), &cancel));
        assert!(r.clock.wait_for_waiters(1, Duration::from_secs(30)));
        cancel.cancel();
        h.join().unwrap()
    });
    assert_eq!(out.unwrap_err(), NetError::Cancelled);
    assert_eq!(r.wire.sends(), 1);
    assert_eq!(r.clock.waiters(), 0);
    assert_eq!(
        r.egress
            .send(&Request::get("https://api.example.org/c"), &cancel)
            .unwrap_err(),
        NetError::Cancelled
    );
}

#[test]
fn wire_errors_pass_through_and_say_whether_a_retry_can_help() {
    for (e, retry) in [
        (NetError::Connect("h".into()), true),
        (NetError::Timeout("h".into()), true),
        (NetError::Tls("h".into()), false),
        (NetError::Proxy("p".into()), false),
        (NetError::Cancelled, false),
        (NetError::TooManyRedirects, false),
        (NetError::NotAllowed("h".into()), false),
        (NetError::Scheme("h".into()), false),
        (NetError::BadRedirect("why".into()), false),
        (
            NetError::Refused {
                host: "h".into(),
                why: Refusal::NoAddress,
            },
            false,
        ),
    ] {
        assert_eq!(e.retryable(), retry, "{e:?}");
        assert!(!e.to_string().is_empty());
        let r = rig(Policy::default(), FakeWire::new().then_error(e.clone()));
        assert_eq!(get(&r, "https://api.example.org/").unwrap_err(), e);
    }
    for why in [
        Refusal::NoAddress,
        Refusal::Never,
        Refusal::TooFar(Class::Loopback),
        Refusal::TooFar(Class::Lan),
    ] {
        assert!(!why.to_string().is_empty());
    }
}

#[test]
fn downloads_go_through_the_same_policy() {
    let wire = FakeWire::new()
        .then(Reply::redirect(302, "https://cdn.example.org/file.bin"))
        .then(
            Reply::status(206)
                .header("Content-Range", "bytes 0-3/10")
                .header("Content-Length", "4")
                .header("ETag", "\"e\"")
                .header("Last-Modified", "x")
                .header("Content-Encoding", "identity")
                .header("Retry-After", "3"),
        )
        .then(Reply::redirect(302, "http://169.254.169.254/x"));
    let r = rig(Policy::default(), wire);
    let t = r.egress.download_transport();
    let ranged = paddock_download::ByteRange {
        start: 0,
        end: Some(3),
    };
    let extra = [paddock_download::Header::new("X-Token", "t").unwrap()];
    let resp = t
        .get(&Get {
            url: "https://api.example.org/file",
            range: Some(ranged),
            if_range: Some("\"e\""),
            body_timeout: None,
            headers: &extra,
        })
        .unwrap();
    assert_eq!(resp.status, 206);
    assert_eq!(resp.headers.content_range.as_deref(), Some("bytes 0-3/10"));
    assert_eq!(
        (
            resp.headers.etag.as_deref(),
            resp.headers.retry_after.as_deref(),
            resp.headers.content_length.as_deref()
        ),
        (Some("\"e\""), Some("3"), Some("4"))
    );
    let sent = &r.wire.seen()[1];
    let names: Vec<&str> = sent.headers.iter().map(|h| h.0.as_str()).collect();
    assert_eq!(names, ["X-Token", "Range", "If-Range"]);
    assert_eq!(sent.headers[1].1, "bytes=0-3");
    let e = t
        .get(&Get {
            url: "https://api.example.org/other",
            range: None,
            if_range: None,
            body_timeout: None,
            headers: &[],
        })
        .err()
        .unwrap();
    assert_eq!(
        e.kind,
        paddock_download::TransportErrorKind::Invalid,
        "a redirect to the metadata address is refused: {e}"
    );
    for (net, kind) in [
        (
            NetError::Connect("h".into()),
            paddock_download::TransportErrorKind::Connect,
        ),
        (
            NetError::Timeout("h".into()),
            paddock_download::TransportErrorKind::Timeout,
        ),
        (
            NetError::Tls("h".into()),
            paddock_download::TransportErrorKind::Tls,
        ),
        (
            NetError::Proxy("h".into()),
            paddock_download::TransportErrorKind::Proxy,
        ),
    ] {
        let r = rig(Policy::default(), FakeWire::new().then_error(net));
        let e = r
            .egress
            .download_transport()
            .get(&Get {
                url: "https://api.example.org/",
                range: None,
                if_range: None,
                body_timeout: None,
                headers: &[],
            })
            .err()
            .unwrap();
        assert_eq!(e.kind, kind);
    }
}

#[test]
fn reach_classes_and_orders() {
    use std::net::IpAddr;
    assert!(Reach::Public < Reach::Lan && Reach::Lan < Reach::Loopback);
    assert_eq!(Reach::default(), Reach::Public);
    assert_eq!(Reach::needed_for(Class::Never), None);
    assert_eq!(Reach::needed_for(Class::Lan), Some(Reach::Lan));
    for (ip, class) in [
        ("8.8.8.8", Class::Public),
        ("100.63.0.1", Class::Public),
        ("100.128.0.1", Class::Public),
        ("172.32.0.1", Class::Public),
        ("2001:db8::1", Class::Public),
        ("0.1.2.3", Class::Never),
    ] {
        assert_eq!(classify(ip.parse::<IpAddr>().unwrap()), class, "{ip}");
    }
    assert!(
        host_is_loopback("LOCALHOST")
            && host_is_loopback("[::1]")
            && host_is_loopback("127.1.2.3")
            && !host_is_loopback("localhost.example.com")
            && !host_is_loopback("10.0.0.1")
    );
    assert_eq!(vet(&[], Reach::Loopback), Err(Refusal::NoAddress));
    assert_eq!(
        vet(
            &["10.0.0.1".parse().unwrap(), "127.0.0.1".parse().unwrap()],
            Reach::Loopback
        ),
        Ok(Class::Loopback)
    );
}

#[test]
fn defaults_are_the_strict_ones() {
    let p = Policy::default();
    assert_eq!(
        (p.reach, p.max_redirects, p.max_body, p.min_gap),
        (Reach::Public, 5, 8 << 20, Duration::ZERO)
    );
    assert!(p.user_agent.starts_with("paddock-net/") && p.hosts.is_none());
    let p = Policy::default()
        .timeout(Duration::from_secs(3))
        .max_body(9)
        .min_gap(Duration::from_millis(5));
    assert_eq!(
        (p.timeout, p.max_body, p.min_gap),
        (Duration::from_secs(3), 9, Duration::from_millis(5))
    );
    let r = rig(p, FakeWire::new());
    assert_eq!(r.egress.policy().max_body, 9);
}

proptest! {
    #[test]
    fn classification_never_panics_and_reach_is_monotonic(a in any::<u32>(), v6 in any::<u128>()) {
        for ip in [std::net::IpAddr::from(std::net::Ipv4Addr::from(a)), std::net::IpAddr::from(std::net::Ipv6Addr::from(v6))] {
            let c = classify(ip);
            for (lo, hi) in [(Reach::Public, Reach::Lan), (Reach::Lan, Reach::Loopback)] {
                prop_assert!(!lo.allows(c) || hi.allows(c));
            }
            prop_assert_eq!(c == Class::Never, !Reach::Loopback.allows(c));
        }
    }

    #[test]
    fn the_egress_never_panics_on_any_address(url in ".{0,80}") {
        let r = rig(Policy::default().reach(Reach::Loopback), FakeWire::new());
        let _ = get(&r, &url);
        let _ = HostPattern::parse(&url);
    }
}
