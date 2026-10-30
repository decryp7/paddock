//! Workbooks and Word files.

use pacenotes_req::{Format, ImportLimits, ReqError, import};

use crate::support::{BASIC, docx, pack, read, xlsx};

#[test]
fn a_generated_sheet_with_merged_cells_maps_by_its_pack() {
    let p = pack(
        "[table]\nname = \"requirements\"\nfill_down = [\"Section\"]\n[columns]\nid = \"Req ID\"\ntext = \"Description\"\n[columns.extra]\nsection = \"Section\"\npriority = \"Priority\"\n",
    );
    let set = read(&xlsx(), Format::Xlsx, &p);
    let rows: Vec<_> = set
        .requirements
        .iter()
        .map(|r| {
            (
                r.id.as_str(),
                r.text.as_str(),
                r.fields["section"].as_str(),
                r.fields.get("priority").map(String::as_str),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("SAF-1", "The brake shall hold.", "Safety", Some("high")),
            ("SAF-2", "The horn shall sound.", "Safety", Some("2")),
            ("SAF-3", "Lights shall switch.", "Safety", None),
        ],
        "the merged section reaches every row, shared strings with runs are joined, numbers are kept"
    );
    assert_eq!(
        (
            set.skipped.empty_rows,
            set.requirements[0].source.place.as_str(),
            set.requirements[0].source.row
        ),
        (0, "Requirements", 2)
    );
}

#[test]
fn without_a_sheet_name_the_first_sheet_is_read_and_a_missing_one_lists_the_sheets() {
    let p = pack("[columns]\nid = \"A\"\ntext = \"A\"\n");
    let set = read(&xlsx(), Format::Xlsx, &p);
    assert!(
        set.requirements.is_empty() || set.requirements[0].text == "just notes",
        "the Notes sheet is the first"
    );
    let err = import(
        &xlsx(),
        Format::Xlsx,
        &pack("[table]\nname = \"Nope\"\n[columns]\nid = \"A\"\ntext = \"B\"\n"),
        &ImportLimits::default(),
    )
    .unwrap_err();
    assert!(
        matches!(err, ReqError::NoTable { what: "sheet", .. }) && err.to_string().contains("\"Notes\", \"Requirements\""),
        "{err}"
    );
}

#[test]
fn a_word_table_is_read_with_spans_vertical_merges_and_paragraphs() {
    let p = pack(
        "[table]\nindex = 2\nfill_down = [\"Section\"]\n[columns]\nid = \"Req ID\"\ntext = \"Description\"\n[columns.extra]\nsection = \"Section\"\npriority = \"Priority\"\n",
    );
    let set = read(&docx(), Format::Docx, &p);
    let rows: Vec<_> = set
        .requirements
        .iter()
        .map(|r| (r.id.as_str(), r.text.as_str(), r.fields["section"].as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("B-1", "Stop <in> time.", "Brakes"),
            ("B-2", "Hold on slopes.\nEven loaded.", "Brakes")
        ]
    );
    assert_eq!(set.skipped.missing_id, 1, "the spanning row has text but no id");
    assert_eq!(set.requirements[0].source.place, "table 2");
    let err = import(
        &docx(),
        Format::Docx,
        &pack("[table]\nindex = 5\n[columns]\nid = \"A\"\ntext = \"B\"\n"),
        &ImportLimits::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("2 table(s)"), "{err}");
}

#[test]
fn a_package_that_is_not_the_format_is_refused_with_a_message() {
    let p = pack(BASIC);
    let l = ImportLimits::default();
    for (bytes, format, expect) in [
        (b"not a zip".to_vec(), Format::Xlsx, "not a zip package"),
        (
            crate::support::zip_of(&[("a.txt", "x")]),
            Format::Xlsx,
            "xl/workbook.xml is missing",
        ),
        (
            crate::support::zip_of(&[("a.txt", "x")]),
            Format::Docx,
            "word/document.xml is missing",
        ),
        (crate::support::zip_of(&[("word/document.xml", "<w:document")]), Format::Docx, "XML"),
    ] {
        let err = import(&bytes, format, &p, &l).unwrap_err();
        assert!(err.to_string().contains(expect), "{expect}: {err}");
    }
}
