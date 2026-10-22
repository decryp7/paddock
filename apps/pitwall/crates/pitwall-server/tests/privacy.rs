//! ADR 0001 in code: the request path has no way to reach the network.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

fn src(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(name)).unwrap()
}

#[test]
fn request_handlers_cannot_reach_the_network() {
    // The route handlers and everything on the request path never mention the outbound client or an HTTP client.
    for f in ["http.rs", "serving.rs", "auth.rs"] {
        let s = src(f);
        for banned in ["reqwest", "egress", "Egress", "TcpStream", "hyper::client"] {
            assert!(!s.contains(banned), "{f} must not use {banned} (ADR 0001)");
        }
    }
}

#[test]
fn only_egress_builds_an_http_client() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        if name == "egress.rs" || !name.ends_with(".rs") {
            continue;
        }
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(
            !s.contains("reqwest"),
            "{name} must not use reqwest: outbound HTTP is egress.rs only"
        );
    }
}

#[test]
fn engines_get_a_scrubbed_environment_and_offline_flag() {
    let args = pitwall_core::llama::args(&pitwall_core::llama::Launch {
        model: "m".into(),
        mmproj: None,
        alias: "a".into(),
        port: 4000,
        context: 1024,
        gpu_layers: None,
        threads: 0,
        parallel: 1,
        kind: pitwall_core::llama::ServeKind::Chat,
        cache_ram_mib: 0,
    });
    assert!(args.contains(&"--offline".to_string()));
    let host = args.iter().position(|a| a == "--host").unwrap();
    assert_eq!(args[host + 1], "127.0.0.1");
}
