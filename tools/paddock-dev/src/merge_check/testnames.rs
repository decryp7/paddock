//! Test names in a source file: Rust `#[test]` functions (also `#[tokio::test]`, `rstest`, `test_case`) read with `syn`, and
//! vitest or Playwright `it(...)` / `test(...)` calls read with a pattern. Limits: tests a macro generates are not seen, and
//! `cargo nextest list` is not used (it would build every parent).

use regex::Regex;

/// The test names in `text`, a file at `path`; empty for a file of another kind. A Rust file that does not parse is an error
/// (the check cannot see its tests, so it reports "not measured").
pub fn names(path: &str, text: &str) -> Result<Vec<String>, String> {
    if path.ends_with(".rs") {
        rust(path, text)
    } else if [".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"].iter().any(|e| path.ends_with(e)) {
        Ok(script(text))
    } else {
        Ok(Vec::new())
    }
}

/// Whether the check reads this path at all.
pub fn is_source(path: &str) -> bool {
    [".rs", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"]
        .iter()
        .any(|e| path.ends_with(e))
}

fn rust(path: &str, text: &str) -> Result<Vec<String>, String> {
    let file = syn::parse_file(text).map_err(|e| format!("{path} does not parse as Rust: {e}"))?;
    let mut out = Vec::new();
    walk(&file.items, &mut out);
    Ok(out)
}

fn is_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path()
            .segments
            .last()
            .is_some_and(|s| matches!(s.ident.to_string().as_str(), "test" | "rstest" | "test_case"))
    })
}

fn walk(items: &[syn::Item], out: &mut Vec<String>) {
    for item in items {
        match item {
            syn::Item::Fn(f) if is_test(&f.attrs) => out.push(f.sig.ident.to_string()),
            syn::Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    walk(inner, out);
                }
            }
            syn::Item::Impl(i) => {
                for it in &i.items {
                    if let syn::ImplItem::Fn(f) = it
                        && is_test(&f.attrs)
                    {
                        out.push(f.sig.ident.to_string());
                    }
                }
            }
            _ => {}
        }
    }
}

fn script(text: &str) -> Vec<String> {
    // `it(`, `test(` and their `.only` / `.skip` / `.fixme` forms, with a quoted name; never `x.test('...')` (a method) and
    // never `test.describe` or `test.step`.
    let call = Regex::new(
        r#"(?:^|[^.\w$])(?:it|test)(?:\.(?:only|skip|fixme|fails|concurrent|todo))?\s*\(\s*(?:'((?:\\.|[^'\\])*)'|"((?:\\.|[^"\\])*)"|`((?:\\.|[^`\\])*)`)"#,
    );
    let Ok(call) = call else { return Vec::new() };
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
            continue;
        }
        for c in call.captures_iter(line) {
            if let Some(m) = c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3)) {
                out.push(m.as_str().to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_tests_in_nested_modules_and_impls() {
        let src = "#[test]\nfn a() {}\n#[tokio::test]\nasync fn b() {}\n#[cfg(test)]\nmod t {\n    #[test]\n    #[ignore]\n    fn c() {}\n    fn not_a_test() {}\n}\nfn d() {}\n";
        assert_eq!(names("x.rs", src).unwrap(), vec!["a", "b", "c"]);
        assert!(names("x.rs", "fn (").is_err());
    }

    #[test]
    fn script_tests_not_methods_or_comments() {
        let src = "it('one', () => {});\n  test.only(\"two\", async () => {});\n// it('commented', 1)\nre.test('method');\ntest.describe('group', () => {\n  test(`three`, () => {});\n});\nsubmit('x');\n";
        assert_eq!(names("x.spec.ts", src).unwrap(), vec!["one", "two", "three"]);
        assert!(names("README.md", "it('x')").unwrap().is_empty());
    }
}
