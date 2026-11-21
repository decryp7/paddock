//! The path text of golden files (ER-46, ADR 0021 amendment 2026-11-11 D-P1).
//!
//! A golden never holds this machine's temp or home folder: [`GoldenPaths::Roots`] (the default of
//! [`crate::golden::check_golden`]) replaces them with `<tmp>` and `<home>`, as whole paths only (`/var/tmp` and `/tmpfs` are not
//! `/tmp`; the `/tmp` of `${RUNNER_TEMP:-/tmp}` is shell text, not a folder of this machine), and changes nothing else. Everything
//! else is what the program printed, so a golden still catches a Windows spelling that regressed (cmd text, a `sc.exe` binPath, a
//! registry value, a long-path helper; ER-33).
//!
//! [`GoldenPaths::Fold`] is the explicit opt-in for a test whose subject is a path a tool prints and that the platforms spell
//! differently: it also reads every Windows-style path through [`paddock_env::winpath`] (one `/` separator, no `\\?\`).

use paddock_env::Os;
use paddock_env::winpath;
use std::path::{Path, PathBuf};

/// How much of the path text of a golden is normalised before it is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GoldenPaths {
    /// Only this machine's temp and home folders become `<tmp>` and `<home>`; any other text is compared as it is.
    #[default]
    Roots,
    /// [`GoldenPaths::Roots`], and every Windows-style path is first spelled as `winpath::lexical_text` does (`/`, no `\\?\`).
    Fold,
}

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

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.')
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

/// The end of the path that starts at `i`: up to the first character that ends a path, without sentence punctuation after it.
fn path_end(text: &str, i: usize) -> usize {
    let mut end = text[i..].find(ends_path).map_or(text.len(), |n| i + n);
    while end > i && text[..end].ends_with(['.', ':', '!', '?']) {
        end -= 1;
    }
    end
}

/// `text` with every Windows-style path spelled as `winpath::lexical_text` does.
fn fold_windows(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if windows_start(text, i) {
            let end = path_end(text, i);
            out.push_str(&winpath::lexical_text(
                Path::new(&text[i..end]),
                Os::Windows,
            ));
            i = end;
            continue;
        }
        let c = text[i..].chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// The roots that can be replaced (longer than `/`), longest first.
fn usable<'a>(roots: &[Root<'a>]) -> Vec<Root<'a>> {
    let mut v: Vec<Root<'a>> = roots
        .iter()
        .copied()
        .filter(|(r, _)| {
            r.to_str()
                .is_some_and(|t| !t.trim_end_matches(['/', '\\']).is_empty())
        })
        .collect();
    v.sort_by_key(|(r, _)| std::cmp::Reverse(r.as_os_str().len()));
    v
}

/// The roots as Windows paths in `text`: the shortest prefix of a Windows-style path that is the same place as a root (by
/// `winpath::same`: separators, `\\?\` and case do not matter) is replaced, and the rest of the path keeps its spelling.
fn replace_windows_roots(text: &str, roots: &[Root<'_>]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if windows_start(text, i) {
            let end = path_end(text, i);
            let run = &text[i..end];
            let cuts: Vec<usize> = run
                .char_indices()
                .filter(|(_, c)| matches!(c, '/' | '\\'))
                .map(|(n, _)| n)
                .chain([run.len()])
                .collect();
            // The longest root wins (`roots` is longest first), at its shortest matching prefix.
            let hit = roots.iter().find_map(|(r, token)| {
                cuts.iter()
                    .find(|&&n| winpath::same(Path::new(&run[..n]), r, Os::Windows))
                    .map(|&n| (n, *token))
            });
            match hit {
                Some((n, token)) => {
                    out.push_str(token);
                    out.push_str(&run[n..]);
                }
                None => out.push_str(run),
            }
            i = end;
            continue;
        }
        let c = text[i..].chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// Whether the root at `at..end` of `text` is a whole path there: it starts the text or follows white space, a quote, `=` or an opening
/// bracket (not `-` or `:`, as in the shell default `${X:-/tmp}`, and not a name character), and it ends the text or is followed by `/`, a
/// character that ends a path, or a full stop that ends a sentence.
fn whole_path(text: &str, at: usize, end: usize) -> bool {
    let before = text[..at].chars().next_back();
    let starts = before.is_none_or(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | '`' | '=' | '(' | '[' | '{' | '<' | ',' | ';'
            )
    });
    let mut after = text[end..].chars();
    let ends = match after.next() {
        None | Some('/') => true,
        Some('.') => after.next().is_none_or(|c| !is_name_char(c)),
        Some(c) => !is_name_char(c),
    };
    starts && ends
}

/// The roots as Unix paths in `text`, each only as a whole path ([`whole_path`]).
fn replace_unix_roots(text: &str, roots: &[Root<'_>]) -> String {
    let mut out = text.to_string();
    for (root, token) in roots {
        let Some(r) = root
            .to_str()
            .map(|r| r.trim_end_matches('/'))
            .filter(|r| !r.is_empty())
        else {
            continue;
        };
        let mut next = String::with_capacity(out.len());
        let mut from = 0;
        while let Some(found) = out[from..].find(r) {
            let at = from + found;
            let end = at + r.len();
            next.push_str(&out[from..at]);
            next.push_str(if whole_path(&out, at, end) { token } else { r });
            from = end;
        }
        next.push_str(&out[from..]);
        out = next;
    }
    out
}

/// `text` with `roots` replaced by their tokens (the root in `root_os`'s spelling and rules), and with `paths` applied.
pub fn normalise_paths_on(
    text: &str,
    roots: &[Root<'_>],
    root_os: Os,
    paths: GoldenPaths,
) -> String {
    let roots = usable(roots);
    let text = if paths == GoldenPaths::Fold {
        fold_windows(text)
    } else {
        text.to_string()
    };
    if root_os == Os::Windows {
        replace_windows_roots(&text, &roots)
    } else {
        replace_unix_roots(&text, &roots)
    }
}

/// The roots of this machine a golden must not hold: the temp folder (also its canonical spelling) and the home folder, as the
/// test kit reads them ([`crate::ambient::ambient`]: `USERPROFILE` on Windows, `HOME` elsewhere).
pub fn machine_roots() -> Vec<(PathBuf, &'static str)> {
    let ambient = crate::ambient::ambient();
    let mut v = Vec::new();
    if let Ok(real) = winpath::real(&ambient.temp_dir) {
        v.push((real, "<tmp>"));
    }
    v.push((ambient.temp_dir.clone(), "<tmp>"));
    if let Some(home) = &ambient.home {
        v.push((home.clone(), "<home>"));
    }
    v
}

/// [`normalise_paths_on`] for the running OS with [`machine_roots`].
pub fn normalise_paths(text: &str, paths: GoldenPaths) -> String {
    let owned = machine_roots();
    let roots: Vec<Root<'_>> = owned.iter().map(|(p, t)| (p.as_path(), *t)).collect();
    normalise_paths_on(text, &roots, Os::current(), paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN_TMP: &str = r"C:\Users\runner\AppData\Local\Temp";
    const WIN_HOME: &str = r"C:\Users\runner";

    fn on_windows(text: &str, paths: GoldenPaths) -> String {
        normalise_paths_on(
            text,
            &[
                (Path::new(WIN_TMP), "<tmp>"),
                (Path::new(WIN_HOME), "<home>"),
            ],
            Os::Windows,
            paths,
        )
    }

    fn fold(text: &str) -> String {
        on_windows(text, GoldenPaths::Fold)
    }

    fn linux(text: &str) -> String {
        normalise_paths_on(
            text,
            &[(Path::new("/tmp"), "<tmp>"), (Path::new("/root"), "<home>")],
            Os::Linux,
            GoldenPaths::Roots,
        )
    }

    /// The positive control of the fold: the mixed-separator text that train B's golden held, unchanged, differs from the stable
    /// spelling; folded it equals it.
    #[test]
    fn folding_makes_a_mixed_separator_windows_path_one_spelling() {
        let raw = r"C:\Users\runner\AppData\Local\Temp\.tmpAB/node-caches\npm";
        assert_ne!(raw, "<tmp>/.tmpAB/node-caches/npm");
        assert_eq!(fold(raw), "<tmp>/.tmpAB/node-caches/npm");
        assert_eq!(
            fold(r"c:/users/RUNNER/appdata/local/temp/.tmpAB\node-caches\npm"),
            "<tmp>/.tmpAB/node-caches/npm"
        );
    }

    /// P1-2 (review of de09dab8a): without the opt-in only the roots change, so a Windows spelling that regressed still shows.
    #[test]
    fn by_default_only_the_roots_change_and_the_rest_keeps_its_spelling() {
        let d = GoldenPaths::default();
        assert_eq!(d, GoldenPaths::Roots);
        let raw = r"C:\Users\runner\AppData\Local\Temp\.tmpAB/node-caches\npm";
        assert_eq!(on_windows(raw, d), r"<tmp>\.tmpAB/node-caches\npm");
        assert_eq!(
            on_windows(r"binPath= C:\Program Files\x", d),
            r"binPath= C:\Program Files\x"
        );
        assert_eq!(on_windows(r"\\?\C:\long\path", d), r"\\?\C:\long\path");
        // a root is found in any spelling of the root itself
        assert_eq!(on_windows(r"\\?\C:\Users\runner\work", d), r"<home>\work");
        assert_eq!(on_windows("c:/users/RUNNER/x", d), "<home>/x");
    }

    #[test]
    fn the_longer_root_wins_where_one_is_inside_the_other() {
        assert_eq!(
            fold(r"in C:\Users\runner\AppData\Local\Temp\x."),
            "in <tmp>/x."
        );
        assert_eq!(
            fold(r"home is C:\Users\runner\work, tmp is \\?\C:\Users\runner\AppData\Local\Temp\x."),
            "home is <home>/work, tmp is <tmp>/x."
        );
        assert_eq!(fold(r#"{"dir":"C:\Users\runner"}"#), r#"{"dir":"<home>"}"#);
        assert_eq!(
            fold("D:\\other\\p and plain text, no path: C:file"),
            "D:/other/p and plain text, no path: C:file"
        );
    }

    #[test]
    fn text_that_is_no_windows_path_is_untouched() {
        let t = "abcC:\\x is not a path start, nor is a/b\\c or 12:30";
        assert_eq!(fold(t), t);
        assert_eq!(on_windows(t, GoldenPaths::Roots), t);
    }

    /// P1-1 (review of de09dab8a): a root is replaced only as a whole path, never inside another name.
    #[test]
    fn a_unix_root_is_replaced_only_at_a_path_boundary() {
        assert_eq!(linux("/var/tmp"), "/var/tmp");
        assert_eq!(linux("/tmpfs and /rootfs"), "/tmpfs and /rootfs");
        assert_eq!(linux("/tmp.bak and /tmp-x"), "/tmp.bak and /tmp-x");
        assert_eq!(
            linux("/tmp/x /tmp. and (/tmp)"),
            "<tmp>/x <tmp>. and (<tmp>)"
        );
        assert_eq!(linux("at /root"), "at <home>");
    }

    /// commands.tsv:97 holds the shell text `${RUNNER_TEMP:-/tmp}`: that `/tmp` is a literal default, not this machine's temp folder,
    /// and a Mac (temp folder under /var/folders) could not write `<tmp>` there.
    #[test]
    fn a_literal_default_in_shell_text_is_not_a_root() {
        let line = "mkdir -p \"${RUNNER_TEMP:-/tmp}/x\"";
        assert_eq!(linux(line), line);
    }

    #[test]
    fn a_unix_root_with_a_trailing_slash_and_a_canonical_spelling_are_both_replaced() {
        let out = normalise_paths_on(
            "cache /private/var/folders/ab/T/x and /var/folders/ab/T",
            &[
                (Path::new("/var/folders/ab/T/"), "<tmp>"),
                (Path::new("/private/var/folders/ab/T"), "<tmp>"),
            ],
            Os::MacOs,
            GoldenPaths::Roots,
        );
        assert_eq!(out, "cache <tmp>/x and <tmp>");
    }
}
