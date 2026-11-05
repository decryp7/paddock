//! The reviewers' inputs, each run through the public API. Every one of these hung, overflowed the stack, panicked or ate gigabytes
//! before the fix.

use pacenotes_cover::{BlendError, CoverageSet, Format, ImportContext, ImportError, PathMapper, import};

use crate::support::{LCOV, ci_mapper, hits, p, rev};

fn cx<'a>(m: &'a PathMapper) -> ImportContext<'a> {
    ImportContext::new(m, Some(rev('a')))
}

#[test]
fn an_entity_bomb_at_the_top_level_is_refused_before_anything_expands() {
    let bomb = format!(
        "<!DOCTYPE coverage [<!ENTITY a \"{}\">]><coverage>{}</coverage>",
        "A".repeat(10_000),
        "&a;".repeat(10_000)
    );
    let m = ci_mapper();
    for format in [Format::Cobertura, Format::JaCoCo] {
        let err = import(&bomb, format, &cx(&m)).unwrap_err();
        assert!(
            matches!(err, ImportError::Malformed { format: "XML", .. }) && err.to_string().contains("nothing was loaded"),
            "{err}"
        );
    }
}

#[test]
fn a_greater_than_in_an_attribute_does_not_defeat_the_depth_guard() {
    let hostile = format!("<coverage>{}{}</coverage>", "<a x=\"/>\">".repeat(5000), "</a>".repeat(5000));
    let m = ci_mapper();
    assert!(matches!(
        import(&hostile, Format::Cobertura, &cx(&m)),
        Err(ImportError::Limit {
            what: "XML nesting depth",
            ..
        })
    ));
    assert!(matches!(
        import(&hostile.replace("coverage", "report"), Format::JaCoCo, &cx(&m)),
        Err(ImportError::Limit {
            what: "XML nesting depth",
            ..
        })
    ));
}

#[test]
fn jacoco_branch_counts_that_do_not_fit_32_bits_are_malformed_not_an_overflow() {
    let xml = |mb: &str, cb: &str| {
        format!(
            "<report><package name=\"p\"><sourcefile name=\"A.java\"><line nr=\"1\" mi=\"0\" ci=\"1\" mb=\"{mb}\" cb=\"{cb}\"/></sourcefile></package></report>"
        )
    };
    let m = PathMapper::new();
    for (mb, cb) in [
        ("18446744073709551615", "1"),
        ("1", "18446744073709551615"),
        ("4294967296", "0"),
        ("-1", "0"),
    ] {
        let err = import(&xml(mb, cb), Format::JaCoCo, &cx(&m)).unwrap_err();
        assert!(
            matches!(err, ImportError::Malformed { .. }) && err.to_string().contains("line 1") && err.to_string().contains("32 bits"),
            "{mb} {cb}: {err}"
        );
    }
    let r = import(&xml("4294967295", "4294967295"), Format::JaCoCo, &cx(&m)).unwrap();
    assert_eq!(r.files[&p("p/A.java")].branches[&1].total, u32::MAX, "saturates, never overflows");
}

fn llvm(segments: &str) -> String {
    format!(r#"{{"data":[{{"files":[{{"filename":"/ci/work/repo/src/m.rs","segments":[{segments}]}}]}}]}}"#)
}

#[test]
fn llvm_lines_past_32_bits_and_segments_out_of_order_are_malformed() {
    let m = ci_mapper();
    let err = import(
        &llvm("[18446744073709551615,1,1,true,true,false],[18446744073709551615,2,0,false,false,false]"),
        Format::LlvmCov,
        &cx(&m),
    )
    .unwrap_err();
    assert!(
        matches!(err, ImportError::Malformed { .. }) && err.to_string().contains("32-bit"),
        "{err}"
    );
    let err = import(
        &llvm("[1,1,1,true,true,false],[4294967295,1,1,true,true,false],[7,1,0,false,false,false]"),
        Format::LlvmCov,
        &cx(&m),
    )
    .unwrap_err();
    assert!(err.to_string().contains("not in order"), "{err}");
    // the gap between two far-apart segments is filled line by line, and the limits end it long before billions
    let far = llvm("[1,1,1,true,true,false],[4294967294,1,0,false,false,false]");
    assert!(
        matches!(import(&far, Format::LlvmCov, &cx(&m)), Err(ImportError::Limit { .. })),
        "a limit ends the gap fill"
    );
}

#[test]
fn a_tiny_llvm_input_cannot_expand_into_millions_of_lines() {
    let m = ci_mapper();
    // many small files that each fill a gap just under the per-file limit: 472 bytes of input became 10M lines
    let file = |i: usize| {
        format!(r#"{{"filename":"/ci/work/repo/src/f{i}.rs","segments":[[1,1,1,true,true,false],[900,1,0,false,false,false]]}}"#)
    };
    let files: Vec<String> = (0..20).map(file).collect();
    let text = format!(r#"{{"data":[{{"files":[{}]}}]}}"#, files.join(","));
    let mut c = cx(&m);
    c.limits.max_total_lines = 5_000;
    let err = import(&text, Format::LlvmCov, &c).unwrap_err();
    assert!(
        matches!(
            err,
            ImportError::Limit {
                what: "instrumented lines in the report",
                max: 5_000,
                ..
            }
        ),
        "{err}"
    );
    c.limits.max_total_lines = 10_000_000;
    assert_eq!(import(&text, Format::LlvmCov, &c).unwrap().files.len(), 20);
}

#[test]
fn malformed_llvm_segments_and_odd_istanbul_hits_are_counted() {
    let m = ci_mapper();
    let r = import(
        &llvm(r#"[1,1,1,true,true,false],"junk",[2],[3,1,0,false,false,false],{"a":1}"#),
        Format::LlvmCov,
        &cx(&m),
    )
    .unwrap();
    assert_eq!(r.skipped.dropped_segments, 3);
    let ist = r#"{"/ci/work/repo/a.js":{"statementMap":{"0":{"start":{"line":1}},"1":{"start":{"line":2}},"2":{"start":{"line":3}}},"s":{"0":1,"1":1.5,"2":-4}}}"#;
    let r = import(ist, Format::Istanbul, &cx(&m)).unwrap();
    assert_eq!(r.skipped.non_integer_hits, 2);
    assert_eq!(hits(&r, "a.js"), [(1, 1), (2, 0), (3, 0)]);
}

#[test]
fn the_longest_prefix_wins_whatever_order_prefixes_were_given_in() {
    let a = PathMapper::new().strip_prefix("/work/repo").strip_prefix("/work/repo/app");
    let b = PathMapper::new().strip_prefix("/work/repo/app").strip_prefix("/work/repo");
    for m in [&a, &b] {
        assert_eq!(m.map("/work/repo/app/src/x.rs"), Some(p("src/x.rs")));
        assert_eq!(m.map("/work/repo/other/y.rs"), Some(p("other/y.rs")));
        assert_eq!(m.map_dir("/work/repo/app"), Some(String::new()));
    }
}

#[test]
fn every_format_uses_the_existence_check_and_cobertura_tries_every_source() {
    let cobertura = r#"<coverage><sources><source>/ci/work/repo/one</source><source>/ci/work/repo/two</source></sources><packages><package><classes>
        <class filename="a.py"><lines><line number="1" hits="1"/></lines></class>
        <class filename="gone.py"><lines><line number="1" hits="1"/></lines></class></classes></package></packages></coverage>"#;
    let m = ci_mapper();
    let exists = |path: &pacenotes_core::RepoPath| path.as_str() == "two/a.py";
    let mut c = cx(&m);
    c.exists = Some(&exists);
    let r = import(cobertura, Format::Cobertura, &c).unwrap();
    assert_eq!(
        r.files.keys().map(|k| k.as_str()).collect::<Vec<_>>(),
        ["two/a.py"],
        "the second source is the one where the file is"
    );
    assert_eq!((r.skipped.missing_files, r.skipped.unmapped_files), (1, 0));
    // lcov, llvm-cov and istanbul too
    let none = |_: &pacenotes_core::RepoPath| false;
    c.exists = Some(&none);
    assert_eq!(import(LCOV, Format::Lcov, &c).unwrap().skipped.missing_files, 2);
    assert_eq!(
        import(&llvm("[1,1,1,true,true,false]"), Format::LlvmCov, &c)
            .unwrap()
            .skipped
            .missing_files,
        1
    );
    assert_eq!(
        import(r#"{"/ci/work/repo/a.js":{}}"#, Format::Istanbul, &c)
            .unwrap()
            .skipped
            .missing_files,
        1
    );
}

#[test]
fn the_same_artefact_added_twice_is_refused_and_a_different_one_adds_up() {
    let m = ci_mapper();
    let mut c = cx(&m);
    c.run_id = Some("ci-run-41".to_owned());
    let one = import(LCOV, Format::Lcov, &c).unwrap();
    let mut set = CoverageSet::new();
    set.add(&one).unwrap();
    assert_eq!(set.add(&one), Err(BlendError::SameRunTwice("ci-run-41".to_owned())));
    c.run_id = Some("ci-run-42".to_owned());
    set.add(&import(LCOV, Format::Lcov, &c).unwrap()).unwrap();
    assert_eq!(set.runs(), 2);
    assert_eq!(set.files()[&p("src/a.c")].lines[&3], 4);
}

#[test]
fn jacoco_with_only_one_of_mb_and_cb_is_malformed_with_its_line() {
    let m = PathMapper::new();
    for attrs in ["mb=\"1\"", "cb=\"2\""] {
        let xml = format!(
            "<report>\n<package name=\"p\"><sourcefile name=\"A.java\">\n<line nr=\"1\" mi=\"0\" ci=\"1\" {attrs}/>\n</sourcefile></package></report>"
        );
        let err = import(&xml, Format::JaCoCo, &cx(&m)).unwrap_err();
        assert!(
            matches!(err, ImportError::Malformed { .. }) && err.to_string().contains("line 3") && err.to_string().contains("come together"),
            "{attrs}: {err}"
        );
    }
}

#[test]
fn the_lines_a_llvm_report_may_describe_grow_with_its_size() {
    let m = ci_mapper();
    // about 150 bytes that describe 1.5 million lines: under the absolute cap of 10 million, far over what its size allows
    let tiny = llvm("[1,1,1,true,true,false],[1500000,1,0,false,false,false]");
    assert!(tiny.len() < 300, "{}", tiny.len());
    let err = import(&tiny, Format::LlvmCov, &cx(&m)).unwrap_err();
    assert!(
        matches!(&err, ImportError::Limit { what: "instrumented lines in the report", max, .. } if *max < 200_000),
        "{err}"
    );
    // the same report is fine when the caller raises the ratio, and a larger report gets a larger allowance
    let mut c = cx(&m);
    c.limits.max_lines_per_input_byte = 100_000;
    assert_eq!(import(&tiny, Format::LlvmCov, &c).unwrap().files.len(), 1);
    let padded = format!("{tiny}{}", " ".repeat(15_000));
    assert!(
        import(&padded, Format::LlvmCov, &cx(&m)).is_err(),
        "15 KB of padding buys about a million lines, not 1.5 million"
    );
    let roomy = format!("{tiny}{}", " ".repeat(30_000));
    assert_eq!(import(&roomy, Format::LlvmCov, &cx(&m)).unwrap().files.len(), 1);
}
