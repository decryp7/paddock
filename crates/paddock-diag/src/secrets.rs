//! Secrets in text. The union of Pitcrew's `redact.rs` (anything after a secret-sounding key, long hexadecimal strings, JSON
//! web tokens, credentials in URLs) and Debrief's detectors (bearer tokens, connection-string keys, passwords, card numbers
//! that pass the Luhn check). It errs on the side of removing too much.

use regex::{Captures, Regex};
use std::sync::OnceLock;

/// What replaces a secret.
pub const REDACTED: &str = "[redacted]";

struct Patterns {
    /// `token=abc`, `"password": "x"`, `--api-key x`, `Authorization: Bearer x`: keeps the key (group 1), drops the value.
    keyed: Regex,
    /// `Bearer abcdef12`.
    bearer: Regex,
    /// `AccountKey=...`, `SharedAccessKey=...`.
    connection: Regex,
    /// Gitea-style 40 hex digit tokens and the like.
    long_hex: Regex,
    jwt: Regex,
    /// `https://user:pass@host`: keeps the scheme (group 1).
    url_credentials: Regex,
    /// 13 to 19 digits that pass the Luhn check.
    card: Regex,
    /// `pass=x`, `pw: x`, `&sig=x`: short key names that need `=` or `:` and a word boundary.
    keyed_short: Regex,
    /// `password: correct horse`: the rest of the line after a colon.
    password_line: Regex,
    /// A PEM private key block (or the rest of the text when its end is missing).
    pem: Regex,
    /// Tokens that carry their own prefix: `ghp_...`, `AKIA...`, `pw_...`, `sk-...`.
    prefixed: Regex,
}

fn compile(pattern: &str) -> Regex {
    // The patterns are constants checked by the unit tests; a defect would be a compile-time-like failure of every test.
    Regex::new(pattern).unwrap_or_else(|e| panic!("secret pattern {pattern}: {e}"))
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| Patterns {
        keyed: compile(
            r#"(?i)((?:token|password|passwd|pwd|secret|api[_-]?key|access[_-]?key|session[_-]?key|authorization|cookie)["']?\s*(?:[:=]\s*["']?|\s+))(?:(?:bearer|token|basic)\s+)?[^\s"',;&]+"#,
        ),
        bearer: compile(r"(?i)(\bBearer\s+)[A-Za-z0-9._~+/=-]{8,}"),
        connection: compile(r#"(?i)((?:AccountKey|SharedAccessKey|SharedAccessSignature)\s*=\s*)[^;\s"']+"#),
        long_hex: compile(r"\b[0-9A-Fa-f]{32,}\b"),
        jwt: compile(r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]*"),
        url_credentials: compile(r"(?i)\b([a-z][a-z0-9+.-]*://)[^/\s:@]+:[^/\s@]+@"),
        card: compile(r"\b\d(?:[ -]?\d){12,18}\b"),
        keyed_short: compile(r#"(?i)(\b(?:pass|pw|sig|signature)["']?\s*[:=]\s*["']?)[^\s"',;&]+"#),
        password_line: compile(r#"(?i)(\b(?:password|passwd)\s*:\s*\[redacted\])[ \t]+[^\r\n"']+"#),
        pem: compile(r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?(?:-----END [A-Z ]*PRIVATE KEY-----|$)"),
        // Pitwall's and the MCP keys have no word boundary before them: they come URL-encoded (`key%3Dpw_...`) or glued to a
        // name (`app_pw_...`), and their prefix is distinctive enough to match anywhere.
        prefixed: compile(r"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|(?:AKIA|ASIA)[A-Z0-9]{16}|sk-[A-Za-z0-9_-]{16,})|(?:pw|pmk)_[A-Za-z0-9_-]{8,}"),
    })
}

/// Luhn checksum over the digits of `s`; 13 to 19 digits.
pub fn luhn_ok(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                *d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

fn replace(
    re: &Regex,
    text: String,
    count: &mut u32,
    with: impl Fn(&Captures<'_>) -> Option<String>,
) -> String {
    if !re.is_match(&text) {
        return text;
    }
    re.replace_all(&text, |c: &Captures<'_>| match with(c) {
        Some(s) => {
            *count += 1;
            s
        }
        None => c[0].to_string(),
    })
    .into_owned()
}

/// `text` with secrets replaced by [`REDACTED`], and how many were replaced.
pub fn redact_secrets(text: &str) -> (String, u32) {
    let p = patterns();
    let mut n = 0;
    let mut out = text.to_string();
    out = replace(&p.url_credentials, out, &mut n, |c| {
        Some(format!("{}{REDACTED}@", &c[1]))
    });
    out = replace(&p.keyed, out, &mut n, |c| {
        Some(format!("{}{REDACTED}", &c[1]))
    });
    out = replace(&p.keyed_short, out, &mut n, |c| {
        Some(format!("{}{REDACTED}", &c[1]))
    });
    out = replace(&p.password_line, out, &mut n, |c| Some(c[1].to_string()));
    out = replace(&p.pem, out, &mut n, |_| Some(REDACTED.into()));
    out = replace(&p.prefixed, out, &mut n, |_| Some(REDACTED.into()));
    out = replace(&p.bearer, out, &mut n, |c| {
        Some(format!("{}{REDACTED}", &c[1]))
    });
    out = replace(&p.connection, out, &mut n, |c| {
        Some(format!("{}{REDACTED}", &c[1]))
    });
    out = replace(&p.jwt, out, &mut n, |_| Some(REDACTED.into()));
    out = replace(&p.long_hex, out, &mut n, |_| Some(REDACTED.into()));
    out = replace(&p.card, out, &mut n, |c| {
        luhn_ok(&c[0]).then(|| REDACTED.to_string())
    });
    (out, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn pitcrews_cases_still_pass() {
        let cases = [
            (
                format!("register --token {TOKEN} --name x"),
                "register --token [redacted] --name x",
            ),
            (
                "token=abc123 other=1".to_string(),
                "token=[redacted] other=1",
            ),
            (
                r#"{"token": "abc", "name": "x"}"#.to_string(),
                r#"{"token": "[redacted]", "name": "x"}"#,
            ),
            (
                "Authorization: Bearer abc.def".to_string(),
                "Authorization: [redacted]",
            ),
            ("password: hunter2".to_string(), "password: [redacted]"),
            (format!("id {TOKEN} end"), "id [redacted] end"),
            (
                "jwt eyJhbGciOiJI.eyJzdWIiOiIx.SflKxwRJSMeKKF2Q".to_string(),
                "jwt [redacted]",
            ),
            (
                "https://user:pass@git.example.com/x".to_string(),
                "https://[redacted]@git.example.com/x",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(redact_secrets(&input).0, want, "{input}");
        }
    }

    /// Regression (Pitwall B4 review): a Pitwall or MCP key right after a word character (URL-encoded in a query, glued to
    /// a name) kept its value, because the rule began at a word boundary.
    #[test]
    fn pitwall_and_mcp_keys_are_removed_wherever_they_start() {
        for (input, want) in [
            (
                "GET /x?key%3Dpw_Zk9Qx2LmZk9Qx2Lm HTTP/1.1",
                "GET /x?key%3D[redacted] HTTP/1.1",
            ),
            ("app_pw_Zk9Qx2LmZk9Qx2Lm", "app_[redacted]"),
            ("first line\npw_Zk9Qx2LmZk9Qx2Lm", "first line\n[redacted]"),
            ("mcp=pmk_Q2xpZW50S2V5Rm9y;", "mcp=[redacted];"),
            ("xpmk_Q2xpZW50S2V5Rm9y", "x[redacted]"),
        ] {
            assert_eq!(redact_secrets(input).0, want, "{input}");
        }
        // Too short to be a key: left as it is.
        assert_eq!(redact_secrets("a pw_short b").0, "a pw_short b");
    }

    #[test]
    fn debriefs_detectors_are_covered() {
        let (s, n) = redact_secrets("call with Bearer abcdefgh12345678 now");
        assert_eq!((s.as_str(), n), ("call with Bearer [redacted] now", 1));
        let (s, _) = redact_secrets("Server=x;AccountKey=abc123def456==;Other=1");
        assert_eq!(s, "Server=x;AccountKey=[redacted];Other=1");
        let (s, _) = redact_secrets("pwd=hunter2 and passwd: s3cret");
        assert_eq!(s, "pwd=[redacted] and passwd: [redacted]");
        // A card number passes Luhn; an order number of the same length does not.
        let (s, n) = redact_secrets("card 4111 1111 1111 1111 order 1234 5678 9012 3456");
        assert_eq!(s, "card [redacted] order 1234 5678 9012 3456");
        assert_eq!(n, 1);
        assert!(luhn_ok("4111111111111111") && !luhn_ok("4111111111111112") && !luhn_ok("123"));
    }

    #[test]
    fn ordinary_lines_are_kept() {
        let line = r#"time="2026-10-22T13:36:25+08:00" level=info msg="task started" job=build job_id=8288 run_id=5983"#;
        assert_eq!(redact_secrets(line), (line.to_string(), 0));
        assert_eq!(
            redact_secrets("work/bd9356900cdd427a").0,
            "work/bd9356900cdd427a",
            "short hashes stay"
        );
        assert_eq!(redact_secrets("").1, 0);
    }

    #[test]
    fn junk_never_panics() {
        for t in [
            "\u{0}",
            "token",
            "token=",
            "Bearer ",
            "https://:@",
            "eyJ..",
            "😀 token=😀",
            &"a".repeat(100_000),
        ] {
            let _ = redact_secrets(t);
        }
    }
}
