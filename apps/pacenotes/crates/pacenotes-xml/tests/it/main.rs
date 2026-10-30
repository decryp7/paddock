#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The reviewers' inputs, each run against the one entry point.

use pacenotes_xml::{XmlError, XmlLimits, clean, line_of, parse, with_document};

fn doc(text: &str) -> Result<usize, XmlError> {
    with_document(text, "t.xml", &XmlLimits::default(), |d| Ok::<_, XmlError>(d.descendants().count()))
}

#[test]
fn an_entity_declaration_is_refused_so_nothing_can_expand() {
    // the reviewer's input: a thousand-times entity referenced ten thousand times at the top level (5 GB, over a minute)
    let bomb = format!(
        "<!DOCTYPE coverage [<!ENTITY a \"{}\">]><coverage>{}</coverage>",
        "A".repeat(10_000),
        "&a;".repeat(10_000)
    );
    let err = doc(&bomb).unwrap_err();
    assert!(
        matches!(&err, XmlError::Dtd { line: 1, .. }) && err.to_string().contains("nothing was loaded"),
        "{err}"
    );
    let external = r#"<?xml version="1.0"?>
<!DOCTYPE d [<!ENTITY x SYSTEM "file:///etc/passwd">]>
<d>&x;</d>"#;
    assert!(matches!(doc(external), Err(XmlError::Dtd { line: 2, .. })));
    // a `[` inside a quoted identifier is not a subset
    assert!(doc(r#"<!DOCTYPE report PUBLIC "-//X [1]//EN" "report.dtd"><report/>"#).is_ok());
    assert!(matches!(doc("<!DOCTYPE d [ never closed"), Err(XmlError::Dtd { .. })));
}

#[test]
fn a_bare_doctype_is_blanked_keeps_its_lines_and_loads_nothing() {
    let jacoco = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE report PUBLIC \"-//JACOCO//DTD Report 1.1//EN\"\n \"report.dtd\">\n<report><x/></report>";
    let cleaned = clean(jacoco, "r", &XmlLimits::default()).unwrap();
    assert_eq!(cleaned.len(), jacoco.len());
    assert_eq!(
        cleaned.matches('\n').count(),
        jacoco.matches('\n').count(),
        "lines are kept, so error lines are the original's"
    );
    assert!(!cleaned.contains("DOCTYPE") && !cleaned.contains("report.dtd"));
    let parsed = parse(&cleaned, "r", &XmlLimits::default()).unwrap();
    assert_eq!(parsed.root_element().tag_name().name(), "report");
    assert_eq!(line_of(&parsed, parsed.descendants().find(|n| n.has_tag_name("x")).unwrap()), 4);
    // wherever a bare DOCTYPE stands it is blanked; one with declarations is refused wherever it stands
    assert!(doc("<r/><!DOCTYPE r>").is_ok());
    assert!(matches!(doc("<r/><!DOCTYPE r [<!ENTITY a \"b\">]>"), Err(XmlError::Dtd { .. })));
}

#[test]
fn a_greater_than_inside_an_attribute_does_not_hide_the_nesting() {
    // the reviewer's input: `<a x="/>">` is one open tag, not a self-closing one; 5000 of them overflowed the stack
    let hostile = format!("<coverage>{}{}</coverage>", "<a x=\"/>\">".repeat(5000), "</a>".repeat(5000));
    let err = doc(&hostile).unwrap_err();
    assert!(
        matches!(&err, XmlError::Depth { max: 256, .. }) && err.to_string().contains("line 1"),
        "{err}"
    );
    let single = format!("<coverage>{}{}</coverage>", "<a x='/>'>".repeat(300), "</a>".repeat(300));
    assert!(matches!(doc(&single), Err(XmlError::Depth { .. })));
}

#[test]
fn depth_is_counted_right_at_the_edge_and_ignores_comments_cdata_and_self_closing_tags() {
    let at = |n: usize| format!("{}{}", "<a>".repeat(n), "</a>".repeat(n));
    assert!(doc(&at(256)).is_ok());
    assert!(matches!(doc(&at(257)), Err(XmlError::Depth { .. })));
    let noise = format!(
        "<r>{}<!-- <a><a><a> --><![CDATA[<b><b>]]><?pi <c> ?><x/><y a=\"1\" />{}</r>",
        "<a>".repeat(200),
        "</a>".repeat(200)
    );
    assert!(doc(&noise).is_ok());
    let nested_p = format!("<w:document>{}{}</w:document>", "<w:p>".repeat(5000), "</w:p>".repeat(5000));
    assert!(matches!(doc(&nested_p), Err(XmlError::Depth { .. })));
    let deep_at_line_three = format!("<r>\n<a>\n{}{}</a></r>", "<b>".repeat(300), "</b>".repeat(300));
    assert!(matches!(doc(&deep_at_line_three), Err(XmlError::Depth { line: 3, .. })));
}

#[test]
fn syntax_and_node_limits_are_typed_and_say_where() {
    let err = doc("<a>\n<b></a>").unwrap_err();
    assert!(matches!(&err, XmlError::Syntax { part, .. } if part == "t.xml"), "{err}");
    assert!(err.to_string().contains("2:") || err.to_string().contains("line 2"), "{err}");
    let many = format!("<r>{}</r>", "<x/>".repeat(100));
    let tight = XmlLimits {
        max_nodes: 20,
        ..XmlLimits::default()
    };
    let err = with_document(&many, "big.xml", &tight, |_| Ok::<_, XmlError>(())).unwrap_err();
    assert_eq!(
        err,
        XmlError::Nodes {
            part: "big.xml".into(),
            max: 20
        }
    );
    // the parser itself refuses a DOCTYPE that was not cleaned
    assert!(matches!(
        parse("<!DOCTYPE r><r/>", "p", &XmlLimits::default()),
        Err(XmlError::Dtd { .. })
    ));
    // entities that were never declared are not expanded
    assert!(matches!(doc("<r>&nope;</r>"), Err(XmlError::Syntax { .. })));
}

#[test]
fn hostile_shapes_never_panic() {
    let seeds: Vec<String> = vec![
        String::new(),
        "<".into(),
        "<a".into(),
        "<a x=\"".into(),
        "<!".into(),
        "<!--".into(),
        "<![CDATA[".into(),
        "<?".into(),
        "<!DOCTYPE".into(),
        "<!DOCTYPE a [".into(),
        "</".repeat(10_000),
        "<a>".repeat(100_000),
        "<a x=\"\">".repeat(100_000),
        "\u{feff}<a/>".into(),
        "<a>é</a>\u{0}".into(),
    ];
    for s in seeds {
        let _ = doc(&s);
    }
}
