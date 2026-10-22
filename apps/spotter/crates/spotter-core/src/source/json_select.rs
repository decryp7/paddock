//! A small JSONPath-like selector language for JSON APIs.
//!
//! - `$` is the current value; `.name` or `["name"]` a field; `[3]` an array element; `[*]` every element of an
//!   array; `.*` every entry of an object (or element of an array).
//! - `@key` is the key of the current object entry (when the item came from `.*`).
//!
//! Examples: `$.versions[*]`, `$.releases.*`, `$[0].digests.sha256`, `@key`.

use crate::limits::MAX_CANDIDATES;
use serde_json::Value;

/// A parsed selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    /// `@key`.
    Key,
    /// `$…`.
    Path(Vec<Step>),
}

/// One step of a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A field.
    Field(String),
    /// An array element.
    Index(usize),
    /// Every element or entry.
    All,
}

/// One item a selector found: the value, and its key when it is an object entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Item<'a> {
    /// The entry's key.
    pub key: Option<&'a str>,
    /// The value.
    pub value: &'a Value,
}

impl Selector {
    /// Parses a selector; the error says what is wrong.
    pub fn parse(text: &str) -> Result<Selector, String> {
        let t = text.trim();
        if t == "@key" {
            return Ok(Selector::Key);
        }
        let bad = |why: &str| format!("\"{t}\" is not a selector: {why}. Start with $ (e.g. $.items[*].version) or use @key.");
        let mut rest = t.strip_prefix('$').ok_or_else(|| bad("it must start with $"))?;
        let mut steps = Vec::new();
        while !rest.is_empty() {
            if let Some(r) = rest.strip_prefix(".*") {
                steps.push(Step::All);
                rest = r;
            } else if let Some(r) = rest.strip_prefix('.') {
                let end = r.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-')).unwrap_or(r.len());
                if end == 0 {
                    return Err(bad("a field name is missing after ."));
                }
                steps.push(Step::Field(r[..end].to_string()));
                rest = &r[end..];
            } else if let Some(r) = rest.strip_prefix('[') {
                let close = r.find(']').ok_or_else(|| bad("a [ is not closed"))?;
                let inner = r[..close].trim();
                steps.push(if inner == "*" {
                    Step::All
                } else if let Ok(i) = inner.parse::<usize>() {
                    Step::Index(i)
                } else if inner.len() >= 2
                    && ((inner.starts_with('"') && inner.ends_with('"')) || (inner.starts_with('\'') && inner.ends_with('\'')))
                {
                    Step::Field(inner[1..inner.len() - 1].to_string())
                } else {
                    return Err(bad("use [*], [number] or [\"name\"]"));
                });
                rest = &r[close + 1..];
            } else {
                return Err(bad("expected . or ["));
            }
            if steps.len() > 32 {
                return Err(bad("it is too long"));
            }
        }
        Ok(Selector::Path(steps))
    }

    /// Every item the selector finds from `root` (at most [`MAX_CANDIDATES`]).
    pub fn items<'a>(&self, root: Item<'a>) -> Vec<Item<'a>> {
        let steps = match self {
            Selector::Key => return Vec::new(),
            Selector::Path(steps) => steps,
        };
        let mut current = vec![root];
        for step in steps {
            let mut next = Vec::new();
            for item in current {
                match (step, item.value) {
                    (Step::Field(f), Value::Object(m)) => next.extend(m.get(f).map(|v| Item { key: None, value: v })),
                    (Step::Index(i), Value::Array(a)) => next.extend(a.get(*i).map(|v| Item { key: None, value: v })),
                    (Step::All, Value::Array(a)) => next.extend(a.iter().map(|v| Item { key: None, value: v })),
                    (Step::All, Value::Object(m)) => next.extend(m.iter().map(|(k, v)| Item {
                        key: Some(k.as_str()),
                        value: v,
                    })),
                    _ => {}
                }
                if next.len() >= MAX_CANDIDATES {
                    next.truncate(MAX_CANDIDATES);
                    break;
                }
            }
            current = next;
        }
        current
    }

    /// The first value the selector finds from `item`, as text (strings as they are, numbers and booleans
    /// printed); `None` for nothing, null, arrays and objects.
    pub fn text(&self, item: Item<'_>) -> Option<String> {
        match self {
            Selector::Key => item.key.map(str::to_string),
            Selector::Path(_) => match self.items(item).first()?.value {
                Value::String(s) => Some(s.clone()),
                Value::Number(n) => Some(n.to_string()),
                Value::Bool(b) => Some(b.to_string()),
                _ => None,
            },
        }
    }

    /// Whether the first value found is "true-ish" (`true`, a non-zero number, a non-empty string other than
    /// `false`), for skip flags such as `yanked`.
    pub fn truthy(&self, item: Item<'_>) -> bool {
        match self.text(item).as_deref() {
            None | Some("") | Some("false") | Some("0") => false,
            Some(_) => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn root(v: &Value) -> Item<'_> {
        Item { key: None, value: v }
    }

    #[test]
    fn selectors_parse() {
        assert_eq!(Selector::parse(" @key "), Ok(Selector::Key));
        assert_eq!(Selector::parse("$"), Ok(Selector::Path(vec![])));
        assert_eq!(
            Selector::parse(r#"$.a[*].b-c[2]["d e"]['f'].*"#),
            Ok(Selector::Path(vec![
                Step::Field("a".into()),
                Step::All,
                Step::Field("b-c".into()),
                Step::Index(2),
                Step::Field("d e".into()),
                Step::Field("f".into()),
                Step::All
            ]))
        );
        for bad in ["a.b", "$.", "$[1", "$[x]", "$x", &format!("${}", ".a".repeat(40))] {
            assert!(Selector::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn selectors_find_items_and_values() {
        let doc = json!({
            "versions": [{"num": "1.0.0", "yanked": false, "size": 12}, {"num": "0.9.0", "yanked": true, "size": null}],
            "time": {"created": "2020", "1.0.0": "2026-10-01T00:00:00Z"},
        });
        let items = Selector::parse("$.versions[*]").unwrap().items(root(&doc));
        assert_eq!(items.len(), 2);
        let num = Selector::parse("$.num").unwrap();
        assert_eq!(num.text(items[0]).as_deref(), Some("1.0.0"));
        assert_eq!(Selector::parse("$.size").unwrap().text(items[0]).as_deref(), Some("12"));
        assert_eq!(Selector::parse("$.size").unwrap().text(items[1]), None);
        let yanked = Selector::parse("$.yanked").unwrap();
        assert!(!yanked.truthy(items[0]) && yanked.truthy(items[1]));
        assert!(!Selector::parse("$.missing").unwrap().truthy(items[0]));

        let entries = Selector::parse("$.time.*").unwrap().items(root(&doc));
        assert_eq!(Selector::Key.text(entries[0]).as_deref(), Some("1.0.0"));
        assert_eq!(
            Selector::parse("$").unwrap().text(entries[0]).as_deref(),
            Some("2026-10-01T00:00:00Z")
        );
        assert_eq!(Selector::Key.text(items[0]), None);
        assert!(Selector::Key.items(root(&doc)).is_empty());
        assert_eq!(
            Selector::parse("$.versions[1].num").unwrap().text(root(&doc)).as_deref(),
            Some("0.9.0")
        );
        assert_eq!(Selector::parse("$.versions.*").unwrap().items(root(&doc)).len(), 2);
        assert_eq!(Selector::parse("$.versions").unwrap().text(root(&doc)), None, "arrays are not text");
        assert!(Selector::parse("$.versions.num").unwrap().items(root(&doc)).is_empty());
        assert!(Selector::parse("$[0]").unwrap().items(root(&doc)).is_empty());
        let flag = json!({"f": "yes", "z": 0});
        assert!(Selector::parse("$.f").unwrap().truthy(root(&flag)));
        assert!(!Selector::parse("$.z").unwrap().truthy(root(&flag)));
    }

    #[test]
    fn results_are_bounded() {
        let big = Value::Array((0..MAX_CANDIDATES + 10).map(|i| json!(i)).collect());
        assert_eq!(Selector::parse("$[*]").unwrap().items(root(&big)).len(), MAX_CANDIDATES);
    }
}
