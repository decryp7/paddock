//! Comparing and spelling paths in tests (ADR 0099), on [`paddock_env::winpath`].
//!
//! A test that builds an expected path as text, or compares path text, in the spelling of the OS it was written on fails on
//! another OS although the product is right: Windows spells one place with `\` or `/`, with or without `\\?\`, as `C:` or `c:`,
//! and Git bash prints it as `/c/...`. Compare with [`same_path`] or [`assert_same_path!`], build expected text with [`text`]
//! and expected paths with [`under`]; never with `format!("{}/x", p.display())` or `.replace('\\', "/")` (the check C122 refuses
//! both in test code).
//!
//! A `PATH`-like list is not covered: build it with `std::env::join_paths`, never by hand with `:` (Windows uses `;`).

use paddock_env::Os;
use paddock_env::winpath;
use std::path::{Path, PathBuf};

/// A Git bash spelling (`/c/x`) read as `C:/x` on Windows, a path read as it is elsewhere.
fn read_bash(p: &Path, os: Os) -> PathBuf {
    match p.to_str() {
        Some(t) => PathBuf::from(winpath::from_git_bash(t, os)),
        None => p.to_path_buf(),
    }
}

/// Whether `actual` and `expected` name the same place on `os`: [`winpath::same`] (separators, `\\?\`, case). Only the EXPECTED
/// side may be written as a Git bash path (`/c/x` reads as `C:/x` on Windows): the product's own output in that spelling is a bug
/// this must show (run 8034: a path that Windows cannot open), so an `actual` of `/c/x` is not `C:/x`. For output that really comes
/// from Git bash use [`same_path_from_shell`]. Off Windows the comparison is exact. Lexical: nothing is opened.
pub fn same_path_on(actual: impl AsRef<Path>, expected: impl AsRef<Path>, os: Os) -> bool {
    winpath::same(actual.as_ref(), &read_bash(expected.as_ref(), os), os)
}

/// [`same_path_on`] for the running OS.
pub fn same_path(actual: impl AsRef<Path>, expected: impl AsRef<Path>) -> bool {
    same_path_on(actual, expected, Os::current())
}

/// [`same_path_on`] for an `actual` that a shell printed (Git bash's `pwd`, `$HOME`, `$RUNNER_TOOL_CACHE`): its Git bash spelling is
/// read as the drive path too.
pub fn same_path_from_shell_on(
    actual_from_shell: impl AsRef<Path>,
    expected: impl AsRef<Path>,
    os: Os,
) -> bool {
    same_path_on(read_bash(actual_from_shell.as_ref(), os), expected, os)
}

/// [`same_path_from_shell_on`] for the running OS.
pub fn same_path_from_shell(
    actual_from_shell: impl AsRef<Path>,
    expected: impl AsRef<Path>,
) -> bool {
    same_path_from_shell_on(actual_from_shell, expected, Os::current())
}

/// `path` as the tools print it on `os`: `/` separators and no `\\?\` prefix on Windows ([`winpath::lexical_text`]), a drive's root as
/// `C:/` (never the drive-relative `C:`), the text itself elsewhere.
pub fn text_on(path: impl AsRef<Path>, os: Os) -> String {
    let t = winpath::lexical_text(path.as_ref(), os);
    if os == Os::Windows && t.len() == 2 && t.ends_with(':') {
        format!("{t}/")
    } else {
        t
    }
}

/// [`text_on`] for the running OS. For building an expected line that a program printed.
pub fn text(path: impl AsRef<Path>) -> String {
    text_on(path, Os::current())
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
