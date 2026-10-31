//! The per-channel "home network" switch: validation, the reach each channel's request goes out with, offline mode, old files.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use crate::common;
use common::*;
use paddock_channels::*;
use paddock_jobs::CancelToken;
use paddock_jobs::time::FakeClock;
use paddock_net::{Egress, FakeWire, Reach, StaticLookup};
use std::sync::Arc;

fn hook(url: &str, allow_lan: bool) -> ChannelConfig {
    let mut c = webhook("w", false);
    c.allow_lan = allow_lan;
    c.transport = Transport::Webhook {
        url: url.into(),
        auth: false,
    };
    c
}

fn valid(c: ChannelConfig) -> Result<(), String> {
    Settings { channels: vec![c] }
        .validate()
        .map_err(|e| e.to_string())
}

#[test]
fn the_switch_is_off_by_default_and_old_files_without_it_load() {
    assert!(!webhook("w", false).allow_lan);
    let s: Settings = serde_json::from_str(
        r#"{"channels":[{"id":"w","transport":{"type":"webhook","url":"https://a.example.org/x"}}]}"#,
    )
    .unwrap();
    assert!(!s.channels[0].allow_lan);
    let mut on = s.clone();
    on.channels[0].allow_lan = true;
    let json = serde_json::to_string(&on).unwrap();
    assert!(json.contains(r#""allowLan":true"#), "{json}");
    assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), on);
}

#[test]
fn plain_http_to_a_private_address_needs_the_switch() {
    for url in [
        "http://192.168.1.20:8080/hook",
        "http://10.0.0.5/x",
        "http://[fd00::5]/x",
        "http://nas.local/x",
        "http://nas/x",
    ] {
        let off = valid(hook(url, false)).unwrap_err();
        assert!(off.contains("home network"), "{url}: {off}");
        assert_eq!(valid(hook(url, true)), Ok(()), "{url}");
    }
}

#[test]
fn https_to_a_private_address_needs_the_switch() {
    for url in [
        "https://192.168.1.20/x",
        "https://nas.local/x",
        "https://[fd00::5]/x",
    ] {
        assert!(
            valid(hook(url, false))
                .unwrap_err()
                .contains("private network"),
            "{url}"
        );
        assert_eq!(valid(hook(url, true)), Ok(()), "{url}");
    }
}

#[test]
fn plain_http_to_a_public_host_stays_rejected_even_with_the_switch() {
    for url in [
        "http://hooks.example.org/x",
        "http://8.8.8.8/x",
        "http://169.254.169.254/x",
    ] {
        assert!(
            valid(hook(url, true)).unwrap_err().contains("plain http"),
            "{url}"
        );
    }
    // This computer keeps working with or without the switch; a public https host never needed it.
    for allow in [false, true] {
        assert_eq!(valid(hook("http://localhost:8080/x", allow)), Ok(()));
        assert_eq!(valid(hook("https://hooks.example.org/x", allow)), Ok(()));
    }
}

#[test]
fn an_ntfy_server_on_the_home_network_follows_the_same_rule() {
    let mut c = ntfy("n", false);
    c.transport = Transport::Ntfy {
        server: "http://ntfy.lan".into(),
        topic: "t".into(),
        auth: false,
    };
    assert!(valid(c.clone()).is_err());
    c.allow_lan = true;
    assert_eq!(valid(c), Ok(()));
}

/// An egress that may reach private networks, over fakes: the wire records the reach each request went out with.
fn egress(wire: Arc<FakeWire>) -> Arc<Egress> {
    let lookup = StaticLookup::new()
        .with("nas.local", &["192.168.1.20"])
        .with("hooks.example.org", &["93.184.216.34"])
        .with("sneaky.example.org", &["192.168.1.20"]);
    Arc::new(Egress::new(
        channel_policy(Reach::Lan),
        wire,
        Arc::new(lookup),
        Arc::new(FakeClock::new()),
    ))
}

fn post(
    egress: &Arc<Egress>,
    url: &str,
    lan: bool,
) -> Result<PostResponse, paddock_download::TransportError> {
    EgressPoster::new(egress.clone()).post(&PostRequest {
        url: url.into(),
        headers: Vec::new(),
        body: b"{}".to_vec(),
        timeout: secs(5),
        lan,
    })
}

#[test]
fn only_a_channel_with_the_switch_goes_out_with_the_lan_reach() {
    let wire = Arc::new(FakeWire::new());
    let e = egress(wire.clone());
    assert!(post(&e, "http://192.168.1.20:8080/hook", true).is_ok());
    assert!(post(&e, "https://nas.local/hook", true).is_ok());
    assert!(post(&e, "https://hooks.example.org/in", true).is_ok());
    assert!(post(&e, "https://hooks.example.org/in", false).is_ok());
    let reach: Vec<Reach> = wire.seen().iter().map(|s| s.reach).collect();
    assert_eq!(
        reach,
        [Reach::Lan, Reach::Lan, Reach::Public, Reach::Public]
    );
}

#[test]
fn a_channel_without_the_switch_never_reaches_a_private_address_or_plain_http() {
    let wire = Arc::new(FakeWire::new());
    let e = egress(wire.clone());
    // The egress allows private networks for the app, but this channel did not ask for it.
    assert!(post(&e, "https://nas.local/hook", false).is_err());
    assert!(post(&e, "https://192.168.1.20/hook", false).is_err());
    assert!(post(&e, "http://192.168.1.20/hook", false).is_err());
    // A name that points at a private address is refused too, and plain http to a public one never goes out.
    assert!(post(&e, "https://sneaky.example.org/hook", false).is_err());
    assert!(post(&e, "http://hooks.example.org/hook", true).is_err());
    assert_eq!(wire.sends(), 0);
}

#[test]
fn a_dispatched_message_carries_the_switch_to_the_poster() {
    let mut lan = hook("http://192.168.1.20/hook", true);
    lan.id = "lan".into();
    let mut public = hook("https://hooks.example.org/in", false);
    public.id = "pub".into();
    let r = rig(
        vec![lan, public],
        FakePoster::new(),
        FakeMail::new(),
        store(),
    );
    r.dispatcher
        .dispatch(&message("diskLow", Severity::Error), &CancelToken::new());
    // Channels deliver on their own threads: the order the poster saw them in is not the settings order.
    let mut seen: Vec<(String, bool)> = r
        .poster
        .requests()
        .into_iter()
        .map(|q| (q.url, q.lan))
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        [
            ("http://192.168.1.20/hook".to_string(), true),
            ("https://hooks.example.org/in".to_string(), false)
        ]
    );
}

#[test]
fn offline_mode_blocks_a_home_network_channel_too() {
    let wire = Arc::new(FakeWire::new());
    let e = egress(wire.clone());
    e.set_offline(true);
    let err = post(&e, "http://192.168.1.20/hook", true).unwrap_err();
    assert!(err.message.contains("offline"), "{err}");
    assert_eq!(wire.sends(), 0);
}

#[test]
fn an_address_is_judged_as_the_network_layer_reads_it() {
    // Octal, hex and short spellings of a private address are the private address, so they need the switch too.
    for url in [
        "https://0300.0250.1.20/x",
        "https://0xc0a80114/x",
        "https://3232235796/x",
        "http://192.168.1/x",
    ] {
        let off = valid(hook(url, false)).unwrap_err();
        assert!(
            off.contains("private network") || off.contains("home network"),
            "{url}: {off}"
        );
        assert_eq!(valid(hook(url, true)), Ok(()), "{url}");
    }
    assert!(
        valid(hook("https://", true))
            .unwrap_err()
            .contains("valid host")
    );
    assert!(valid(hook("https://a b/x", true)).is_err());
    assert!(
        valid(hook("https://user:pw@nas.local/x", true))
            .unwrap_err()
            .contains("login")
    );
}

#[test]
fn the_switch_is_for_webhook_and_ntfy_only() {
    let mut c = email("m", None);
    c.allow_lan = true;
    assert!(valid(c).unwrap_err().contains("webhook and ntfy"));
}
