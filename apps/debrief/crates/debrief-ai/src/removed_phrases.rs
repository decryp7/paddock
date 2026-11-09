//! ADR 0070 D7.8, extended to the engine: no text the user can read points to something that was removed (the built-in
//! engine, its download proxy and model settings, image projectors). Scans the string literals of the crate's sources
//! (comments and tests aside) and the messages of the error types.

use std::path::{Path, PathBuf};

const REMOVED: &[&str] = &["built-in engine", "Download proxy", "image support", "llama", "projector"];

/// Files where a removed word is data, not text for the user: the removed settings' names and the llama.cpp release URL
/// of the catalog validator (both leave with M2c).
const DATA_FILES: &[&str] = &["settings.rs", "runtimes/catalog.rs", "runtimes/github.rs"];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The string literals of the non-comment code before the file's `#[cfg(test)]` module.
fn literals(text: &str) -> Vec<String> {
    let code = text.split("#[cfg(test)]").next().unwrap_or(text);
    let mut out = Vec::new();
    for line in code.lines().filter(|l| !l.trim_start().starts_with("//")) {
        let mut rest = line;
        while let Some(i) = rest.find('"') {
            let after = &rest[i + 1..];
            let Some(j) = after.find('"') else { break };
            out.push(after[..j].to_string());
            rest = &after[j + 1..];
        }
    }
    out
}

#[test]
fn no_user_text_points_to_removed_things() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&root, &mut files);
    let mut hits = Vec::new();
    for f in files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        if rel == "removed_phrases.rs" || rel.ends_with("tests.rs") || rel.contains("/tests/") || rel.ends_with("fake.rs") {
            continue;
        }
        for lit in literals(&std::fs::read_to_string(&f).unwrap()) {
            for word in REMOVED {
                if lit.contains(word) && !DATA_FILES.contains(&rel.as_str()) {
                    hits.push(format!("{rel}: {lit:?} mentions {word:?}"));
                }
            }
        }
    }
    assert!(hits.is_empty(), "text for the user names removed things:\n{}", hits.join("\n"));
}

#[test]
fn the_messages_of_the_error_types_name_nothing_removed() {
    use crate::engine::pitwall::PitwallError as P;
    use crate::net::{NetError, NetKind};
    use crate::AiError as E;
    let mut texts = vec![
        E::NoModel.to_string(),
        E::NoVision.to_string(),
        E::Memory("x".into()).to_string(),
        E::Gated { url: "u".into() }.to_string(),
        P::NotRunning("x".into()).to_string(),
        P::Stopped.to_string(),
        P::Unauthorized("x".into()).to_string(),
        P::NoModel("x".into()).to_string(),
        P::Memory("x".into()).to_string(),
        P::Busy("x".into()).to_string(),
        P::Protocol("x".into()).to_string(),
    ];
    for k in [
        NetKind::Tls,
        NetKind::ProxyAuth,
        NetKind::Proxy,
        NetKind::Offline,
        NetKind::Timeout,
        NetKind::Http(403),
    ] {
        texts.push(NetError::new(k, "example.org", "detail").to_string());
    }
    for t in texts {
        for word in REMOVED {
            assert!(!t.contains(word), "{t:?} mentions {word:?}");
        }
        assert!(!t.contains("Settings → AI"), "{t:?} points to the removed network settings");
    }
}
