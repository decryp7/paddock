//! A source scan that keeps the leak class out (ER-21): in every crate the store worker links, a `log::warn!`, `log::error!` or
//! `log::info!` call must not carry an error's text (`{e}`, `{err:#}`, `{:#}`), a path (`.display()`, a `path =` property) or a
//! user's name. A failure goes through `debrief_core::log_failure!` (the error's class at warn or error, its text at debug).
//! A call that is safe for a reason says so on a `// log-allow: <reason>` comment on one of the three lines above it or inside it.
//!
//! The crates are the `debrief-*` ones the worker depends on, read from `Cargo.lock`. Code after a `#[cfg(test)]` is not scanned.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const LEVELS: [&str; 3] = ["log::warn!(", "log::error!(", "log::info!("];
/// Text in a call that quotes an error, a path or a name.
const LEAKS: [&str; 10] = [
    "{e}",
    "{e:",
    "{err}",
    "{err:",
    "{error}",
    "{error:",
    "{text}",
    "{:#}",
    ".display()",
    "{why}",
];

/// The `debrief-*` crates `root` (the worker) depends on, by name.
fn worker_crates(lock: &str) -> BTreeSet<String> {
    let mut deps: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for block in lock.split("[[package]]").skip(1) {
        let name = block
            .lines()
            .find_map(|l| l.strip_prefix("name = \""))
            .and_then(|l| l.strip_suffix('"'))
            .unwrap_or_default()
            .to_string();
        let list = block
            .split_once("dependencies = [")
            .map(|(_, rest)| rest.split_once(']').map_or(rest, |(l, _)| l))
            .unwrap_or_default();
        let names = list
            .lines()
            .filter_map(|l| l.trim().trim_matches(',').trim_matches('"').split(' ').next().map(str::to_string))
            .filter(|n| !n.is_empty())
            .collect();
        deps.insert(name, names);
    }
    let mut seen = BTreeSet::new();
    let mut todo = vec!["debrief-store-worker".to_string()];
    while let Some(n) = todo.pop() {
        if !n.starts_with("debrief-") || !seen.insert(n.clone()) {
            continue;
        }
        todo.extend(deps.get(&n).cloned().unwrap_or_default());
    }
    seen
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The text of `source` without its `#[cfg(test)]` items (the item's braces are matched).
fn without_tests(source: &str) -> String {
    let mut out = String::new();
    let mut rest = source;
    while let Some(i) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        let (brace, semi) = (after.find('{'), after.find(';'));
        match (brace, semi) {
            (Some(b), Some(s)) if s < b => rest = &after[s + 1..],
            (Some(b), _) => {
                let mut depth = 0usize;
                let mut end = after.len();
                for (k, c) in after[b..].char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = b + k + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                rest = &after[end..];
            }
            _ => rest = "",
        }
    }
    out.push_str(rest);
    out
}

/// The text of the call that starts at `from` (just after its opening parenthesis), up to the matching one.
fn call_text(source: &str, from: usize) -> &str {
    let (mut depth, mut in_str, mut escaped) = (1usize, false, false);
    for (k, c) in source[from..].char_indices() {
        if in_str {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &source[from..from + k];
                }
            }
            _ => {}
        }
    }
    &source[from..]
}

/// The leaks in `source`: (line, call) for each call with a leak and no reasoned allowance.
fn leaks(source: &str) -> Vec<(usize, String)> {
    let source = without_tests(source);
    let lines: Vec<&str> = source.lines().collect();
    let mut found = Vec::new();
    for level in LEVELS {
        for (at, _) in source.match_indices(level) {
            let call = call_text(&source, at + level.len());
            let line = source[..at].matches('\n').count();
            let leaky = LEAKS.iter().any(|l| call.contains(l)) || has_path_property(call);
            let window = lines[line.saturating_sub(3)..=line].join("\n") + call;
            if leaky && !allowed(&window) {
                found.push((line + 1, call.split_whitespace().collect::<Vec<_>>().join(" ")));
            }
        }
    }
    found
}

/// A key/value named like a path (`path = ...`, `Path = ...`), before the `;` that ends the properties.
fn has_path_property(call: &str) -> bool {
    let props = call.split_once(';').map_or("", |(p, _)| p);
    props
        .split(',')
        .filter_map(|kv| kv.split_once('='))
        .any(|(k, _)| k.trim().to_ascii_lowercase().contains("path"))
}

/// A `// log-allow: <reason>` with a real reason.
fn allowed(window: &str) -> bool {
    window
        .lines()
        .any(|l| l.split_once("// log-allow:").is_some_and(|(_, reason)| reason.trim().len() >= 8))
}

#[test]
fn no_error_text_path_or_name_in_a_log_call_of_the_crates_the_worker_links() {
    let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let lock = std::fs::read_to_string(crates_dir.join("../Cargo.lock")).expect("Cargo.lock");
    let crates = worker_crates(&lock);
    for must in [
        "debrief-core",
        "debrief-plugin-host",
        "debrief-store-duck",
        "debrief-store-worker",
        "debrief-ipc",
        "debrief-store-client",
    ] {
        assert!(crates.contains(must), "the worker links {must}: {crates:?}");
    }
    let mut report = Vec::new();
    for name in &crates {
        let mut files = Vec::new();
        rust_files(&crates_dir.join(name).join("src"), &mut files);
        for file in files {
            let base = file.file_name().unwrap().to_string_lossy().into_owned();
            if base.contains("test") {
                continue;
            }
            let source = std::fs::read_to_string(&file).unwrap();
            for (line, call) in leaks(&source) {
                report.push(format!("{}:{line}: {call}", file.strip_prefix(&crates_dir).unwrap().display()));
            }
        }
    }
    assert!(
        report.is_empty(),
        "log calls that can quote an error, a path or a name; use debrief_core::log_failure! or add `// log-allow: <reason>`:\n{}",
        report.join("\n")
    );
}

#[test]
fn the_scan_sees_a_leak_and_honours_a_reasoned_allowance() {
    let leak = "fn f(e: Error) {\n    log::warn!(\"could not remove: {e:#}\");\n}\n";
    assert_eq!(leaks(leak).len(), 1);
    assert_eq!(leaks("fn f() { log::error!(\"x {}\", p.display()); }").len(), 1);
    assert_eq!(leaks("fn f() { log::info!(path = p.as_str(); \"opened\"); }").len(), 1);
    assert_eq!(leaks("fn f() { log::warn!(\"multi\n line {err}\"); }").len(), 1);
    assert!(leaks("fn f() { log::warn!(\"no error here {id}\"); log::debug!(\"{e}\"); }").is_empty());
    let allowed = "fn f() {\n    // log-allow: the id is a plugin id, never content\n    log::warn!(\"plugin {e}\");\n}\n";
    assert!(leaks(allowed).is_empty());
    let no_reason = "fn f() {\n    // log-allow:\n    log::warn!(\"plugin {e}\");\n}\n";
    assert_eq!(leaks(no_reason).len(), 1, "an allowance needs a reason");
    // Test code is not scanned.
    assert!(leaks("#[cfg(test)]\nmod tests {\n    fn f() { log::warn!(\"{e}\"); }\n}\nfn g() {}\n").is_empty());
    let after = "#[cfg(test)]\nmod tests { fn f() {} }\nfn g() { log::warn!(\"{e}\"); }\n";
    assert_eq!(leaks(after).len(), 1, "code after a test module is scanned");
}

#[test]
fn the_crates_come_from_the_lock_file() {
    let lock = "[[package]]\nname = \"debrief-store-worker\"\nversion = \"0.1.0\"\ndependencies = [\n \"debrief-core\",\n \"serde 1.0\",\n]\n\n[[package]]\nname = \"debrief-core\"\ndependencies = [\n \"debrief-leaf\",\n]\n\n[[package]]\nname = \"debrief-leaf\"\n\n[[package]]\nname = \"debrief-unrelated\"\n";
    let got: Vec<String> = worker_crates(lock).into_iter().collect();
    assert_eq!(got, ["debrief-core", "debrief-leaf", "debrief-store-worker"]);
}
