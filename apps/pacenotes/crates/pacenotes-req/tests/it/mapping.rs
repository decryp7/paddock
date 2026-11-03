//! The mapping pack and the mapping of rows.

use pacenotes_req::{Format, ImportLimits, MappingPack, PackError, ReqError, import};

use crate::support::{BASIC, pack, read};

#[test]
fn a_pack_is_checked_and_every_refusal_says_what_to_fix() {
    let ok = MappingPack::from_toml("version = 1\nname = \"n\"\n[table]\nname = \"S\"\nindex = 2\nheader_row = 3\nfill_down = [\"A\"]\n[columns]\nid = \"I\"\ntext = \"T\"\ntitle = \"Ti\"\n[columns.extra]\nowner = \"Owner\"\n").unwrap();
    assert_eq!(
        (ok.table_name.as_deref(), ok.table_index.get(), ok.header_row.get()),
        (Some("S"), 2, 3)
    );
    let bad = [
        ("not toml [", "cannot be read"),
        ("version = 2\nname = \"n\"\n[columns]\nid = \"a\"\ntext = \"b\"\n", "version 2"),
        ("version = 1\nname = \" \"\n[columns]\nid = \"a\"\ntext = \"b\"\n", "short name"),
        ("version = 1\nname = \"n\"\n[columns]\nid = \"\"\ntext = \"b\"\n", "columns.id"),
        ("version = 1\nname = \"n\"\n[columns]\nid = \"a\"\n", "cannot be read"),
        (
            "version = 1\nname = \"n\"\nsurprise = 1\n[columns]\nid = \"a\"\ntext = \"b\"\n",
            "cannot be read",
        ),
        (
            "version = 1\nname = \"n\"\n[table]\nheader_row = 0\n[columns]\nid = \"a\"\ntext = \"b\"\n",
            "header_row",
        ),
        (
            "version = 1\nname = \"n\"\n[table]\nindex = 0\n[columns]\nid = \"a\"\ntext = \"b\"\n",
            "table.index",
        ),
        (
            "version = 1\nname = \"n\"\n[table]\nname = \"\"\n[columns]\nid = \"a\"\ntext = \"b\"\n",
            "table.name",
        ),
        (
            "version = 1\nname = \"n\"\n[columns]\nid = \"a\"\ntext = \"b\"\n[columns.extra]\nBad-Name = \"c\"\n",
            "not a field name",
        ),
    ];
    for (text, expect) in bad {
        let err = MappingPack::from_toml(text).unwrap_err();
        assert!(err.to_string().contains(expect), "{text:?}: {err}");
    }
    assert!(matches!(MappingPack::from_toml(&"#".repeat(70_000)), Err(PackError::Syntax(_))));
    let many: String = (0..65).map(|i| format!("f{i} = \"c\"\n")).collect();
    let err = MappingPack::from_toml(&format!(
        "version = 1\nname = \"n\"\n[columns]\nid = \"a\"\ntext = \"b\"\n[columns.extra]\n{many}"
    ))
    .unwrap_err();
    assert!(err.to_string().contains("more than 64 extra columns"), "{err}");
}

#[test]
fn rows_without_an_id_or_text_and_repeated_ids_are_counted_not_dropped_silently() {
    let csv = "Req ID,Description,Title,Priority\nR-1,One,,\n,No id,,\nR-2,,,\nR-1,Again,,\n,,,\nR-3,Three,,\n";
    let set = read(csv.as_bytes(), Format::Csv, &pack(BASIC));
    assert_eq!(set.requirements.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["R-1", "R-3"]);
    let s = set.skipped;
    assert_eq!((s.missing_id, s.missing_text, s.duplicate_id, s.empty_rows), (1, 1, 1, 1));
}

#[test]
fn a_column_that_is_not_there_is_an_error_that_lists_the_headers() {
    let err = import(
        b"a,b\n1,2\n",
        Format::Csv,
        &pack("[columns]\nid = \"Req ID\"\ntext = \"b\"\n"),
        &ImportLimits::default(),
    )
    .unwrap_err();
    assert!(
        matches!(err, ReqError::Mapping(_)) && err.to_string().contains("\"Req ID\" (id)") && err.to_string().contains("\"a\", \"b\""),
        "{err}"
    );
    let err = import(
        b"a,b\n",
        Format::Csv,
        &pack("[table]\nheader_row = 5\n[columns]\nid = \"a\"\ntext = \"b\"\n"),
        &ImportLimits::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("header row is 5"), "{err}");
    let err = import(
        b"a,b\n1,2\n",
        Format::Csv,
        &pack("[columns]\nid = \"a\"\ntext = \"b\"\ntitle = \"zz\"\n"),
        &ImportLimits::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("(title)"), "{err}");
}

#[test]
fn long_texts_are_cut_and_counted() {
    let csv = format!("id,text\nR-1,{}\n", "é".repeat(10_000));
    let set = read(csv.as_bytes(), Format::Csv, &pack("[columns]\nid = \"id\"\ntext = \"text\"\n"));
    assert!(set.requirements[0].text.len() <= 16 * 1024 && set.requirements[0].text.is_char_boundary(set.requirements[0].text.len()));
    assert_eq!(set.skipped.cut, 1);
}
