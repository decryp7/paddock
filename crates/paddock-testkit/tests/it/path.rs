use paddock_env::Os;
use paddock_testkit::assert_same_path;
use paddock_testkit::path::{same_path_on, text, under};
use std::path::{Path, PathBuf};

#[test]
fn windows_spellings_of_one_place_are_the_same_path() {
    let w = Os::Windows;
    for other in [
        r"C:\a\b",
        "C:/a/b",
        r"c:\A\B",
        r"\\?\C:\a\b",
        "/c/a/b",
        "C:/a/b/",
    ] {
        assert!(same_path_on("C:/a/b", other, w), "{other}");
        assert!(same_path_on(other, "C:/a/b", w), "{other}");
    }
    assert!(!same_path_on("C:/a/b", "C:/a/c", w));
    assert!(!same_path_on("/c/a", "/d/a", w));
}

#[test]
fn off_windows_the_comparison_is_exact() {
    for os in [Os::Linux, Os::MacOs] {
        assert!(same_path_on("/c/a", "/c/a", os));
        assert!(
            !same_path_on("/c/a", "C:/a", os),
            "a Git bash spelling is read only on Windows"
        );
        assert!(!same_path_on("/a/B", "/a/b", os));
    }
}

#[test]
fn text_is_the_tools_spelling_and_under_joins_by_component() {
    let base = PathBuf::from(if cfg!(windows) { r"C:\w" } else { "/w" });
    let p = under(&base, "a/b//c");
    assert_eq!(p, base.join("a").join("b").join("c"));
    let t = text(&p);
    assert!(t.ends_with("w/a/b/c"), "{t}");
    assert!(!t.contains('\\'), "{t}");
}

#[test]
fn assert_same_path_passes_for_the_same_place_and_shows_both_spellings_when_not() {
    let here = Path::new(".");
    assert_same_path!(here, PathBuf::from("."));
    let msg =
        std::panic::catch_unwind(|| assert_same_path!("/x/a", "/x/b")).expect_err("must fail");
    let msg = msg.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(
        msg.contains("actual:")
            && msg.contains("/x/a")
            && msg.contains("expected:")
            && msg.contains("/x/b"),
        "{msg}"
    );
}
