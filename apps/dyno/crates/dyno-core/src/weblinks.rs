//! Web links Dyno opens in the user's browser on a click (WP-09, WP-33). Dyno never fetches anything and never uploads
//! a file: a search carries only the text the user saw and edited, a VirusTotal link carries only a SHA-256 hash.
//!
//! The text a search starts from is redacted first ([`search_seed`]): home folders, profile names, the user and machine
//! names, e-mail and IP addresses, GUIDs, MAC addresses and serial numbers are removed, and the person edits the result
//! before anything is opened.

use serde::{Deserialize, Serialize};

/// The most characters a search text may have.
pub const MAX_QUERY_CHARS: usize = 200;
/// Where a search goes. Chosen for not tracking the person; the address is built here, never in the webview.
pub const SEARCH_BASE: &str = "https://duckduckgo.com/?q=";
/// Where a hash lookup goes (the public file page; nothing is uploaded).
pub const VIRUSTOTAL_BASE: &str = "https://www.virustotal.com/gui/file/";

/// Why a link cannot be built (the text says what to change).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LinkError {
    /// The search text is empty after trimming.
    #[error("Type something to search for.")]
    EmptyQuery,
    /// The search text is longer than the limit.
    #[error("The search text may have at most {MAX_QUERY_CHARS} characters.")]
    QueryTooLong,
    /// The hash is not 64 hexadecimal digits.
    #[error("A SHA-256 hash has 64 hexadecimal digits.")]
    BadHash,
}

/// The starting text of a search with a summary of what was removed from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSeed {
    /// The redacted text, for the person to edit.
    pub text: String,
    /// What was removed, in words ("1 home folder", "2 IP addresses"); empty when nothing was.
    pub removed: Vec<String>,
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Removes the characters the redactor's placeholders leave behind and collapses blanks.
fn tidy(text: &str) -> String {
    let mut out = text.to_string();
    for p in ["<user>", "<machine>", "<ip>", "<email>"] {
        out = out.replace(p, " ");
    }
    // `~/Library/x` and `/Users/<user>/x` lose their private head and keep what a search can use.
    out = out.replace("~/", " ").replace("~\\", " ");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_guid(s: &str) -> bool {
    let t = s.trim_matches(|c| c == '{' || c == '}');
    let parts: Vec<&str> = t.split('-').collect();
    parts.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(&parts)
            .all(|(n, p)| p.len() == *n && p.chars().all(|c| c.is_ascii_hexdigit()))
}

fn is_mac(s: &str) -> bool {
    let sep = if s.contains(':') { ':' } else { '-' };
    let parts: Vec<&str> = s.split(sep).collect();
    parts.len() == 6 && parts.iter().all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Drops GUIDs, MAC addresses and the token after "serial" / "S/N" (counted).
fn drop_identifiers(text: &str) -> (String, u32, u32, u32) {
    let (mut guids, mut macs, mut serials) = (0, 0, 0);
    let mut out: Vec<&str> = Vec::new();
    let mut skip_next = false;
    for tok in text.split_whitespace() {
        let bare = tok.trim_matches(|c: char| matches!(c, ',' | ';' | '(' | ')' | '"' | '\'' | '[' | ']'));
        if skip_next {
            skip_next = false;
            serials += 1;
            continue;
        }
        let lower = bare.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "serial" | "serial:" | "serial=" | "s/n" | "s/n:" | "sn:" | "serialnumber" | "serialnumber:"
        ) {
            skip_next = true;
            continue;
        }
        if let Some(rest) = lower.strip_prefix("serial:").or_else(|| lower.strip_prefix("serial=")) {
            if !rest.is_empty() {
                serials += 1;
                continue;
            }
        }
        if is_guid(bare) {
            guids += 1;
        } else if is_mac(bare) {
            macs += 1;
        } else {
            out.push(tok);
        }
    }
    (out.join(" "), guids, macs, serials)
}

/// The text a search starts from: `text` with private details removed, cut to the limit, plus what was removed.
pub fn search_seed(text: &str, redact: &dyn Fn(&str) -> (String, Removed)) -> SearchSeed {
    let (redacted, c) = redact(text);
    let (without_ids, guids, macs, serials) = drop_identifiers(&redacted);
    let cleaned: String = tidy(&without_ids).chars().take(MAX_QUERY_CHARS).collect();
    SearchSeed {
        text: cleaned,
        removed: removal_summary(&c, guids, macs, serials),
    }
}

/// What a redactor removed from a search seed, by kind (the agent maps `paddock_diag`'s counts onto this; the core names no
/// redactor).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Removed {
    /// Home folders and profile paths.
    pub home_paths: u32,
    /// User names.
    pub user_names: u32,
    /// The machine's name.
    pub machine_names: u32,
    /// E-mail addresses.
    pub emails: u32,
    /// IP addresses.
    pub ip_addresses: u32,
}

fn removal_summary(c: &Removed, guids: u32, macs: u32, serials: u32) -> Vec<String> {
    let mut out = Vec::new();
    let mut add = |n: u32, one: &str, many: &str| {
        if n > 0 {
            out.push(plural(n, one, many));
        }
    };
    add(c.home_paths, "home folder", "home folders");
    add(c.user_names, "user name", "user names");
    add(c.machine_names, "computer name", "computer names");
    add(c.emails, "e-mail address", "e-mail addresses");
    add(c.ip_addresses, "IP address", "IP addresses");
    add(guids, "GUID", "GUIDs");
    add(macs, "MAC address", "MAC addresses");
    add(serials, "serial number", "serial numbers");
    out
}

/// Percent-encodes everything but unreserved characters; a space becomes `+`.
fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b' ' => out.push('+'),
            b if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') => out.push(char::from(b)),
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The search address for exactly the text the person confirmed (not redacted again: they edited it on purpose).
pub fn search_url(query: &str) -> Result<String, LinkError> {
    let q = query.split_whitespace().collect::<Vec<_>>().join(" ");
    if q.is_empty() {
        return Err(LinkError::EmptyQuery);
    }
    if q.chars().count() > MAX_QUERY_CHARS {
        return Err(LinkError::QueryTooLong);
    }
    Ok(format!("{SEARCH_BASE}{}", encode(&q)))
}

/// Whether `s` is 64 hexadecimal digits.
pub fn valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// The VirusTotal page for a file hash (lower-cased). Opening it uploads nothing.
pub fn virustotal_url(sha256: &str) -> Result<String, LinkError> {
    if !valid_sha256(sha256) {
        return Err(LinkError::BadHash);
    }
    Ok(format!("{VIRUSTOTAL_BASE}{}", sha256.to_ascii_lowercase()))
}

/// A file's SHA-256 as the agent computed it, shown before anything is opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHashInfo {
    /// The file that was read.
    pub path: String,
    /// Its size in bytes.
    pub bytes: u64,
    /// The SHA-256, 64 lower-case hexadecimal digits.
    pub sha256: String,
}

/// Where the file to hash comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HashSource {
    /// The program a running process was started from.
    Process(crate::process::ProcessRef),
    /// A program the list shows (an absolute path to a regular file).
    Path(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in redactor with the shape the agent gives: replaces what it knows and counts it.
    fn me(text: &str) -> (String, Removed) {
        let mut c = Removed::default();
        let mut out = text.to_string();
        for (needle, with, count) in [
            ("/Users/alex/", "", &mut c.home_paths),
            ("alexs-mac", "<machine>", &mut c.machine_names),
            ("192.168.1.5", "<ip>", &mut c.ip_addresses),
            ("bob@example.com", "<email>", &mut c.emails),
            ("alex", "<user>", &mut c.user_names),
        ] {
            if out.contains(needle) {
                *count += 1;
                out = out.replace(needle, with);
            }
        }
        (out, c)
    }

    #[test]
    fn a_seed_loses_private_details_and_says_what() {
        let s = search_seed(
            "Slack crashed at /Users/alex/Library/Slack/x.log for alex on alexs-mac 192.168.1.5 {123e4567-e89b-12d3-a456-426614174000} serial C02XYZ12 aa:bb:cc:dd:ee:ff bob@example.com",
            &me,
        );
        assert_eq!(s.text, "Slack crashed at Library/Slack/x.log for on");
        assert_eq!(
            s.removed,
            vec![
                "1 home folder",
                "1 user name",
                "1 computer name",
                "1 e-mail address",
                "1 IP address",
                "1 GUID",
                "1 MAC address",
                "1 serial number",
            ]
        );
    }

    #[test]
    fn plain_text_is_kept_and_nothing_is_reported() {
        let s = search_seed("kernel_task high cpu", &me);
        assert_eq!(s.text, "kernel_task high cpu");
        assert!(s.removed.is_empty());
    }

    #[test]
    fn the_seed_is_cut_to_the_limit() {
        let long = "word ".repeat(200);
        assert!(search_seed(&long, &me).text.chars().count() <= MAX_QUERY_CHARS);
    }

    #[test]
    fn serial_forms_are_all_dropped() {
        for t in [
            "id serial: ABC123",
            "id serial=ABC123",
            "id S/N ABC123",
            "id serial ABC123",
            "id serial:ABC123",
        ] {
            let s = search_seed(t, &me);
            assert_eq!(s.text, "id", "{t}");
            assert_eq!(s.removed, vec!["1 serial number"]);
        }
        // A word that merely starts like it stays.
        assert_eq!(search_seed("serialize data", &me).text, "serialize data");
        assert_eq!(search_seed("a serial", &me).text, "a");
    }

    #[test]
    fn search_urls_encode_the_text_and_check_it() {
        assert_eq!(
            search_url("  kernel_task  100% cpu & más ").unwrap(),
            "https://duckduckgo.com/?q=kernel_task+100%25+cpu+%26+m%C3%A1s"
        );
        assert_eq!(search_url("   "), Err(LinkError::EmptyQuery));
        assert_eq!(search_url(&"a".repeat(MAX_QUERY_CHARS + 1)), Err(LinkError::QueryTooLong));
        assert!(search_url(&"a".repeat(MAX_QUERY_CHARS)).is_ok());
        // Nothing but the encoded text follows the base: no way to add parameters or change the host.
        let u = search_url("x&q=evil#frag?y").unwrap();
        assert!(u.starts_with(SEARCH_BASE) && !u[SEARCH_BASE.len()..].contains(['&', '#', '?']));
    }

    #[test]
    fn hashes_are_checked_and_lower_cased() {
        let h = "AB".repeat(32);
        assert!(valid_sha256(&h));
        assert_eq!(virustotal_url(&h).unwrap(), format!("{VIRUSTOTAL_BASE}{}", "ab".repeat(32)));
        for bad in ["", "abc", &"g".repeat(64), &"a".repeat(65), "../../x"] {
            assert_eq!(virustotal_url(bad), Err(LinkError::BadHash));
        }
    }
}
