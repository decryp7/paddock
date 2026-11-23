//! The model's own guard against path rules in code (ADR 0117 §14): the model's non-test code holds no path literal, so every
//! path fact is data in `docs/repo-graph.toml`, read through paddock-dev's `repo_inputs` (the one accessor). A test, not a
//! conformance check yet (WP-5 builds the check over paddock-dev's planning code); its positive control puts a literal back.

/// A string literal that names a path or a glob: after its `{..}` placeholders are dropped it holds a `/` next to a name
/// character or `*`, a `**`, a `*.` extension glob, a dotfile or extension name (`.gitattributes`, `.md`), or a bare file name
/// (`README.md`, `Cargo.toml`: a name, a dot and a letter-bearing extension).
pub fn is_path_literal(lit: &str) -> bool {
    let mut s = String::new();
    let mut depth = 0;
    for c in lit.chars() {
        match c {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            _ if depth == 0 => s.push(c),
            _ => {}
        }
    }
    let b = s.as_bytes();
    let slash_word = b.windows(2).any(|w| {
        (w[0] == b'/' && (w[1].is_ascii_alphanumeric() || w[1] == b'*' || w[1] == b'.'))
            || (w[1] == b'/' && (w[0].is_ascii_alphanumeric() || w[0] == b'*'))
    });
    let name_char = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    let dotname =
        s.len() > 1 && s.starts_with('.') && s[1..].chars().all(|c| name_char(c) || c == '.');
    let bare_file = match s.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && stem.chars().all(|c| name_char(c) || c == '.')
                && !ext.is_empty()
                && ext.len() <= 6
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
                && ext.chars().any(|c| c.is_ascii_alphabetic())
        }
        None => false,
    };
    slash_word || s.contains("**") || s.contains("*.") || dotname || bare_file
}

/// The string literals of a source's non-test code with their line numbers. It reads Rust's lexical shapes (line and block
/// comments, strings with escapes, raw strings, character literals) and skips every item marked `#[cfg(test)]`: to the end of its
/// braces, or to its `;` (`#[cfg(test)] mod fixture;`), never the rest of the file (review P1-6).
pub fn path_literals(src: &str) -> Vec<(usize, String)> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line) = (0usize, 1usize);
    let (mut depth, mut pending_test, mut skip_at): (i64, bool, Option<i64>) = (0, false, None);
    let starts = |i: usize, pat: &str| {
        pat.chars()
            .enumerate()
            .all(|(k, c)| chars.get(i + k) == Some(&c))
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
        } else if starts(i, "//") {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if starts(i, "/*") {
            let mut nest = 0;
            while i < chars.len() {
                if starts(i, "/*") {
                    nest += 1;
                    i += 2;
                } else if starts(i, "*/") {
                    nest -= 1;
                    i += 2;
                    if nest == 0 {
                        break;
                    }
                } else {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
        } else if c == '"'
            || (c == 'r'
                && (starts(i, "r\"") || starts(i, "r#"))
                && raw_hashes(&chars, i).is_some())
        {
            // A string: raw (`r#"..."#`) or ordinary.
            let (raw, body) = match raw_hashes(&chars, i).filter(|_| c == 'r') {
                Some(n) => (Some(n), i + 2 + n),
                None => (None, i + 1),
            };
            let (start_line, mut lit, mut j) = (line, String::new(), body);
            loop {
                if j >= chars.len() {
                    break;
                }
                let d = chars[j];
                match raw {
                    Some(n) if d == '"' && (0..n).all(|k| chars.get(j + 1 + k) == Some(&'#')) => {
                        j += 1 + n;
                        break;
                    }
                    None if d == '\\' => {
                        lit.push(d);
                        if let Some(e) = chars.get(j + 1) {
                            lit.push(*e);
                            if *e == '\n' {
                                line += 1;
                            }
                        }
                        j += 2;
                        continue;
                    }
                    None if d == '"' => {
                        j += 1;
                        break;
                    }
                    _ => {}
                }
                if d == '\n' {
                    line += 1;
                }
                lit.push(d);
                j += 1;
            }
            if skip_at.is_none() && is_path_literal(&lit) {
                out.push((start_line, lit));
            }
            i = j;
        } else if c == '\'' {
            // A character literal (`'{'`, `'\\n'`) is skipped whole; a lifetime has no closing quote after one character.
            if chars.get(i + 1) == Some(&'\\') {
                let close = chars[i + 2..]
                    .iter()
                    .position(|x| *x == '\'')
                    .map_or(1, |p| p + 1);
                i += 2 + close;
            } else if chars.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
            }
        } else {
            if c == '#' && starts(i, "#[cfg(test)]") {
                if skip_at.is_none() {
                    pending_test = true;
                }
                i += "#[cfg(test)]".len();
                continue;
            }
            match c {
                '{' => {
                    if pending_test && skip_at.is_none() {
                        skip_at = Some(depth);
                        pending_test = false;
                    }
                    depth += 1;
                }
                '}' => {
                    depth -= 1;
                    if skip_at == Some(depth) {
                        skip_at = None;
                    }
                }
                ';' if pending_test && skip_at.is_none() => pending_test = false,
                _ => {}
            }
            i += 1;
        }
    }
    out
}

/// The number of `#` of a raw string that starts at `i` (`r"` is 0, `r#"` is 1), or `None` when it is not one.
fn raw_hashes(chars: &[char], i: usize) -> Option<usize> {
    if i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_') {
        return None;
    }
    let n = chars[i + 1..].iter().take_while(|c| **c == '#').count();
    (chars.get(i + 1 + n) == Some(&'"')).then_some(n)
}

/// The findings in `files` (name, source): every path literal in non-test code that `allowed` does not cover, as
/// `name:line: "literal"`. An allowed token counts only alone (with punctuation around it), never a longer path that merely
/// contains it.
pub fn findings(files: &[(String, String)], allowed: &[(&str, &str)]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, src) in files {
        for (line, lit) in path_literals(src) {
            let mut bare = String::new();
            let mut depth = 0;
            for c in lit.chars() {
                match c {
                    '{' => depth += 1,
                    '}' if depth > 0 => depth -= 1,
                    _ if depth == 0 => bare.push(c),
                    _ => {}
                }
            }
            let ok = allowed.iter().any(|(f, a)| {
                f == name
                    && bare.contains(a)
                    && !bare
                        .replace(a, "")
                        .chars()
                        .any(|c| c.is_ascii_alphanumeric())
            });
            if !ok {
                out.push(format!("{name}:{line}: {lit:?}"));
            }
        }
    }
    out
}

/// The modules a source declares test-only (`#[cfg(test)] mod name;`): their files are test code.
pub fn test_modules(src: &str) -> Vec<String> {
    let lines: Vec<&str> = src.lines().map(str::trim).collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if *l != "#[cfg(test)]" {
            continue;
        }
        let next = lines[i + 1..].iter().find(|n| !n.starts_with("#["));
        if let Some(m) = next
            .map(|n| {
                n.trim_start_matches("pub ")
                    .trim_start_matches("pub(crate) ")
            })
            .and_then(|n| n.strip_prefix("mod "))
            .and_then(|n| n.strip_suffix(';'))
        {
            out.push(m.trim().to_string());
        }
    }
    out
}

/// A source folder's non-test Rust files, recursively, by path relative to `dir`: a file a sibling declares
/// `#[cfg(test)] mod name;` (as `name.rs` or `name/mod.rs`, and the folder `name/`) is test code and left out.
pub fn sources_under(dir: &std::path::Path) -> std::io::Result<Vec<(String, String)>> {
    use std::collections::BTreeMap;
    fn walk(
        dir: &std::path::Path,
        rel: &str,
        test_only: bool,
        inherited: &[String],
        out: &mut Vec<(String, String)>,
    ) -> std::io::Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        // The test-only module names this folder's files are declared as: by a Rust file of this folder (a sibling, or its own
        // `mod.rs`) and, for `x/`, by `x.rs` next to it (passed down as `inherited`).
        let mut tests: Vec<String> = inherited.to_vec();
        let mut declared: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for e in &entries {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "rs") {
                let mods = test_modules(&std::fs::read_to_string(&p)?);
                tests.extend(mods.iter().cloned());
                let stem = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                declared.insert(stem, mods);
            }
        }
        for e in entries {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            let stem = name.trim_end_matches(".rs").to_string();
            let path = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            let is_test = test_only || tests.contains(&stem);
            if p.is_dir() {
                let down = declared.get(&stem).cloned().unwrap_or_default();
                walk(&p, &path, is_test, &down, out)?;
            } else if p.extension().is_some_and(|x| x == "rs") && !is_test {
                out.push((path, std::fs::read_to_string(&p)?));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, "", false, &[], &mut out)?;
    Ok(out)
}

/// What the guard allows, each with its reason: the glob grammar's own token, the file formats the adapters read (a format's
/// file name, not a fact about this repository: Cargo's manifest and its integration-test folder, npm's manifest and the
/// configuration files whose path references it reads), and the guard's own detection patterns.
pub const ALLOWED: &[(&str, &str)] = &[
    ("glob.rs", "**"),
    ("extract.rs", ".rs"),
    ("extract.rs", ".json"),
    ("extract.rs", "/tests/"),
    ("extract.rs", "Cargo.toml"),
    ("extract.rs", "package.json"),
    ("extract.rs", "tauri.conf.json"),
    ("extract.rs", "angular.json"),
    ("extract.rs", "eslint.config.js"),
    ("extract.rs", "eslint.config.mjs"),
    ("extract.rs", "eslint.config.cjs"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The model's non-test sources. guard.rs itself is left out: it is the checker, and its [`ALLOWED`] list is the one place
    /// that holds path-shaped literals on purpose (reviewed by hand, each with its reason).
    fn sources() -> Vec<(String, String)> {
        let mut files =
            sources_under(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/model"))
                .unwrap();
        files.retain(|(n, _)| n != "guard.rs");
        files
    }

    #[test]
    fn the_models_code_holds_no_path_literal() {
        let files = sources();
        let names: Vec<&String> = files.iter().map(|f| &f.0).collect();
        // mod.rs and guard.rs are read (mod.rs declares a test module on line 11); a folder's `tests.rs` is test code.
        for want in ["inputs.rs", "mod.rs", "extract.rs", "glob.rs"] {
            assert!(
                names.contains(&&want.to_string()),
                "{want} is scanned: {names:?}"
            );
        }
        assert!(
            !names
                .iter()
                .any(|n| n.ends_with("tests.rs") || n.as_str() == "fixture.rs"),
            "{names:?}"
        );
        let found = findings(&files, ALLOWED);
        assert!(
            found.is_empty(),
            "path facts belong in docs/repo-graph.toml, not in code: {found:#?}"
        );
    }

    #[test]
    fn the_guard_catches_a_literal_put_back_into_inputs_rs() {
        // Positive control (ER-33): a default input written back into inputs.rs is found.
        let mut files = sources();
        let inputs = files.iter_mut().find(|(n, _)| n == "inputs.rs").unwrap();
        inputs.1 = format!("const BACK: &[&str] = &[\"docs/mocks/**\"];\n{}", inputs.1);
        assert!(
            findings(&files, ALLOWED)
                .iter()
                .any(|f| f.starts_with("inputs.rs:1:")),
            "{:?}",
            findings(&files, ALLOWED)
        );
        for lit in [
            "docs/mocks/**",
            "/package.json",
            ".gitattributes",
            "*.md",
            "{d}/tests/x",
            "README.md",
            "Cargo.toml",
        ] {
            assert!(is_path_literal(lit), "{lit}");
        }
        for lit in [
            "crate:{dir}",
            "input {} ({f})",
            "code",
            "{}/{}",
            "1.2.3",
            "e.g. a",
        ] {
            assert!(!is_path_literal(lit), "{lit}");
        }
    }

    #[test]
    fn a_literal_after_a_cfg_test_module_declaration_is_found() {
        // Failing first (P1-6): the scan stopped at the first `#[cfg(test)]` line, and mod.rs line 11 is `#[cfg(test)] mod fixture;`,
        // so everything after it (reach, selection) was never scanned. The control puts a literal after that line in the real file.
        let mut files = sources();
        let m = files.iter_mut().find(|(n, _)| n == "mod.rs").unwrap();
        assert!(
            m.1.lines().any(|l| l.trim() == "#[cfg(test)]"),
            "mod.rs still declares a test module"
        );
        m.1.push_str("\nfn late() -> &'static str { \"docs/late/x\" }\n");
        let last = m.1.lines().count();
        assert!(
            findings(&files, ALLOWED)
                .iter()
                .any(|f| f.starts_with(&format!("mod.rs:{last}:")))
        );
        // Control: a literal inside a `#[cfg(test)]` block is test code and not a finding.
        let ok = "#[cfg(test)]\nmod t {\n    const X: &str = \"docs/in/test\";\n}\nfn f() { let _c = '{'; }\nconst Y: &str = \"docs/after\";\n";
        assert_eq!(path_literals(ok), [(6, "docs/after".to_string())]);
        assert_eq!(
            path_literals("#[cfg(test)]\nmod fixture;\nconst Z: &str = \"a/b\";\n").len(),
            1
        );
        assert_eq!(
            path_literals("let r = r#\"docs/raw\"#; // \"no/comment\"\n/* \"no/block\" */").len(),
            1
        );
    }

    #[test]
    fn a_subfolder_and_a_bare_file_name_are_scanned() {
        // Failing first: the folder walk did not recurse, and a bare file name (`README.md`) was allowed.
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("deep/er")).unwrap();
        std::fs::write(d.path().join("top.rs"), "#[cfg(test)]\nmod tests;\n").unwrap();
        std::fs::write(d.path().join("tests.rs"), "const T: &str = \"a/b\";\n").unwrap();
        std::fs::write(
            d.path().join("deep/er/x.rs"),
            "const X: &str = \"README.md\";\n",
        )
        .unwrap();
        let files = sources_under(d.path()).unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(
            names,
            ["deep/er/x.rs", "top.rs"],
            "tests.rs is declared test-only by its parent and left out"
        );
        assert_eq!(findings(&files, &[]), ["deep/er/x.rs:1: \"README.md\""]);
    }
}
