//! llvm-cov and istanbul.

use pacenotes_cover::{Format, ImportContext, ImportError, import};

use crate::support::{ISTANBUL, LLVM, ci_mapper, hits, p, read};

#[test]
fn llvm_cov_segments_become_line_counts_the_way_llvm_cov_derives_them() {
    let r = read(LLVM, Format::LlvmCov, &ci_mapper());
    // line 1 starts a region of 5 that runs through line 2; line 3 starts a region of 0 that ends at line 5; 7 starts two regions
    assert_eq!(hits(&r, "src/m.rs"), [(1, 5), (2, 5), (3, 0), (4, 0), (7, 9)]);
    // a region that ends mid-line still counts that line; one that ends at column 1 does not
    let mid =
        r#"{"data":[{"files":[{"filename":"/ci/work/repo/src/n.rs","segments":[[1,1,3,true,true,false],[2,5,0,false,false,false]]}]}]}"#;
    let r = read(mid, Format::LlvmCov, &ci_mapper());
    assert_eq!(hits(&r, "src/n.rs"), [(1, 3), (2, 3)]);
}

#[test]
fn istanbul_statements_give_line_hits_and_branches() {
    let r = read(ISTANBUL, Format::Istanbul, &ci_mapper());
    assert_eq!(
        hits(&r, "src/x.js"),
        [(1, 3), (2, 4)],
        "two statements on line 2: the line ran if either did"
    );
    let b = r.files[&p("src/x.js")].branches[&2];
    assert_eq!((b.taken, b.total), (1, 2));
}

#[test]
fn json_that_is_not_the_format_is_a_malformed_error() {
    let mapper = ci_mapper();
    let cx = ImportContext::new(&mapper, None);
    for (text, format) in [
        ("{", Format::LlvmCov),
        ("{}", Format::LlvmCov),
        ("[]", Format::Istanbul),
        ("nope", Format::Istanbul),
    ] {
        assert!(matches!(import(text, format, &cx), Err(ImportError::Malformed { .. })), "{text}");
    }
    let outside = r#"{"data":[{"files":[{"filename":"/usr/lib/x.rs","segments":[[1,1,1,true,true,false]]}]}]}"#;
    assert_eq!(import(outside, Format::LlvmCov, &cx).unwrap().skipped.unmapped_files, 1);
    let zero = r#"{"/ci/work/repo/a.js":{"statementMap":{"0":{"start":{"line":0}}},"s":{"0":1}}}"#;
    assert_eq!(import(zero, Format::Istanbul, &cx).unwrap().skipped.bad_lines, 1);
}
