//! Every node kind and field name that a language table (or the extractor) uses exists in the grammar it is loaded from, so
//! a grammar update that renames one fails here and not silently in the outline.

use pacenotes_parse::ParseLanguage;
use pacenotes_parse::spec::{Names, spec};

fn kinds_and_fields(language: ParseLanguage) -> (Vec<&'static str>, Vec<&'static str>) {
    let table = spec(language);
    let mut kinds = Vec::new();
    let mut fields = Vec::new();
    for r in table.symbols {
        kinds.push(r.kind);
        if let Some(f) = r.needs_field {
            fields.push(f);
        }
        match r.names {
            Names::Field(f) | Names::AllFields(f) | Names::FieldOr(f, _) => fields.push(f),
            Names::CDeclarator | Names::TypedefDeclarator => fields.push("declarator"),
            Names::GoMethod => fields.extend(["name", "receiver", "type"]),
            Names::ImplType => fields.push("type"),
            Names::Literal(_) | Names::Declarator => {}
        }
    }
    for r in table.imports {
        kinds.push(r.kind);
        fields.extend(r.field);
    }
    for r in table.calls {
        kinds.push(r.kind);
        if !r.callee.is_empty() {
            fields.push(r.callee);
        }
    }
    (kinds, fields)
}

/// Kinds and fields the extractor names in code (`last_name`, the declarators, the refine functions), per language.
fn used_in_code(language: ParseLanguage) -> (&'static [&'static str], &'static [&'static str]) {
    match language {
        ParseLanguage::Go => (
            &[
                "struct_type",
                "interface_type",
                "parameter_declaration",
                "type_identifier",
                "identifier",
                "field_identifier",
            ],
            &["type", "receiver", "function", "field", "name"],
        ),
        ParseLanguage::C | ParseLanguage::Cpp => (
            &["identifier", "field_identifier", "function_declarator", "pointer_declarator"],
            &["declarator", "function", "field"],
        ),
        ParseLanguage::Java => (&["variable_declarator", "identifier"], &["declarator", "name"]),
        ParseLanguage::CSharp => (
            &["variable_declarator", "variable_declaration", "identifier"],
            &["name", "function"],
        ),
        ParseLanguage::TypeScript | ParseLanguage::Tsx | ParseLanguage::JavaScript => (
            &["variable_declarator", "arrow_function", "identifier", "property_identifier"],
            &["name", "value", "function", "property"],
        ),
        ParseLanguage::Rust => (
            &[
                "generic_type",
                "type_identifier",
                "scoped_identifier",
                "identifier",
                "field_identifier",
            ],
            &["type", "name", "function", "macro", "field"],
        ),
        ParseLanguage::Python => (&["identifier"], &["attribute", "function", "name"]),
        ParseLanguage::Kotlin => (
            &["interface", "modifiers", "class_modifier", "identifier", "variable_declaration"],
            &["name"],
        ),
    }
}

#[test]
fn every_kind_and_field_in_every_table_exists_in_its_grammar() {
    for language in ParseLanguage::ALL {
        let grammar = language.grammar().unwrap();
        let (mut kinds, mut fields) = kinds_and_fields(language);
        let (more_kinds, more_fields) = used_in_code(language);
        kinds.extend(more_kinds);
        fields.extend(more_fields);
        for kind in kinds {
            // a named node kind has a non-zero id; the keyword `interface` of Kotlin is an anonymous token
            let named = grammar.id_for_node_kind(kind, true) != 0;
            let anonymous = grammar.id_for_node_kind(kind, false) != 0;
            assert!(named || anonymous, "{}: the grammar has no node kind {kind:?}", language.id());
        }
        for field in fields {
            assert!(
                grammar.field_id_for_name(field).is_some(),
                "{}: the grammar has no field {field:?}",
                language.id()
            );
        }
    }
}

#[test]
fn the_tables_have_no_rule_twice_and_every_language_has_symbols_and_calls() {
    for language in ParseLanguage::ALL {
        let table = spec(language);
        let mut seen = std::collections::BTreeSet::new();
        for r in table.symbols {
            assert!(seen.insert(r.kind), "{}: {} twice", language.id(), r.kind);
        }
        assert!(!table.symbols.is_empty() && !table.calls.is_empty(), "{}", language.id());
    }
}
