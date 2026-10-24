//! Proxy settings and response headers are untrusted text: no input makes the parsers panic (conformance check C36).
use paddock_download::{
    NoProxy, ProxyServer, parse_content_range, parse_length, parse_retry_after,
};
use paddock_testkit::assert_never_panics;
use std::time::SystemTime;

#[test]
fn proxy_addresses_and_headers_survive_hostile_text() {
    let seeds: [&[u8]; 5] = [
        b"http://user:p%40ss@proxy.example:8080",
        b"[::1]:3128",
        b"localhost,.example.org,10.0.0.0/8",
        b"bytes 0-99/1000",
        b"120",
    ];
    assert_never_panics(&seeds, |b| {
        let text = String::from_utf8_lossy(b);
        let _ = ProxyServer::parse(&text);
        let _ = NoProxy::parse(&text);
        let _ = parse_content_range(&text);
        let _ = parse_length(&text);
        let _ = parse_retry_after(&text, SystemTime::UNIX_EPOCH);
    });
}
