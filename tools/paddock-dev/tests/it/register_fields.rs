//! The register declarations against the typed structs (ER-40, ER-42): every real register parses into its structs, an unknown key in
//! each is refused, and the `fields` lists of docs/registers.toml are the structs' own, regenerated with
//! `PADDOCK_GOLDEN_BLESS=1 cargo nextest run -E 'test(register_fields)'`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::check::repo_root;
use paddock_dev::register_schema::{fields_of, parse_file};

const REGISTERS: &str = "docs/registers.toml";

fn text(rel: &str) -> String {
    std::fs::read_to_string(repo_root().join(rel)).unwrap()
}

/// `(file, table)` of every declared entry, in file order.
fn entries() -> Vec<(String, String)> {
    let doc: toml::Table = text(REGISTERS).parse().unwrap();
    doc["register"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["file"].as_str().unwrap().to_string(), r["table"].as_str().unwrap().to_string()))
        .collect()
}

#[test]
fn every_real_register_parses_into_its_typed_structs() {
    let files: std::collections::BTreeSet<String> = entries().into_iter().map(|(f, _)| f).collect();
    assert!(files.len() >= 10, "the declaration names the registers ({files:?})");
    for f in &files {
        parse_file(f, &text(f)).unwrap_or_else(|e| panic!("{e}"));
    }
}

#[test]
fn every_register_refuses_an_unknown_key_at_the_top_and_inside_each_of_its_tables() {
    let mut refused = 0;
    for (file, table) in entries() {
        let original = text(&file);
        let top = format!("zz_unknown_key = 1\n{original}");
        let err = parse_file(&file, &top).expect_err(&format!("{file}: an unknown top-level key is refused"));
        assert!(err.contains("zz_unknown_key") || err.contains("unknown"), "{file}: {err}");
        refused += 1;
        if fields_of(&file, &table).is_some() {
            let header = format!("[[{table}]]\n");
            if let Some(at) = original.find(&header).or_else(|| original.find(&format!("[{table}]\n"))) {
                let end = at + original[at..].find('\n').unwrap() + 1;
                let inner = format!("{}zz_unknown_key = 1\n{}", &original[..end], &original[end..]);
                let err = parse_file(&file, &inner).expect_err(&format!("{file} [{table}]: an unknown key is refused"));
                assert!(err.contains("zz_unknown_key"), "{file} [{table}]: {err}");
                refused += 1;
            }
        }
    }
    assert!(refused >= 25, "the refusals were measured on many tables ({refused}), not on none");
}

/// The `fields = [..]` line of each entry, rewritten from the structs (a line edit: the file keeps its comments and order).
fn regenerated(original: &str) -> String {
    let mut out = String::new();
    let (mut file, mut table) = (String::new(), String::new());
    for line in original.lines() {
        if let Some(v) = line.strip_prefix("file = \"") {
            file = v.trim_end_matches('"').to_string();
        } else if let Some(v) = line.strip_prefix("table = \"") {
            table = v.trim_end_matches('"').to_string();
        }
        if line.starts_with("fields = [")
            && let Some(f) = fields_of(&file, &table)
        {
            let list: Vec<String> = f.iter().map(|k| format!("\"{k}\"")).collect();
            out.push_str(&format!("fields = [{}]\n", list.join(", ")));
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[test]
fn the_fields_of_the_declaration_are_generated_from_the_structs() {
    let original = text(REGISTERS);
    let want = regenerated(&original);
    if crate::golden::bless() {
        std::fs::write(repo_root().join(REGISTERS), &want).unwrap();
        return;
    }
    assert_eq!(
        original, want,
        "docs/registers.toml `fields` differ from the structs of register_schema.rs (ER-40): regenerate with `PADDOCK_GOLDEN_BLESS=1 cargo nextest run -E 'test(register_fields)'` in tools/paddock-dev"
    );
}

#[test]
fn a_hand_edited_fields_list_is_what_the_generation_check_sees() {
    let original = text(REGISTERS);
    let edited = original.replacen("\"contract\", \"by\", ", "\"contract\", ", 1);
    assert_ne!(edited, original, "the control found the list it edits");
    assert_ne!(regenerated(&edited), edited);
}
