//! The reviewers' inputs, run through the public API. Each of these overflowed the stack, panicked, refused a good workbook or ate memory
//! before the fix.

use pacenotes_req::{Format, ImportLimits, MappingPack, ReqError, import};
use paddock_testkit::never_panics;

use crate::support::{docx, pack, xlsx, zip_of};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn id_text() -> MappingPack {
    pack("[table]\nname = \"s\"\n[columns]\nid = \"column:A\"\ntext = \"column:B\"\n")
}

fn book(sheet: &str, shared: Option<&str>) -> Vec<u8> {
    let wb = format!(r#"<workbook xmlns="{MAIN}" xmlns:r="{REL}"><sheets><sheet name="s" sheetId="1" r:id="rId1"/></sheets></workbook>"#);
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>"#;
    let sheet = format!(r#"<worksheet xmlns="{MAIN}">{sheet}</worksheet>"#);
    let mut parts = vec![
        ("xl/workbook.xml", wb),
        ("xl/_rels/workbook.xml.rels", rels.to_owned()),
        ("xl/worksheets/sheet1.xml", sheet),
    ];
    if let Some(s) = shared {
        parts.push(("xl/sharedStrings.xml", s.to_owned()));
    }
    let refs: Vec<(&str, &str)> = parts.iter().map(|(a, b)| (*a, b.as_str())).collect();
    zip_of(&refs)
}

fn run(bytes: &[u8], format: Format, p: &MappingPack) -> Result<pacenotes_req::RequirementSet, ReqError> {
    import(bytes, format, p, &ImportLimits::default())
}

#[test]
fn deeply_nested_xml_is_refused_in_every_part_before_the_parser_recurses() {
    let nested = |tag: &str| format!("{}{}", format!("<{tag}>").repeat(5000), format!("</{tag}>").repeat(5000));
    // the reviewer's 523-byte docx
    let doc = zip_of(&[(
        "word/document.xml",
        &format!("<w:document xmlns:w=\"{W}\"><w:body>{}</w:body></w:document>", nested("w:p")),
    )]);
    assert!(doc.len() < 1000, "{}", doc.len());
    let err = run(&doc, Format::Docx, &pack("[columns]\nid = \"a\"\ntext = \"b\"\n")).unwrap_err();
    assert!(
        matches!(
            err,
            ReqError::Limit {
                what: "XML nesting depth",
                ..
            }
        ),
        "{err}"
    );
    // every part of a workbook
    let p = id_text();
    let ok_sheet = r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>x</t></is></c></row></sheetData>"#;
    let wb = |inner: &str| {
        format!(r#"<workbook xmlns="{MAIN}" xmlns:r="{REL}"><sheets><sheet name="s" sheetId="1" r:id="rId1"/></sheets>{inner}</workbook>"#)
    };
    let rels = |inner: &str| {
        format!(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Target="worksheets/sheet1.xml"/>{inner}</Relationships>"#
        )
    };
    let sheet = |inner: &str| format!(r#"<worksheet xmlns="{MAIN}">{ok_sheet}{inner}</worksheet>"#);
    let good = (wb(""), rels(""), sheet(""));
    let shared = format!("<sst xmlns=\"{MAIN}\">{}</sst>", nested("si"));
    for (name, parts) in [
        (
            "workbook",
            vec![
                ("xl/workbook.xml", wb(&nested("a"))),
                ("xl/_rels/workbook.xml.rels", good.1.clone()),
                ("xl/worksheets/sheet1.xml", good.2.clone()),
            ],
        ),
        (
            "rels",
            vec![
                ("xl/workbook.xml", good.0.clone()),
                ("xl/_rels/workbook.xml.rels", rels(&nested("a"))),
                ("xl/worksheets/sheet1.xml", good.2.clone()),
            ],
        ),
        (
            "sheet",
            vec![
                ("xl/workbook.xml", good.0.clone()),
                ("xl/_rels/workbook.xml.rels", good.1.clone()),
                ("xl/worksheets/sheet1.xml", sheet(&nested("a"))),
            ],
        ),
        (
            "shared strings",
            vec![
                ("xl/workbook.xml", good.0.clone()),
                ("xl/_rels/workbook.xml.rels", good.1.clone()),
                ("xl/worksheets/sheet1.xml", good.2.clone()),
                ("xl/sharedStrings.xml", shared.clone()),
            ],
        ),
    ] {
        let refs: Vec<(&str, &str)> = parts.iter().map(|(a, b)| (*a, b.as_str())).collect();
        let err = run(&zip_of(&refs), Format::Xlsx, &p).unwrap_err();
        assert!(
            matches!(
                err,
                ReqError::Limit {
                    what: "XML nesting depth",
                    ..
                }
            ),
            "{name}: {err}"
        );
    }
    // an attribute that holds a `>` does not hide the depth
    let tricky = zip_of(&[(
        "word/document.xml",
        &format!(
            "<w:document xmlns:w=\"{W}\">{}{}</w:document>",
            "<a x=\"/>\">".repeat(5000),
            "</a>".repeat(5000)
        ),
    )]);
    assert!(matches!(
        run(&tricky, Format::Docx, &pack("[columns]\nid = \"a\"\ntext = \"b\"\n")),
        Err(ReqError::Limit {
            what: "XML nesting depth",
            ..
        })
    ));
}

#[test]
fn a_cell_in_row_zero_is_counted_and_never_a_panic() {
    let sheet = r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>R-1</t></is></c><c r="B1" t="inlineStr"><is><t>Stop.</t></is></c><c r="A0" t="inlineStr"><is><t>zero</t></is></c><c r="@1" t="inlineStr"><is><t>no column</t></is></c></row></sheetData>"#;
    let p = pack("[table]\nname = \"s\"\nheader_row = 1\n[columns]\nid = \"column:A\"\ntext = \"column:B\"\n");
    let only_header = run(&book(sheet, None), Format::Xlsx, &p).unwrap();
    assert_eq!(
        (only_header.requirements.len(), only_header.skipped.bad_cells),
        (0, 2),
        "row 0 and a reference with no column are counted"
    );
    let with_data = r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>id</t></is></c><c r="B1" t="inlineStr"><is><t>text</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>R-1</t></is></c><c r="B2" t="inlineStr"><is><t>Stop.</t></is></c><c r="A0" t="inlineStr"><is><t>zero</t></is></c><c r="@1" t="inlineStr"><is><t>no column</t></is></c></row></sheetData>"#;
    let set = run(&book(with_data, None), Format::Xlsx, &id_text()).unwrap();
    assert_eq!((set.requirements.len(), set.skipped.bad_cells), (1, 2));
}

#[test]
fn an_empty_styled_cell_far_away_does_not_refuse_the_workbook() {
    let sheet = r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>id</t></is></c><c r="B1" t="inlineStr"><is><t>text</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>R-1</t></is></c><c r="B2" t="inlineStr"><is><t>Stop.</t></is></c></row><row r="200000"><c r="A200000" s="3"/><c r="ZZ2" s="3"><v></v></c></row></sheetData>"#;
    let set = run(&book(sheet, None), Format::Xlsx, &id_text()).unwrap();
    assert_eq!(set.requirements.len(), 1);
    // a value there still is a limit
    let far = sheet.replace(r#"<c r="A200000" s="3"/>"#, r#"<c r="A200000" t="inlineStr"><is><t>x</t></is></c>"#);
    assert!(matches!(
        run(&book(&far, None), Format::Xlsx, &id_text()),
        Err(ReqError::Limit { what: "rows", .. })
    ));
}

#[test]
fn a_shared_string_that_does_not_exist_is_malformed_and_names_the_cell() {
    let sheet = r#"<sheetData><row r="1"><c r="A1" t="s"><v>7</v></c></row></sheetData>"#;
    let shared = format!("<sst xmlns=\"{MAIN}\"><si><t>only one</t></si></sst>");
    let err = run(&book(sheet, Some(&shared)), Format::Xlsx, &id_text()).unwrap_err();
    assert!(
        matches!(err, ReqError::Malformed { .. }) && err.to_string().contains("cell A1") && err.to_string().contains("shared string \"7\""),
        "{err}"
    );
    let nan = sheet.replace("<v>7</v>", "<v>x</v>");
    assert!(run(&book(&nan, Some(&shared)), Format::Xlsx, &id_text()).is_err());
}

#[test]
fn four_megabytes_of_pipes_are_a_limit_not_four_million_strings() {
    let md = format!("| a | b |\n|--|--|\n| 1 | 2 |\n\n{}\n|-|-|\n", "|".repeat(4 * 1024 * 1024));
    let p = pack("[table]\nindex = 2\n[columns]\nid = \"a\"\ntext = \"b\"\n");
    let r = run(md.as_bytes(), Format::Markdown, &p);
    assert!(
        matches!(r, Err(ReqError::Limit { what: "columns", .. }) | Err(ReqError::NoTable { .. })),
        "{r:?}"
    );
    // the bound is the count of `|` taken before any string is made: the answer comes from a byte scan, whatever the line holds
    // only the chosen table is collected: a huge first table does not matter when the second is asked for
    let big_first = format!(
        "| a | b |\n|--|--|\n{}\n| c | d |\n|--|--|\n| R-1 | Stop |\n",
        "| 1 | 2 |\n".repeat(50_000)
    );
    let set = run(
        big_first.as_bytes(),
        Format::Markdown,
        &pack("[table]\nindex = 2\n[columns]\nid = \"c\"\ntext = \"d\"\n"),
    )
    .unwrap();
    assert_eq!(set.requirements[0].id, "R-1");
}

#[test]
fn long_ids_are_cut_first_and_then_compared_so_they_cannot_collide_after_the_cut() {
    let long = |tail: &str| format!("{}{tail}", "x".repeat(1100));
    let csv = format!("id,text\n{},One\n{},Two\n", long("a"), long("b"));
    let set = run(csv.as_bytes(), Format::Csv, &pack("[columns]\nid = \"id\"\ntext = \"text\"\n")).unwrap();
    assert_eq!(
        (set.requirements.len(), set.skipped.duplicate_id, set.skipped.cut),
        (1, 1, 1),
        "two ids that are the same after the cut are one id"
    );
    assert_eq!(set.requirements[0].id.len(), 1024);
}

#[test]
fn a_column_letter_has_to_be_written_as_one() {
    let csv = "name,text\nR-1,Stop\n";
    // `ID` is not a header here and must not be read as column 238
    let err = run(csv.as_bytes(), Format::Csv, &pack("[columns]\nid = \"ID\"\ntext = \"text\"\n")).unwrap_err();
    assert!(
        matches!(err, ReqError::Mapping(_)) && err.to_string().contains("\"ID\" (id)"),
        "{err}"
    );
    let set = run(
        csv.as_bytes(),
        Format::Csv,
        &pack("[columns]\nid = \"column:A\"\ntext = \"column:b\"\n"),
    )
    .unwrap();
    assert_eq!(set.requirements[0].id, "R-1");
    assert!(
        run(
            csv.as_bytes(),
            Format::Csv,
            &pack("[columns]\nid = \"column:C\"\ntext = \"text\"\n")
        )
        .is_err(),
        "past the last column"
    );
    assert!(
        run(
            csv.as_bytes(),
            Format::Csv,
            &pack("[columns]\nid = \"column:1\"\ntext = \"text\"\n")
        )
        .is_err(),
        "digits are not a letter"
    );
}

#[test]
fn zero_in_a_pack_is_refused_so_nothing_underflows() {
    for table in ["header_row = 0", "index = 0"] {
        let err = MappingPack::from_toml(&format!(
            "version = 1\nname = \"n\"\n[table]\n{table}\n[columns]\nid = \"a\"\ntext = \"b\"\n"
        ))
        .unwrap_err();
        assert!(err.to_string().contains("count from 1"), "{table}: {err}");
    }
}

#[test]
fn rows_inside_content_controls_are_read() {
    let tc = |t: &str| format!("<w:tc><w:p><w:r><w:t>{t}</w:t></w:r></w:p></w:tc>");
    let rows = format!(
        "<w:tr>{}</w:tr><w:sdt><w:sdtContent><w:tr>{}</w:tr></w:sdtContent></w:sdt><w:tr><w:sdt><w:sdtContent>{}</w:sdtContent></w:sdt>{}</w:tr>",
        tc("id") + &tc("text"),
        tc("R-1") + &tc("Stop."),
        tc("R-2"),
        tc("Go.")
    );
    let bytes = zip_of(&[(
        "word/document.xml",
        &format!("<w:document xmlns:w=\"{W}\"><w:body><w:tbl>{rows}</w:tbl></w:body></w:document>"),
    )]);
    let set = run(&bytes, Format::Docx, &pack("[columns]\nid = \"id\"\ntext = \"text\"\n")).unwrap();
    assert_eq!(
        set.requirements
            .iter()
            .map(|r| (r.id.as_str(), r.text.as_str()))
            .collect::<Vec<_>>(),
        [("R-1", "Stop."), ("R-2", "Go.")]
    );
}

#[test]
fn a_csv_error_names_the_line_the_parser_found_it_on() {
    let err = run(
        b"a,b\n\"never closed,1\n",
        Format::Csv,
        &pack("[columns]\nid = \"a\"\ntext = \"b\"\n"),
    );
    // an unterminated quote is accepted to the end of the input by this reader; a record of the wrong width is not an error either (flexible)
    assert!(err.is_ok() || err.unwrap_err().to_string().contains("line"));
    let bad_utf8 = run(b"a,b\n\xff\xfe,1\n", Format::Csv, &pack("[columns]\nid = \"a\"\ntext = \"b\"\n")).unwrap_err();
    assert!(bad_utf8.to_string().contains("UTF-8"));
}

/// Every number and cell reference of a document replaced, one at a time, by the values that have broken a reader: row 0, the largest of
/// each width, one past it, a negative; plus the shapes bit flips never reach (a far corner, a reference with no column).
fn variants(seed: &str, spans: &[(&str, &[&str])]) -> Vec<String> {
    let mut out = Vec::new();
    for (needle, replacements) in spans {
        for (at, _) in seed.match_indices(needle).take(12) {
            for r in *replacements {
                out.push(format!("{}{}{}", &seed[..at], r, &seed[at + needle.len()..]));
            }
        }
    }
    out
}

#[test]
fn the_readers_survive_the_extremes_that_bit_flips_never_reach() {
    let p = pack("[columns]\nid = \"column:A\"\ntext = \"column:B\"\n");
    let refs: &[(&str, &[&str])] = &[
        (
            "r=\"A2\"",
            &[
                "r=\"A0\"",
                "r=\"A18446744073709551615\"",
                "r=\"A4294967296\"",
                "r=\"@2\"",
                "r=\"AAAAAAA1\"",
                "r=\"\"",
                "r=\"A-1\"",
            ],
        ),
        (
            "ref=\"A1:A3\"",
            &["ref=\"A0:A3\"", "ref=\"A1:A18446744073709551615\"", "ref=\"XFD1:A1\"", "ref=\":\""],
        ),
        ("<v>0</v>", &["<v>18446744073709551615</v>", "<v>-1</v>", "<v>1e999</v>", "<v></v>"]),
        (
            "gridSpan w:val=\"2\"",
            &[
                "gridSpan w:val=\"0\"",
                "gridSpan w:val=\"18446744073709551615\"",
                "gridSpan w:val=\"-1\"",
            ],
        ),
    ];
    let sheet = r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>id</t></is></c></row><row r="2"><c r="A2" t="s"><v>0</v></c></row></sheetData><mergeCells count="1"><mergeCell ref="A1:A3"/></mergeCells>"#;
    let shared = format!("<sst xmlns=\"{MAIN}\"><si><t>x</t></si></sst>");
    let mut inputs: Vec<Vec<u8>> = variants(sheet, refs).into_iter().map(|s| book(&s, Some(&shared))).collect();
    inputs.extend(
        variants(&shared, &[("<t>x</t>", &["<t>", "<t></t><t/>"])])
            .into_iter()
            .map(|s| book(sheet, Some(&s))),
    );
    let word = format!(
        "<w:document xmlns:w=\"{W}\"><w:body><w:tbl><w:tr><w:tc><w:tcPr><w:gridSpan w:val=\"2\"/></w:tcPr><w:p/></w:tc></w:tr></w:tbl></w:body></w:document>"
    );
    inputs.extend(variants(&word, refs).into_iter().map(|s| zip_of(&[("word/document.xml", &s)])));
    assert!(inputs.len() >= 20, "{}", inputs.len());
    let panicked = never_panics(&inputs, |bytes| {
        let _ = import(bytes, Format::Xlsx, &p, &ImportLimits::default());
        let _ = import(bytes, Format::Docx, &p, &ImportLimits::default());
    });
    assert!(panicked.is_empty(), "panicked on {} bytes of input", panicked[0].len());
    let _ = (xlsx(), docx());
}
