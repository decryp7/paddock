//! Removing secrets from text that leaves the machine (the diagnostics bundle) or is shown in Pitcrew's own logs.
//!
//! Pitcrew never reads the runner's `.runner` file and never stores registration tokens, but runner logs and
//! configuration files can still contain secrets (a token on a command line, an `Authorization` header in an error
//! message). Redaction errs on the side of removing too much: anything after a secret-sounding key, long hexadecimal
//! strings (Gitea's tokens are 40 hex digits) and JSON web tokens.

use regex::Regex;
use std::sync::OnceLock;

/// What replaces a secret.
pub const REDACTED: &str = "[redacted]";

fn patterns() -> &'static [Regex] {
    static P: OnceLock<Vec<Regex>> = OnceLock::new();
    P.get_or_init(|| {
        [
            // key=value, key: value, "key": "value", --key value, Authorization: Bearer x
            r#"(?i)((?:token|password|passwd|secret|api[_-]?key|authorization|cookie|uuid)["']?\s*(?:[:=]\s*["']?|\s+))(?:(?:bearer|token|basic)\s+)?[^\s"',;&]+"#,
            // Long hexadecimal strings: tokens, keys.
            r"\b[0-9A-Fa-f]{32,}\b",
            // JSON web tokens.
            r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\b",
            // Credentials in URLs.
            r"(?i)(https?://)[^/\s:@]+:[^/\s@]+@",
        ]
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect()
    })
}

/// `text` with secrets replaced by [`REDACTED`].
pub fn redact(text: &str) -> String {
    let p = patterns();
    let mut out = text.to_string();
    if let Some(re) = p.first() {
        out = re.replace_all(&out, format!("${{1}}{REDACTED}")).into_owned();
    }
    for re in p.iter().skip(1).take(2) {
        out = re.replace_all(&out, REDACTED).into_owned();
    }
    if let Some(re) = p.get(3) {
        out = re.replace_all(&out, format!("${{1}}{REDACTED}@")).into_owned();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_removed() {
        let token = "0123456789abcdef0123456789abcdef01234567";
        let cases = [
            (format!("register --token {token} --name x"), "register --token [redacted] --name x"),
            ("token=abc123 other=1".to_string(), "token=[redacted] other=1"),
            (
                r#"{"token": "abc", "name": "x"}"#.to_string(),
                r#"{"token": "[redacted]", "name": "x"}"#,
            ),
            ("Authorization: Bearer abc.def".to_string(), "Authorization: [redacted]"),
            ("password: hunter2".to_string(), "password: [redacted]"),
            (format!("id {token} end"), "id [redacted] end"),
            ("jwt eyJhbGciOiJI.eyJzdWIiOiIx.SflKxwRJSMeKKF2Q".to_string(), "jwt [redacted]"),
            (
                "https://user:pass@git.example.com/x".to_string(),
                "https://[redacted]@git.example.com/x",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(redact(&input), want, "{input}");
        }
    }

    #[test]
    fn ordinary_lines_are_kept() {
        let line = r#"time="2026-10-22T13:36:25+08:00" level=info msg="task started" job=build job_id=8288 run_id=5983"#;
        assert_eq!(redact(line), line);
        assert_eq!(redact("work/bd9356900cdd427a"), "work/bd9356900cdd427a", "short hashes stay");
    }
}
