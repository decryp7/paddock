mod common;
use common::*;
use paddock_packs::user_regex::{RegexError, RegexLimits, compile, compile_set, compile_with};
use paddock_packs::{Limits, Origin, PackError, parse_pack};

#[test]
fn compiles_within_limits() {
    assert!(compile(r"(?P<a>\d+)", false).unwrap().is_match("42"));
    assert!(compile("abc", true).unwrap().is_match("ABC"));
    assert!(!compile("abc", false).unwrap().is_match("ABC"));
}

#[test]
fn a_pattern_over_the_length_cap_is_refused_with_both_numbers() {
    let e = compile(&"a".repeat(4097), false).unwrap_err();
    assert_eq!(
        e,
        RegexError::TooLong {
            len: 4097,
            max: 4096
        }
    );
    assert!(e.to_string().contains("too long"));
    let tiny = RegexLimits {
        max_pattern_len: 3,
        ..RegexLimits::DEFAULT
    };
    assert!(compile_with("abcd", false, &tiny).is_err());
    assert!(compile_with("abc", false, &tiny).is_ok());
}

#[test]
fn a_pattern_whose_program_is_too_big_is_refused() {
    let e = compile_with(r"\w{1000}\w{1000}", false, &RegexLimits::STRICT).unwrap_err();
    assert_eq!(e, RegexError::TooComplex);
    assert!(e.to_string().contains("simplify"));
    assert!(compile(r"((((a{100}){100}){100}){100})", false).is_err());
}

#[test]
fn an_invalid_pattern_says_why_in_one_line() {
    let e = compile("(", false).unwrap_err();
    let RegexError::Invalid(m) = &e else {
        panic!("{e:?}")
    };
    assert!(!m.contains('\n') && !m.is_empty(), "{m}");
    assert!(
        e.to_string()
            .starts_with("The pattern is not a valid regular expression")
    );
}

#[test]
fn a_catastrophic_looking_pattern_is_fine_because_matching_is_linear() {
    let re = compile(r"(a+)+$", false).unwrap();
    assert!(!re.is_match(&format!("{}!", "a".repeat(50_000))));
}

#[test]
fn sets_compile_under_the_same_caps() {
    let set = compile_set(&["^a".into(), "b$".into()], &RegexLimits::DEFAULT).unwrap();
    assert_eq!(set.matches("ab").iter().collect::<Vec<_>>(), [0, 1]);
    assert!(matches!(
        compile_set(&["(".into()], &RegexLimits::DEFAULT),
        Err(RegexError::Invalid(_))
    ));
    assert!(matches!(
        compile_set(&["x".repeat(5000)], &RegexLimits::DEFAULT),
        Err(RegexError::TooLong { .. })
    ));
    assert_eq!(
        compile_set(&[r"\w{1000}\w{1000}".into()], &RegexLimits::STRICT).unwrap_err(),
        RegexError::TooComplex
    );
}

#[test]
fn a_malicious_regex_in_a_pack_refuses_that_pack_only() {
    let evil = "format = 2\n[[scheme]]\nid = 'evil'\npattern = '\\w{1000}\\w{1000}'\n";
    let e = parse_pack::<Schemes>(evil.as_bytes(), Origin::User, &Limits::default()).unwrap_err();
    assert!(
        matches!(&e, PackError::Rejected(m) if m.contains("evil") && m.contains("too complex")),
        "{e:?}"
    );
    let long = format!(
        "format = 2\n[[scheme]]\nid = 'long'\npattern = '{}'\n",
        "a".repeat(4096)
    );
    // 4096 bytes is at the cap, the pack's own string limit (4096) is not exceeded.
    assert!(parse_pack::<Schemes>(long.as_bytes(), Origin::User, &Limits::default()).is_ok());
    let longer = format!(
        "format = 2\n[[scheme]]\nid = 'long'\npattern = '{}'\n",
        "a".repeat(4097)
    );
    assert!(matches!(
        parse_pack::<Schemes>(longer.as_bytes(), Origin::User, &Limits::default()),
        Err(PackError::Limit(_))
    ));
}

#[test]
fn the_default_limits_are_debriefs_and_strict_is_spotters() {
    assert_eq!(RegexLimits::default(), RegexLimits::DEFAULT);
    assert_eq!(
        (
            RegexLimits::DEFAULT.size_limit,
            RegexLimits::STRICT.size_limit
        ),
        (8 << 20, 1 << 20)
    );
}
