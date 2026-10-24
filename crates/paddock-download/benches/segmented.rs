//! `cargo bench -p paddock-download`: one file from an in-process server that limits every connection to the same rate (as many
//! CDNs do), downloaded over one stream and over 2, 4 and 8 segmented connections. Prints a table; no external harness, no
//! network. `PADDOCK_BENCH_MIB` (default 32) sets the file size, `PADDOCK_BENCH_RATE_MIB` (default 8) the per-connection rate.

#[path = "../tests/it/support/mod.rs"]
mod support;

use paddock_download::{
    Config, Downloader, HostCaps, HttpConfig, Integrity, Request, Sha256Digest,
};
use std::sync::Arc;
use std::time::Instant;
use support::{Server, blob, job, partial, sha256_hex};

fn env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let mib = env("PADDOCK_BENCH_MIB", 32);
    let rate = env("PADDOCK_BENCH_RATE_MIB", 8) * 1024 * 1024;
    let data = blob((mib * 1024 * 1024) as usize);
    let digest = Sha256Digest::from_hex(&sha256_hex(&data)).expect("digest");
    let d2 = data.clone();
    let server = Server::start(move |r, _| partial(r, &d2).rate(rate));
    let dir = tempfile::tempdir().expect("temp dir");
    println!("{mib} MiB at {} MiB/s per connection", rate / (1024 * 1024));
    println!(
        "{:<12} {:>10} {:>12} {:>9}",
        "connections", "seconds", "MiB/s", "speed-up"
    );
    let mut single = None;
    for connections in [1usize, 2, 4, 8] {
        let config = Config {
            connections,
            segment_threshold: 1,
            ..Config::default()
        };
        let d = Downloader::http(HttpConfig {
            proxy: paddock_download::ProxyMode::Direct,
            ..HttpConfig::default()
        })
        .expect("transport")
        .with_config(config)
        .with_hosts(Arc::new(HostCaps::new(connections)));
        let req = Request::new(
            &server.url("/f"),
            dir.path().join(format!("f{connections}")),
            Integrity::Sha256(digest),
        )
        .expect("request");
        let (ctx, _) = job();
        let t = Instant::now();
        let got = d.download(&ctx, &req).expect("download");
        let secs = t.elapsed().as_secs_f64();
        assert_eq!(got.bytes, data.len() as u64);
        let base = *single.get_or_insert(secs);
        println!(
            "{connections:<12} {secs:>10.2} {:>12.1} {:>8.1}x",
            mib as f64 / secs,
            base / secs
        );
    }
}
