//! A source scan that keeps the leak class out (ER-21): in every crate the store worker links, a log call at info, warn or error
//! must not carry an error's text, a path or a user's name. A failure goes through `debrief_core::log_failure!` (the error's
//! class at warn or error, its text at debug); its *message* argument is scanned like a log call's.
//!
//! What is flagged, in the format string and its arguments of `log::warn!`, `error!`, `info!`, `log::log!`, `tracing::…!`,
//! bare `warn!(`…, `::log::…!` and the message of `log_failure!`:
//! - a placeholder named like an error or a name (`{e}`, `{err:#}`, `{reason}`, `{msg}`, `{path}`, `{p}`, `{file}`, `{source}`…);
//! - a `{:?}` of anything, and `{:#}` with no name;
//! - a positional argument (`"{}", e`, `"{:?}", err`) whose expression names one of those, or ends in `.display()`;
//! - `.display()`, `.to_string_lossy()` and `.to_str()` anywhere in the call;
//! - a key/value property named like a path.
//!
//! A call that is safe for a reason says so on a `// log-allow: <reason>` comment inside it or on one of the three lines above
//! (a reason of at least 8 characters).
//!
//! Scanned: the `debrief-*` crates the worker depends on, read from `Cargo.lock`. NOT scanned yet: the `paddock-*` crates it
//! links (paddock-log, -env, -proc, -diag, -dialogs, -commands, -jobs, -settings). They are shared with the other apps, and a
//! scan of them finds log calls with a message or a window label in them (paddock-env's `versioned.rs` prints a path,
//! paddock-log's panic hook prints the panic message, paddock-settings prints a setting's message): each needs a shared fix or
//! a reasoned allowance in code the other apps ship, which waits for its own change.
//!
//! Not scanned in a crate: `tests.rs`, `*_tests.rs`, files under `tests/`, modules declared behind `#[cfg(test)]` (found by the
//! attribute, in code and not in comments or strings), and `#[cfg(test)]` items. Shipped test-support modules (`testing.rs`) are
//! scanned.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Names that quote an error, a path or a user's name when printed.
const NAMES: [&str; 14] = [
    "e", "err", "error", "text", "reason", "msg", "message", "path", "p", "file", "source", "why", "cause", "stderr",
];

/// The `debrief-*` crates the worker depends on, by name.
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
            if p.file_name().is_some_and(|n| n == "tests") {
                continue;
            }
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// `source` with the inside of comments, string and character literals blanked (same length, newlines kept), so that what is
/// searched for (an attribute, a macro, a bracket) is code.
fn mask(source: &str) -> String {
    let b = source.as_bytes();
    let mut out = b.to_vec();
    let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
        for c in &mut out[from..to] {
            if *c != b'\n' {
                *c = b' ';
            }
        }
    };
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                let end = source[i..].find('\n').map_or(b.len(), |n| i + n);
                blank(&mut out, i, end);
                i = end;
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let (mut depth, mut j) = (1, i + 2);
                while j < b.len() && depth > 0 {
                    if b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
                        depth += 1;
                        j += 2;
                    } else if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
                        depth -= 1;
                        j += 2;
                    } else {
                        j += 1;
                    }
                }
                blank(&mut out, i, j);
                i = j;
            }
            b'r' if matches!(b.get(i + 1), Some(b'"') | Some(b'#')) && (i == 0 || !is_ident(b[i - 1])) => {
                let hashes = b[i + 1..].iter().take_while(|c| **c == b'#').count();
                if b.get(i + 1 + hashes) == Some(&b'"') {
                    let close = format!("\"{}", "#".repeat(hashes));
                    let from = i + 2 + hashes;
                    let end = source[from..].find(&close).map_or(b.len(), |n| from + n + close.len());
                    blank(&mut out, from, end.saturating_sub(close.len()).max(from));
                    i = end;
                } else {
                    i += 1;
                }
            }
            b'"' => {
                let (mut j, mut esc) = (i + 1, false);
                while j < b.len() {
                    match (esc, b[j]) {
                        (true, _) => esc = false,
                        (false, b'\\') => esc = true,
                        (false, b'"') => break,
                        _ => {}
                    }
                    j += 1;
                }
                blank(&mut out, i + 1, j.min(b.len()));
                i = j + 1;
            }
            b'\'' => {
                // A character literal ('"', '\n') or a lifetime ('a).
                if b.get(i + 2) == Some(&b'\'') {
                    blank(&mut out, i + 1, i + 2);
                    i += 3;
                } else if b.get(i + 1) == Some(&b'\\') {
                    let end = source[i + 2..].find('\'').map_or(b.len(), |n| i + 2 + n);
                    blank(&mut out, i + 1, end);
                    i = end + 1;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| source.to_string())
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Byte ranges of the `#[cfg(test)]` items of `masked` (an attribute in code, up to the end of the item's braces or its `;`).
fn test_items(masked: &str) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(i) = masked[from..].find("#[cfg(test)]") {
        let start = from + i;
        let after = &masked[start..];
        let (brace, semi) = (after.find('{'), after.find(';'));
        let end = match (brace, semi) {
            (Some(b), Some(s)) if s < b => start + s + 1,
            (Some(b), _) => {
                let mut depth = 0usize;
                let mut end = masked.len();
                for (k, c) in after[b..].char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = start + b + k + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                end
            }
            _ => masked.len(),
        };
        found.push((start, end));
        from = end.max(start + 1);
    }
    found
}

/// Modules declared behind `#[cfg(test)]` as files (`#[cfg(test)] mod name;`): their names.
fn test_modules(source: &str) -> Vec<String> {
    let masked = mask(source);
    test_items(&masked)
        .into_iter()
        .filter_map(|(s, e)| {
            let item = masked[s..e].trim_end_matches(';');
            let rest = item.trim_start_matches("#[cfg(test)]").trim();
            let rest = rest.strip_prefix("pub ").unwrap_or(rest);
            rest.strip_prefix("mod ")
                .filter(|_| !item.contains('{'))
                .map(|n| n.trim().to_string())
        })
        .collect()
}

/// The text between the parentheses of the call that starts at `from` (after the opening one), in `masked` terms.
fn call_end(masked: &str, from: usize) -> usize {
    let mut depth = 1usize;
    for (k, c) in masked[from..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return from + k;
                }
            }
            _ => {}
        }
    }
    masked.len()
}

/// Splits `text` at its top-level commas (outside strings, brackets, braces and parentheses).
fn top_args(text: &str) -> Vec<&str> {
    let masked = mask(text);
    let (mut depth, mut last, mut out) = (0i32, 0usize, Vec::new());
    for (k, c) in masked.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(text[last..k].trim());
                last = k + 1;
            }
            _ => {}
        }
    }
    let tail = text[last..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

/// The index of the first top-level `;` of `text`.
fn top_semicolon(text: &str) -> Option<usize> {
    let masked = mask(text);
    let mut depth = 0i32;
    for (k, c) in masked.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ';' if depth == 0 => return Some(k),
            _ => {}
        }
    }
    None
}

/// Words of an expression (identifiers), for matching names.
fn words(expr: &str) -> impl Iterator<Item = &str> {
    expr.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
}

fn leaky_expr(expr: &str) -> bool {
    words(expr).any(|w| NAMES.contains(&w)) || expr.contains(".display()") || expr.contains("to_string_lossy") || expr.contains(".to_str()")
}

/// The placeholders of a format string: (name or "", spec).
fn placeholders(fmt: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '{' {
            continue;
        }
        if chars.peek() == Some(&'{') {
            chars.next();
            continue;
        }
        let inner: String = chars.by_ref().take_while(|c| *c != '}').collect();
        let (name, spec) = inner.split_once(':').unwrap_or((inner.as_str(), ""));
        out.push((name.trim().to_string(), spec.to_string()));
    }
    out
}

/// Whether a format string and its positional arguments quote an error, a path or a name.
fn format_leaks(fmt_args: &[&str]) -> bool {
    let Some(first) = fmt_args.iter().position(|a| a.starts_with('"')) else {
        return false;
    };
    let fmt = fmt_args[first];
    let positional: Vec<&str> = fmt_args[first + 1..].iter().filter(|a| !is_named_arg(a)).copied().collect();
    let named_exprs: Vec<&str> = fmt_args[first + 1..].iter().filter(|a| is_named_arg(a)).copied().collect();
    let mut next = 0usize;
    for (name, spec) in placeholders(fmt) {
        let debug = spec.contains('?');
        if name.is_empty() {
            let arg = positional.get(next).copied().unwrap_or("");
            next += 1;
            if debug || (spec.contains('#') && arg.is_empty()) || leaky_expr(arg) {
                return true;
            }
        } else if name.chars().all(|c| c.is_ascii_digit()) {
            let arg = positional.get(name.parse::<usize>().unwrap_or(usize::MAX)).copied().unwrap_or("");
            if debug || leaky_expr(arg) {
                return true;
            }
        } else if debug
            || NAMES.contains(&name.as_str())
            || named_exprs.iter().any(|a| a.starts_with(&format!("{name} =")) && leaky_expr(a))
        {
            return true;
        }
    }
    false
}

/// `name = expr` among a format's arguments.
fn is_named_arg(arg: &str) -> bool {
    arg.split_once('=').is_some_and(|(n, rest)| {
        !rest.starts_with('=') && !n.trim().is_empty() && n.trim().chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// A key/value named like a path (`path = ...`, `Path = ...`), before the `;` that ends the properties.
fn has_path_property(inner: &str) -> bool {
    let Some(semi) = top_semicolon(inner) else { return false };
    top_args(&inner[..semi])
        .iter()
        .filter_map(|kv| kv.split_once('='))
        .any(|(k, _)| k.trim().to_ascii_lowercase().contains("path"))
}

/// Whether the inside of one log call (`inner`) leaks.
fn call_leaks(inner: &str) -> bool {
    if inner.contains(".display()") || inner.contains("to_string_lossy") || inner.contains(".to_str()") || has_path_property(inner) {
        return true;
    }
    let fmt_part = top_semicolon(inner).map_or(inner, |s| &inner[s + 1..]);
    format_leaks(&top_args(fmt_part))
}

/// Whether the inside of a `log_failure!` (`level, message, error; props`) leaks through its message argument.
fn failure_call_leaks(inner: &str) -> bool {
    let head = top_semicolon(inner).map_or(inner, |s| &inner[..s]);
    let args = top_args(head);
    let Some(msg) = args.get(1) else { return false };
    let inside_format = msg.strip_prefix("format!(").and_then(|m| m.strip_suffix(')'));
    let parts = inside_format.map_or_else(|| vec![*msg], top_args);
    if msg.contains(".display()") || msg.contains("to_string_lossy") || msg.contains(".to_str()") {
        return true;
    }
    format_leaks(&parts) || (!msg.starts_with('"') && inside_format.is_none() && leaky_expr(msg))
}

/// Macro names whose call is a log call, and the way they are found.
const LOG_MACROS: [&str; 7] = ["warn!(", "error!(", "info!(", "log!(", "log_failure!(", "debug!(", "trace!("];

/// The leaks in `source`: (line, call) for each call with a leak and no reasoned allowance.
fn leaks(source: &str) -> Vec<(usize, String)> {
    let masked = mask(source);
    let skip = test_items(&masked);
    let lines: Vec<&str> = source.lines().collect();
    let mut found = Vec::new();
    for mac in LOG_MACROS {
        if mac == "debug!(" || mac == "trace!(" {
            continue;
        }
        for (at, _) in masked.match_indices(mac) {
            // A macro of its own name, not the tail of another identifier.
            if at > 0 && is_ident(masked.as_bytes()[at - 1]) {
                continue;
            }
            let path = &masked[..at];
            let qualified = path.ends_with("log::") || path.ends_with("tracing::") || path.ends_with("::");
            let bare_ok = !qualified && !path.ends_with('.');
            if mac == "log!(" && !(path.ends_with("log::") || path.ends_with("tracing::")) {
                continue;
            }
            if !(qualified || bare_ok) {
                continue;
            }
            if skip.iter().any(|(s, e)| at >= *s && at < *e) {
                continue;
            }
            let from = at + mac.len();
            let end = call_end(&masked, from);
            let inner = &source[from..end];
            let leaky = if mac == "log_failure!(" {
                failure_call_leaks(inner)
            } else {
                call_leaks(inner)
            };
            let line = source[..at].matches('\n').count();
            let window = lines[line.saturating_sub(3)..=line.min(lines.len() - 1)].join("\n") + inner;
            if leaky && !allowed(&window) {
                found.push((line + 1, inner.split_whitespace().collect::<Vec<_>>().join(" ")));
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// A `// log-allow: <reason>` with a real reason.
fn allowed(window: &str) -> bool {
    window
        .lines()
        .any(|l| l.split_once("// log-allow:").is_some_and(|(_, reason)| reason.trim().len() >= 8))
}

#[test]
fn no_error_text_path_or_name_in_a_log_call_of_the_crates_the_worker_links() {
    let debrief_crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let lock = std::fs::read_to_string(debrief_crates.join("../Cargo.lock")).expect("Cargo.lock");
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
    let mut scanned = 0usize;
    for name in &crates {
        let root = &debrief_crates;
        let src = root.join(name).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        // Modules declared behind #[cfg(test)] in another file are not shipped code.
        let mut test_files: BTreeSet<PathBuf> = BTreeSet::new();
        for file in &files {
            let dir = if matches!(file.file_name().and_then(|n| n.to_str()), Some("lib.rs" | "main.rs" | "mod.rs")) {
                file.parent().unwrap().to_path_buf()
            } else {
                file.with_extension("")
            };
            for m in test_modules(&std::fs::read_to_string(file).unwrap()) {
                test_files.insert(dir.join(format!("{m}.rs")));
                test_files.insert(dir.join(&m).join("mod.rs"));
            }
        }
        for file in files {
            let base = file.file_name().unwrap().to_string_lossy().into_owned();
            if base == "tests.rs" || base.ends_with("_tests.rs") || test_files.contains(&file) {
                continue;
            }
            scanned += 1;
            let source = std::fs::read_to_string(&file).unwrap();
            for (line, call) in leaks(&source) {
                report.push(format!("{}:{line}: {call}", file.strip_prefix(root).unwrap().display()));
            }
        }
    }
    assert!(scanned > 100, "the scan found its files: {scanned}");
    assert!(
        report.is_empty(),
        "log calls that can quote an error, a path or a name; use debrief_core::log_failure! (with a message that has none) or add `// log-allow: <reason>`:\n{}",
        report.join("\n")
    );
}

fn count(source: &str) -> usize {
    leaks(source).len()
}

#[test]
fn the_scan_sees_each_way_a_leak_gets_in() {
    // Named placeholders.
    for name in ["e", "err", "reason", "msg", "path", "p", "file", "source", "text"] {
        assert_eq!(count(&format!("fn f() {{ log::warn!(\"x {{{name}}}\"); }}")), 1, "{{{name}}}");
        assert_eq!(count(&format!("fn f() {{ log::warn!(\"x {{{name}:#}}\"); }}")), 1, "{{{name}:#}}");
    }
    assert_eq!(count("fn f() { log::warn!(\"x {:#}\"); }"), 1);
    // Positional arguments, debug, display, lossy, str.
    assert_eq!(count("fn f() { log::warn!(\"failed: {}\", e); }"), 1);
    assert_eq!(count("fn f() { log::error!(\"failed: {:?}\", err); }"), 1);
    assert_eq!(
        count("fn f() { log::error!(\"failed: {:?}\", thing.len()); }"),
        1,
        "{{:?}} of anything"
    );
    assert_eq!(count("fn f() { log::warn!(\"a {} b {}\", id, why.to_string()); }"), 1);
    assert_eq!(count("fn f() { log::warn!(\"in {}\", p.display()); }"), 1);
    assert_eq!(count("fn f() { log::warn!(\"in {}\", name.to_string_lossy()); }"), 1);
    assert_eq!(count("fn f() { log::warn!(\"in {}\", name.to_str().unwrap()); }"), 1);
    assert_eq!(count("fn f() { log::warn!(\"in {x}\", x = path.display()); }"), 1);
    assert_eq!(count("fn f() { log::warn!(\"in {0} {0:?}\", id); }"), 1);
    assert_eq!(count("fn f() { log::info!(path = p.as_str(); \"opened\"); }"), 1);
    assert_eq!(count("fn f() { log::warn!(\"multi\n line {err}\"); }"), 1);
    // log_failure!: the message is scanned, the error argument is the point.
    assert_eq!(
        count("fn f() { debrief_core::log_failure!(warn, \"could not read {path}\", e); }"),
        1
    );
    assert_eq!(
        count("fn f() { crate::log_failure!(warn, format!(\"could not read {}\", p.display()), e); }"),
        1
    );
    assert_eq!(count("fn f() { crate::log_failure!(error, format!(\"x {}\", err), e; A = 1); }"), 1);
    assert_eq!(
        count("fn f() { crate::log_failure!(warn, message_text, e); }"),
        0,
        "an identifier that is not a name"
    );
    assert_eq!(count("fn f() { crate::log_failure!(warn, reason, e); }"), 1);
    assert_eq!(count("fn f() { crate::log_failure!(warn, format!(\"source {id} gone\"), e); }"), 0);
    assert_eq!(count("fn f() { crate::log_failure!(warn, \"fine\", e; Source = id); }"), 0);
    // Other spellings of the macros.
    assert_eq!(count("use log::warn;\nfn f() { warn!(\"x {e}\"); }"), 1, "bare");
    assert_eq!(count("fn f() { ::log::warn!(\"x {e}\"); }"), 1);
    assert_eq!(count("fn f() { tracing::error!(\"x {e}\"); }"), 1);
    assert_eq!(count("fn f() { log::log!(Level::Warn, \"x {e}\"); }"), 1);
    assert_eq!(count("fn f() { log::log!(log::Level::Info, \"x {}\", e); }"), 1);
    assert_eq!(count("fn f() { x.warn!(\"{e}\"); }"), 0, "not a path call");
    assert_eq!(count("fn f() { my_warn!(\"{e}\"); }"), 0, "another macro");
    // Safe: ids, counts, debug level, a reasoned allowance.
    assert_eq!(
        count("fn f() { log::warn!(\"no error here {id} {}\", n); log::debug!(\"{e}\"); }"),
        0
    );
    assert_eq!(
        count("fn f() { log::warn!(target: LOG, Plugin = plugin; \"Plugin {plugin} crashed\"); }"),
        0
    );
    let allowed = "fn f() {\n    // log-allow: the id is a plugin id, never content\n    log::warn!(\"plugin {e}\");\n}\n";
    assert_eq!(count(allowed), 0);
    let no_reason = "fn f() {\n    // log-allow:\n    log::warn!(\"plugin {e}\");\n}\n";
    assert_eq!(count(no_reason), 1, "an allowance needs a reason");
}

#[test]
fn test_code_is_skipped_by_attribute_not_by_name_and_comments_and_strings_do_not_count() {
    assert_eq!(
        count("#[cfg(test)]\nmod tests {\n    fn f() { log::warn!(\"{e}\"); }\n}\nfn g() {}\n"),
        0
    );
    assert_eq!(
        count("#[cfg(test)]\nmod tests { fn f() {} }\nfn g() { log::warn!(\"{e}\"); }\n"),
        1,
        "code after a test module"
    );
    // A `#[cfg(test)]` in a comment or a string does not hide what follows.
    assert_eq!(count("// #[cfg(test)]\nfn g() { log::warn!(\"{e}\"); }\n"), 1);
    assert_eq!(count("/* #[cfg(test)] */\nfn g() { log::warn!(\"{e}\"); }\n"), 1);
    assert_eq!(count("const A: &str = \"#[cfg(test)]\";\nfn g() { log::warn!(\"{e}\"); }\n"), 1);
    assert_eq!(count("const A: &str = r#\"#[cfg(test)]\"#;\nfn g() { log::warn!(\"{e}\"); }\n"), 1);
    // A macro name inside a comment or a string is not a call.
    assert_eq!(count("// log::warn!(\"{e}\")\nconst S: &str = \"log::warn!(\\\"{e}\\\")\";\n"), 0);
    // A character literal that looks like a quote does not start a string.
    assert_eq!(count("fn f() { let q = '\"'; log::warn!(\"{e}\"); }"), 1);
    // Modules behind #[cfg(test)] are found by name; shipped `testing` modules are not.
    assert_eq!(
        test_modules("#[cfg(test)]\nmod maintain_tests;\n#[cfg(test)]\nmod inline { fn f() {} }\npub mod testing;\n"),
        ["maintain_tests"]
    );
    assert_eq!(test_modules("// #[cfg(test)]\nmod not_a_test_module;\n"), Vec::<String>::new());
}

#[test]
fn the_crates_come_from_the_lock_file() {
    let lock = "[[package]]\nname = \"debrief-store-worker\"\nversion = \"0.1.0\"\ndependencies = [\n \"debrief-core\",\n \"paddock-log\",\n \"serde 1.0\",\n]\n\n[[package]]\nname = \"debrief-core\"\ndependencies = [\n \"debrief-leaf\",\n]\n\n[[package]]\nname = \"debrief-leaf\"\n\n[[package]]\nname = \"paddock-log\"\n\n[[package]]\nname = \"debrief-unrelated\"\n";
    let got: Vec<String> = worker_crates(lock).into_iter().collect();
    assert_eq!(
        got,
        ["debrief-core", "debrief-leaf", "debrief-store-worker"],
        "paddock-* crates are not scanned"
    );
}
