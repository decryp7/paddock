//! This process's environment as text, without ever panicking on a variable that is not UTF-8.
//!
//! `std::env::vars()` panics on the first name or value that is not valid Unicode (a Unix user can have one, and a CI job inherits
//! what its runner sets), which would end a gate before it printed a line. [`vars`] skips such a variable, and says so once on
//! standard error: a variable the tool cannot read as text is one no step of this tool reads either, and a child gets the
//! variables it is given through the clearing function (`paddock_env::clear_env_from`) from this same map. Clippy forbids
//! `std::env::vars` in this crate, so this is the one reader.

/// The variables of this process whose name and value are both valid Unicode.
pub fn vars() -> impl Iterator<Item = (String, String)> {
    skipping(std::env::vars_os(), |name| {
        eprintln!("warning: the environment variable {name:?} is not valid Unicode: this tool does not read it");
    })
}

/// `vars_os`-shaped pairs as text; `skipped` is told the name of each pair left out.
pub fn skipping(
    pairs: impl Iterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
    mut skipped: impl FnMut(&std::ffi::OsStr),
) -> impl Iterator<Item = (String, String)> {
    pairs.filter_map(move |(k, v)| match (k.to_str(), v.to_str()) {
        (Some(k), Some(v)) => Some((k.to_string(), v.to_string())),
        _ => {
            skipped(&k);
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn a_pair_that_is_not_unicode_is_skipped_and_named_and_the_rest_is_kept() {
        let bad = bytes_to_os(&[b'B', 0xff, b'D']);
        let pairs = vec![
            (OsString::from("GOOD"), OsString::from("1")),
            (OsString::from("BADVALUE"), bad.clone()),
            (bad.clone(), OsString::from("x")),
            (OsString::from("ALSO_GOOD"), OsString::from("2")),
        ];
        let mut named = Vec::new();
        let got: Vec<(String, String)> = skipping(pairs.into_iter(), |n| named.push(n.to_os_string())).collect();
        assert_eq!(
            got,
            vec![("GOOD".to_string(), "1".to_string()), ("ALSO_GOOD".to_string(), "2".to_string())]
        );
        assert_eq!(named.len(), 2, "{named:?}");
    }

    #[cfg(unix)]
    fn bytes_to_os(b: &[u8]) -> OsString {
        std::os::unix::ffi::OsStringExt::from_vec(b.to_vec())
    }
    #[cfg(windows)]
    fn bytes_to_os(_: &[u8]) -> OsString {
        std::os::windows::ffi::OsStringExt::from_wide(&[0x42, 0xD800, 0x44])
    }
}
