use paddock_env::Os;
use paddock_testkit::assert_same_path;
use paddock_testkit::path::{
    abs, abs_on, same_path_from_shell_on, same_path_on, text, text_on, under,
};
use std::path::{Path, PathBuf};

#[test]
fn windows_spellings_of_one_place_are_the_same_path() {
    let w = Os::Windows;
    for other in [r"C:\a\b", "C:/a/b", r"c:\A\B", r"\\?\C:\a\b", "C:/a/b/"] {
        assert!(same_path_on("C:/a/b", other, w), "{other}");
        assert!(same_path_on(other, "C:/a/b", w), "{other}");
    }
    assert!(!same_path_on("C:/a/b", "C:/a/c", w));
    assert!(!same_path_on("/c/a", "/d/a", w));
}

/// Run 8034: a Git bash path in the product's output is a bug, not a spelling to forgive.
#[test]
fn a_git_bash_path_as_the_actual_side_is_not_the_drive_path_but_as_the_expected_side_it_is() {
    let w = Os::Windows;
    assert!(
        !same_path_on("/c/x", "C:/x", w),
        "actual `/c/x` must not equal `C:/x`"
    );
    assert!(!same_path_on("/c/x", r"C:\x", w));
    assert!(
        same_path_on("C:/x", "/c/x", w),
        "the expected side may be written in Git bash's spelling"
    );
    assert!(
        same_path_from_shell_on("/c/x", "C:/x", w),
        "output that really comes from Git bash is read too"
    );
    assert!(same_path_from_shell_on("/c/x", "/c/x", w));
    assert!(!same_path_from_shell_on("/c/x", "C:/y", w));
}

#[test]
fn a_drive_root_is_spelled_with_its_slash() {
    assert_eq!(text_on(r"C:\", Os::Windows), "C:/");
    assert_eq!(text_on("C:/", Os::Windows), "C:/");
    assert_eq!(text_on(r"c:\a", Os::Windows), "c:/a");
    assert_eq!(text_on("/", Os::Linux), "/");
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

#[test]
fn an_absolute_path_from_parts_is_rooted_on_every_os_and_has_no_text_to_split() {
    assert_eq!(
        text_on(abs_on(["srv", "dt"], Os::Windows), Os::Windows),
        "C:/srv/dt"
    );
    // conformance: abs-path — the expected value of the Unix spelling itself, compared as a value on every OS
    assert_eq!(abs_on(["srv", "dt"], Os::Linux), PathBuf::from("/srv/dt"));
    // conformance: abs-path — the same, for macOS
    assert_eq!(abs_on(["srv", "dt"], Os::MacOs), PathBuf::from("/srv/dt"));
    // No parts: the root itself, which validators must refuse as a folder.
    assert_eq!(
        text_on(abs_on([] as [&str; 0], Os::Windows), Os::Windows),
        "C:/"
    );
    assert_eq!(abs_on([] as [&str; 0], Os::Linux), PathBuf::from("/"));
    // For the running OS: absolute here, which a Unix literal is not on Windows.
    assert!(abs(["srv", "dt"]).is_absolute());
    assert_same_path!(abs(["a", "b"]), under(abs([] as [&str; 0]), "a/b"));
}
