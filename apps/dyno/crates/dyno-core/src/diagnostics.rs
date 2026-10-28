//! The diagnostics bundle's data model: what goes into the zip a user can send with a bug report, how private details
//! are removed first (user names, home folders, the machine name, e-mail and IP addresses), and the preview that lists
//! every file with its size before anything is written. Pure: the agent collects the texts and writes the zip.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The manifest schema version.
pub const SCHEMA_VERSION: u32 = 1;
/// Most files in a bundle.
pub const MAX_FILES: usize = 32;
/// Largest single file, bytes (longer ones are cut with a note).
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
/// Largest bundle in total, bytes.
pub const MAX_TOTAL_BYTES: usize = 8_000_000;
/// Longest path inside the zip, characters.
pub const MAX_PATH_CHARS: usize = 100;
/// The marker appended to a cut file.
pub const TRUNCATION_NOTE: &str = "\n[cut: the file was longer than the bundle allows]\n";

/// Why a bundle or a path was refused (the message says what to change).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BundleError {
    /// The destination is not usable.
    #[error("{0}")]
    BadDestination(String),
    /// A file name inside the bundle is not allowed.
    #[error("The bundle file name \"{0}\" is not allowed.")]
    BadName(String),
    /// Two files share a name.
    #[error("The bundle already has a file called \"{0}\".")]
    Duplicate(String),
    /// Too many files.
    #[error("A bundle holds at most {MAX_FILES} files.")]
    TooManyFiles,
    /// Too many bytes.
    #[error("The bundle would be larger than {}.", crate::units::whole(MAX_TOTAL_BYTES as u64, crate::units::Kind::Storage))]
    TooLarge,
}

/// What redaction removed, by kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactionCounts {
    /// Home folders replaced by `~`.
    pub home_paths: u32,
    /// Other profile paths (`/Users/x/`, `C:\Users\x\`) whose owner is not known.
    pub profile_paths: u32,
    /// The machine's name.
    pub machine_names: u32,
    /// User names.
    pub user_names: u32,
    /// E-mail addresses.
    pub emails: u32,
    /// IPv4 addresses.
    pub ip_addresses: u32,
}

impl RedactionCounts {
    /// The sum over all kinds.
    pub fn total(&self) -> u32 {
        self.home_paths + self.profile_paths + self.machine_names + self.user_names + self.emails + self.ip_addresses
    }

    fn add(&mut self, o: &RedactionCounts) {
        self.home_paths += o.home_paths;
        self.profile_paths += o.profile_paths;
        self.machine_names += o.machine_names;
        self.user_names += o.user_names;
        self.emails += o.emails;
        self.ip_addresses += o.ip_addresses;
    }
}

/// Removes private details from text. Built once from what the agent knows about this machine.
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    user_names: Vec<String>,
    home_dirs: Vec<String>,
    machine_names: Vec<String>,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Replaces every whole-word, case-insensitive occurrence of `needle` in `text`; returns the count.
fn replace_word(text: &str, needle: &str, with: &str) -> (String, u32) {
    if needle.chars().count() < 2 {
        return (text.to_string(), 0);
    }
    let lower = text.to_lowercase();
    let n = needle.to_lowercase();
    // Lowercasing can change byte lengths (rare Unicode); fall back to the exact text then.
    if lower.len() != text.len() {
        return replace_exact(text, needle, with);
    }
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    let mut last = 0;
    let mut from = 0;
    while let Some(i) = lower[from..].find(&n) {
        let start = from + i;
        let end = start + n.len();
        let before_ok = text[..start].chars().next_back().is_none_or(|c| !is_word(c));
        let after_ok = text[end..].chars().next().is_none_or(|c| !is_word(c));
        if before_ok && after_ok && text.is_char_boundary(start) && text.is_char_boundary(end) {
            out.push_str(&text[last..start]);
            out.push_str(with);
            last = end;
            count += 1;
        }
        from = end;
    }
    out.push_str(&text[last..]);
    (out, count)
}

fn replace_exact(text: &str, needle: &str, with: &str) -> (String, u32) {
    let count = text.matches(needle).count() as u32;
    (text.replace(needle, with), count)
}

/// Replaces a path prefix (case-insensitive, either separator) with `with`.
fn replace_prefix(text: &str, prefix: &str, with: &str) -> (String, u32) {
    let norm = |s: &str| s.to_lowercase().replace('\\', "/");
    let (t, p) = (norm(text), norm(prefix.trim_end_matches(['/', '\\'])));
    if p.is_empty() || t.len() != text.len() {
        return (text.to_string(), 0);
    }
    let mut out = String::with_capacity(text.len());
    let (mut last, mut from, mut count) = (0, 0, 0);
    while let Some(i) = t[from..].find(&p) {
        let start = from + i;
        let end = start + p.len();
        let after_ok = text[end..].chars().next().is_none_or(|c| c == '/' || c == '\\' || !is_word(c));
        let before_ok = text[..start].chars().next_back().is_none_or(|c| !is_word(c));
        if after_ok && before_ok && text.is_char_boundary(start) && text.is_char_boundary(end) {
            out.push_str(&text[last..start]);
            out.push_str(with);
            last = end;
            count += 1;
        }
        from = end;
    }
    out.push_str(&text[last..]);
    (out, count)
}

/// Replaces the owner segment of `/Users/<x>`, `/home/<x>` and `C:\Users\<x>` with `<user>`.
fn replace_profiles(text: &str) -> (String, u32) {
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    let mut rest = text;
    'outer: while !rest.is_empty() {
        for marker in ["/Users/", "\\Users\\", "/home/", "\\home\\", "/users/", "\\users\\"] {
            if let Some(stripped) = rest.strip_prefix(marker) {
                let seg_len = stripped.find(['/', '\\', ' ', '"', '\'', '\n']).unwrap_or(stripped.len());
                let seg = &stripped[..seg_len];
                out.push_str(marker);
                if !seg.is_empty() && seg != "<user>" && seg != "Shared" && seg != "Public" {
                    out.push_str("<user>");
                    count += 1;
                } else {
                    out.push_str(seg);
                }
                rest = &stripped[seg_len..];
                continue 'outer;
            }
        }
        let c = rest.chars().next().unwrap_or(' ');
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    (out, count)
}

fn replace_emails(text: &str) -> (String, u32) {
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    for token in text.split_inclusive(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ',' | ';' | '(' | ')')) {
        let body = token.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ',' | ';' | '(' | ')'));
        let tail = &token[body.len()..];
        let looks = body.matches('@').count() == 1
            && body
                .split_once('@')
                .is_some_and(|(l, d)| !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.'));
        if looks {
            out.push_str("<email>");
            count += 1;
        } else {
            out.push_str(body);
        }
        out.push_str(tail);
    }
    (out, count)
}

fn replace_ipv4(text: &str) -> (String, u32) {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let (mut i, mut count) = (0, 0);
    while i < chars.len() {
        let boundary_before = i == 0 || !(chars[i - 1].is_ascii_alphanumeric() || chars[i - 1] == '.');
        if chars[i].is_ascii_digit() && boundary_before {
            let mut j = i;
            let mut groups = 0;
            let mut ok = true;
            loop {
                let s = j;
                while j < chars.len() && chars[j].is_ascii_digit() && j - s < 4 {
                    j += 1;
                }
                let n: String = chars[s..j].iter().collect();
                if n.is_empty() || n.len() > 3 || n.parse::<u16>().is_ok_and(|v| v > 255) {
                    ok = false;
                    break;
                }
                groups += 1;
                if groups == 4 {
                    break;
                }
                if j < chars.len() && chars[j] == '.' {
                    j += 1;
                } else {
                    ok = false;
                    break;
                }
            }
            let after_ok = j >= chars.len()
                || !(chars[j].is_ascii_alphanumeric() || (chars[j] == '.' && chars.get(j + 1).is_some_and(|c| c.is_ascii_digit())));
            if ok && groups == 4 && after_ok {
                out.push_str("<ip>");
                count += 1;
                i = j;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    (out, count)
}

/// Replaces every IPv6 address (a run of hex digits, colons and dots that parses as one) with `<ip>`. Times
/// (`12:30:45`) and MAC addresses do not parse as IPv6 and are left alone.
fn replace_ipv6(text: &str) -> (String, u32) {
    let is_part = |c: char| c.is_ascii_hexdigit() || c == ':' || c == '.';
    let mut out = String::with_capacity(text.len());
    let (mut count, mut rest) = (0, text);
    while !rest.is_empty() {
        let start = rest.find(is_part).unwrap_or(rest.len());
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        if rest.is_empty() {
            break;
        }
        let end = rest.find(|c: char| !is_part(c)).unwrap_or(rest.len());
        let run = &rest[..end];
        // A trailing colon or dot belongs to the sentence, not the address.
        let body = run.trim_end_matches([':', '.']);
        let tail = &run[body.len()..];
        let word_before = out.chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !word_before && body.matches(':').count() >= 2 && body.parse::<std::net::Ipv6Addr>().is_ok() {
            out.push_str("<ip>");
            count += 1;
        } else {
            out.push_str(body);
        }
        out.push_str(tail);
        rest = &rest[end..];
    }
    (out, count)
}

impl Redactor {
    /// A redactor for this machine: the user names, home folders and machine names to remove. Empty and one-letter
    /// entries are ignored.
    pub fn new(user_names: &[String], home_dirs: &[String], machine_names: &[String]) -> Redactor {
        let clean = |v: &[String], min: usize| {
            let mut v: Vec<String> = v
                .iter()
                .map(|s| s.trim().to_string())
                .filter(|s| s.chars().count() >= min)
                .collect();
            v.sort_by_key(|s| std::cmp::Reverse(s.len()));
            v.dedup();
            v
        };
        Redactor {
            user_names: clean(user_names, 2),
            home_dirs: clean(home_dirs, 3),
            machine_names: clean(machine_names, 2),
        }
    }

    /// Redacts `text` and reports what was removed.
    pub fn apply(&self, text: &str) -> (String, RedactionCounts) {
        let mut c = RedactionCounts::default();
        let mut s = text.to_string();
        for h in &self.home_dirs {
            let (t, n) = replace_prefix(&s, h, "~");
            s = t;
            c.home_paths += n;
        }
        let (t, n) = replace_profiles(&s);
        s = t;
        c.profile_paths += n;
        for m in &self.machine_names {
            let (t, n) = replace_word(&s, m, "<machine>");
            s = t;
            c.machine_names += n;
        }
        for u in &self.user_names {
            let (t, n) = replace_word(&s, u, "<user>");
            s = t;
            c.user_names += n;
        }
        let (t, n) = replace_emails(&s);
        s = t;
        c.emails += n;
        let (t, n) = replace_ipv4(&s);
        s = t;
        c.ip_addresses += n;
        let (t, n) = replace_ipv6(&s);
        s = t;
        c.ip_addresses += n;
        (s, c)
    }
}

/// One file in the bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleFile {
    /// Path inside the zip (relative, `/` separated).
    pub path: String,
    /// What it is, for the preview.
    pub description: String,
    /// The (redacted, bounded) text.
    pub text: String,
}

/// The manifest written into the bundle as `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Schema version.
    pub version: u32,
    /// When it was made, ms since the epoch.
    pub created_at: crate::time::Timestamp,
    /// Dyno's version.
    pub app_version: String,
    /// The OS.
    pub os: String,
    /// What redaction removed across all files.
    pub redactions: RedactionCounts,
    /// The files, in order.
    pub files: Vec<ManifestEntry>,
}

/// A file in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    /// Path inside the zip.
    pub path: String,
    /// What it is.
    pub description: String,
    /// Size in bytes.
    pub bytes: u32,
}

/// The bundle: the manifest and its files (the manifest itself is written last by the zip writer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsBundle {
    /// The manifest.
    pub manifest: Manifest,
    /// The files.
    pub files: Vec<BundleFile>,
}

/// What a bundle contains, shown before anything is saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundlePreview {
    /// Every file with its size (the manifest included).
    pub files: Vec<ManifestEntry>,
    /// Total bytes before compression.
    pub total_bytes: u32,
    /// What was removed from the texts.
    pub redactions: RedactionCounts,
}

/// Whether `path` is a safe name inside the zip: relative, forward slashes, no `..`, no odd characters.
pub fn valid_entry_name(path: &str) -> bool {
    !path.is_empty()
        && path.chars().count() <= MAX_PATH_CHARS
        && !path.starts_with('/')
        && path
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != ".." && !seg.starts_with(' ') && !seg.ends_with(' '))
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '_'))
}

/// Builds a bundle: redacts every text, bounds it and keeps the manifest in step.
#[derive(Debug)]
pub struct BundleBuilder {
    redactor: Redactor,
    files: Vec<BundleFile>,
    redactions: RedactionCounts,
    total: usize,
    created_at: crate::time::Timestamp,
    app_version: String,
    os: String,
}

impl BundleBuilder {
    /// A builder.
    pub fn new(redactor: Redactor, created_at: crate::time::Timestamp, app_version: &str, os: &str) -> BundleBuilder {
        BundleBuilder {
            redactor,
            files: Vec::new(),
            redactions: RedactionCounts::default(),
            total: 0,
            created_at,
            app_version: app_version.chars().take(32).collect(),
            os: os.chars().take(100).collect(),
        }
    }

    /// Adds a text file (redacted; cut at [`MAX_FILE_BYTES`]).
    pub fn add(&mut self, path: &str, description: &str, text: &str) -> Result<(), BundleError> {
        if !valid_entry_name(path) || path == "manifest.json" {
            return Err(BundleError::BadName(path.into()));
        }
        if self.files.iter().any(|f| f.path == path) {
            return Err(BundleError::Duplicate(path.into()));
        }
        if self.files.len() >= MAX_FILES - 1 {
            return Err(BundleError::TooManyFiles);
        }
        let mut cut = text;
        let mut truncated = false;
        if cut.len() > MAX_FILE_BYTES {
            let mut end = MAX_FILE_BYTES;
            while !cut.is_char_boundary(end) {
                end -= 1;
            }
            cut = &cut[..end];
            truncated = true;
        }
        let (mut body, counts) = self.redactor.apply(cut);
        if truncated {
            body.push_str(TRUNCATION_NOTE);
        }
        if self.total + body.len() > MAX_TOTAL_BYTES {
            return Err(BundleError::TooLarge);
        }
        self.total += body.len();
        self.redactions.add(&counts);
        self.files.push(BundleFile {
            path: path.into(),
            description: description.chars().take(120).collect(),
            text: body,
        });
        Ok(())
    }

    /// The finished bundle.
    pub fn build(self) -> DiagnosticsBundle {
        let files = self
            .files
            .iter()
            .map(|f| ManifestEntry {
                path: f.path.clone(),
                description: f.description.clone(),
                bytes: f.text.len() as u32,
            })
            .collect();
        DiagnosticsBundle {
            manifest: Manifest {
                version: SCHEMA_VERSION,
                created_at: self.created_at,
                app_version: self.app_version,
                os: self.os,
                redactions: self.redactions,
                files,
            },
            files: self.files,
        }
    }
}

impl DiagnosticsBundle {
    /// The manifest as the JSON written to `manifest.json`.
    pub fn manifest_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.manifest)
    }
}

/// The listing of a bundle: every file and its size, the manifest included.
pub fn preview(bundle: &DiagnosticsBundle) -> BundlePreview {
    let mut files = bundle.manifest.files.clone();
    let manifest_bytes = bundle.manifest_json().map_or(0, |j| j.len() as u32);
    files.push(ManifestEntry {
        path: "manifest.json".into(),
        description: "What this bundle contains".into(),
        bytes: manifest_bytes,
    });
    BundlePreview {
        total_bytes: files.iter().fold(0u32, |a, f| a.saturating_add(f.bytes)),
        files,
        redactions: bundle.manifest.redactions,
    }
}

/// The answer to a save request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedDiagnostics {
    /// Where the zip was written.
    pub path: String,
    /// Its size on disk, bytes.
    pub bytes: u64,
    /// How many files it holds (the manifest included).
    pub files: u32,
}

/// Checks the destination the user picked, without touching the disk: absolute, a `.zip` file name, no `..`. The
/// agent also checks that the parent folder exists.
pub fn validate_destination(path: &str) -> Result<PathBuf, BundleError> {
    let bad = |m: &str| BundleError::BadDestination(m.into());
    if path.trim().is_empty() || path.len() > 1024 || path.contains('\0') {
        return Err(bad("Choose where to save the bundle."));
    }
    let p = Path::new(path);
    if !p.is_absolute() {
        return Err(bad(
            "Give the full path of the file to save, for example C:\\Users\\you\\Desktop\\dyno-diagnostics.zip.",
        ));
    }
    if crate::cleanup::has_dot_segment(p) {
        return Err(bad("The save path may not contain \"..\"."));
    }
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !name.to_ascii_lowercase().ends_with(".zip") || name.len() <= 4 {
        return Err(bad("Save the bundle as a file ending in .zip."));
    }
    Ok(p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::Timestamp;

    fn redactor() -> Redactor {
        Redactor::new(
            &["alex".into(), "a".into(), "".into()],
            &["/Users/alex".into(), "C:\\Users\\alex".into()],
            &["ALEX-PC".into()],
        )
    }

    #[test]
    fn home_paths_machine_names_and_users_are_removed() {
        let (s, c) = redactor().apply("open /Users/alex/Library/x and c:\\users\\Alex\\Desktop\\y on alex-pc for Alex");
        assert_eq!(s, "open ~/Library/x and ~\\Desktop\\y on <machine> for <user>");
        assert_eq!((c.home_paths, c.machine_names, c.user_names), (2, 1, 1));
        // Whole words only: "alexander" and "valex" stay.
        let (s, c) = redactor().apply("alexander valex alex_1 (alex)");
        assert_eq!(s, "alexander valex alex_1 (<user>)");
        assert_eq!(c.user_names, 1);
        // The home prefix must end at a path boundary.
        let (s, _) = redactor().apply("/Users/alexandra/file");
        assert_eq!(s, "/Users/<user>/file");
    }

    #[test]
    fn unknown_profiles_emails_and_addresses_are_removed() {
        let r = Redactor::default();
        let (s, c) = r.apply("C:\\Users\\bob\\x /home/carol/y /Users/Shared/z mail bob@example.com, host 192.168.1.20:8080 and 1.2.3 and 999.1.1.1 v1.2.3.4x");
        assert_eq!(c.profile_paths, 2);
        assert!(
            s.contains("C:\\Users\\<user>\\x") && s.contains("/home/<user>/y") && s.contains("/Users/Shared/z"),
            "{s}"
        );
        assert!(s.contains("<email>,") && !s.contains("example.com"), "{s}");
        assert!(s.contains("host <ip>:8080"), "{s}");
        assert!(s.contains("1.2.3 ") && s.contains("999.1.1.1") && s.contains("v1.2.3.4x"), "{s}");
        assert_eq!((c.emails, c.ip_addresses), (1, 1));
        assert_eq!(c.total(), 4);
        // Not emails.
        let (s, c) = r.apply("a@b @handle x@y. @@ ");
        assert_eq!(c.emails, 0, "{s}");
    }

    #[test]
    fn junk_and_unicode_never_panic() {
        let r = redactor();
        for t in [
            "",
            "\u{0}",
            "/Users/",
            "/Users/alex",
            "İİİ alex",
            "😀 /home/😀/x",
            "1.1.1.1.1.1.1.1",
            "....",
            "@",
            "alex-pc-alex-pc",
        ] {
            let _ = r.apply(t);
        }
        let (s, _) = r.apply("İ alex İ");
        assert!(s.contains("<user>") || s.contains("alex"));
        assert_eq!(r.apply("a b").1.total(), 0, "one-letter names are never matched");
        assert_eq!(Redactor::new(&["x".into()], &[], &[]).apply("x").1.total(), 0);
        let big = "alex ".repeat(100_000);
        assert_eq!(r.apply(&big).1.user_names, 100_000);
    }

    #[test]
    fn the_builder_redacts_bounds_and_counts() {
        let mut b = BundleBuilder::new(redactor(), Timestamp(5), "0.1.0", "macOS 15");
        // A cut that would land inside a multi-byte character moves back to its start.
        b.add("utf.txt", "Wide", &format!("a{}", "é".repeat(MAX_FILE_BYTES))).unwrap();
        b.add("system/os.txt", "OS", "user alex at /Users/alex/x").unwrap();
        b.add("big.txt", "Big", &"é".repeat(MAX_FILE_BYTES)).unwrap();
        assert_eq!(
            b.add("system/os.txt", "again", "x"),
            Err(BundleError::Duplicate("system/os.txt".into()))
        );
        for bad in [
            "",
            "/abs",
            "../up",
            "a/../b",
            "a//b",
            "a b",
            "manifest.json",
            "a/",
            "naïve.txt",
            &"x".repeat(101),
        ] {
            assert_eq!(b.add(bad, "d", "x"), Err(BundleError::BadName(bad.into())), "{bad}");
        }
        let bundle = b.build();
        assert_eq!(bundle.files[1].text, "user <user> at ~/x");
        assert!(bundle.files[0].text.ends_with(TRUNCATION_NOTE));
        assert!(bundle.files[2].text.ends_with(TRUNCATION_NOTE));
        assert!(bundle.files[2].text.len() <= MAX_FILE_BYTES + TRUNCATION_NOTE.len());
        assert_eq!(bundle.manifest.redactions.user_names, 1);
        assert_eq!(bundle.manifest.files.len(), 3);
        assert_eq!(bundle.manifest.files[1].bytes as usize, bundle.files[1].text.len());
        let p = preview(&bundle);
        assert_eq!(p.files.len(), 4);
        assert_eq!(p.files[3].path, "manifest.json");
        assert!(p.files[3].bytes > 0);
        assert_eq!(p.total_bytes, p.files.iter().map(|f| f.bytes).sum::<u32>());
        assert_eq!(p.redactions.total(), bundle.manifest.redactions.total());
        let back: DiagnosticsBundle = serde_json::from_str(&serde_json::to_string(&bundle).unwrap()).unwrap();
        assert_eq!(back, bundle);
        assert!(bundle.manifest_json().unwrap().contains("\"appVersion\": \"0.1.0\""));
    }

    #[test]
    fn limits_hold() {
        let mut b = BundleBuilder::new(Redactor::default(), Timestamp(0), "v", "os");
        for i in 0..MAX_FILES - 1 {
            b.add(&format!("f{i}.txt"), "d", "x").unwrap();
        }
        assert_eq!(b.add("one-more.txt", "d", "x"), Err(BundleError::TooManyFiles));
        let mut b = BundleBuilder::new(Redactor::default(), Timestamp(0), "v", "os");
        let chunk = "x".repeat(MAX_FILE_BYTES);
        let mut last = Ok(());
        for i in 0..10 {
            last = b.add(&format!("f{i}.txt"), "d", &chunk);
        }
        assert_eq!(last, Err(BundleError::TooLarge));
        assert!(BundleError::TooLarge.to_string().contains("8 MB"));
        assert!(BundleError::TooManyFiles.to_string().contains("32"));
        assert!(BundleError::BadName("x".into()).to_string().contains('x'));
    }

    #[test]
    fn destinations_are_validated() {
        let ok = if cfg!(windows) { "C:\\Users\\me\\out.zip" } else { "/tmp/out.ZIP" };
        assert_eq!(validate_destination(ok).unwrap(), PathBuf::from(ok));
        for bad in [
            "",
            "  ",
            "out.zip",
            "relative/out.zip",
            "/tmp/out.txt",
            "/tmp/.zip",
            "/tmp/a/../out.zip",
            "/tmp/out.zip\0",
            "/tmp/out",
        ] {
            let e = validate_destination(bad);
            assert!(matches!(e, Err(BundleError::BadDestination(_))) || cfg!(windows), "{bad}: {e:?}");
        }
        assert!(validate_destination(&format!("/{}", "a".repeat(2_000))).is_err() || cfg!(windows));
        let msg = validate_destination("/tmp/out.txt").unwrap_err().to_string();
        assert!(msg.contains(".zip"));
    }

    #[test]
    fn entry_names() {
        assert!(valid_entry_name("a/b-c_d.json") && !valid_entry_name("a\\b") && !valid_entry_name("./a") && !valid_entry_name("a/ b"));
    }

    #[test]
    fn ipv6_addresses_are_redacted_but_times_and_mac_addresses_are_not() {
        let (t, n) =
            replace_ipv6("peer 2a00:1450:4001:82b::200e and [::1]:443, fe80::1ff:fe23:4567:890a. at 12:30:45 mac aa:bb:cc:dd:ee:ff");
        assert_eq!(n, 3);
        assert_eq!(t, "peer <ip> and [<ip>]:443, <ip>. at 12:30:45 mac aa:bb:cc:dd:ee:ff");
        let (t, n) = replace_ipv6("nothing here: 1:2");
        assert_eq!((t.as_str(), n), ("nothing here: 1:2", 0));
        let r = Redactor::new(&[], &[], &[]);
        let (t, c) = r.apply("a 10.1.2.3 b 2001:db8::5");
        assert_eq!(t, "a <ip> b <ip>");
        assert_eq!(c.ip_addresses, 2);
    }
}
