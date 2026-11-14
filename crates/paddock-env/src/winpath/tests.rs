use super::*;
use std::path::Path;

const W: Os = Os::Windows;

fn p(t: &str) -> &Path {
    Path::new(t)
}

fn plain_text(t: &str) -> String {
    plain(p(t), W).to_string_lossy().into_owned()
}

/// A drive path of exactly `units` UTF-16 units, made of valid folder names.
fn drive_path_of(units: usize) -> String {
    let mut s = String::from(r"C:\");
    while s.len() < units {
        let room = units - s.len();
        let name = "a".repeat(room.min(50));
        s.push_str(&name);
        if s.len() < units {
            s.push('\\');
        }
    }
    // A trailing backslash would end a name; the loop above only adds one when more units follow.
    assert_eq!(s.encode_utf16().count(), units);
    s
}

#[test]
fn a_drive_path_is_plain_without_its_prefix() {
    assert_eq!(plain_text(r"\\?\C:\Users\x"), r"C:\Users\x");
    assert_eq!(plain_text(r"\\?\C:\"), r"C:\");
}

#[test]
fn a_unc_path_is_plain_with_two_backslashes() {
    assert_eq!(plain_text(r"\\?\UNC\srv\share\x"), r"\\srv\share\x");
    assert_eq!(plain_text(r"\\?\unc\srv\share"), r"\\srv\share");
}

#[test]
fn a_volume_guid_path_keeps_its_prefix_in_plain() {
    let t = r"\\?\Volume{1b2c3d}\a";
    assert_eq!(plain_text(t), t);
}

#[test]
fn a_device_path_keeps_its_prefix_in_plain() {
    for t in [r"\\?\GLOBALROOT\Device\x", r"\\.\COM3", r"\\?\C:"] {
        assert_eq!(plain_text(t), t);
    }
}

#[test]
fn a_reserved_name_keeps_the_prefix_in_plain() {
    for t in [
        r"\\?\C:\x\con.txt",
        r"\\?\C:\x\NUL",
        r"\\?\C:\x\com1",
        r"\\?\C:\x\com0",
        r"\\?\C:\x\LPT0.txt",
        r"\\?\C:\x\Lpt9.log",
        r"\\?\C:\x\aux.tar.gz",
        r"\\?\C:\x\prn ",
        r"\\?\C:\x\con.",
        "\\\\?\\C:\\x\\COM\u{b9}",
        r"\\?\C:\x\CONIN$",
        r"\\?\UNC\srv\share\con",
    ] {
        assert_eq!(plain_text(t), t, "{t}");
    }
}

#[test]
fn names_that_only_look_reserved_are_stripped() {
    for t in [
        r"\\?\C:\x\console",
        r"\\?\C:\x\com10",
        r"\\?\C:\x\lpt0x",
        r"\\?\C:\x\nul2.txt",
        r"\\?\C:\x\com",
    ] {
        assert!(safe_to_strip(p(t), W, Purpose::Plain), "{t}");
    }
}

#[test]
fn a_bad_name_or_a_dot_component_keeps_the_prefix() {
    for t in [
        r"\\?\C:\x\a.",
        r"\\?\C:\x\a ",
        r"\\?\C:\x\a<b",
        r"\\?\C:\x\a?b",
        r"\\?\C:\x\a/b",
        r"\\?\C:\x\a:b",
        "\\\\?\\C:\\x\\a\u{1}b",
        r"\\?\C:\x\..\y",
        r"\\?\C:\x\.\y",
        r"\\?\C:\x\\y",
        r"\\?\UNC\srv",
        r"\\?\UNC\srv\sh?are\x",
    ] {
        assert!(!safe_to_strip(p(t), W, Purpose::Plain), "{t}");
        assert_eq!(plain_text(t), t, "{t}");
    }
}

#[test]
fn a_trailing_separator_is_allowed() {
    assert_eq!(plain_text(r"\\?\C:\x\y\"), r"C:\x\y\");
}

#[test]
fn a_path_of_260_units_keeps_the_prefix_and_259_loses_it() {
    let ok = drive_path_of(259);
    let long = drive_path_of(260);
    assert_eq!(plain_text(&format!(r"\\?\{ok}")), ok);
    assert_eq!(plain_text(&format!(r"\\?\{long}")), format!(r"\\?\{long}"));
}

#[test]
fn a_new_folder_must_stay_within_247_units() {
    let ok = format!(r"\\?\{}", drive_path_of(247));
    let over = format!(r"\\?\{}", drive_path_of(248));
    assert!(safe_to_strip(p(&ok), W, Purpose::CreateDir));
    assert!(!safe_to_strip(p(&over), W, Purpose::CreateDir));
    assert!(safe_to_strip(p(&over), W, Purpose::Plain));
}

#[test]
fn the_unc_length_counts_the_plain_spelling() {
    // `\\srv\share\` is 12 units; pad the rest.
    let head = r"\\srv\share\";
    let ok = format!("{head}{}", "a".repeat(259 - head.len()));
    let over = format!("{head}{}", "a".repeat(260 - head.len()));
    assert!(safe_to_strip(
        p(&format!(r"\\?\UNC\{}", &ok[2..])),
        W,
        Purpose::Plain
    ));
    assert!(!safe_to_strip(
        p(&format!(r"\\?\UNC\{}", &over[2..])),
        W,
        Purpose::Plain
    ));
}

#[test]
fn units_are_utf16_not_bytes() {
    // 'é' is 2 bytes and 1 unit; an astral char is 4 bytes and 2 units.
    let head = r"C:\";
    let e = format!("{head}{}", "\u{e9}".repeat(256));
    assert_eq!(e.encode_utf16().count(), 259);
    assert!(safe_to_strip(p(&format!(r"\\?\{e}")), W, Purpose::Plain));
    let astral = format!("{head}{}", "\u{1F600}".repeat(129));
    assert_eq!(astral.encode_utf16().count(), 261);
    assert!(!safe_to_strip(
        p(&format!(r"\\?\{astral}")),
        W,
        Purpose::Plain
    ));
}

#[test]
fn off_windows_nothing_is_stripped() {
    for os in [Os::MacOs, Os::Linux] {
        assert_eq!(plain(p(r"\\?\C:\x"), os), p(r"\\?\C:\x"));
        assert!(!safe_to_strip(p(r"\\?\C:\x"), os, Purpose::Plain));
    }
}

#[test]
fn an_ordinary_path_is_not_changed() {
    for t in [r"C:\x", r"\\srv\share\x", "rel/x", r"\x"] {
        assert_eq!(plain_text(t), t);
    }
}

#[test]
fn for_tools_gives_the_plain_spelling_with_forward_slashes() {
    assert_eq!(for_tools(p(r"\\?\C:\work\x"), W).unwrap(), "C:/work/x");
    assert_eq!(
        for_tools(p(r"\\?\UNC\srv\share\x"), W).unwrap(),
        "//srv/share/x"
    );
    assert_eq!(for_tools(p(r"C:\work\x"), W).unwrap(), "C:/work/x");
    assert_eq!(for_tools(p("/work/x"), Os::MacOs).unwrap(), "/work/x");
}

#[test]
fn for_tools_refuses_a_volume_guid_path() {
    let e = for_tools(p(r"\\?\Volume{1b2c}\x"), W).unwrap_err();
    let text = e.to_string();
    assert!(text.contains(r"\\?\Volume{1b2c}\x"), "{text}");
    assert!(text.contains("without a drive letter"), "{text}");
    assert!(text.contains("mount it on a drive letter"), "{text}");
}

#[test]
fn for_tools_refuses_a_reserved_name() {
    let e = for_tools(p(r"\\?\C:\work\con.txt"), W).unwrap_err();
    let text = e.to_string();
    assert!(text.contains(r"\\?\C:\work\con.txt"), "{text}");
    assert!(
        text.contains("`con.txt` is a reserved Windows name"),
        "{text}"
    );
    assert!(text.contains("rename"), "{text}");
}

#[test]
fn for_tools_refuses_a_path_over_259_units() {
    assert!(for_tools(p(&format!(r"\\?\{}", drive_path_of(259))), W).is_ok());
    let e = for_tools(p(&format!(r"\\?\{}", drive_path_of(260))), W).unwrap_err();
    let text = e.to_string();
    assert!(text.contains("260 characters"), "{text}");
    assert!(text.contains("259"), "{text}");
    assert!(text.contains("longpaths"), "{text}");
}

#[test]
fn for_tools_refuses_a_plain_path_over_259_units_too() {
    assert!(for_tools(p(&drive_path_of(259)), W).is_ok());
    let e = for_tools(p(&drive_path_of(260)), W).unwrap_err();
    assert!(e.to_string().contains("260 characters"), "{e}");
    let unc = format!(r"\\srv\share\{}", "a".repeat(260));
    assert!(for_tools(p(&unc), W).is_err());
}

#[test]
fn for_tools_native_keeps_the_backslashes() {
    assert_eq!(for_tools_native(p(r"\\?\C:\a\b"), W).unwrap(), r"C:\a\b");
    assert_eq!(
        for_tools_native(p(r"\\?\UNC\srv\share\x"), W).unwrap(),
        r"\\srv\share\x"
    );
    assert_eq!(for_tools(p(r"\\?\C:\a\b"), W).unwrap(), "C:/a/b");
    assert!(for_tools_native(p(&format!(r"\\?\{}", drive_path_of(300))), W).is_err());
    assert_eq!(for_tools_native(p("/a/b"), Os::Linux).unwrap(), "/a/b");
}

#[test]
fn for_tools_refuses_a_device_path() {
    assert!(for_tools(p(r"\\.\C:\x"), W).is_err());
    assert!(for_tools(p(r"\\?\GLOBALROOT\x"), W).is_err());
}

#[test]
fn same_ignores_the_prefix_even_for_a_reserved_name() {
    assert!(same(p(r"\\?\C:\x\con"), p(r"C:\x\con"), W));
    assert!(same(p(r"\\?\UNC\srv\share\x"), p(r"\\srv\share\x"), W));
}

#[test]
fn same_ignores_case_separators_and_a_trailing_separator_on_windows() {
    assert!(same(p(r"C:\Users\X"), p("c:/users/x/"), W));
    assert!(same(p(r"\\SRV\Share\a"), p(r"\\srv\share\A"), W));
    assert!(same(
        p("\\\\?\\C:\\\u{c9}t\u{e9}"),
        p("c:\\\u{e9}T\u{c9}"),
        W
    ));
    assert!(!same(p(r"C:\a"), p(r"C:\b"), W));
    assert!(!same(p(r"C:\a"), p(r"D:\a"), W));
}

#[test]
fn same_does_not_equate_different_kinds() {
    assert!(!same(p(r"\\?\Volume{a}\x"), p(r"C:\x"), W));
    assert!(same(p(r"\\?\Volume{a}\x"), p(r"\\?\VOLUME{A}\X"), W));
    assert!(!same(p(r"\\srv\share\x"), p(r"C:\srv\share\x"), W));
    assert!(!same(p(r"\x"), p(r"x"), W));
    assert!(!same(p(r"\\.\x"), p(r"x"), W));
}

#[test]
fn same_is_exact_off_windows() {
    assert!(same(p("/a/b"), p("/a/b"), Os::Linux));
    assert!(!same(p("/a/B"), p("/a/b"), Os::Linux));
}

#[test]
fn starts_with_compares_by_component_ignoring_prefix_case_and_length() {
    let long = format!(r"\\?\{}", drive_path_of(300));
    assert!(starts_with(
        p(&long),
        p(&format!("C:\\{}", "a".repeat(50))),
        W
    ));
    assert!(starts_with(p(r"\\?\C:\a\b"), p(r"C:\A"), W));
    assert!(starts_with(p(r"C:\a"), p(r"\\?\C:\a"), W), "equal paths");
    assert!(
        !starts_with(p(r"C:\ab"), p(r"C:\a"), W),
        "a name is not a prefix of a longer name"
    );
    assert!(!starts_with(p(r"D:\a"), p(r"C:\a"), W));
    assert!(!starts_with(p(r"\\srv\share\a"), p(r"C:\srv"), W));
    assert!(starts_with(p(r"\\?\UNC\srv\share\a"), p(r"\\srv\share"), W));
    assert!(starts_with(p("/a/b"), p("/a"), Os::Linux));
    assert!(!starts_with(p("/ab"), p("/a"), Os::Linux));
}

#[test]
fn lexical_text_drops_the_prefix_without_any_limit_or_refusal() {
    let long = format!(r"\\?\{}", drive_path_of(300));
    let t = lexical_text(p(&long), W);
    assert!(
        t.starts_with("C:/aaaa") && !t.contains('\\') && !t.contains('?'),
        "{t}"
    );
    assert_eq!(lexical_text(p(r"\\?\C:\a\con\b"), W), "C:/a/con/b");
    assert_eq!(lexical_text(p(r"\\?\UNC\srv\share\x"), W), "//srv/share/x");
    assert_eq!(lexical_text(p("C:/a/b/"), W), "C:/a/b");
    assert_eq!(lexical_text(p("/a/b"), Os::Linux), "/a/b");
}

#[test]
fn components_start_with_the_server_not_unc() {
    assert_eq!(
        components(p(r"\\?\UNC\srv\share\a"), W),
        ["srv", "share", "a"]
    );
    assert_eq!(components(p(r"\\srv\share\a"), W), ["srv", "share", "a"]);
    assert_eq!(components(p(r"\\?\C:\x\y"), W), ["C:", "x", "y"]);
    assert_eq!(components(p("c:/x//y/"), W), ["c:", "x", "y"]);
    assert_eq!(components(p("/a/b"), Os::Linux), ["/", "a", "b"]);
}

#[test]
fn kind_names_every_spelling() {
    assert_eq!(
        kind(p(r"C:\x"), W),
        Kind::Drive {
            letter: 'C',
            verbatim: false
        }
    );
    assert_eq!(
        kind(p(r"\\?\d:\x"), W),
        Kind::Drive {
            letter: 'd',
            verbatim: true
        }
    );
    assert_eq!(kind(p(r"\\srv\share\x"), W), Kind::Unc { verbatim: false });
    assert_eq!(
        kind(p(r"\\?\UNC\srv\share\x"), W),
        Kind::Unc { verbatim: true }
    );
    assert_eq!(kind(p(r"\\?\Volume{1}\x"), W), Kind::Volume);
    assert_eq!(kind(p(r"\\.\x"), W), Kind::Device);
    assert_eq!(kind(p(r"\\?\GLOBALROOT\x"), W), Kind::Device);
    assert_eq!(kind(p(r"rel\x"), W), Kind::Relative);
    assert_eq!(kind(p(r"\x"), W), Kind::Relative);
    assert_eq!(kind(p(r"C:x"), W), Kind::Relative);
    assert_eq!(kind(p("/a"), Os::Linux), Kind::Unix);
    assert_eq!(kind(p(r"\\?\C:\x"), Os::MacOs), Kind::Unix);
}

#[test]
fn a_not_plain_error_is_one_line_with_the_cause_and_the_next_step() {
    let e = for_tools(p(r"\\?\C:\x\nul"), W).unwrap_err();
    assert_eq!(e.path, p(r"\\?\C:\x\nul"));
    assert!(!e.reason.is_empty() && !e.next_step.is_empty());
    assert!(!e.to_string().contains('\n'));
}

// The real functions run on every OS; off Windows they must not change a path.

#[test]
fn real_resolves_dots_and_links_and_gives_a_usable_path() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("a")).unwrap();
    let dotted = d.path().join("a").join("..").join("a");
    let r = real(&dotted).unwrap();
    assert!(!r.to_string_lossy().contains(".."));
    assert!(r.is_dir());
    assert_eq!(real(&dotted).unwrap(), real(&d.path().join("a")).unwrap());
    assert!(real(&d.path().join("missing")).is_err());
}

#[test]
fn real_or_given_keeps_a_path_that_cannot_be_resolved() {
    let d = tempfile::tempdir().unwrap();
    let missing = d.path().join("nope");
    assert_eq!(real_or_given(&missing), missing);
    std::fs::create_dir(d.path().join("a")).unwrap();
    let dotted = d.path().join("a").join("..").join("a");
    assert_eq!(real_or_given(&dotted), real(&dotted).unwrap());
}

#[test]
fn real_for_tools_and_real_verbatim_agree_with_real_off_windows() {
    let d = tempfile::tempdir().unwrap();
    let t = real_for_tools(d.path()).unwrap();
    assert!(!t.starts_with(r"\\?\"));
    if cfg!(not(windows)) {
        assert_eq!(real_verbatim(d.path()).unwrap(), real(d.path()).unwrap());
        assert_eq!(std::path::Path::new(&t), real(d.path()).unwrap());
    }
    let e = real_for_tools(&d.path().join("missing")).unwrap_err();
    assert!(matches!(e, RealError::Io { .. }));
    assert!(e.to_string().contains("missing"));
}

#[cfg(windows)]
mod on_windows {
    use super::*;

    #[test]
    fn canonicalize_is_verbatim_and_real_is_plain() {
        let d = tempfile::tempdir().unwrap();
        let verbatim = real_verbatim(d.path()).unwrap();
        assert!(
            verbatim.to_string_lossy().starts_with(r"\\?\"),
            "{verbatim:?}"
        );
        let r = real(d.path()).unwrap();
        assert!(!r.to_string_lossy().starts_with(r"\\?\"), "{r:?}");
        assert!(same(&verbatim, &r, Os::Windows));
        let t = real_for_tools(d.path()).unwrap();
        assert!(!t.contains('\\') && !t.starts_with("//?/"), "{t}");
    }

    /// `CompareStringOrdinal(a, b, ignoreCase = TRUE) == CSTR_EQUAL`.
    fn os_equal_ignoring_case(a: &str, b: &str) -> bool {
        use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
        let (a, b): (Vec<u16>, Vec<u16>) = (a.encode_utf16().collect(), b.encode_utf16().collect());
        // SAFETY: both buffers are live for the call and their lengths are passed.
        let r = unsafe {
            CompareStringOrdinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1)
        };
        r == CSTR_EQUAL
    }

    #[test]
    fn the_fold_agrees_with_compare_string_ordinal_on_the_hard_cases() {
        let pairs = [
            ("\u{df}", "SS"),
            ("\u{df}", "\u{1e9e}"),
            ("\u{17f}", "S"),
            ("\u{17f}", "s"),
            ("\u{131}", "I"),
            ("\u{131}", "i"),
            ("\u{130}", "i"),
            ("\u{130}", "I"),
            ("i", "I"),
            ("\u{10428}", "\u{10400}"),
            ("\u{3c2}", "\u{3a3}"),
            ("\u{3c3}", "\u{3a3}"),
            ("\u{212a}", "k"),
            ("\u{e9}", "\u{c9}"),
        ];
        for (a, b) in pairs {
            let (pa, pb) = (format!("C:\\{a}"), format!("C:\\{b}"));
            assert_eq!(
                same(Path::new(&pa), Path::new(&pb), Os::Windows),
                os_equal_ignoring_case(a, b),
                "{a:?} against {b:?}"
            );
        }
    }

    /// `subst X: <folder>` makes a drive that is another name for a folder: `real` resolves it to the folder, and the result is
    /// plain and compares equal with the folder's own path.
    #[test]
    #[ignore = "makes a session-wide drive letter with `subst` (ER-14); run on the Windows CI leg or by hand with --run-ignored"]
    fn a_subst_drive_resolves_to_its_folder() {
        struct Subst(String);
        impl Drop for Subst {
            fn drop(&mut self) {
                let _ = std::process::Command::new("subst")
                    .args([&self.0, "/d"])
                    .status();
            }
        }
        let d = tempfile::tempdir().unwrap();
        let free = ('G'..='Z')
            .rev()
            .map(|l| format!("{l}:"))
            .find(|dr| !Path::new(&format!("{dr}\\")).exists())
            .unwrap();
        let status = std::process::Command::new("subst")
            .arg(&free)
            .arg(d.path())
            .status()
            .unwrap();
        assert!(status.success(), "subst {free} failed");
        let _guard = Subst(free.clone());
        let via_drive = real(Path::new(&format!("{free}\\"))).unwrap();
        let direct = real(d.path()).unwrap();
        assert!(
            !via_drive.to_string_lossy().starts_with(r"\\?\"),
            "{via_drive:?}"
        );
        assert!(
            same(&via_drive, &direct, Os::Windows),
            "{via_drive:?} against {direct:?}"
        );
    }

    #[test]
    fn real_or_given_is_plain_for_an_existing_path() {
        let d = tempfile::tempdir().unwrap();
        let r = real_or_given(d.path());
        assert!(!r.to_string_lossy().starts_with(r"\\?\"), "{r:?}");
    }

    #[test]
    fn the_ordinal_fold_agrees_with_the_os_for_common_letters() {
        // The OS table is CompareStringOrdinal; here the file system is the oracle: a file made with one case opens with the other.
        let d = tempfile::tempdir().unwrap();
        let word_list = [
            "Stra\u{df}e",
            "\u{e9}t\u{e9}",
            "\u{44f}\u{437}\u{44b}\u{43a}",
            "ABC",
            "\u{3c3}\u{3bf}\u{3c6}\u{3af}\u{3b1}",
        ];
        for w in word_list {
            let lower = d.path().join(w.to_lowercase());
            std::fs::write(&lower, b"x").unwrap();
            let upper = d.path().join(w.to_uppercase());
            let opens = std::fs::metadata(&upper).is_ok();
            assert_eq!(same(&lower, &upper, Os::Windows), opens, "{w}");
        }
    }
}

#[cfg(unix)]
#[test]
fn a_name_that_is_not_text_is_left_alone_and_refused_for_programs() {
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;
    let odd = PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', 0xff, b'a']));
    assert_eq!(plain(&odd, W), odd.as_path());
    assert!(!safe_to_strip(&odd, W, Purpose::Plain));
    assert!(same(&odd, &odd, W));
    assert_eq!(kind(&odd, W), Kind::Relative);
    let e = for_tools(&odd, Os::Linux).unwrap_err();
    assert!(e.reason.contains("Unicode") && !e.next_step.is_empty());
    assert_eq!(components(&odd, W).len(), 2);
}

#[test]
fn a_real_error_names_the_path_and_keeps_its_cause() {
    use std::error::Error;
    let e = real_for_tools(Path::new("/definitely/not/here")).unwrap_err();
    assert!(e.to_string().contains("/definitely/not/here"));
    assert!(e.source().is_some());
    let np = RealError::NotPlain(for_tools(p(r"\\?\Volume{1}\x"), W).unwrap_err());
    assert!(np.to_string().contains("drive letter") && np.source().is_some());
}

#[test]
fn an_incomplete_share_and_a_device_without_root_keep_the_prefix() {
    for t in [r"\\?\UNC\srv", r"\\?\UNC\\srv\share", r"\\?\UNC\"] {
        assert!(!safe_to_strip(p(t), W, Purpose::Plain), "{t}");
        assert!(for_tools(p(t), W).is_err(), "{t}");
    }
}

#[test]
fn for_tools_explains_a_bad_name_and_a_dot_part() {
    let bad = for_tools(p(r"\\?\C:\work\a?b"), W).unwrap_err().to_string();
    assert!(
        bad.contains("`a?b` is not a valid Windows name") && bad.contains("rename"),
        "{bad}"
    );
    let dots = for_tools(p(r"\\?\C:\work\..\x"), W)
        .unwrap_err()
        .to_string();
    assert!(
        dots.contains("`.` or `..`") && dots.contains("resolved path"),
        "{dots}"
    );
}

#[test]
fn the_fold_leaves_characters_above_the_basic_plane_alone() {
    // Deseret small and capital letter long i: two UTF-16 units each; CompareStringOrdinal does not fold surrogate pairs.
    assert!(!same(p("C:\\\u{10428}"), p("C:\\\u{10400}"), W));
    assert!(same(p("C:\\\u{10428}"), p("c:\\\u{10428}"), W));
}

#[test]
fn the_fold_keeps_dotless_i_and_long_s_apart_from_the_ascii_letters() {
    assert!(!same(p("C:\\\u{131}"), p(r"C:\I"), W));
    assert!(!same(p("C:\\\u{17f}"), p(r"C:\s"), W));
    assert!(same(p("C:\\\u{131}"), p("c:\\\u{131}"), W));
    // Final sigma is not capital sigma for CompareStringOrdinal (ci.yml run 7969); small sigma is.
    assert!(!same(p("C:\\\u{3c2}"), p("C:\\\u{3a3}"), W));
    assert!(same(p("C:\\\u{3c3}"), p("C:\\\u{3a3}"), W));
}

#[test]
fn the_fold_keeps_a_letter_whose_uppercase_is_two_letters() {
    // `char::to_uppercase` gives "SS" for the sharp s: the fold leaves it alone, as a one-to-one table would.
    assert!(!same(p("C:\\Stra\u{df}e"), p(r"C:\STRASSE"), W));
    assert!(same(p("C:\\Stra\u{df}e"), p("c:\\STRA\u{df}E"), W));
}

#[test]
fn strip_prefix_gives_the_rest_across_spellings_and_none_outside() {
    assert_eq!(
        strip_prefix(p(r"\\?\C:\ws\a\B.toml"), p(r"c:\WS"), W).as_deref(),
        Some("a/B.toml")
    );
    assert_eq!(
        strip_prefix(p(r"C:\ws"), p(r"\\?\C:\ws"), W).as_deref(),
        Some("")
    );
    assert_eq!(strip_prefix(p(r"C:\wsx\a"), p(r"C:\ws"), W), None);
    assert_eq!(strip_prefix(p(r"D:\ws\a"), p(r"C:\ws"), W), None);
    assert_eq!(
        strip_prefix(p("/ws/a/b"), p("/ws"), Os::Linux).as_deref(),
        Some("a/b")
    );
    assert_eq!(strip_prefix(p("/other/a"), p("/ws"), Os::Linux), None);
}

#[test]
fn strip_prefix_refuses_a_remainder_that_can_leave_the_prefix() {
    assert_eq!(strip_prefix(p(r"C:\ws\..\x"), p(r"C:\ws"), W), None);
    assert_eq!(
        strip_prefix(p(r"\\?\C:\ws\a\..\..\x"), p(r"C:\ws"), W),
        None
    );
    assert_eq!(strip_prefix(p(r"C:\ws\.\x"), p(r"C:\ws"), W), None);
    assert_eq!(strip_prefix(p("/ws/../x"), p("/ws"), Os::Linux), None);
    assert_eq!(
        strip_prefix(p(r"C:\ws\a.b\..c"), p(r"C:\ws"), W).as_deref(),
        Some("a.b/..c")
    );
}

#[test]
fn strip_prefix_reads_unc_and_verbatim_unc_paths() {
    assert_eq!(
        strip_prefix(p(r"\\srv\share\a\b"), p(r"\\srv\share"), W).as_deref(),
        Some("a/b")
    );
    assert_eq!(
        strip_prefix(p(r"\\?\UNC\SRV\Share\a\b"), p(r"\\srv\share"), W).as_deref(),
        Some("a/b")
    );
    assert_eq!(
        strip_prefix(p(r"\\srv\share\a"), p(r"\\?\UNC\srv\share"), W).as_deref(),
        Some("a")
    );
    assert_eq!(
        strip_prefix(p(r"\\srv\share"), p(r"\\srv\share"), W).as_deref(),
        Some("")
    );
    assert_eq!(
        strip_prefix(p(r"\\srv\other\a"), p(r"\\srv\share"), W),
        None
    );
    assert_eq!(
        strip_prefix(p(r"\\srv\share\..\x"), p(r"\\srv\share"), W),
        None
    );
    assert_eq!(
        strip_prefix(p(r"C:\srv\share\a"), p(r"\\srv\share"), W),
        None
    );
}

#[test]
fn from_git_bash_reads_a_one_letter_first_part_as_a_drive_on_windows_only() {
    assert_eq!(
        from_git_bash("/c/x", Os::Linux),
        "/c/x",
        "off Windows `/c/x` is a folder of that name"
    );
    assert_eq!(from_git_bash("/c/x", Os::MacOs), "/c/x");
    assert_eq!(from_git_bash("/c/x", W), "C:/x");
    assert_eq!(from_git_bash("/C/Users/x/", W), "C:/Users/x/");
    assert_eq!(
        from_git_bash("/cc/x", W),
        "/cc/x",
        "only a one-letter first part is a drive"
    );
    assert_eq!(from_git_bash("/usr", W), "/usr");
    assert_eq!(
        from_git_bash("/c", W),
        "C:/",
        "a drive's root, not the drive-relative C:"
    );
    assert_eq!(from_git_bash("/c/", W), "C:/");
    assert_eq!(from_git_bash("/c", Os::Linux), "/c");
    assert_eq!(from_git_bash("C:/x", W), "C:/x");
    assert_eq!(from_git_bash(r"C:\x", W), r"C:\x");
    assert_eq!(from_git_bash("", W), "");
    assert_eq!(
        from_git_bash("/1/x", W),
        "/1/x",
        "a digit is not a drive letter"
    );
}
