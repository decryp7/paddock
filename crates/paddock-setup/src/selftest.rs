//! `self-test`: what the ADR 0039 artifact gate reads from a built `paddock-setup.exe` (the app's identifier, product, main
//! binary and the update key's fingerprint) so a release gate can compare them with the app's configuration.

use base64::Engine;

/// The include's version string, bumped when the installer include's contract changes (the gate also checks the include).
pub const SETUP_VERSION: &str = "1";

/// The minisign key id (16 uppercase hex digits, as `minisign -V` shows it) of Tauri's base64 public key text, `None` when
/// the text is not a minisign public key.
pub fn key_fingerprint(key_text: &str) -> Option<String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let outer = b64.decode(key_text.trim()).ok()?;
    let outer = String::from_utf8(outer).ok()?;
    let inner = outer.lines().nth(1)?;
    let raw = b64.decode(inner.trim()).ok()?;
    if raw.len() < 10 || &raw[..2] != b"Ed" {
        return None;
    }
    Some(
        raw[2..10]
            .iter()
            .rev()
            .map(|b| format!("{b:02X}"))
            .collect(),
    )
}

/// The lines `self-test` prints: `name=value`, one per line.
pub fn report(b: &crate::Baked) -> Vec<String> {
    let or_none = |v: Option<&str>| v.unwrap_or("none").to_string();
    let fingerprint = match b.update_key {
        None => "none".to_string(),
        Some(k) => key_fingerprint(k).unwrap_or_else(|| "invalid".to_string()),
    };
    vec![
        format!("setup_version={SETUP_VERSION}"),
        format!("identifier={}", or_none(b.identifier)),
        format!("product={}", or_none(b.product)),
        format!("main={}", or_none(b.main)),
        format!(
            "update_key={}",
            if b.update_key.is_some() {
                "present"
            } else {
                "none"
            }
        ),
        format!("key_fingerprint={fingerprint}"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: [u8; 8]) -> String {
        let mut raw = vec![b'E', b'd'];
        raw.extend(id);
        raw.extend([7u8; 32]);
        let inner = base64::engine::general_purpose::STANDARD.encode(raw);
        base64::engine::general_purpose::STANDARD.encode(format!(
            "untrusted comment: minisign public key: X\n{inner}\n"
        ))
    }

    #[test]
    fn fingerprint_is_the_key_id_most_significant_byte_first() {
        assert_eq!(
            key_fingerprint(&key([0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF])).as_deref(),
            Some("EFCDAB8967452301")
        );
    }

    #[test]
    fn not_a_key_has_no_fingerprint() {
        assert_eq!(key_fingerprint(""), None);
        assert_eq!(key_fingerprint("!!!"), None);
        assert_eq!(
            key_fingerprint(&base64::engine::general_purpose::STANDARD.encode("only one line")),
            None
        );
        let short = base64::engine::general_purpose::STANDARD.encode("c\nQUJD\n");
        assert_eq!(key_fingerprint(&short), None);
    }

    #[test]
    fn report_names_everything_baked_in() {
        let b = crate::Baked {
            identifier: Some("com.decryp7.dyno"),
            product: Some("Dyno"),
            main: Some("dyno.exe"),
            update_key: None,
        };
        assert_eq!(
            report(&b),
            [
                "setup_version=1",
                "identifier=com.decryp7.dyno",
                "product=Dyno",
                "main=dyno.exe",
                "update_key=none",
                "key_fingerprint=none"
            ]
        );
        let k: &'static str = Box::leak(key([1, 2, 3, 4, 5, 6, 7, 8]).into_boxed_str());
        let b2 = crate::Baked {
            update_key: Some(k),
            ..b
        };
        let r = report(&b2);
        assert_eq!(r[4], "update_key=present");
        assert_eq!(r[5], "key_fingerprint=0807060504030201");
        let b3 = crate::Baked {
            identifier: None,
            product: None,
            main: None,
            update_key: Some("garbage"),
        };
        let r3 = report(&b3);
        assert_eq!(&r3[1..4], ["identifier=none", "product=none", "main=none"]);
        assert_eq!(r3[5], "key_fingerprint=invalid");
    }
}
