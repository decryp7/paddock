//! Per-language tables: which syntax node kinds are symbols, imports and calls. Data, not code, so a language is added by a
//! table and a golden outline (open/closed).

use pacenotes_core::SymbolKind;
use tree_sitter::Node;

use crate::lang::ParseLanguage;

/// Where a symbol's name comes from.
#[derive(Debug, Clone, Copy)]
pub enum Names {
    /// The node's `field` child.
    Field(&'static str),
    /// Every child with that field name (`var a, b int`), one symbol each.
    AllFields(&'static str),
    /// A C or C++ function definition: the identifier under the `declarator` chain.
    CDeclarator,
    /// A C or C++ declarator of a typedef: the identifier under the `declarator` chain.
    TypedefDeclarator,
    /// A Go method: its name, qualified by the receiver type.
    GoMethod,
    /// The type of a Rust `impl` block (a container, not a symbol).
    ImplType,
    /// A Kotlin function: its name, qualified by the receiver type of an extension function (`String.shout`).
    KotlinFunction,
    /// The `field` child, or this fixed name when the node has none (a Kotlin `companion object` is unnamed).
    FieldOr(&'static str, &'static str),
    /// A fixed name (a Kotlin secondary constructor has none of its own).
    Literal(&'static str),
    /// The variable name of a JavaScript or TypeScript declarator, or of a Python-style assignment target.
    Declarator,
}

/// A syntax node kind that declares a symbol (or only contains symbols, when `symbol` is `None`).
#[derive(Debug, Clone, Copy)]
pub struct SymbolRule {
    /// The tree-sitter node kind.
    pub kind: &'static str,
    /// What it declares; `None` for a container that is not itself a symbol (a Rust `impl`).
    pub symbol: Option<SymbolKind>,
    /// Decide the kind from the node (Go `type_spec`: struct, interface or alias).
    pub refine: Option<fn(Node<'_>) -> SymbolKind>,
    /// Whether symbols inside it are qualified by it.
    pub container: bool,
    /// Skip it inside a function (locals).
    pub skip_in_callable: bool,
    /// Skip it unless the node has this field (a C `struct` without a body is only a use of the type).
    pub needs_field: Option<&'static str>,
    /// The name.
    pub names: Names,
}

/// A syntax node kind that is an import.
#[derive(Debug, Clone, Copy)]
pub struct ImportRule {
    /// The node kind.
    pub kind: &'static str,
    /// The field holding the target; without it the node's text minus its keyword is used.
    pub field: Option<&'static str>,
}

/// A syntax node kind that is a call.
#[derive(Debug, Clone, Copy)]
pub struct CallRule {
    /// The node kind.
    pub kind: &'static str,
    /// The field holding the callee (name or expression).
    pub callee: &'static str,
}

/// Everything the extractor needs to know about a language.
#[derive(Debug, Clone, Copy)]
pub struct LangSpec {
    /// Symbol rules.
    pub symbols: &'static [SymbolRule],
    /// Import rules.
    pub imports: &'static [ImportRule],
    /// Call rules.
    pub calls: &'static [CallRule],
}

macro_rules! rule {
    ($kind:expr, $symbol:expr, $container:expr, $names:expr) => {
        SymbolRule {
            kind: $kind,
            symbol: Some($symbol),
            refine: None,
            container: $container,
            skip_in_callable: false,
            needs_field: None,
            names: $names,
        }
    };
}

macro_rules! local {
    ($rule:expr) => {
        SymbolRule {
            skip_in_callable: true,
            ..$rule
        }
    };
}

macro_rules! with_body {
    ($rule:expr) => {
        SymbolRule {
            needs_field: Some("body"),
            ..$rule
        }
    };
}

macro_rules! imp {
    ($kind:expr, $field:expr) => {
        ImportRule {
            kind: $kind,
            field: $field,
        }
    };
}

macro_rules! call {
    ($kind:expr, $callee:expr) => {
        CallRule {
            kind: $kind,
            callee: $callee,
        }
    };
}

use SymbolKind::{
    Class, Constant, Constructor, Enum, Field, Function, Interface, Macro, Method, Module, Property, Struct, Trait, TypeAlias, Variable,
};

fn go_type_kind(node: Node<'_>) -> SymbolKind {
    match node.child_by_field_name("type").map(|t| t.kind()) {
        Some("struct_type") => Struct,
        Some("interface_type") => Interface,
        _ => TypeAlias,
    }
}

const GO: LangSpec = LangSpec {
    symbols: &[
        rule!("function_declaration", Function, false, Names::Field("name")),
        rule!("method_declaration", Method, false, Names::GoMethod),
        SymbolRule {
            refine: Some(go_type_kind),
            container: true,
            ..rule!("type_spec", TypeAlias, true, Names::Field("name"))
        },
        rule!("method_elem", Method, false, Names::Field("name")),
        rule!("field_declaration", Field, false, Names::AllFields("name")),
        local!(rule!("const_spec", Constant, false, Names::AllFields("name"))),
        local!(rule!("var_spec", Variable, false, Names::AllFields("name"))),
    ],
    imports: &[imp!("import_spec", Some("path"))],
    calls: &[call!("call_expression", "function")],
};

const JAVA: LangSpec = LangSpec {
    symbols: &[
        rule!("class_declaration", Class, true, Names::Field("name")),
        rule!("interface_declaration", Interface, true, Names::Field("name")),
        rule!("enum_declaration", Enum, true, Names::Field("name")),
        rule!("record_declaration", Struct, true, Names::Field("name")),
        rule!("annotation_type_declaration", Interface, true, Names::Field("name")),
        rule!("method_declaration", Method, false, Names::Field("name")),
        rule!("constructor_declaration", Constructor, false, Names::Field("name")),
        local!(rule!("field_declaration", Field, false, Names::Declarator)),
        rule!("enum_constant", Constant, false, Names::Field("name")),
    ],
    imports: &[imp!("import_declaration", None)],
    calls: &[call!("method_invocation", "name"), call!("object_creation_expression", "type")],
};

fn kotlin_class_kind(node: Node<'_>) -> SymbolKind {
    let mut cursor = node.walk();
    let children: Vec<Node<'_>> = node.children(&mut cursor).collect();
    if children.iter().any(|c| c.kind() == "interface") {
        return Interface;
    }
    let mut inner = children.iter().filter(|c| c.kind() == "modifiers").flat_map(|m| {
        let mut w = m.walk();
        m.children(&mut w).collect::<Vec<_>>()
    });
    if inner.any(|c| c.kind() == "class_modifier" && c.child(0).is_some_and(|k| k.kind() == "enum")) {
        return Enum;
    }
    Class
}

const KOTLIN: LangSpec = LangSpec {
    symbols: &[
        SymbolRule {
            refine: Some(kotlin_class_kind),
            ..rule!("class_declaration", Class, true, Names::Field("name"))
        },
        rule!("object_declaration", Class, true, Names::Field("name")),
        rule!("companion_object", Class, true, Names::FieldOr("name", "Companion")),
        rule!("function_declaration", Function, false, Names::KotlinFunction),
        rule!("secondary_constructor", Constructor, false, Names::Literal("constructor")),
        local!(rule!("property_declaration", Property, false, Names::Declarator)),
        rule!("enum_entry", Constant, false, Names::Field("name")),
        rule!("type_alias", TypeAlias, false, Names::Field("type")),
    ],
    imports: &[imp!("import", None)],
    calls: &[call!("call_expression", "")],
};

const C: LangSpec = LangSpec {
    symbols: &[
        rule!("function_definition", Function, false, Names::CDeclarator),
        with_body!(rule!("struct_specifier", Struct, true, Names::Field("name"))),
        with_body!(rule!("union_specifier", Struct, true, Names::Field("name"))),
        with_body!(rule!("enum_specifier", Enum, true, Names::Field("name"))),
        rule!("type_definition", TypeAlias, false, Names::TypedefDeclarator),
        rule!("preproc_def", Macro, false, Names::Field("name")),
        rule!("preproc_function_def", Macro, false, Names::Field("name")),
    ],
    imports: &[imp!("preproc_include", Some("path"))],
    calls: &[call!("call_expression", "function")],
};

const CPP: LangSpec = LangSpec {
    symbols: &[
        rule!("function_definition", Function, false, Names::CDeclarator),
        rule!("namespace_definition", Module, true, Names::Field("name")),
        with_body!(rule!("class_specifier", Class, true, Names::Field("name"))),
        with_body!(rule!("struct_specifier", Struct, true, Names::Field("name"))),
        with_body!(rule!("union_specifier", Struct, true, Names::Field("name"))),
        with_body!(rule!("enum_specifier", Enum, true, Names::Field("name"))),
        rule!("type_definition", TypeAlias, false, Names::TypedefDeclarator),
        rule!("alias_declaration", TypeAlias, false, Names::Field("name")),
        rule!("preproc_def", Macro, false, Names::Field("name")),
        rule!("preproc_function_def", Macro, false, Names::Field("name")),
    ],
    imports: &[imp!("preproc_include", Some("path"))],
    calls: &[call!("call_expression", "function")],
};

const CSHARP: LangSpec = LangSpec {
    symbols: &[
        rule!("namespace_declaration", Module, true, Names::Field("name")),
        rule!("file_scoped_namespace_declaration", Module, true, Names::Field("name")),
        rule!("class_declaration", Class, true, Names::Field("name")),
        rule!("struct_declaration", Struct, true, Names::Field("name")),
        rule!("interface_declaration", Interface, true, Names::Field("name")),
        rule!("enum_declaration", Enum, true, Names::Field("name")),
        rule!("record_declaration", Struct, true, Names::Field("name")),
        rule!("method_declaration", Method, false, Names::Field("name")),
        rule!("constructor_declaration", Constructor, false, Names::Field("name")),
        rule!("property_declaration", Property, false, Names::Field("name")),
        local!(rule!("field_declaration", Field, false, Names::Declarator)),
        rule!("enum_member_declaration", Constant, false, Names::Field("name")),
    ],
    imports: &[imp!("using_directive", None)],
    calls: &[call!("invocation_expression", "function")],
};

const TS_SYMBOLS: &[SymbolRule] = &[
    rule!("function_declaration", Function, false, Names::Field("name")),
    rule!("generator_function_declaration", Function, false, Names::Field("name")),
    rule!("class_declaration", Class, true, Names::Field("name")),
    rule!("abstract_class_declaration", Class, true, Names::Field("name")),
    rule!("method_definition", Method, false, Names::Field("name")),
    rule!("interface_declaration", Interface, true, Names::Field("name")),
    rule!("type_alias_declaration", TypeAlias, false, Names::Field("name")),
    local!(rule!("method_signature", Method, false, Names::Field("name"))),
    local!(rule!("property_signature", Field, false, Names::Field("name"))),
    rule!("enum_declaration", Enum, true, Names::Field("name")),
    rule!("internal_module", Module, true, Names::Field("name")),
    rule!("public_field_definition", Field, false, Names::Field("name")),
    local!(rule!("variable_declarator", Variable, false, Names::Declarator)),
];

const TS_IMPORTS: &[ImportRule] = &[imp!("import_statement", Some("source"))];
const TS_CALLS: &[CallRule] = &[call!("call_expression", "function"), call!("new_expression", "constructor")];

const TYPESCRIPT: LangSpec = LangSpec {
    symbols: TS_SYMBOLS,
    imports: TS_IMPORTS,
    calls: TS_CALLS,
};

const JAVASCRIPT: LangSpec = LangSpec {
    symbols: &[
        rule!("function_declaration", Function, false, Names::Field("name")),
        rule!("generator_function_declaration", Function, false, Names::Field("name")),
        rule!("class_declaration", Class, true, Names::Field("name")),
        rule!("method_definition", Method, false, Names::Field("name")),
        rule!("field_definition", Field, false, Names::Field("property")),
        local!(rule!("variable_declarator", Variable, false, Names::Declarator)),
    ],
    imports: TS_IMPORTS,
    calls: TS_CALLS,
};

const RUST: LangSpec = LangSpec {
    symbols: &[
        rule!("function_item", Function, false, Names::Field("name")),
        rule!("function_signature_item", Function, false, Names::Field("name")),
        rule!("struct_item", Struct, true, Names::Field("name")),
        rule!("enum_item", Enum, true, Names::Field("name")),
        rule!("union_item", Struct, true, Names::Field("name")),
        rule!("trait_item", Trait, true, Names::Field("name")),
        SymbolRule {
            symbol: None,
            ..rule!("impl_item", Struct, true, Names::ImplType)
        },
        rule!("mod_item", Module, true, Names::Field("name")),
        rule!("const_item", Constant, false, Names::Field("name")),
        rule!("static_item", Constant, false, Names::Field("name")),
        rule!("type_item", TypeAlias, false, Names::Field("name")),
        rule!("macro_definition", Macro, false, Names::Field("name")),
        rule!("field_declaration", Field, false, Names::Field("name")),
        rule!("enum_variant", Constant, false, Names::Field("name")),
    ],
    imports: &[
        imp!("use_declaration", Some("argument")),
        imp!("extern_crate_declaration", Some("name")),
    ],
    calls: &[call!("call_expression", "function"), call!("macro_invocation", "macro")],
};

const PYTHON: LangSpec = LangSpec {
    symbols: &[
        rule!("function_definition", Function, false, Names::Field("name")),
        rule!("class_definition", Class, true, Names::Field("name")),
    ],
    imports: &[imp!("import_statement", None), imp!("import_from_statement", Some("module_name"))],
    calls: &[call!("call", "function")],
};

/// The tables of a language.
pub fn spec(language: ParseLanguage) -> &'static LangSpec {
    match language {
        ParseLanguage::Go => &GO,
        ParseLanguage::Java => &JAVA,
        ParseLanguage::Kotlin => &KOTLIN,
        ParseLanguage::C => &C,
        ParseLanguage::Cpp => &CPP,
        ParseLanguage::CSharp => &CSHARP,
        ParseLanguage::TypeScript | ParseLanguage::Tsx => &TYPESCRIPT,
        ParseLanguage::JavaScript => &JAVASCRIPT,
        ParseLanguage::Rust => &RUST,
        ParseLanguage::Python => &PYTHON,
    }
}
