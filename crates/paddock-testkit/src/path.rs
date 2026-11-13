//! Comparing and spelling paths in tests (ADR 0099), on [`paddock_env::winpath`].
//!
//! A test that builds an expected path as text, or compares path text, in the spelling of the OS it was written on fails on
//! another OS although the product is right: Windows spells one place with `\` or `/`, with or without `\\?\`, as `C:` or `c:`,
//! and Git bash prints it as `/c/...`. Compare with [`same_path`] or [`assert_same_path!`], build expected text with [`text`]
//! and expected paths with [`under`]; never with `format!("{}/x", p.display())` or `.replace('\\', "/")` (the check C119 refuses
//! both in test code).
//!
//! A `PATH`-like list is not covered: build it with `std::env::join_paths`, never by hand with `:` (Windows uses `;`).

use paddock_env::Os;
use paddock_env::winpath;
use std::path::{Path, PathBuf};

/// Whether `actual` and `expected` name the same place on `os`: [`winpath::same`] (separators, `\\?\`, case) after reading a Git
/// bash spelling (`/c/x`) of either side as `C:/x`. Off Windows the comparison is exact. Lexical: nothing is opened.
pub fn same_path_on(actual: impl AsRef<Path>, expected: impl AsRef<Path>, os: Os) -> bool {
    let read = |p: &Path| match p.to_str() {
        Some(t) => PathBuf::from(winpath::from_git_bash(t, os)),
        None => p.to_path_buf(),
    };
    winpath::same(&read(actual.as_ref()), &read(expected.as_ref()), os)
}

/// [`same_path_on`] for the running OS.
pub fn same_path(actual: impl AsRef<Path>, expected: impl AsRef<Path>) -> bool {
    same_path_on(actual, expected, Os::current())
}

/// `path` as the tools print it: `/` separators and no `\\?\` prefix on Windows ([`winpath::lexical_text`]), the text itself
/// elsewhere. For building an expected line that a program printed.
pub fn text(path: impl AsRef<Path>) -> String {
    winpath::lexical_text(path.as_ref(), Os::current())
}

/// `base` joined with `rel` component by component; `rel` is written with `/` on every OS (`"a/b"`), and empty parts are skipped.
pub fn under(base: impl AsRef<Path>, rel: &str) -> PathBuf {
    rel.split('/')
        .filter(|c| !c.is_empty())
        .fold(base.as_ref().to_path_buf(), |p, c| p.join(c))
}

/// The panic message of [`assert_same_path!`]: both spellings as given and as compared.
#[doc(hidden)]
pub fn mismatch(actual: &Path, expected: &Path, os: Os) -> String {
    format!(
        "paths differ\n  actual:   {}  (as text: {})\n  expected: {}  (as text: {})",
        actual.display(),
        winpath::lexical_text(actual, os),
        expected.display(),
        winpath::lexical_text(expected, os),
    )
}

/// Asserts that two paths name the same place ([`same_path`]); on failure prints both spellings and the normalised forms.
#[macro_export]
macro_rules! assert_same_path {
    ($actual:expr, $expected:expr $(,)?) => {{
        let actual = &$actual;
        let expected = &$expected;
        let a: &::std::path::Path = ::std::convert::AsRef::as_ref(actual);
        let e: &::std::path::Path = ::std::convert::AsRef::as_ref(expected);
        if !$crate::path::same_path(a, e) {
            panic!(
                "{}",
                $crate::path::mismatch(a, e, ::paddock_env::Os::current())
            );
        }
    }};
}
