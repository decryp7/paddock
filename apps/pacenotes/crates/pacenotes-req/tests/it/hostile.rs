//! Zip bombs, entities, huge sheets, and garbage (C36).

use pacenotes_req::{Format, ImportLimits, ReqError, RequirementSource, import, import_from};
use paddock_testkit::{corpus, never_panics};

use crate::support::{BASIC, docx, pack, xlsx, zip_of};

fn sheet_pack() -> pacenotes_req::MappingPack {
    pack("[table]\nname = \"s\"\n[columns]\nid = \"column:A\"\ntext = \"column:B\"\n")
}

fn workbook_with(sheet: &str) -> Vec<u8> {
    let main = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    let wb = format!(
        r#"<workbook xmlns="{main}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="s" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>"#;
    let sheet = format!(r#"<worksheet xmlns="{main}">{sheet}</worksheet>"#);
    zip_of(&[
        ("xl/workbook.xml", &wb),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/worksheets/sheet1.xml", &sheet),
    ])
}

#[test]
fn a_zip_bomb_is_refused_as_it_is_read_whatever_the_archive_claims() {
    // 40 MiB of one byte stored in a few kilobytes
    let bomb = zip_of(&[("xl/workbook.xml", &"a".repeat(40 * 1024 * 1024))]);
    assert!(bomb.len() < 200 * 1024, "the bomb is small: {}", bomb.len());
    let err = import(&bomb, Format::Xlsx, &sheet_pack(), &ImportLimits::default()).unwrap_err();
    assert!(
        matches!(err, ReqError::ZipBomb { .. }) && err.to_string().contains("zip bomb"),
        "{err}"
    );
    // and by size when the ratio is allowed
    let wide = ImportLimits {
        max_ratio: u64::MAX / 2,
        max_part_bytes: 1024 * 1024,
        ..ImportLimits::default()
    };
    assert!(matches!(
        import(&bomb, Format::Xlsx, &sheet_pack(), &wide),
        Err(ReqError::Limit {
            what: "decompressed bytes",
            ..
        })
    ));
}

#[test]
fn too_many_entries_and_a_crowd_of_cells_are_limits() {
    let parts: Vec<(String, String)> = (0..30).map(|i| (format!("f{i}.txt"), "x".to_owned())).collect();
    let refs: Vec<(&str, &str)> = parts.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let many = zip_of(&refs);
    let tight = ImportLimits {
        max_zip_entries: 10,
        ..ImportLimits::default()
    };
    assert!(matches!(
        import(&many, Format::Docx, &pack(BASIC), &tight),
        Err(ReqError::Limit { what: "zip entries", .. })
    ));
    let cells: String = (1..=50)
        .map(|r| format!("<row r=\"{r}\"><c r=\"A{r}\" t=\"inlineStr\"><is><t>x</t></is></c></row>"))
        .collect();
    let book = workbook_with(&format!("<sheetData>{cells}</sheetData>"));
    let few = ImportLimits {
        max_cells: 10,
        ..ImportLimits::default()
    };
    assert!(matches!(
        import(&book, Format::Xlsx, &sheet_pack(), &few),
        Err(ReqError::Limit { what: "cells", .. })
    ));
}

#[test]
fn a_cell_in_the_far_corner_does_not_allocate_a_grid_and_a_huge_merge_is_a_limit() {
    let far = workbook_with("<sheetData><row r=\"1048576\"><c r=\"XFD1048576\" t=\"inlineStr\"><is><t>x</t></is></c></row></sheetData>");
    let err = import(&far, Format::Xlsx, &sheet_pack(), &ImportLimits::default()).unwrap_err();
    assert!(
        matches!(
            err,
            ReqError::Limit {
                what: "rows" | "columns",
                ..
            }
        ),
        "{err}"
    );
    let merge = workbook_with(
        "<sheetData><row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>x</t></is></c></row></sheetData><mergeCells count=\"1\"><mergeCell ref=\"A1:IV100000\"/></mergeCells>",
    );
    let small = ImportLimits {
        max_cells: 1_000,
        ..ImportLimits::default()
    };
    assert!(matches!(
        import(&merge, Format::Xlsx, &sheet_pack(), &small),
        Err(ReqError::Limit { what: "merged cells", .. })
    ));
}

#[test]
fn a_document_type_definition_is_refused_and_nothing_is_loaded() {
    let xxe = r#"<?xml version="1.0"?><!DOCTYPE d [<!ENTITY x SYSTEM "file:///etc/passwd">]><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tr><w:tc><w:p><w:r><w:t>&x;</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#;
    let err = import(
        &zip_of(&[("word/document.xml", xxe)]),
        Format::Docx,
        &pack(BASIC),
        &ImportLimits::default(),
    )
    .unwrap_err();
    assert!(
        matches!(err, ReqError::Dtd { .. }) && err.to_string().contains("nothing was loaded"),
        "{err}"
    );
    let laughs =
        r#"<?xml version="1.0"?><!DOCTYPE lolz [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;">]><lolz>&b;</lolz>"#;
    let wb = zip_of(&[("xl/workbook.xml", laughs)]);
    assert!(matches!(
        import(&wb, Format::Xlsx, &sheet_pack(), &ImportLimits::default()),
        Err(ReqError::Dtd { .. })
    ));
}

#[test]
fn no_importer_panics_on_truncations_bit_flips_or_garbage_of_its_own_documents() {
    let p = pack("[columns]\nid = \"Req ID\"\ntext = \"Description\"\n");
    let csv = b"Req ID,Description\nR-1,\"Stop, \"\"now\"\"\"\n".to_vec();
    let md = b"| Req ID | Description |\n|---|---|\n| R-1 | Stop |\n".to_vec();
    for (seed, format) in [
        (csv, Format::Csv),
        (md, Format::Markdown),
        (xlsx(), Format::Xlsx),
        (docx(), Format::Docx),
    ] {
        let panicked = never_panics(&corpus(&seed), |bytes| {
            let _ = import(bytes, format, &p, &ImportLimits::default());
        });
        assert!(
            panicked.is_empty(),
            "{format:?} panicked on {:?}",
            String::from_utf8_lossy(&panicked[0])
        );
    }
}

struct Fixed(Vec<u8>, Format);

impl RequirementSource for Fixed {
    fn fetch(&self) -> Result<(Vec<u8>, Format), ReqError> {
        Ok((self.0.clone(), self.1))
    }
}

#[test]
fn a_source_hands_over_a_document_and_a_format_can_be_guessed_from_a_name() {
    let set = import_from(
        &Fixed(b"id,text\nR-1,Stop\n".to_vec(), Format::Csv),
        &pack("[columns]\nid = \"id\"\ntext = \"text\"\n"),
        &ImportLimits::default(),
    )
    .unwrap();
    assert_eq!(set.requirements.len(), 1);
    for (name, format) in [
        ("a.XLSX", Some(Format::Xlsx)),
        ("a.docx", Some(Format::Docx)),
        ("a.csv", Some(Format::Csv)),
        ("a.md", Some(Format::Markdown)),
        ("a.pdf", None),
        ("noext", None),
    ] {
        assert_eq!(Format::from_name(name), format, "{name}");
    }
}

#[test]
fn too_many_xml_nodes_is_a_limit() {
    let cells: String = (1..=50)
        .map(|r| format!("<row r=\"{r}\"><c r=\"A{r}\" t=\"inlineStr\"><is><t>x</t></is></c></row>"))
        .collect();
    let book = workbook_with(&format!("<sheetData>{cells}</sheetData>"));
    let few = ImportLimits {
        max_xml_nodes: 40,
        ..ImportLimits::default()
    };
    assert!(matches!(
        import(&book, Format::Xlsx, &sheet_pack(), &few),
        Err(ReqError::Limit { what: "XML nodes", .. })
    ));
}
