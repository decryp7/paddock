//! The pure redaction: text in, text with placeholders and the counts out. E-mail addresses become `[email]`, IPv4
//! addresses `[ipv4]`, and long secrets `[secret]`: runs of 32 or more hex digits, or 32 or more base64 / base64url
//! characters that mix letters and digits (API keys, tokens, JWT parts). Regexes run in linear time (the `regex`
//! crate), so no input makes this slow.

use regex::Regex;
use std::sync::OnceLock;

/// How many things were redacted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// E-mail addresses.
    pub emails: u64,
    /// IPv4 addresses.
    pub ipv4: u64,
    /// Long hex or base64 secrets.
    pub secrets: u64,
}

impl Counts {
    /// Adds `other`.
    pub fn add(&mut self, other: Counts) {
        self.emails += other.emails;
        self.ipv4 += other.ipv4;
        self.secrets += other.secrets;
    }

    /// Everything redacted.
    pub fn total(&self) -> u64 {
        self.emails + self.ipv4 + self.secrets
    }
}

struct Patterns {
    email: Regex,
    ipv4: Regex,
    hex: Regex,
    base64: Regex,
}

/// The patterns, compiled once. They are constants (a test compiles them), so `None` only if the regex engine
/// refused them; redaction then leaves text alone rather than panicking in a plugin.
fn patterns() -> Option<&'static Patterns> {
    static P: OnceLock<Option<Patterns>> = OnceLock::new();
    P.get_or_init(|| {
        Some(Patterns {
            email: Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)*\.[A-Za-z]{2,}").ok()?,
            ipv4: Regex::new(r"\b(?:(?:25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9]?[0-9])\.){3}(?:25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9]?[0-9])\b").ok()?,
            hex: Regex::new(r"\b[0-9A-Fa-f]{32,}\b").ok()?,
            base64: Regex::new(r"[A-Za-z0-9+/_\-]{32,}={0,2}").ok()?,
        })
    })
    .as_ref()
}

/// Whether a base64-looking run is a secret rather than, say, a long path or word: it mixes letters and digits.
fn looks_secret(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit()) && s.chars().any(|c| c.is_ascii_alphabetic())
}

/// `text` with e-mail addresses, IPv4 addresses and long secrets replaced, and what was replaced.
pub fn redact(text: &str) -> (String, Counts) {
    let Some(p) = patterns() else {
        return (text.to_string(), Counts::default());
    };
    let mut counts = Counts::default();
    let step = p.email.replace_all(text, |_: &regex::Captures| {
        counts.emails += 1;
        "[email]"
    });
    let step = p.ipv4.replace_all(&step, |_: &regex::Captures| {
        counts.ipv4 += 1;
        "[ipv4]"
    });
    let step = p.hex.replace_all(&step, |_: &regex::Captures| {
        counts.secrets += 1;
        "[secret]"
    });
    let step = p.base64.replace_all(&step, |c: &regex::Captures| {
        let m = c.get(0).map_or("", |m| m.as_str());
        if looks_secret(m) {
            counts.secrets += 1;
            "[secret]".to_string()
        } else {
            m.to_string()
        }
    });
    (step.into_owned(), counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(text: &str) -> String {
        redact(text).0
    }

    #[test]
    fn the_patterns_compile() {
        assert!(patterns().is_some());
    }

    #[test]
    fn emails_are_redacted() {
        assert_eq!(r("write to jane.doe+x@mail.example.org today"), "write to [email] today");
        assert_eq!(r("a@b.io, c_d@e-f.co.uk"), "[email], [email]");
        assert_eq!(r("not an address: a@b or @handle"), "not an address: a@b or @handle");
        assert_eq!(redact("x@y.com y@z.com").1.emails, 2);
    }

    #[test]
    fn ipv4_addresses_are_redacted_but_not_versions_or_bad_octets() {
        assert_eq!(r("server 192.168.0.10 and 8.8.8.8."), "server [ipv4] and [ipv4].");
        assert_eq!(r("255.255.255.255"), "[ipv4]");
        assert_eq!(r("version 1.2.3 and 999.1.1.1 and 1.2.3.4.5"), "version 1.2.3 and 999.1.1.1 and [ipv4].5");
        assert_eq!(redact("10.0.0.1").1.ipv4, 1);
    }

    #[test]
    fn long_hex_and_base64_secrets_are_redacted() {
        let hex = "deadbeef".repeat(5);
        assert_eq!(r(&format!("key={hex}")), "key=[secret]");
        let token = "sk-AbCdEf0123456789GhIjKlMnOpQrStUvWx";
        assert_eq!(r(&format!("token {token} end")), "token [secret] end");
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4ifQ";
        assert_eq!(r(jwt), "[secret].[secret]");
        assert_eq!(r("c2VjcmV0LXNlY3JldC1zZWNyZXQtc2VjcmV0LXg0Mg=="), "[secret]");
        let (_, counts) = redact(&format!("{hex} {token}"));
        assert_eq!(counts.secrets, 2);
    }

    #[test]
    fn ordinary_text_is_left_alone() {
        for text in [
            "The quick brown fox jumps over the lazy dog.",
            "/usr/local/share/applications/somethingverylongbutnodigits",
            "short abc123 tokens and 0xdeadbeef stay",
            "Pneumonoultramicroscopicsilicovolcanoconiosis",
            "",
            "héllo wörld — ünïcode stays",
        ] {
            assert_eq!(redact(text), (text.to_string(), Counts::default()), "{text}");
        }
    }

    #[test]
    fn counts_add_up() {
        let mut c = Counts::default();
        c.add(Counts {
            emails: 1,
            ipv4: 2,
            secrets: 3,
        });
        c.add(redact("a@b.io 1.1.1.1").1);
        assert_eq!((c.emails, c.ipv4, c.secrets, c.total()), (2, 3, 3, 8));
    }

    #[test]
    fn huge_inputs_stay_linear() {
        let big = "a".repeat(2_000_000) + "@";
        let started = std::time::Instant::now();
        let _ = redact(&big);
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
    }
}
