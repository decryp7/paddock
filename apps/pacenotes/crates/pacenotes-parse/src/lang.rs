//! The languages of tier 2 and their grammars (ADR 0003).

use pacenotes_core::{KnownLanguage, RepoPath};

use crate::ParseError;

/// A loaded grammar.
pub type Grammar = tree_sitter::Language;

/// A language Pacenotes parses with a compiled-in grammar, in the order of work of ADR 0003.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ParseLanguage {
    /// Go.
    Go,
    /// Java.
    Java,
    /// Kotlin.
    Kotlin,
    /// C.
    C,
    /// C++.
    Cpp,
    /// C#.
    CSharp,
    /// TypeScript.
    TypeScript,
    /// TypeScript with JSX.
    Tsx,
    /// JavaScript and JSX.
    JavaScript,
    /// Rust.
    Rust,
    /// Python.
    Python,
}

/// The ABI versions the linked runtime can load, inclusive.
pub fn abi_range() -> (usize, usize) {
    (tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION, tree_sitter::LANGUAGE_VERSION)
}

impl ParseLanguage {
    /// Every language, in the order of ADR 0003.
    pub const ALL: [ParseLanguage; 11] = [
        ParseLanguage::Go,
        ParseLanguage::Java,
        ParseLanguage::Kotlin,
        ParseLanguage::C,
        ParseLanguage::Cpp,
        ParseLanguage::CSharp,
        ParseLanguage::TypeScript,
        ParseLanguage::Tsx,
        ParseLanguage::JavaScript,
        ParseLanguage::Rust,
        ParseLanguage::Python,
    ];

    /// The id used in messages and tests.
    pub fn id(self) -> &'static str {
        match self {
            ParseLanguage::Go => "go",
            ParseLanguage::Java => "java",
            ParseLanguage::Kotlin => "kotlin",
            ParseLanguage::C => "c",
            ParseLanguage::Cpp => "cpp",
            ParseLanguage::CSharp => "csharp",
            ParseLanguage::TypeScript => "typescript",
            ParseLanguage::Tsx => "tsx",
            ParseLanguage::JavaScript => "javascript",
            ParseLanguage::Rust => "rust",
            ParseLanguage::Python => "python",
        }
    }

    /// The separator between a container and a member in qualified names.
    pub fn separator(self) -> &'static str {
        match self {
            ParseLanguage::Rust | ParseLanguage::Cpp => "::",
            _ => ".",
        }
    }

    /// The language for a known language (TSX and JSX are chosen by [`ParseLanguage::from_path`]).
    pub fn from_known(known: KnownLanguage) -> Option<ParseLanguage> {
        Some(match known {
            KnownLanguage::Go => ParseLanguage::Go,
            KnownLanguage::Java => ParseLanguage::Java,
            KnownLanguage::Kotlin => ParseLanguage::Kotlin,
            KnownLanguage::C => ParseLanguage::C,
            KnownLanguage::Cpp => ParseLanguage::Cpp,
            KnownLanguage::CSharp => ParseLanguage::CSharp,
            KnownLanguage::TypeScript => ParseLanguage::TypeScript,
            KnownLanguage::JavaScript => ParseLanguage::JavaScript,
            KnownLanguage::Rust => ParseLanguage::Rust,
            KnownLanguage::Python => ParseLanguage::Python,
            _ => return None,
        })
    }

    /// The language of a file by its extension, or `None` (the file stays at tier 1).
    pub fn from_path(path: &RepoPath) -> Option<ParseLanguage> {
        Some(match path.extension()? {
            "go" => ParseLanguage::Go,
            "java" => ParseLanguage::Java,
            "kt" | "kts" => ParseLanguage::Kotlin,
            "c" | "h" => ParseLanguage::C,
            "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => ParseLanguage::Cpp,
            "cs" => ParseLanguage::CSharp,
            "ts" | "mts" | "cts" => ParseLanguage::TypeScript,
            "tsx" => ParseLanguage::Tsx,
            "js" | "mjs" | "cjs" | "jsx" => ParseLanguage::JavaScript,
            "rs" => ParseLanguage::Rust,
            "py" | "pyi" => ParseLanguage::Python,
            _ => return None,
        })
    }

    /// The grammar, checked against the runtime's ABI range.
    pub fn grammar(self) -> Result<Grammar, ParseError> {
        let language: Grammar = match self {
            ParseLanguage::Go => tree_sitter_go::LANGUAGE.into(),
            ParseLanguage::Java => tree_sitter_java::LANGUAGE.into(),
            ParseLanguage::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
            ParseLanguage::C => tree_sitter_c::LANGUAGE.into(),
            ParseLanguage::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            ParseLanguage::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            ParseLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            ParseLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            ParseLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            ParseLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
            ParseLanguage::Python => tree_sitter_python::LANGUAGE.into(),
        };
        let (min, max) = abi_range();
        let abi = language.abi_version();
        if abi < min || abi > max {
            return Err(ParseError::Grammar {
                language: self.id(),
                why: format!("its ABI is {abi}; the runtime reads {min} to {max}"),
            });
        }
        Ok(language)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_grammar_loads_and_its_abi_is_inside_the_runtimes_range() {
        let (min, max) = abi_range();
        assert_eq!((min, max), (13, 15), "ADR 0003 records the range of tree-sitter 0.27.1");
        for language in ParseLanguage::ALL {
            let grammar = language.grammar().unwrap_or_else(|e| panic!("{e}"));
            assert!((min..=max).contains(&grammar.abi_version()), "{}", language.id());
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&grammar).unwrap_or_else(|e| panic!("{}: {e}", language.id()));
        }
    }

    #[test]
    fn languages_are_found_by_extension_and_by_known_language() {
        let p = |s: &str| RepoPath::parse(s).unwrap();
        assert_eq!(ParseLanguage::from_path(&p("a/b.go")), Some(ParseLanguage::Go));
        assert_eq!(ParseLanguage::from_path(&p("A.kt")), Some(ParseLanguage::Kotlin));
        assert_eq!(ParseLanguage::from_path(&p("x.h")), Some(ParseLanguage::C));
        assert_eq!(ParseLanguage::from_path(&p("x.hpp")), Some(ParseLanguage::Cpp));
        assert_eq!(ParseLanguage::from_path(&p("x.tsx")), Some(ParseLanguage::Tsx));
        assert_eq!(ParseLanguage::from_path(&p("x.jsx")), Some(ParseLanguage::JavaScript));
        assert_eq!(ParseLanguage::from_path(&p("Makefile")), None);
        assert_eq!(ParseLanguage::from_path(&p("a.unknown")), None);
        assert_eq!(ParseLanguage::from_known(KnownLanguage::Rust), Some(ParseLanguage::Rust));
        assert_eq!(ParseLanguage::from_known(KnownLanguage::Json), None);
        for known in KnownLanguage::ALL {
            let expected = !matches!(
                known,
                KnownLanguage::Json
                    | KnownLanguage::Yaml
                    | KnownLanguage::Toml
                    | KnownLanguage::Xml
                    | KnownLanguage::Ini
                    | KnownLanguage::Sql
                    | KnownLanguage::Markdown
            );
            assert_eq!(ParseLanguage::from_known(known).is_some(), expected, "{}", known.id());
        }
        for (file, language) in [
            ("A.java", ParseLanguage::Java),
            ("a.kts", ParseLanguage::Kotlin),
            ("a.c", ParseLanguage::C),
            ("a.cc", ParseLanguage::Cpp),
            ("a.cpp", ParseLanguage::Cpp),
            ("a.cs", ParseLanguage::CSharp),
            ("a.ts", ParseLanguage::TypeScript),
            ("a.mts", ParseLanguage::TypeScript),
            ("a.js", ParseLanguage::JavaScript),
            ("a.mjs", ParseLanguage::JavaScript),
            ("a.rs", ParseLanguage::Rust),
            ("a.py", ParseLanguage::Python),
            ("a.pyi", ParseLanguage::Python),
        ] {
            assert_eq!(ParseLanguage::from_path(&p(file)), Some(language), "{file}");
        }
        assert_eq!(ParseLanguage::Rust.separator(), "::");
        assert_eq!(ParseLanguage::Go.separator(), ".");
        let ids: std::collections::BTreeSet<_> = ParseLanguage::ALL.iter().map(|l| l.id()).collect();
        assert_eq!(ids.len(), ParseLanguage::ALL.len());
    }
}
