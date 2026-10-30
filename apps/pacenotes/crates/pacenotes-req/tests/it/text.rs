//! CSV and Markdown.

use pacenotes_req::{Format, ImportLimits, ReqError, import};

use crate::support::{BASIC, pack, read};

#[test]
fn csv_maps_by_header_with_quotes_a_bom_and_the_delimiter_the_header_uses() {
    let csv = "\u{feff}Req ID;Title;Description;Priority\nR-1;Brakes;\"Stop; in \"\"time\"\"\nand safely.\";high\nR-2;;Horn;\n";
    let set = read(csv.as_bytes(), Format::Csv, &pack(BASIC));
    assert_eq!(set.requirements.len(), 2);
    let r = &set.requirements[0];
    assert_eq!(
        (r.id.as_str(), r.title.as_deref(), r.text.as_str()),
        ("R-1", Some("Brakes"), "Stop; in \"time\"\nand safely.")
    );
    assert_eq!(r.fields["priority"], "high");
    assert_eq!((r.source.place.as_str(), r.source.row), ("csv", 2));
    let second = &set.requirements[1];
    assert!(second.title.is_none() && !second.fields.contains_key("priority"));
}

#[test]
fn a_column_may_be_named_by_its_letter_and_the_header_row_may_be_lower_down() {
    let csv = "Exported 2026\nby a tool\nid,x,desc\nR-1,a,Stop.\n";
    let set = read(
        csv.as_bytes(),
        Format::Csv,
        &pack("[table]\nheader_row = 3\n[columns]\nid = \"A\"\ntext = \"C\"\n"),
    );
    assert_eq!(
        (set.requirements[0].id.as_str(), set.requirements[0].text.as_str()),
        ("R-1", "Stop.")
    );
    assert_eq!(set.requirements[0].source.row, 4);
}

#[test]
fn markdown_pipe_tables_are_found_by_number_and_code_blocks_are_skipped() {
    let md = "# Spec\n\n```\n| Req ID | Description |\n|---|---|\n| X | not a table |\n```\n\n| Req ID | Description | Priority |\n|:--|---|--:|\n| R-1 | Stop \\| halt | high |\n| R-2 | Horn | low |\n\nText.\n\n| Req ID | Description |\n|---|---|\n| S-1 | Second table |\n";
    let first = read(
        md.as_bytes(),
        Format::Markdown,
        &pack("[columns]\nid = \"req id\"\ntext = \"Description\"\n[columns.extra]\npriority = \"Priority\"\n"),
    );
    assert_eq!(first.requirements.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["R-1", "R-2"]);
    assert_eq!(first.requirements[0].text, "Stop | halt");
    let second = read(
        md.as_bytes(),
        Format::Markdown,
        &pack("[table]\nindex = 2\n[columns]\nid = \"Req ID\"\ntext = \"Description\"\n"),
    );
    assert_eq!(second.requirements[0].id, "S-1");
    let err = import(
        md.as_bytes(),
        Format::Markdown,
        &pack("[table]\nindex = 3\n[columns]\nid = \"A\"\ntext = \"B\"\n"),
        &ImportLimits::default(),
    )
    .unwrap_err();
    assert!(
        matches!(err, ReqError::NoTable { .. }) && err.to_string().contains("2 pipe table(s)"),
        "{err}"
    );
}

#[test]
fn text_limits_and_encodings_are_errors_that_say_what_to_do() {
    let limits = ImportLimits {
        max_rows: 2,
        max_cols: 3,
        max_cells: 100,
        ..ImportLimits::default()
    };
    let p = pack("[columns]\nid = \"A\"\ntext = \"B\"\n");
    assert!(matches!(
        import(b"a,b\n1,2\n3,4\n", Format::Csv, &p, &limits),
        Err(ReqError::Limit { what: "rows", .. })
    ));
    assert!(matches!(
        import(b"a,b,c,d\n", Format::Csv, &p, &limits),
        Err(ReqError::Limit { what: "columns", .. })
    ));
    assert!(matches!(
        import(
            b"| a | b |\n|--|--|\n| 1 | 2 |\n| 3 | 4 |\n| 5 | 6 |\n",
            Format::Markdown,
            &p,
            &limits
        ),
        Err(ReqError::Limit { what: "rows", .. })
    ));
    let err = import(&[0xff, 0xfe, 0x41], Format::Csv, &p, &ImportLimits::default()).unwrap_err();
    assert!(err.to_string().contains("UTF-8"), "{err}");
    assert!(import(&[0xff], Format::Markdown, &p, &ImportLimits::default()).is_err());
    assert!(matches!(
        import(
            &[b'a'; 11],
            Format::Csv,
            &p,
            &ImportLimits {
                max_bytes: 10,
                ..ImportLimits::default()
            }
        ),
        Err(ReqError::Limit {
            what: "document size in bytes",
            ..
        })
    ));
}
