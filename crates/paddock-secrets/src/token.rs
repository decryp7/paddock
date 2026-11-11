use crate::SecretsError;

/// The most random bytes one token holds.
pub const MAX_TOKEN_BYTES: usize = 64;

/// A token of `bytes` random bytes from the operating system, as lowercase hex (`2 * bytes` characters): an id nobody can
/// guess (a pairing id needs 16), a secret to hand over once (32). Fails closed: when the OS random source fails there is
/// no token and no time-seeded stand-in, so a caller never ends up with a guessable secret. `bytes` is 1 to
/// [`MAX_TOKEN_BYTES`].
pub fn random_token(bytes: usize) -> Result<String, SecretsError> {
    random_token_with(bytes, |b| getrandom::fill(b).map_err(|e| e.to_string()))
}

/// [`random_token`] over a given source (tests inject a failing one). Whatever a failing source wrote is discarded.
pub fn random_token_with(
    bytes: usize,
    fill: impl FnOnce(&mut [u8]) -> Result<(), String>,
) -> Result<String, SecretsError> {
    if bytes == 0 || bytes > MAX_TOKEN_BYTES {
        return Err(SecretsError::InvalidValue(format!(
            "a token has 1 to {MAX_TOKEN_BYTES} random bytes, not {bytes}"
        )));
    }
    let mut buf = zeroize::Zeroizing::new(vec![0u8; bytes]);
    fill(&mut buf).map_err(|e| {
        SecretsError::Unavailable(format!(
            "the operating system's random source failed ({e}), so no token can be made; restart the app, and if it keeps failing check the system's entropy source"
        ))
    })?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_hex_of_the_asked_length_and_differs_each_time() {
        let (a, b) = (random_token(16).unwrap(), random_token(16).unwrap());
        assert_eq!(a.len(), 32);
        assert!(
            a.bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
        assert_ne!(a, b);
        assert_eq!(random_token(32).unwrap().len(), 64);
    }

    #[test]
    fn a_failing_source_gives_an_error_and_no_token_even_after_a_partial_fill() {
        let e = random_token_with(16, |b| {
            b[0] = 7;
            Err("no entropy".into())
        })
        .unwrap_err();
        assert!(matches!(e, SecretsError::Unavailable(_)), "{e:?}");
        assert!(
            e.to_string().contains("no entropy") && e.to_string().contains("no token can be made"),
            "{e}"
        );
    }

    #[test]
    fn a_length_of_zero_or_above_the_limit_is_refused() {
        assert!(random_token(0).is_err());
        assert!(random_token(MAX_TOKEN_BYTES + 1).is_err());
        assert!(random_token(MAX_TOKEN_BYTES).is_ok());
    }
}
