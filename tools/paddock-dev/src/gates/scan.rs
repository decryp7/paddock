//! The line scanners of ADR 0060 §1: ignored Rust tests (`ignored-test`) and tests that return early when an environment
//! variable is missing, or skip on one (`env-gated-test`). Scanners, not parsers: attributes over several lines and
//! `mod tests` are covered by fixtures; anything stranger is a miss the register's baseline run shows up as a surprise.

use regex::Regex;
use std::sync::LazyLock;

/// A Rust or TypeScript test that CI may not really run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TestSite {
    /// The file.
    pub path: String,
    /// The test function (or the nearest test title for a TypeScript skip).
    pub name: String,
    /// The line of the attribute or the skip.
    pub line: usize,
    /// For an ignore: the `cfg_attr` condition (`debug_assertions`), if any. For an env gate: the variable.
    pub condition: Option<String>,
}

fn re(p: &str) -> Regex {
    Regex::new(p).unwrap_or_else(|e| panic!("bad constant pattern {p}: {e}"))
}

static FN_NAME: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(?:pub\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+([A-Za-z0-9_]+)"));
static ENV_VAR: LazyLock<Regex> = LazyLock::new(|| re(r#"env::var(?:_os)?\(\s*"([A-Za-z0-9_]+)"\s*\)"#));
static GUARD_SHAPE: LazyLock<Regex> =
    LazyLock::new(|| re(r"is_err\(\)|is_none\(\)|\.ok\(\)\s*(?:\?|else)|\belse\s*\{|!=\s*Ok|unwrap_or\(false\)"));

/// The attribute that starts at `lines[i]`: its text (joined, brackets balanced) and the index of its last line.
fn attribute(lines: &[&str], i: usize) -> (String, usize) {
    let mut text = String::new();
    let mut depth = 0i32;
    let mut j = i;
    while j < lines.len() {
        let l = lines[j].trim();
        text.push_str(l);
        text.push(' ');
        for ch in l.chars() {
            match ch {
                '[' => depth += 1,
                ']' => depth -= 1,
                _ => {}
            }
        }
        if depth <= 0 {
            break;
        }
        j += 1;
    }
    (text.trim().to_string(), j.min(lines.len() - 1))
}

/// Whether an attribute text ignores a test; the second value is the `cfg_attr` condition.
fn ignore_attribute(attr: &str) -> Option<Option<String>> {
    let inner = attr.strip_prefix("#[")?.trim_end().strip_suffix(']')?.trim();
    if inner == "ignore" || inner.starts_with("ignore =") || inner.starts_with("ignore=") || inner.starts_with("ignore(") {
        return Some(None);
    }
    let args = inner.strip_prefix("cfg_attr(")?.strip_suffix(')')?;
    // `cond, attr1, attr2`: split at top-level commas.
    let mut parts: Vec<String> = Vec::new();
    let (mut depth, mut quote, mut cur) = (0i32, false, String::new());
    for ch in args.chars() {
        match ch {
            '"' => {
                quote = !quote;
                cur.push(ch);
            }
            '(' if !quote => {
                depth += 1;
                cur.push(ch);
            }
            ')' if !quote => {
                depth -= 1;
                cur.push(ch);
            }
            ',' if !quote && depth == 0 => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    parts.push(cur);
    let cond = parts.first()?.trim().to_string();
    let ignores = parts.iter().skip(1).any(|p| {
        let p = p.trim();
        p == "ignore" || p.starts_with("ignore =") || p.starts_with("ignore=")
    });
    ignores.then_some(Some(cond))
}

/// Every ignored test of a Rust file: `#[ignore]`, `#[ignore = "…"]` and `#[cfg_attr(<cond>, ignore…)]` before a `fn`.
pub fn ignored_tests(path: &str, text: &str) -> Vec<TestSite> {
    if !text.contains("ignore") {
        return Vec::new();
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let t = lines[i].trim_start();
        if t.starts_with("#[") {
            let (attr, end) = attribute(&lines, i);
            if let Some(cond) = ignore_attribute(&attr) {
                // The function: skip more attributes, comments and blank lines.
                let mut j = end + 1;
                while j < lines.len() {
                    let n = lines[j].trim_start();
                    if n.starts_with("#[") {
                        j = attribute(&lines, j).1 + 1;
                    } else if n.is_empty() || n.starts_with("//") {
                        j += 1;
                    } else {
                        break;
                    }
                }
                if let Some(name) = lines.get(j).and_then(|l| FN_NAME.captures(l)).map(|c| c[1].to_string()) {
                    out.push(TestSite {
                        path: path.to_string(),
                        name,
                        line: i + 1,
                        condition: cond,
                    });
                }
            }
            i = end + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// Rust tests whose first statements return when an environment variable is missing.
pub fn env_gated_rust(path: &str, text: &str) -> Vec<TestSite> {
    if !text.contains("env::var") {
        return Vec::new();
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut is_test = false;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        if t.starts_with("#[test]") || t.starts_with("#[tokio::test") || t.starts_with("#[rstest") || t.starts_with("#[test_case") {
            is_test = true;
            continue;
        }
        if t.starts_with("#[") || t.starts_with("//") || t.is_empty() {
            continue;
        }
        if let Some(c) = FN_NAME.captures(l)
            && is_test
        {
            {
                // The first statements of the body.
                let window: String = lines[i..(i + 8).min(lines.len())].join("\n");
                let head = window.split_once('{').map_or("", |(_, b)| b);
                // Only this function's body: stop where its braces balance.
                let mut depth = 1i32;
                let mut end = head.len();
                for (k, ch) in head.char_indices() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = k;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                let head: String = head[..end].lines().take(6).collect::<Vec<_>>().join("\n");
                if let Some(v) = ENV_VAR.captures(&head)
                    && head.contains("return")
                    && GUARD_SHAPE.is_match(&head)
                {
                    out.push(TestSite {
                        path: path.to_string(),
                        name: c[1].to_string(),
                        line: i + 1,
                        condition: Some(v[1].to_string()),
                    });
                }
            }
        }
        is_test = false;
    }
    out
}

static TS_CONST: LazyLock<Regex> = LazyLock::new(|| re(r"^(?:export\s+)?const\s+([A-Za-z0-9_]+)\s*=(.*process\.env.*)$"));
static TS_ENV_READ: LazyLock<Regex> = LazyLock::new(|| re(r#"process\.env(?:\.([A-Za-z0-9_]+)|\[\s*['"]([A-Za-z0-9_]+)['"]\s*\])"#));
static TS_SKIP: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?:test|it)(?:\.describe)?\.skip\(|\bdescribe\.skip\("));
static TS_TITLE: LazyLock<Regex> =
    LazyLock::new(|| re(r#"\b(?:test|it|describe|test\.describe)(?:\.\w+)?\(\s*(?:'([^']*)'|"([^"]*)"|`([^`]*)`)"#));

/// The environment variable a `process.env['X']` / `process.env.X` read names, if the text has one.
fn env_var(text: &str) -> Option<String> {
    TS_ENV_READ
        .captures(text)
        .and_then(|c| c.get(1).or(c.get(2)).map(|m| m.as_str().to_string()))
}

/// TypeScript skips whose condition reads `process.env`, directly or through a module-level constant (one level). The
/// condition is the variable's real name; a read the scanner cannot name has no condition and is never counted as satisfied.
pub fn env_gated_ts(path: &str, text: &str) -> Vec<TestSite> {
    if !text.contains("process.env") || !text.contains("skip") {
        return Vec::new();
    }
    let lines: Vec<&str> = text.lines().collect();
    // (constant, variable it reads)
    let consts: Vec<(String, Option<String>)> = lines
        .iter()
        .filter_map(|l| TS_CONST.captures(l).map(|c| (c[1].to_string(), env_var(&c[2]))))
        .collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(m) = TS_SKIP.find(l) else { continue };
        let args = &l[m.end()..];
        let cond = args.split(',').next().unwrap_or(args);
        let direct = cond.contains("process.env");
        let via = consts
            .iter()
            .find(|(c, _)| Regex::new(&format!(r"\b{}\b", regex::escape(c))).is_ok_and(|r| r.is_match(cond)));
        if !direct && via.is_none() {
            continue;
        }
        let var = if direct { env_var(cond) } else { via.and_then(|(_, v)| v.clone()) };
        let title = (0..=i)
            .rev()
            .find_map(|k| {
                TS_TITLE
                    .captures(lines[k])
                    .and_then(|c| c.get(1).or(c.get(2)).or(c.get(3)).map(|m| m.as_str().to_string()))
            })
            .unwrap_or_else(|| format!("line {}", i + 1));
        out.push(TestSite {
            path: path.to_string(),
            name: title,
            line: i + 1,
            condition: var,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[TestSite]) -> Vec<(&str, Option<&str>)> {
        v.iter().map(|s| (s.name.as_str(), s.condition.as_deref())).collect()
    }

    #[test]
    fn ignored_tests_good_and_bad_shapes() {
        // helper_wiring.rs: a reason, then more attributes and a doc comment before the fn
        let src = "#[test]\n#[ignore = \"until WP-INT\"]\n/// doc\nfn every_advertised_reading_has_a_production_sender() {}\n\n#[ignore]\n#[test]\nfn plain() {}\n";
        let v = ignored_tests("a.rs", src);
        assert_eq!(
            names(&v),
            vec![("every_advertised_reading_has_a_production_sender", None), ("plain", None)]
        );
        assert_eq!(v[0].line, 2);
        // a doc comment that mentions the attribute, and a test that is not ignored, are not sites
        assert!(ignored_tests("a.rs", "/// `#[ignore]` marks it\n#[test]\nfn live() {}\n").is_empty());
        // an attribute over several lines, inside mod tests
        let multi = "mod tests {\n    #[test]\n    #[ignore =\n        \"slow\"\n    ]\n    async fn slow() {}\n}\n";
        assert_eq!(names(&ignored_tests("a.rs", multi)), vec![("slow", None)]);
    }

    #[test]
    fn cfg_attr_ignore_keeps_its_condition() {
        let src = "#[cfg_attr(debug_assertions, ignore = \"needs release\")]\n#[test]\nfn heavy() {}\n#[cfg_attr(feature = \"x\", derive(Debug))]\nstruct S;\n";
        let v = ignored_tests("a.rs", src);
        assert_eq!(names(&v), vec![("heavy", Some("debug_assertions"))]);
    }

    #[test]
    fn env_gated_rust_tests() {
        let bad = "#[test]\nfn vm_only() {\n    if std::env::var(\"PADDOCK_VM\").is_err() {\n        return;\n    }\n    run();\n}\n";
        assert_eq!(names(&env_gated_rust("a.rs", bad)), vec![("vm_only", Some("PADDOCK_VM"))]);
        let let_else = "#[test]\nfn vm_only() {\n    let Ok(v) = std::env::var(\"PADDOCK_SETUP_VM\") else { return };\n    run(v);\n}\n";
        assert_eq!(names(&env_gated_rust("a.rs", let_else)).len(), 1);
        // good: a test that merely reads an env var it sets itself, or one that does not return early
        let good = "#[test]\nfn reads() {\n    let h = std::env::var(\"HOME\").unwrap_or_default();\n    assert!(h.len() < 1000);\n}\n";
        assert!(env_gated_rust("a.rs", good).is_empty());
        // not a test: a helper
        assert!(env_gated_rust("a.rs", "fn helper() {\n    if std::env::var(\"X\").is_err() { return; }\n}\n").is_empty());
    }

    #[test]
    fn env_gated_ts_skips_follow_one_level_of_const() {
        // apps/dyno/app/e2e/screenshots-power.spec.ts
        let src = "const OUT = process.env['POWER_SHOTS_DIR'] ?? '';\ntest.describe('power screenshots', () => {\n  test('a window', async () => {\n    test.skip(!OUT, 'set POWER_SHOTS_DIR');\n  });\n});\n";
        let v = env_gated_ts("s.spec.ts", src);
        assert_eq!(names(&v), vec![("a window", Some("POWER_SHOTS_DIR"))]);
        // good: a skip on the browser, with an unrelated env const in the file
        let good = "const OUT = process.env['X'] ?? '';\ntest('b', async ({ browserName }) => {\n  test.skip(browserName === 'webkit', 'no');\n  use(OUT);\n});\n";
        assert!(env_gated_ts("s.spec.ts", good).is_empty());
        // a direct read names its variable (contact sheets, dashboard grid), in both spellings
        let direct = "test('grid', async () => {\n  test.skip(!process.env['SHOTS_DIR'], 'x');\n});\ntest('dot', async () => {\n  test.skip(!process.env.OTHER_DIR, 'x');\n});\n";
        assert_eq!(
            names(&env_gated_ts("s.spec.ts", direct)),
            vec![("grid", Some("SHOTS_DIR")), ("dot", Some("OTHER_DIR"))]
        );
    }
}
