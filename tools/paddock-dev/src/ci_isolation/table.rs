//! The two data files of the CI kit, `kept-folders.txt` and `ports.txt`, and the one place that turns a layout into a glob
//! (ADR 0025 and its 2026-11-10 amendment). Nothing here names a Paddock folder: every name comes from the files.
//!
//! A layout is a path under the CI home with placeholders: `{name}` (the caller's name), `{src}` (the first ten hex digits of the
//! SHA-256 of the canonical source root) and `{slot}` (`s1`, `s2`, ...). [`glob_of`] is the only function that reads a layout
//! as a glob, so a placeholder cannot be handled by one reader and forgotten by another (the review P1 of the amendment).

use regex::Regex;
use std::path::{Path, PathBuf};

/// One line of `kept-folders.txt`: `<kind> <layout> <scope>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The kind the actions ask for (`target`, `src`, ...).
    pub kind: String,
    /// The path under the CI home, with placeholders.
    pub layout: String,
    /// `slot` (one folder per job slot) or `shared`.
    pub scope: String,
}

impl Layout {
    /// A per-slot layout.
    pub fn is_slot(&self) -> bool {
        self.scope == "slot"
    }
}

/// The parsed `kept-folders.txt`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Kept {
    /// The current layouts, in file order.
    pub layouts: Vec<Layout>,
    /// Retired layouts: `# retired: <layout> <date>` comment lines. Folders that match one and no current layout are removed once.
    pub retired: Vec<String>,
}

/// One line of `ports.txt`: a block of `width` ports per slot, slot `n` starting at `base + (n - 1) * stride`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortBlock {
    /// The block's name.
    pub name: String,
    /// First port of slot s1.
    pub base: u64,
    /// Ports in a block (checked by the conformance test, not needed to place one).
    pub width: u64,
    /// Distance between the starts of two slots' blocks.
    pub stride: u64,
}

/// What a text file reads as with Windows line ends: the same.
fn lines_of(text: &str) -> impl Iterator<Item = String> + '_ {
    text.replace('\r', "").lines().map(str::to_string).collect::<Vec<_>>().into_iter()
}

/// The placeholders a layout may hold.
const PLACEHOLDERS: [&str; 3] = ["{name}", "{src}", "{slot}"];

/// The first `{...}` in `layout` that is not a known placeholder, if any.
fn unknown_placeholder(layout: &str) -> Option<String> {
    let mut rest = layout;
    while let Some(i) = rest.find('{') {
        let tail = &rest[i..];
        let end = tail.find('}').map_or(tail.len(), |e| e + 1);
        let word = &tail[..end];
        if !PLACEHOLDERS.contains(&word) {
            return Some(word.to_string());
        }
        rest = &tail[end..];
    }
    None
}

/// Parses `kept-folders.txt` (CRLF-safe). A line of fewer than three fields is not a layout. A layout with a placeholder other than
/// `{name}`, `{src}` and `{slot}` is refused with the line named: it would be read as a literal by every glob and never match.
pub fn parse_kept(text: &str) -> Result<Kept, String> {
    let mut kept = Kept::default();
    for (index, line) in lines_of(text).enumerate() {
        let number = index + 1;
        let check = |layout: &str| match unknown_placeholder(layout) {
            Some(p) => Err(format!(
                "kept-folders.txt line {number}: the layout '{layout}' has the unknown placeholder {p}; only {{name}}, {{src}} and {{slot}} exist. Fix the line."
            )),
            None => Ok(()),
        };
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some(first) = fields.first() else { continue };
        if first.starts_with('#') {
            // `# retired: <layout> <date>`
            if let Some(rest) = line.trim_start().strip_prefix('#')
                && let Some(rest) = rest.trim_start().strip_prefix("retired:")
                && let Some(layout) = rest.split_whitespace().next()
            {
                check(layout)?;
                kept.retired.push(layout.to_string());
            }
            continue;
        }
        if fields.len() >= 3 {
            check(fields[1])?;
            kept.layouts.push(Layout {
                kind: fields[0].to_string(),
                layout: fields[1].to_string(),
                scope: fields[2].to_string(),
            });
        }
    }
    Ok(kept)
}

/// Parses `ports.txt` (CRLF-safe). Lines whose numbers do not parse are not blocks.
pub fn parse_ports(text: &str) -> Vec<PortBlock> {
    lines_of(text)
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.is_empty() || f[0].starts_with('#') || f.len() < 4 {
                return None;
            }
            Some(PortBlock {
                name: f[0].to_string(),
                base: f[1].parse().ok()?,
                width: f[2].parse().ok()?,
                stride: f[3].parse().ok()?,
            })
        })
        .collect()
}

/// A layout read as a glob under the CI home: `{name}` and `{src}` are `*` (`?*`, at least one character, when `non_empty`),
/// `{slot}` is `slot`. The single reader of a layout as a glob (amendment §1): every scan of kept folders goes through it.
pub fn glob_of(layout: &str, slot: &str, non_empty: bool) -> String {
    let any = if non_empty { "?*" } else { "*" };
    layout.replace("{name}", any).replace("{src}", any).replace("{slot}", slot)
}

/// Shell-style wildcard match of a whole text: `*` any run (also of `/`), `?` one character.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) && p[pi] != '*' {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// The folders under `home` that a glob (with `/` separators) names, sorted. Only real folders, never a link (a link is not ours).
pub fn expand_dirs(home: &Path, glob: &str) -> Vec<PathBuf> {
    let mut found = vec![home.to_path_buf()];
    for segment in glob.split('/').filter(|s| !s.is_empty()) {
        let mut next = Vec::new();
        for base in &found {
            if segment.contains(['*', '?']) {
                let Ok(entries) = std::fs::read_dir(base) else { continue };
                let mut names: Vec<String> = entries
                    .flatten()
                    .filter_map(|e| e.file_name().into_string().ok())
                    // A leading dot is matched only by a pattern that starts with a literal dot, as the shell does.
                    .filter(|n| (!n.starts_with('.') || segment.starts_with('.')) && glob_match(segment, n))
                    .collect();
                names.sort();
                next.extend(names.into_iter().map(|n| base.join(n)));
            } else {
                next.push(base.join(segment));
            }
        }
        found = next;
    }
    found
        .into_iter()
        .filter(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()))
        .collect()
}

/// The pattern of a layout for slot `n` as a regular expression over the whole path under the CI home, strictly: `{name}` is
/// letters, digits, `.`, `_` and `-`, `{src}` is exactly ten hex digits (amendment §1), `{slot}` is `s<n>`.
pub fn current_regex(layout: &str, n: u32) -> Option<Regex> {
    let mut out = String::from("^");
    let mut rest = layout;
    while let Some(i) = rest.find('{') {
        out.push_str(&regex::escape(&rest[..i]));
        let tail = &rest[i..];
        if let Some(t) = tail.strip_prefix("{name}") {
            out.push_str("[A-Za-z0-9._-]+");
            rest = t;
        } else if let Some(t) = tail.strip_prefix("{src}") {
            out.push_str("[0-9a-f]{10}");
            rest = t;
        } else if let Some(t) = tail.strip_prefix("{slot}") {
            out.push_str(&format!("s{n}"));
            rest = t;
        } else {
            out.push_str(&regex::escape("{"));
            rest = &tail[1..];
        }
    }
    out.push_str(&regex::escape(rest));
    out.push('$');
    Regex::new(&out).ok()
}

impl Kept {
    /// The slot layouts.
    pub fn slot_layouts(&self) -> impl Iterator<Item = &Layout> {
        self.layouts.iter().filter(|l| l.is_slot())
    }

    /// Does `rel` (a path under the CI home) match a current slot layout of slot `n`, strictly?
    pub fn matches_current(&self, rel: &str, n: u32) -> bool {
        self.slot_layouts()
            .filter_map(|l| current_regex(&l.layout, n))
            .any(|re| re.is_match(rel))
    }

    /// The retired rule (amendment §1): `rel` is retired only if it matches a retired layout **and no current layout**. The retired
    /// glob `target-*-s1` also matches a current `target-<name>-<src>-s1`, so the current layouts are tested first, with `{src}` as
    /// exactly ten hex digits.
    pub fn is_retired(&self, rel: &str, n: u32) -> bool {
        if self.matches_current(rel, n) {
            return false;
        }
        self.retired
            .iter()
            .filter(|l| l.contains("{slot}"))
            .any(|l| glob_match(&glob_of(l, &format!("s{n}"), true), rel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "# comment\r\ntarget  target-{name}-{src}-{slot}  slot\r\nsrc  src/{name}-{slot}  slot\r\ntools tools shared\r\n\
        # retired: target-{name}-{slot} 2026-11-10\r\n";

    #[test]
    fn the_table_reads_with_crlf_and_keeps_retired_layouts() {
        let k = parse_kept(TABLE).unwrap();
        assert_eq!(k.layouts.len(), 3);
        assert_eq!(k.layouts[0].kind, "target");
        assert_eq!(k.layouts[0].layout, "target-{name}-{src}-{slot}");
        assert!(k.layouts[2].scope == "shared" && !k.layouts[2].is_slot());
        assert_eq!(k.retired, ["target-{name}-{slot}"]);
    }

    #[test]
    fn a_placeholder_that_does_not_exist_is_refused_with_its_line() {
        let e = parse_kept("src src/{name}-{slot} slot\ntarget target-{name}-{arch}-{slot} slot\n").unwrap_err();
        assert!(e.contains("line 2") && e.contains("{arch}"), "{e}");
        let e = parse_kept("# retired: target-{name}-{os}-{slot} 2026-11-10\n").unwrap_err();
        assert!(e.contains("line 1") && e.contains("{os}"), "{e}");
        assert!(parse_kept("a {name} slot\n").is_ok());
    }

    #[test]
    fn glob_of_reads_every_placeholder_and_src_means_star() {
        assert_eq!(glob_of("target-{name}-{src}-{slot}", "s2", false), "target-*-*-s2");
        assert_eq!(glob_of("target-{name}-{src}-{slot}", "s2", true), "target-?*-?*-s2");
        assert_eq!(glob_of("src/{name}-{slot}", "s1", false), "src/*-s1");
        assert_eq!(glob_of("tools", "s1", false), "tools");
    }

    #[test]
    fn wildcards_match_whole_texts() {
        assert!(glob_match("target-*-s1", "target-a-b-s1"));
        assert!(!glob_match("target-*-s1", "target-a-s12"));
        assert!(!glob_match("target-?*-s1", "target--s1"), "an empty name is no name");
        assert!(glob_match("a*b*c", "aXbYbZc"));
        assert!(!glob_match("a*b", "ab-"));
    }

    fn rel(name: &str, src: &str, n: u32) -> String {
        format!("target-{name}-{src}-s{n}")
    }

    #[test]
    fn an_old_layout_folder_is_retired() {
        let k = parse_kept(TABLE).unwrap();
        assert!(k.is_retired("target-pitwall-app-s1", 1), "a hyphenated name, old layout");
        assert!(k.is_retired("target-x-s2", 2));
    }

    #[test]
    fn a_current_folder_is_never_retired_also_with_hyphens_in_the_name() {
        // The negative controls of the amendment: the retired glob `target-*-s<n>` matches these, the current layout owns them.
        let k = parse_kept(TABLE).unwrap();
        for (name, src) in [("x", "0123456789"), ("pitwall-app", "abcdef0123"), ("a-b-c-d", "ffffffffff")] {
            let r = rel(name, src, 1);
            assert!(glob_match("target-*-s1", &r), "{r}: the retired glob alone would take it");
            assert!(!k.is_retired(&r, 1), "{r} is current");
        }
    }

    #[test]
    fn src_is_exactly_ten_lower_case_hex_digits_before_the_slot() {
        let k = parse_kept(TABLE).unwrap();
        for bad in ["012345678", "01234567890", "0123456789A", "0123456789g"] {
            let r = rel("x", bad, 1);
            assert!(!k.matches_current(&r, 1), "{r} is not a current folder");
            assert!(k.is_retired(&r, 1), "{r}: no current layout matches, the retired glob does");
        }
        assert!(k.matches_current(&rel("x", "0123456789", 1), 1));
        assert!(!k.matches_current(&rel("x", "0123456789", 2), 1), "another slot's folder");
    }

    #[test]
    fn a_folder_matching_no_layout_at_all_is_not_retired() {
        let k = parse_kept(TABLE).unwrap();
        assert!(!k.is_retired("something-else", 1));
        assert!(!k.is_retired("src/x-s1", 1), "a current layout");
        assert!(!k.is_retired("target-x-s1", 2), "another slot");
    }

    #[test]
    fn ports_parse_and_skip_comments() {
        let p = parse_ports("# c\r\nsccache 4227 1 1\r\nwebkit 4320 3 100\r\nbad x y z\r\n");
        assert_eq!(p.len(), 2);
        assert_eq!((p[1].name.as_str(), p[1].base, p[1].width, p[1].stride), ("webkit", 4320, 3, 100));
    }

    #[test]
    fn expand_finds_real_folders_only() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("src/a-s1")).unwrap();
        std::fs::create_dir_all(d.path().join("src/b-s1")).unwrap();
        std::fs::write(d.path().join("src/file-s1"), "").unwrap();
        std::fs::create_dir_all(d.path().join("src/c-s2")).unwrap();
        let found = expand_dirs(d.path(), "src/*-s1");
        assert_eq!(found, [d.path().join("src/a-s1"), d.path().join("src/b-s1")]);
        assert!(expand_dirs(d.path(), "nothing/*-s1").is_empty());
    }
}
