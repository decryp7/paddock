use std::io::Write;

use pacenotes_req::{Format, ImportLimits, MappingPack, RequirementSet, import};
use zip::write::SimpleFileOptions;

pub fn pack(extra: &str) -> MappingPack {
    MappingPack::from_toml(&format!("version = 1\nname = \"test\"\n{extra}")).unwrap()
}

pub const BASIC: &str = "[columns]\nid = \"Req ID\"\ntext = \"Description\"\ntitle = \"Title\"\n[columns.extra]\npriority = \"Priority\"\n";

pub fn read(bytes: &[u8], format: Format, pack: &MappingPack) -> RequirementSet {
    import(bytes, format, pack, &ImportLimits::default()).unwrap()
}

pub fn zip_of(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in parts {
        w.start_file(
            *name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        w.write_all(body.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// A workbook with a "Notes" sheet and a "Requirements" sheet. The second has merged cells and shared and inline strings.
pub fn xlsx() -> Vec<u8> {
    let workbook = format!(
        r#"<workbook xmlns="{MAIN}" xmlns:r="{REL}"><sheets><sheet name="Notes" sheetId="1" r:id="rId1"/><sheet name="Requirements" sheetId="2" r:id="rId2"/></sheets></workbook>"#
    );
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Target="worksheets/sheet2.xml"/></Relationships>"#;
    let shared = format!(
        r#"<sst xmlns="{MAIN}"><si><t>Section</t></si><si><t>Req ID</t></si><si><t>Description</t></si><si><t>Priority</t></si><si><t>Safety</t></si><si><r><t>The brake </t></r><r><t>shall hold.</t></r></si><si><t>high</t></si></sst>"#
    );
    let notes = format!(
        r#"<worksheet xmlns="{MAIN}"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>just notes</t></is></c></row></sheetData></worksheet>"#
    );
    let sheet = format!(
        r#"<worksheet xmlns="{MAIN}"><sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="s"><v>2</v></c><c r="D1" t="s"><v>3</v></c></row>
<row r="2"><c r="A2" t="s"><v>4</v></c><c r="B2" t="inlineStr"><is><t>SAF-1</t></is></c><c r="C2" t="s"><v>5</v></c><c r="D2" t="s"><v>6</v></c></row>
<row r="3"><c r="B3" t="inlineStr"><is><t>SAF-2</t></is></c><c r="C3" t="inlineStr"><is><t>The horn shall sound.</t></is></c><c r="D3"><v>2</v></c></row>
<row r="4"><c r="B4" t="inlineStr"><is><t>SAF-3</t></is></c><c r="C4" t="inlineStr"><is><t>Lights shall switch.</t></is></c></row>
<row r="6"><c r="B6"/></row>
</sheetData><mergeCells count="1"><mergeCell ref="A2:A4"/></mergeCells></worksheet>"#
    );
    zip_of(&[
        ("xl/workbook.xml", &workbook),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/sharedStrings.xml", &shared),
        ("xl/worksheets/sheet1.xml", &notes),
        ("xl/worksheets/sheet2.xml", &sheet),
    ])
}

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

/// A Word file with an intro table and a requirements table that has a column span and a vertical merge.
pub fn docx() -> Vec<u8> {
    let cell = |text: &str, props: &str| format!("<w:tc><w:tcPr>{props}</w:tcPr><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>");
    let row = |cells: String| format!("<w:tr>{cells}</w:tr>");
    let first = format!("<w:tbl>{}</w:tbl>", row(cell("intro", "")));
    let second = format!(
        "<w:tbl>{}{}{}{}</w:tbl>",
        row(cell("Section", "") + &cell("Req ID", "") + &cell("Description", "") + &cell("Priority", "")),
        row(cell("Brakes", "<w:vMerge w:val=\"restart\"/>") + &cell("B-1", "") + &cell("Stop <in> time.", "") + &cell("high", "")),
        row(cell("", "<w:vMerge/>")
            + &cell("B-2", "")
            + &("<w:tc><w:p><w:r><w:t>Hold on slopes.</w:t></w:r></w:p><w:p><w:r><w:t>Even loaded.</w:t></w:r></w:p></w:tc>".to_owned())
            + &cell("low", "")),
        row(cell("Spanning row", "<w:gridSpan w:val=\"2\"/>") + &cell("", "") + &cell("", "")),
    )
    .replace("<in>", "&lt;in&gt;");
    let doc = format!(r#"<w:document xmlns:w="{W}"><w:body>{first}<w:p/>{second}</w:body></w:document>"#);
    zip_of(&[("word/document.xml", &doc)])
}
