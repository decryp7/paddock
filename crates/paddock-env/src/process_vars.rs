//! This process's environment as text, without ever panicking on a variable that is not UTF-8.
//!
//! `std::env::vars()` panics on the first name or value that is not valid Unicode (a Unix user can have one, and a CI job inherits
//! what its runner sets), which would end a tool before it printed a line. [`vars`] skips such a pair and warns once on standard
//! error with the variable's name: a variable that cannot be read as text is one no caller of this reader reads either.
//! Clippy forbids `std::env::vars` in every Paddock workspace (`lint/clippy.toml`), so this is the one reader of the whole
//! environment as text.

use std::ffi::{OsStr, OsString};

/// The variables of this process whose name and value are both valid Unicode; a pair that is not is skipped with a warning on
/// standard error that names the variable.
pub fn vars() -> impl Iterator<Item = (String, String)> {
    vars_skipping(std::env::vars_os(), |name| {
        eprintln!("warning: the environment variable {name:?} is not valid Unicode and is skipped");
    })
}

/// `vars_os`-shaped pairs as text; `skipped` is told the name of each pair left out (a test gives its own pairs).
pub fn vars_skipping(
    pairs: impl Iterator<Item = (OsString, OsString)>,
    mut skipped: impl FnMut(&OsStr),
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

    #[cfg(unix)]
    fn not_unicode() -> OsString {
        std::os::unix::ffi::OsStringExt::from_vec(vec![b'B', 0xff, b'D'])
    }
    #[cfg(windows)]
    fn not_unicode() -> OsString {
        std::os::windows::ffi::OsStringExt::from_wide(&[0x42, 0xD800, 0x44])
    }

    #[test]
    fn a_pair_that_is_not_unicode_is_skipped_and_named_and_the_rest_is_kept() {
        let bad = not_unicode();
        let pairs = vec![
            (OsString::from("GOOD"), OsString::from("1")),
            (OsString::from("BADVALUE"), bad.clone()),
            (bad.clone(), OsString::from("x")),
            (OsString::from("ALSO_GOOD"), OsString::from("2")),
        ];
        let mut named = Vec::new();
        let got: Vec<(String, String)> =
            vars_skipping(pairs.into_iter(), |n| named.push(n.to_os_string())).collect();
        assert_eq!(
            got,
            vec![
                ("GOOD".to_string(), "1".to_string()),
                ("ALSO_GOOD".to_string(), "2".to_string())
            ]
        );
        assert_eq!(named.len(), 2, "{named:?}");
    }

    #[test]
    fn the_process_reader_has_the_variables_it_can_read() {
        assert!(vars().any(|(k, _)| k == "PATH") || cfg!(windows));
    }
}
