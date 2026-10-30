//! The remaining bounds and the corners of each reader.

use pacenotes_req::{Format, ImportLimits, ReqError, import};

use crate::support::{pack, zip_of};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn id_text() -> pacenotes_req::MappingPack {
    pack("[columns]\nid = \"A\"\ntext = \"B\"\n")
}

fn docx_of(rows: &str) -> Vec<u8> {
    zip_of(&[(
        "word/document.xml",
        &format!(r#"<w:document xmlns:w="{W}"><w:body><w:tbl>{rows}</w:tbl></w:body></w:document>"#),
    )])
}

fn tc(text: &str, props: &str) -> String {
    format!("<w:tc><w:tcPr>{props}</w:tcPr><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>")
}

fn xlsx_with(target: &str, sheet: &str) -> Vec<u8> {
    let wb = format!(r#"<workbook xmlns="{MAIN}" xmlns:r="{REL}"><sheets><sheet name="s" sheetId="1" r:id="rId1"/></sheets></workbook>"#);
    let rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Target="{target}"/></Relationships>"#
    );
    let sheet = format!(r#"<worksheet xmlns="{MAIN}">{sheet}</worksheet>"#);
    zip_of(&[
        ("xl/workbook.xml", &wb),
        ("xl/_rels/workbook.xml.rels", &rels),
        ("xl/worksheets/sheet1.xml", &sheet),
    ])
}

#[test]
fn csv_and_markdown_cell_and_column_limits() {
    let l = ImportLimits {
        max_cells: 3,
        ..ImportLimits::default()
    };
    assert!(matches!(
        import(b"a,b\n1,2\n", Format::Csv, &id_text(), &l),
        Err(ReqError::Limit { what: "cells", .. })
    ));
    let md = ImportLimits {
        max_cols: 2,
        ..ImportLimits::default()
    };
    assert!(matches!(
        import(b"| a | b | c |\n|--|--|--|\n| 1 | 2 | 3 |\n", Format::Markdown, &id_text(), &md),
        Err(ReqError::Limit { what: "columns", .. })
    ));
}

#[test]
fn word_cells_keep_tabs_and_line_breaks_and_the_table_bounds_hold() {
    let tabbed = format!(
        "<w:tr>{}</w:tr><w:tr><w:tc><w:p><w:r><w:t>R-1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>a</w:t><w:tab/><w:t>b</w:t><w:br/><w:t>c</w:t></w:r></w:p></w:tc></w:tr>",
        tc("A", "") + &tc("B", "")
    );
    let set = import(&docx_of(&tabbed), Format::Docx, &id_text(), &ImportLimits::default()).unwrap();
    assert_eq!(set.requirements[0].text, "a\tb\nc");

    let two_rows = format!(
        "<w:tr>{}</w:tr><w:tr>{}</w:tr>",
        tc("A", "") + &tc("B", ""),
        tc("R", "") + &tc("t", "")
    );
    let rows = ImportLimits {
        max_rows: 1,
        ..ImportLimits::default()
    };
    assert!(matches!(
        import(&docx_of(&two_rows), Format::Docx, &id_text(), &rows),
        Err(ReqError::Limit { what: "rows", .. })
    ));

    let wide = format!(
        "<w:tr>{}</w:tr>",
        tc("A", "<w:gridSpan w:val=\"9\"/>") + &tc("B", "<w:gridSpan w:val=\"9\"/>")
    );
    let cols = ImportLimits {
        max_cols: 10,
        ..ImportLimits::default()
    };
    assert!(matches!(
        import(&docx_of(&wide), Format::Docx, &id_text(), &cols),
        Err(ReqError::Limit { what: "columns", .. })
    ));

    let cells = ImportLimits {
        max_cells: 3,
        ..ImportLimits::default()
    };
    assert!(matches!(
        import(&docx_of(&two_rows), Format::Docx, &id_text(), &cells),
        Err(ReqError::Limit { what: "cells", .. })
    ));
}

#[test]
fn workbook_corners_bad_references_booleans_absolute_targets_and_merges_without_a_value() {
    let sheet = r#"<sheetData>
<row r="1"><c r="A1" t="inlineStr"><is><t>id</t></is></c><c r="B1" t="inlineStr"><is><t>text</t></is></c></row>
<row r="2"><c r="??" t="inlineStr"><is><t>ignored</t></is></c><c t="inlineStr"><is><t>no reference</t></is></c><c r="A2" t="inlineStr"><is><t>R-1</t></is></c><c r="B2" t="b"><v>1</v></c></row>
<row r="3"><c r="A3" t="inlineStr"><is><t>R-2</t></is></c><c r="B3" t="b"><v>0</v></c></row>
</sheetData><mergeCells count="3"><mergeCell ref="C9:C10"/><mergeCell ref="nonsense"/><mergeCell ref="A1:??"/></mergeCells>"#;
    let set = import(
        &xlsx_with("/xl/worksheets/sheet1.xml", sheet),
        Format::Xlsx,
        &pack("[table]\nname = \"s\"\n[columns]\nid = \"id\"\ntext = \"text\"\n"),
        &ImportLimits::default(),
    )
    .unwrap();
    assert_eq!(
        set.requirements
            .iter()
            .map(|r| (r.id.as_str(), r.text.as_str()))
            .collect::<Vec<_>>(),
        [("R-1", "TRUE"), ("R-2", "FALSE")]
    );
    let cols = ImportLimits {
        max_cols: 1,
        ..ImportLimits::default()
    };
    assert!(matches!(
        import(&xlsx_with("worksheets/sheet1.xml", sheet), Format::Xlsx, &id_text(), &cols),
        Err(ReqError::Limit { what: "columns", .. })
    ));
    let missing = import(
        &xlsx_with("worksheets/gone.xml", sheet),
        Format::Xlsx,
        &id_text(),
        &ImportLimits::default(),
    )
    .unwrap_err();
    assert!(missing.to_string().contains("gone.xml is missing"), "{missing}");
}

#[test]
fn fill_down_reaches_short_rows_and_long_multibyte_texts_are_cut_on_a_character_boundary() {
    let csv = "id,text,section\nR-1,One,Brakes\nR-2,Two\nR-3,Three,\n";
    let set = import(
        csv.as_bytes(),
        Format::Csv,
        &pack("[table]\nfill_down = [\"section\"]\n[columns]\nid = \"id\"\ntext = \"text\"\n[columns.extra]\nsection = \"section\"\n"),
        &ImportLimits::default(),
    )
    .unwrap();
    assert_eq!(
        set.requirements.iter().map(|r| r.fields["section"].as_str()).collect::<Vec<_>>(),
        ["Brakes", "Brakes", "Brakes"]
    );
    let long = format!("id,text\nR-1,{}\n", "€".repeat(6_000));
    let set = import(
        long.as_bytes(),
        Format::Csv,
        &pack("[columns]\nid = \"id\"\ntext = \"text\"\n"),
        &ImportLimits::default(),
    )
    .unwrap();
    assert!(set.requirements[0].text.len() <= 16 * 1024 && set.requirements[0].text.chars().all(|c| c == '€'));
    assert_eq!(set.skipped.cut, 1);
}
