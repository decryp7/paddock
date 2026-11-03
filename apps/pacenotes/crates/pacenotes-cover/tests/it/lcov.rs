//! LCOV.

use pacenotes_cover::{Format, ImportContext, ImportError, PathMapper, import};

use crate::support::{LCOV, ci_mapper, hits, p, read, rev};

#[test]
fn an_lcov_tracefile_gives_line_hits_branches_and_counts_what_is_outside_the_repository() {
    let r = read(LCOV, Format::Lcov, &ci_mapper());
    assert_eq!(hits(&r, "src/a.c"), [(3, 2), (4, 0), (5, 7)]);
    assert_eq!(hits(&r, "src/b.c"), [(1, 1)]);
    let b = r.files[&p("src/a.c")].branches[&4];
    assert_eq!((b.taken, b.total), (1, 3), "taken, not executed (-), not taken");
    assert_eq!(r.skipped.unmapped_files, 1, "/usr/include/stdio.h is outside the repository");
    assert_eq!(r.files.len(), 2);
    assert_eq!(r.totals(), (4, 3));
    assert_eq!(r.revision, Some(rev('a')));
}

#[test]
fn a_repeated_file_adds_up_and_windows_paths_are_mapped() {
    let text = "SF:C:\\work\\repo\\src\\a.c\nDA:1,1\nend_of_record\nSF:C:\\work\\repo\\src\\a.c\nDA:1,2\nDA:2,0\nend_of_record\n";
    let r = read(text, Format::Lcov, &PathMapper::new().strip_prefix("c:/work/repo"));
    assert_eq!(hits(&r, "src/a.c"), [(1, 3), (2, 0)]);
}

#[test]
fn malformed_lcov_is_an_error_that_names_the_line() {
    let mapper = ci_mapper();
    let cx = ImportContext::new(&mapper, Some(rev('a')));
    for (text, expect) in [
        ("DA:1,1\n", "line 1"),
        ("SF:/ci/work/repo/a.c\nDA:x,1\nend_of_record\n", "line 2"),
        ("SF:/ci/work/repo/a.c\nSF:/ci/work/repo/b.c\n", "line 2"),
        ("end_of_record\n", "line 1"),
        ("SF:/ci/work/repo/a.c\nDA:1,1\n", "cut short"),
        ("SF:/ci/work/repo/a.c\nBRDA:1,0\nend_of_record\n", "line 2"),
        ("SF:/ci/work/repo/a.c\nBRDA:1,0,0,zz\nend_of_record\n", "line 2"),
        ("BRDA:1,0,0,1\n", "line 1"),
    ] {
        let err = import(text, Format::Lcov, &cx).unwrap_err();
        assert!(
            matches!(err, ImportError::Malformed { .. }) && err.to_string().contains(expect),
            "{text:?}: {err}"
        );
    }
    // a line number of 0 is counted, not an error
    let r = import(
        "SF:/ci/work/repo/a.c\nDA:0,1\nDA:2,1\nBRDA:0,0,0,1\nend_of_record\n",
        Format::Lcov,
        &cx,
    )
    .unwrap();
    assert_eq!((r.skipped.bad_lines, hits(&r, "a.c")), (1, vec![(2, 1)]));
}

#[test]
fn limits_are_errors_that_say_what_to_raise() {
    let mapper = ci_mapper();
    let mut cx = ImportContext::new(&mapper, None);
    cx.limits.max_files = 1;
    let err = import(LCOV, Format::Lcov, &cx).unwrap_err();
    assert!(
        matches!(err, ImportError::Limit { what: "files", .. }) && err.to_string().contains("raise the limit"),
        "{err}"
    );
    cx.limits.max_files = 10;
    cx.limits.max_lines_per_file = 2;
    assert!(matches!(
        import(LCOV, Format::Lcov, &cx),
        Err(ImportError::Limit {
            what: "instrumented lines in one file",
            ..
        })
    ));
    cx.limits.max_lines_per_file = 100;
    cx.limits.max_bytes = 10;
    assert!(matches!(
        import(LCOV, Format::Lcov, &cx),
        Err(ImportError::Limit {
            what: "report size in bytes",
            ..
        })
    ));
}
