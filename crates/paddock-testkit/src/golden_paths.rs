//! The path spelling of golden text (ER-46, ADR 0021 amendment 2026-11-11 D-P1).
//!
//! A path in a golden has the capturing machine's spelling: Windows prints `C:\a\b`, a `Path::join` onto a `/` base prints
//! `C:/a\b`, a verbatim path starts `\\?\`, and the temp and home folders are the machine's own. [`normalise_paths`] reads every
//! Windows-style path in the text through [`paddock_env::winpath`] (one `/` separator, no `\\?\`) and replaces the test's temp and
//! home roots with `<tmp>` and `<home>`, so a golden never holds a form only one OS prints. [`crate::golden::check_golden`]
//! applies it to both sides of a comparison and to the text it writes.

use paddock_env::Os;
use paddock_env::winpath;
use std::path::{Path, PathBuf};

/// A root and the token that stands for it.
pub type Root<'a> = (&'a Path, &'a str);

/// What ends a path in running text.
fn ends_path(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | '`' | '<' | '>' | '|' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}'
        )
}

/// Whether a Windows-style path (a drive path or a `\\?\` path) starts at byte `i` of `text`, after a character that cannot be part
/// of a name.
fn windows_start(text: &str, i: usize) -> bool {
    let rest = &text[i..];
    let boundary = text[..i]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric() && c != '_');
    if !boundary {
        return false;
    }
    let b = rest.as_bytes();
    rest.starts_with(r"\\?\")
        || (b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && (b[2] == b'/' || b[2] == b'\\'))
}

/// `run` (a Windows-style path) in winpath's spelling, with the first matching root replaced by its token.
fn spell(run: &str, roots: &[Root<'_>], root_os: Os) -> String {
    let lexical = winpath::lexical_text(Path::new(run), Os::Windows);
    for (root, token) in roots {
        if let Some(rest) = winpath::strip_prefix(Path::new(&lexical), root, root_os) {
            return if rest.is_empty() {
                (*token).to_string()
            } else {
                format!("{token}/{rest}")
            };
        }
    }
    lexical
}

/// `text` with every Windows-style path in winpath's spelling, and `roots` (longest first) replaced by their tokens. A root is
/// compared by `root_os`'s rules (case-insensitive on Windows); a Unix root also replaces its plain text wherever it occurs.
pub fn normalise_paths_on(text: &str, roots: &[Root<'_>], root_os: Os) -> String {
    let mut roots: Vec<Root<'_>> = roots
        .iter()
        .copied()
        .filter(|(r, _)| r.as_os_str().len() > 1)
        .collect();
    roots.sort_by_key(|(r, _)| std::cmp::Reverse(r.as_os_str().len()));
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if windows_start(text, i) {
            let mut end = text[i..].find(ends_path).map_or(text.len(), |n| i + n);
            // sentence punctuation after the path is not part of it
            while end > i && text[..end].ends_with(['.', ':', '!', '?']) {
                end -= 1;
            }
            out.push_str(&spell(&text[i..end], &roots, root_os));
            i = end;
            continue;
        }
        let c = text[i..].chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
    }
    if root_os != Os::Windows {
        for (root, token) in &roots {
            // `temp_dir()` ends with a separator on macOS: the text after the root keeps its own.
            if let Some(r) = root
                .to_str()
                .map(|r| r.trim_end_matches('/'))
                .filter(|r| !r.is_empty())
            {
                out = out.replace(r, token);
            }
        }
    }
    out
}

/// The roots of this machine a golden must not hold: the temp folder (also its canonical spelling) and the home folder.
pub fn machine_roots() -> Vec<(PathBuf, &'static str)> {
    let mut v = Vec::new();
    let tmp = std::env::temp_dir();
    if let Ok(real) = winpath::real(&tmp) {
        v.push((real, "<tmp>"));
    }
    v.push((tmp, "<tmp>"));
    for var in ["HOME", "USERPROFILE"] {
        if let Some(h) = std::env::var_os(var) {
            v.push((PathBuf::from(h), "<home>"));
        }
    }
    v
}

/// [`normalise_paths_on`] for the running OS with [`machine_roots`].
pub fn normalise_paths(text: &str) -> String {
    let owned = machine_roots();
    let roots: Vec<Root<'_>> = owned.iter().map(|(p, t)| (p.as_path(), *t)).collect();
    normalise_paths_on(text, &roots, Os::current())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN_TMP: &str = r"C:\Users\runner\AppData\Local\Temp";

    fn win(text: &str) -> String {
        normalise_paths_on(
            text,
            &[
                (Path::new(WIN_TMP), "<tmp>"),
                (Path::new(r"C:\Users\runner"), "<home>"),
            ],
            Os::Windows,
        )
    }

    /// The positive control: the mixed-separator text that train B's golden held, unchanged, differs from the stable spelling;
    /// through the normaliser it equals it.
    #[test]
    fn a_mixed_separator_windows_path_is_one_spelling() {
        let raw = r"C:\Users\runner\AppData\Local\Temp\.tmpAB/node-caches\npm";
        assert_ne!(
            raw, "<tmp>/.tmpAB/node-caches/npm",
            "without normalising the raw text differs"
        );
        assert_eq!(win(raw), "<tmp>/.tmpAB/node-caches/npm");
        assert_eq!(
            win(r"c:/users/RUNNER/appdata/local/temp/.tmpAB\node-caches\npm"),
            "<tmp>/.tmpAB/node-caches/npm"
        );
    }

    #[test]
    fn roots_longest_first_verbatim_prefix_and_surrounding_text() {
        assert_eq!(
            win(r"home is C:\Users\runner\work, tmp is \\?\C:\Users\runner\AppData\Local\Temp\x."),
            "home is <home>/work, tmp is <tmp>/x."
        );
        assert_eq!(win(r#"{"dir":"C:\Users\runner"}"#), r#"{"dir":"<home>"}"#);
        assert_eq!(
            win("D:\\other\\p and plain text, no path: C:file"),
            "D:/other/p and plain text, no path: C:file"
        );
    }

    #[test]
    fn text_that_is_no_windows_path_is_untouched() {
        let t = "abcC:\\x is not a path start, nor is a/b\\c or 12:30";
        assert_eq!(win(t), t);
    }

    #[test]
    fn a_unix_root_is_replaced_wherever_it_occurs() {
        let out = normalise_paths_on(
            "cache /var/folders/ab/T/x and /var/folders/ab/T",
            &[(Path::new("/var/folders/ab/T"), "<tmp>")],
            Os::Linux,
        );
        assert_eq!(out, "cache <tmp>/x and <tmp>");
    }
}
