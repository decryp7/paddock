//! Redacting secrets from request text before the inspector keeps it (opt-in, local and bounded, ADR 0001). A prompt may
//! contain API keys or tokens the user pasted; they are replaced with `[redacted]` so the stored copy cannot leak them.
//! This is a safety net, not a guarantee: the inspector is off by default.

/// What replaces a secret.
pub const MASK: &str = "[redacted]";

/// Prefixes that start well-known secret formats, with the shortest total length that still looks like one.
const PREFIXES: &[(&str, usize)] = &[
    ("pw_", 24),
    ("sk-", 20),
    ("sk_", 20),
    ("ghp_", 24),
    ("gho_", 24),
    ("github_pat_", 30),
    ("hf_", 20),
    ("xox", 20),
    ("AKIA", 20),
    ("AIza", 30),
    ("eyJ", 40),
];

fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+' | '/')
}

fn is_secret(word: &str) -> bool {
    if PREFIXES.iter().any(|(p, min)| word.starts_with(p) && word.len() >= *min) {
        return true;
    }
    // Long opaque strings: 40+ characters with letters and digits mixed (hashes, base64 keys), not plain words.
    word.len() >= 40 && word.chars().any(|c| c.is_ascii_digit()) && word.chars().any(|c| c.is_ascii_alphabetic())
}

/// `text` with secrets masked. `Bearer <token>` and `Authorization: <value>` lose the value whatever it looks like.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let mut after_bearer = false;
    let flush = |word: &mut String, out: &mut String, after_bearer: &mut bool| {
        if word.is_empty() {
            return;
        }
        // Punctuation at the end belongs to the sentence, not to the token.
        let core = word.trim_end_matches(['.', '-', '/', '+']);
        if *after_bearer || is_secret(core) {
            out.push_str(MASK);
            out.push_str(&word[core.len()..]);
        } else {
            out.push_str(word);
        }
        *after_bearer = word.eq_ignore_ascii_case("bearer");
        word.clear();
    };
    for c in text.chars() {
        if token_char(c) {
            word.push(c);
        } else {
            flush(&mut word, &mut out, &mut after_bearer);
            if !c.is_whitespace() {
                after_bearer = false;
            }
            out.push(c);
        }
    }
    flush(&mut word, &mut out, &mut after_bearer);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_tokens_are_masked_ordinary_text_is_not() {
        let key = format!("pw_{}", "A1b2C3d4".repeat(5));
        assert_eq!(redact(&format!("use {key} here")), "use [redacted] here");
        assert_eq!(redact("key=sk-abcdefghijklmnopqrstuvwxyz123456 ok"), "key=[redacted] ok");
        assert_eq!(redact("token ghp_abcdefghijklmnopqrstuvwxyz0123456789"), "token [redacted]");
        assert_eq!(redact("Authorization: Bearer abc.def"), "Authorization: Bearer [redacted]");
        assert_eq!(redact("hello world, this is fine: 12345."), "hello world, this is fine: 12345.");
        assert_eq!(redact("sk-short"), "sk-short", "too short to be a key");
        assert_eq!(redact(""), "");
    }

    #[test]
    fn long_opaque_strings_are_masked_long_words_are_not() {
        let hash = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        assert_eq!(redact(&format!("sha {hash}.")), "sha [redacted].");
        assert_eq!(
            redact("Pneumonoultramicroscopicsilicovolcanoconiosis-Pneumonoultramicroscopic"),
            "Pneumonoultramicroscopicsilicovolcanoconiosis-Pneumonoultramicroscopic"
        );
    }

    #[test]
    fn non_ascii_text_survives() {
        assert_eq!(redact("héllo wörld 日本語 pw_x"), "héllo wörld 日本語 pw_x");
    }
}
