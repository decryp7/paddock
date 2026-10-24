//! Personal data in text: user names, home folders, profile paths, the machine's name, e-mail and IP addresses. Ported from
//! Dyno's `diagnostics.rs`; [`Personal`] is one layer of the [`Redactor`](crate::Redactor).

use serde::{Deserialize, Serialize};

/// What redaction removed, by kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RedactionCounts {
    /// Secrets: tokens, passwords, keys, credentials in URLs, card numbers.
    pub secrets: u32,
    /// Properties of log lines and settings the log's rules masked or left out (secret keys, file contents).
    pub fields: u32,
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
        self.secrets
            + self.fields
            + self.home_paths
            + self.profile_paths
            + self.machine_names
            + self.user_names
            + self.emails
            + self.ip_addresses
    }

    pub(crate) fn add(&mut self, o: &RedactionCounts) {
        self.secrets += o.secrets;
        self.fields += o.fields;
        self.home_paths += o.home_paths;
        self.profile_paths += o.profile_paths;
        self.machine_names += o.machine_names;
        self.user_names += o.user_names;
        self.emails += o.emails;
        self.ip_addresses += o.ip_addresses;
    }
}

/// Removes personal details from text: home folders, profile paths, the machine's name, user names, e-mail and IP addresses.
/// Built once from what the app knows about this machine (ported from Dyno's diagnostics, the best of the three designs).
#[derive(Debug, Clone, Default)]
pub struct Personal {
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
        let before_ok = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word(c));
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
        let after_ok = text[end..]
            .chars()
            .next()
            .is_none_or(|c| c == '/' || c == '\\' || !is_word(c));
        let before_ok = text[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word(c));
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
        for marker in [
            "/Users/",
            "\\Users\\",
            "/home/",
            "\\home\\",
            "/users/",
            "\\users\\",
        ] {
            if let Some(stripped) = rest.strip_prefix(marker) {
                let seg_len = stripped
                    .find(['/', '\\', ' ', '"', '\'', '\n'])
                    .unwrap_or(stripped.len());
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
    for token in text.split_inclusive(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ',' | ';' | '(' | ')')
    }) {
        let body = token.trim_end_matches(|c: char| {
            c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ',' | ';' | '(' | ')')
        });
        let tail = &token[body.len()..];
        let looks = body.matches('@').count() == 1
            && body.split_once('@').is_some_and(|(l, d)| {
                !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.')
            });
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
        let boundary_before =
            i == 0 || !(chars[i - 1].is_ascii_alphanumeric() || chars[i - 1] == '.');
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
                || !(chars[j].is_ascii_alphanumeric()
                    || (chars[j] == '.' && chars.get(j + 1).is_some_and(|c| c.is_ascii_digit())));
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
        let word_before = out
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !word_before
            && body.matches(':').count() >= 2
            && body.parse::<std::net::Ipv6Addr>().is_ok()
        {
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

impl Personal {
    /// A redactor for this machine: the user names, home folders and machine names to remove. Empty and one-letter
    /// entries are ignored.
    pub fn new(user_names: &[String], home_dirs: &[String], machine_names: &[String]) -> Personal {
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
        Personal {
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
        let (t, n) = replace_emails(&s);
        s = t;
        c.emails += n;
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
        let (t, n) = replace_ipv4(&s);
        s = t;
        c.ip_addresses += n;
        let (t, n) = replace_ipv6(&s);
        s = t;
        c.ip_addresses += n;
        (s, c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redactor() -> Personal {
        Personal::new(
            &["alex".into(), "a".into(), "".into()],
            &["/Users/alex".into(), "C:\\Users\\alex".into()],
            &["ALEX-PC".into()],
        )
    }

    #[test]
    fn home_paths_machine_names_and_users_are_removed() {
        let (s, c) = redactor().apply(
            "open /Users/alex/Library/x and c:\\users\\Alex\\Desktop\\y on alex-pc for Alex",
        );
        assert_eq!(
            s,
            "open ~/Library/x and ~\\Desktop\\y on <machine> for <user>"
        );
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
        let r = Personal::default();
        let (s, c) = r.apply("C:\\Users\\bob\\x /home/carol/y /Users/Shared/z mail bob@example.com, host 192.168.1.20:8080 and 1.2.3 and 999.1.1.1 v1.2.3.4x");
        assert_eq!(c.profile_paths, 2);
        assert!(
            s.contains("C:\\Users\\<user>\\x")
                && s.contains("/home/<user>/y")
                && s.contains("/Users/Shared/z"),
            "{s}"
        );
        assert!(s.contains("<email>,") && !s.contains("example.com"), "{s}");
        assert!(s.contains("host <ip>:8080"), "{s}");
        assert!(
            s.contains("1.2.3 ") && s.contains("999.1.1.1") && s.contains("v1.2.3.4x"),
            "{s}"
        );
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
        assert_eq!(
            r.apply("a b").1.total(),
            0,
            "one-letter names are never matched"
        );
        assert_eq!(
            Personal::new(&["x".into()], &[], &[]).apply("x").1.total(),
            0
        );
        let big = "alex ".repeat(100_000);
        assert_eq!(r.apply(&big).1.user_names, 100_000);
    }

    #[test]
    fn a_one_letter_needle_matches_nothing() {
        assert_eq!(replace_word("a b", "a", "<x>"), ("a b".to_string(), 0));
    }

    #[test]
    fn ipv6_addresses_are_redacted_but_times_and_mac_addresses_are_not() {
        let (t, n) = replace_ipv6(
            "peer 2a00:1450:4001:82b::200e and [::1]:443, fe80::1ff:fe23:4567:890a. at 12:30:45 mac aa:bb:cc:dd:ee:ff",
        );
        assert_eq!(n, 3);
        assert_eq!(
            t,
            "peer <ip> and [<ip>]:443, <ip>. at 12:30:45 mac aa:bb:cc:dd:ee:ff"
        );
        let (t, n) = replace_ipv6("nothing here: 1:2");
        assert_eq!((t.as_str(), n), ("nothing here: 1:2", 0));
        let r = Personal::new(&[], &[], &[]);
        let (t, c) = r.apply("a 10.1.2.3 b 2001:db8::5");
        assert_eq!(t, "a <ip> b <ip>");
        assert_eq!(c.ip_addresses, 2);
    }
}
