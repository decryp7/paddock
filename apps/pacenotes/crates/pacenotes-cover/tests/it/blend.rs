//! Mapping, detection, the source port, and blending runs.

use pacenotes_cover::import::import_from;
use pacenotes_cover::{BlendError, CoverageSet, Format, ImportContext, ImportError, PathMapper, ReportSource, Tool};

use crate::support::{COBERTURA, ISTANBUL, JACOCO, LCOV, LLVM, ci_mapper, p, read, rev};

#[test]
fn a_path_is_mapped_into_the_repository_or_left_out() {
    let m = PathMapper::new().strip_prefix("/home/ci/build").strip_prefix("D:\\agent\\work");
    assert_eq!(m.map("/home/ci/build/src/a.rs"), Some(p("src/a.rs")));
    assert_eq!(
        m.map("d:/agent/work\\src\\b.rs"),
        Some(p("src/b.rs")),
        "separators and the case of a drive letter"
    );
    assert_eq!(m.map("./src/c.rs"), Some(p("src/c.rs")));
    for outside in [
        "/usr/include/x.h",
        "C:\\Windows\\x.h",
        "../escape.rs",
        "/home/ci/build/../etc/x",
        "",
    ] {
        assert_eq!(m.map(outside), None, "{outside}");
    }
    assert_eq!(m.map_dir("/home/ci/build/app/"), Some("app/".to_owned()));
    assert_eq!(m.map_dir("/home/ci/build"), Some(String::new()));
    assert_eq!(m.map_dir("app"), Some("app/".to_owned()));
    assert_eq!(m.map_dir("/elsewhere"), None);
    assert_eq!(m.map_dir("/home/ci/build/../x"), None);
}

#[test]
fn the_format_is_detected_from_the_text_and_a_source_hands_over_a_report() {
    for (text, format) in [
        (LCOV, Format::Lcov),
        (COBERTURA, Format::Cobertura),
        (JACOCO, Format::JaCoCo),
        (LLVM, Format::LlvmCov),
        (ISTANBUL, Format::Istanbul),
    ] {
        assert_eq!(Format::detect(text), Some(format));
    }
    assert_eq!(Format::detect("hello"), None);
    assert_eq!(Format::detect("<html/>"), None);
    assert_eq!(Format::detect("{\"a\":1}"), None);
    let mapper = ci_mapper();
    let cx = ImportContext::new(&mapper, Some(rev('a')));
    let r = import_from(
        &ReportSource {
            text: LCOV.to_owned(),
            format: None,
        },
        &cx,
    )
    .unwrap();
    assert_eq!(r.tool, Tool::Lcov);
    let r = import_from(
        &ReportSource {
            text: JACOCO.to_owned(),
            format: Some(Format::JaCoCo),
        },
        &cx,
    )
    .unwrap();
    assert_eq!(r.tool, Tool::JaCoCo);
    assert!(matches!(
        import_from(
            &ReportSource {
                text: "hello".into(),
                format: None
            },
            &cx
        ),
        Err(ImportError::UnknownFormat)
    ));
}

#[test]
fn runs_of_one_revision_add_up_and_runs_of_different_revisions_refuse_to_blend() {
    let mapper = ci_mapper();
    let one = read(LCOV, Format::Lcov, &mapper);
    let two = read(LCOV, Format::Lcov, &mapper);
    let mut set = CoverageSet::new();
    set.add(&one).unwrap();
    set.add(&two).unwrap();
    assert_eq!((set.runs(), set.tools(), set.revision()), (2, &[Tool::Lcov][..], Some(&rev('a'))));
    assert_eq!(set.files()[&p("src/a.c")].lines[&3], 4, "hits add up");
    let istanbul = read(ISTANBUL, Format::Istanbul, &mapper);
    set.add(&istanbul).unwrap();
    assert_eq!(set.tools().len(), 2);

    let other = pacenotes_cover::import(LCOV, Format::Lcov, &ImportContext::new(&mapper, Some(rev('b')))).unwrap();
    let err = set.add(&other).unwrap_err();
    assert_eq!(err, BlendError::DifferentRevisions { a: rev('a'), b: rev('b') });
    assert!(err.to_string().contains("run both on one revision"));
    let unknown = pacenotes_cover::import(LCOV, Format::Lcov, &ImportContext::new(&mapper, None)).unwrap();
    assert_eq!(set.add(&unknown), Err(BlendError::UnknownRevision));
    assert_eq!(set.runs(), 3, "a refused run changed nothing");

    // a single run with no recorded revision is allowed, a second is not
    let mut lone = CoverageSet::new();
    lone.add(&unknown).unwrap();
    assert_eq!(lone.add(&unknown), Err(BlendError::UnknownRevision));
}
