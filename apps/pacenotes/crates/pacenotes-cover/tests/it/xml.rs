//! Cobertura and JaCoCo.

use pacenotes_cover::{Format, ImportContext, ImportError, PathMapper, import};

use crate::support::{COBERTURA, JACOCO, ci_mapper, hits, p, read, rev};

#[test]
fn cobertura_gives_lines_and_branches_with_names_relative_to_the_source_folder() {
    let r = read(COBERTURA, Format::Cobertura, &ci_mapper());
    assert_eq!(
        hits(&r, "app/src/A.py"),
        [(1, 1), (2, 0)],
        "source /ci/work/repo/app puts `app/` in front"
    );
    let b = r.files[&p("app/src/A.py")].branches[&2];
    assert_eq!((b.taken, b.total), (1, 2));
    assert_eq!((r.skipped.unmapped_files, r.skipped.bad_lines), (1, 1));
}

#[test]
fn cobertura_without_a_source_folder_in_the_repository_takes_names_as_they_are() {
    let r = read(COBERTURA, Format::Cobertura, &PathMapper::new());
    assert_eq!(hits(&r, "src/A.py"), [(1, 1), (2, 0)]);
}

#[test]
fn jacoco_marks_a_line_covered_by_its_covered_instructions_and_needs_source_folders_for_paths() {
    let none = read(JACOCO, Format::JaCoCo, &PathMapper::new());
    assert_eq!(hits(&none, "com/acme/A.java"), [(3, 1), (4, 0), (5, 1)]);
    let b = none.files[&p("com/acme/A.java")].branches[&5];
    assert_eq!((b.taken, b.total), (1, 2));
    assert!(!none.files[&p("com/acme/A.java")].branches.contains_key(&3));
    let mapper = PathMapper::new().source_root("lib/src/main/java/").source_root("app/src/main/java");
    let first = read(JACOCO, Format::JaCoCo, &mapper);
    assert!(
        first.files.contains_key(&p("lib/src/main/java/com/acme/A.java")),
        "without an existence check the first folder is used"
    );
    let cx = ImportContext {
        exists: Some(&|path: &pacenotes_core::RepoPath| path.as_str().starts_with("app/")),
        ..ImportContext::new(&mapper, Some(rev('a')))
    };
    let chosen = import(JACOCO, Format::JaCoCo, &cx).unwrap();
    assert!(
        chosen.files.contains_key(&p("app/src/main/java/com/acme/A.java")),
        "the folder where the file exists wins"
    );
}

#[test]
fn xml_that_is_not_the_format_or_is_hostile_is_refused_and_nothing_is_fetched() {
    let mapper = ci_mapper();
    let cx = ImportContext::new(&mapper, None);
    let wrong_root = import("<report/>", Format::Cobertura, &cx).unwrap_err();
    assert!(wrong_root.to_string().contains("not <coverage>"), "{wrong_root}");
    assert!(
        import("<coverage/>", Format::JaCoCo, &cx)
            .unwrap_err()
            .to_string()
            .contains("not <report>")
    );
    assert!(matches!(
        import("<coverage><unclosed>", Format::Cobertura, &cx),
        Err(ImportError::Malformed { format: "XML", .. })
    ));
    assert!(import(r#"<coverage><packages><package><classes><class filename="a.py"><lines><line number="x" hits="1"/></lines></class></classes></package></packages></coverage>"#, Format::Cobertura, &cx).is_err());
    assert!(
        import(
            r#"<report><package name="p"><sourcefile name="A.java"><line nr="1"/></sourcefile></package></report>"#,
            Format::JaCoCo,
            &cx
        )
        .is_err()
    );
    // an external entity names a file; the parser does not load it, so the report is refused or the entity stays empty, never read
    let xxe = r#"<?xml version="1.0"?><!DOCTYPE coverage [<!ENTITY x SYSTEM "file:///etc/passwd">]><coverage><sources><source>&x;</source></sources></coverage>"#;
    let result = import(xxe, Format::Cobertura, &cx);
    if let Ok(report) = &result {
        assert!(report.files.is_empty());
    }
    // a "billion laughs" expansion is refused
    let laughs = format!(
        "<?xml version=\"1.0\"?><!DOCTYPE coverage [<!ENTITY a \"aaaaaaaaaa\"><!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;\"><!ENTITY c \"&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;\"><!ENTITY d \"&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;\"><!ENTITY e \"&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;\">]><coverage><sources><source>{}</source></sources></coverage>",
        "&e;".repeat(50)
    );
    assert!(import(&laughs, Format::Cobertura, &cx).is_err() || import(&laughs, Format::Cobertura, &cx).unwrap().files.is_empty());
    let mut small = ImportContext::new(&mapper, None);
    small.limits.max_xml_nodes = 20;
    let big = format!("<coverage>{}</coverage>", "<x/>".repeat(100));
    assert!(matches!(
        import(&big, Format::Cobertura, &small),
        Err(ImportError::Limit { what: "XML nodes", .. })
    ));
}
