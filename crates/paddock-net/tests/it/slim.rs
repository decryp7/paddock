//! The slim build: `paddock-net` without its `http` feature (address classes, `Lookup`, `Egress::check_host`) must not pull
//! the TLS stack. Reads the dependency graph with `cargo tree` (offline: it reads the lock file, nothing is downloaded or built).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

fn tree(extra: &[&str]) -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "tree",
            "--offline",
            "--locked",
            "-p",
            "paddock-net",
            "-e",
            "normal",
            "--prefix",
            "none",
        ])
        .args(extra)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn has(tree: &str, krate: &str) -> bool {
    tree.lines()
        .any(|l| l.split_whitespace().next() == Some(krate))
}

#[test]
fn without_the_http_feature_no_tls_stack_is_pulled() {
    let slim = tree(&["--no-default-features"]);
    assert!(
        has(&slim, "paddock-net") && has(&slim, "paddock-download"),
        "{slim}"
    );
    for krate in ["ureq", "rustls"] {
        assert!(!has(&slim, krate), "{krate} is in the slim build:\n{slim}");
    }
}

#[test]
fn the_default_build_still_has_the_real_wire() {
    let full = tree(&[]);
    for krate in ["ureq", "rustls"] {
        assert!(
            has(&full, krate),
            "{krate} is missing from the default build:\n{full}"
        );
    }
}
