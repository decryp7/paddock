//! The model's own guard against path rules in code (ADR 0117 §14): the model's non-test code holds no path literal, so every
//! path fact is data in `docs/repo-graph.toml`, read through paddock-dev's `repo_inputs` (the one accessor). A test, not a
//! conformance check yet (WP-5 builds the check over paddock-dev's planning code); its positive control puts a literal back.

/// A string literal that names a path or a glob: after its `{..}` placeholders are dropped it holds a `/` next to a name
/// character or `*`, a `**`, a `*.` extension glob, or it is a dotfile or extension name (`.gitattributes`, `.md`).
fn is_path_literal(lit: &str) -> bool {
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
    let dotname = s.len() > 1
        && s.starts_with('.')
        && s[1..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    slash_word || s.contains("**") || s.contains("*.") || dotname
}

/// The string literals of a source's non-test code (up to its `#[cfg(test)]`), skipping comments, with their line numbers.
pub fn path_literals(src: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let l = line.trim_start();
        if l.starts_with("#[cfg(test)]") {
            break;
        }
        if l.starts_with("//") {
            continue;
        }
        let mut cur: Option<String> = None;
        let mut esc = false;
        let code = line.split("//").next().unwrap_or(line);
        for c in code.chars() {
            match (&mut cur, c) {
                (Some(s), _) if esc => {
                    s.push(c);
                    esc = false;
                }
                (Some(_), '\\') => esc = true,
                (Some(s), '"') => {
                    if is_path_literal(s) {
                        out.push((i + 1, s.clone()));
                    }
                    cur = None;
                }
                (Some(s), c) => s.push(c),
                (None, '"') => cur = Some(String::new()),
                (None, _) => {}
            }
        }
    }
    out
}

/// What the guard allows, each with its reason: the glob grammar's own token and the file formats the adapters read (a format's
/// file name, not a fact about this repository: Cargo's manifest and its integration-test folder, npm's manifest and the
/// configuration files whose path references it reads).
pub const ALLOWED: &[(&str, &str)] = &[
    ("glob.rs", "**"),
    ("extract.rs", ".rs"),
    ("extract.rs", ".json"),
    ("extract.rs", "/tests/"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The model's non-test sources, by file name (the tests, the guard and the test fixture are test code).
    fn sources() -> Vec<(String, String)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/model");
        let mut out = Vec::new();
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if p.extension().is_some_and(|x| x == "rs")
                && !matches!(name.as_str(), "tests.rs" | "guard.rs" | "fixture.rs")
            {
                out.push((name, std::fs::read_to_string(&p).unwrap()));
            }
        }
        out.sort();
        out
    }

    fn findings(files: &[(String, String)]) -> Vec<String> {
        let mut out = Vec::new();
        for (name, src) in files {
            for (line, lit) in path_literals(src) {
                // An allowed token alone (with punctuation around it), never a longer path that merely contains it.
                let bare: String = {
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
                    s
                };
                let allowed = ALLOWED.iter().any(|(f, a)| {
                    f == name
                        && bare.contains(a)
                        && !bare
                            .replace(a, "")
                            .chars()
                            .any(|c| c.is_ascii_alphanumeric())
                });
                if !allowed {
                    out.push(format!("{name}:{line}: {lit:?}"));
                }
            }
        }
        out
    }

    #[test]
    fn the_models_code_holds_no_path_literal() {
        let files = sources();
        assert!(
            files.iter().any(|(n, _)| n == "inputs.rs"),
            "the guard reads the model's sources"
        );
        let found = findings(&files);
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
            findings(&files)
                .iter()
                .any(|f| f.starts_with("inputs.rs:1:")),
            "{:?}",
            findings(&files)
        );
        for lit in [
            "docs/mocks/**",
            "/package.json",
            ".gitattributes",
            "*.md",
            "{d}/tests/x",
            "README.md",
        ] {
            assert!(is_path_literal(lit) || lit == "README.md", "{lit}");
        }
        for lit in ["crate:{dir}", "input {} ({f})", "code", "{}/{}"] {
            assert!(!is_path_literal(lit), "{lit}");
        }
    }
}
