//! Shapes of declarations that the golden samples do not cover: receivers, declarators, generics, odd files.

use pacenotes_parse::ParseLanguage;

use crate::support::{outline, render};

fn names(language: ParseLanguage, source: &str) -> Vec<String> {
    outline(language, source)
        .symbols
        .iter()
        .map(|s| format!("{:?} {}", s.kind, s.qualified_name))
        .collect()
}

#[test]
fn go_methods_are_qualified_by_their_receiver_type_whatever_its_shape() {
    let source = "package p\ntype Box[T any] struct{}\nfunc (b Box[T]) Get() {}\nfunc (b *Box[T]) Set() {}\nfunc (Plain) Anon() {}\nfunc (x []int) Odd() {}\n";
    let got = names(ParseLanguage::Go, source);
    assert!(
        got.contains(&"Method Box.Get".to_owned()) && got.contains(&"Method Box.Set".to_owned()),
        "{got:?}"
    );
    assert!(got.contains(&"Method Plain.Anon".to_owned()), "{got:?}");
    assert!(
        got.iter().any(|g| g.ends_with("Odd")),
        "a receiver with no type name still gives the method: {got:?}"
    );
}

#[test]
fn c_declarators_are_followed_to_the_name() {
    let source = "int *make(int n) { return 0; }\nint (*pick(int k))(int) { return 0; }\nstatic void **many(void) { return 0; }\n";
    let got = names(ParseLanguage::C, source);
    assert!(got.contains(&"Function make".to_owned()), "{got:?}");
    assert!(got.contains(&"Function many".to_owned()), "{got:?}");
    assert!(got.iter().any(|g| g.contains("pick")), "{got:?}");
}

#[test]
fn rust_impl_blocks_name_their_type_with_or_without_generics() {
    let source = "struct A; struct B<T>(T);\nimpl A { fn a() {} }\nimpl<T> B<T> { fn b() {} }\nimpl std::fmt::Display for A { fn fmt(&self) {} }\nimpl<'a> Tr for &'a A { fn t() {} }\n";
    let got = names(ParseLanguage::Rust, source);
    for want in ["Method A::a", "Method B::b", "Method A::fmt"] {
        assert!(got.contains(&want.to_owned()), "{want} in {got:?}");
    }
}

#[test]
fn nested_functions_and_locals_are_not_symbols() {
    let source = "function outer() {\n  const inner = () => 1;\n  function deep() {}\n  return inner;\n}\n";
    let got = names(ParseLanguage::JavaScript, source);
    assert!(got.contains(&"Function outer".to_owned()), "{got:?}");
    assert!(
        !got.iter().any(|g| g.contains("inner")),
        "a local arrow function is not listed: {got:?}"
    );
}

#[test]
fn calls_name_their_caller_and_unnamed_callees_are_skipped() {
    let o = outline(ParseLanguage::JavaScript, "function f() { g(); (function () {})(); x.y.z(1); }\n");
    let callees: Vec<_> = o.calls.iter().map(|c| c.callee.as_str()).collect();
    assert!(callees.contains(&"g") && callees.contains(&"z"), "{callees:?}");
    assert!(o.calls.iter().all(|c| c.caller.as_deref() == Some("f")), "{:?}", render(&o));
}

#[test]
fn a_file_without_symbols_is_chunked_in_windows() {
    let source = "x = 1\n".repeat(130);
    let o = outline(ParseLanguage::Python, &source);
    assert!(o.symbols.is_empty());
    assert_eq!(o.chunks.len(), 3);
    assert!(o.chunks.iter().all(|c| c.symbol.is_none()));
}

#[test]
fn a_class_with_methods_is_chunked_by_its_methods() {
    let source = "class A:\n    def f(self):\n        pass\n\n    def g(self):\n        pass\n";
    let o = outline(ParseLanguage::Python, source);
    let chunked: Vec<_> = o.chunks.iter().filter_map(|c| c.symbol.as_deref()).collect();
    assert_eq!(chunked, ["A.f", "A.g"]);
}

#[test]
fn imports_lose_their_keyword_and_quotes() {
    let o = outline(ParseLanguage::TypeScript, "import x from 'a/b';\nimport \"side-effect\";\n");
    assert_eq!(
        o.imports.iter().map(|i| i.target.as_str()).collect::<Vec<_>>(),
        ["a/b", "side-effect"]
    );
    let o = outline(ParseLanguage::CSharp, "using System.IO;\nusing static System.Math;\n");
    assert_eq!(
        o.imports.iter().map(|i| i.target.as_str()).collect::<Vec<_>>(),
        ["System.IO", "static System.Math"]
    );
}
