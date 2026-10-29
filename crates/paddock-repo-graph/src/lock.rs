use crate::GraphError;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

fn packages(text: Option<&str>) -> Result<Map<String, Value>, GraphError> {
    let Some(text) = text else {
        return Ok(Map::new());
    };
    let v: Value = serde_json::from_str(text).map_err(|e| GraphError::Parse {
        path: "package-lock.json".into(),
        reason: e.to_string(),
    })?;
    Ok(v.get("packages")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default())
}

/// What a workspace resolves to: every reachable package with its version and integrity, found the way node looks packages up
/// (`<at>/node_modules/<name>`, then the enclosing `node_modules`, then the root).
fn closure(pk: &Map<String, Value>, ws: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut todo = vec![ws.to_string()];
    while let Some(at) = todo.pop() {
        let Some(e) = pk.get(&at) else { continue };
        if out.contains_key(&at) {
            continue;
        }
        let text = |k: &str| e.get(k).and_then(Value::as_str);
        let version = e
            .get("version")
            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string))
            .unwrap_or_default();
        out.insert(
            at.clone(),
            format!(
                "{version}|{}",
                text("integrity").or(text("resolved")).unwrap_or_default()
            ),
        );
        let mut names: Vec<&str> = Vec::new();
        for key in [
            "dependencies",
            "devDependencies",
            "peerDependencies",
            "optionalDependencies",
        ] {
            names.extend(
                e.get(key)
                    .and_then(Value::as_object)
                    .into_iter()
                    .flat_map(|o| o.keys().map(String::as_str)),
            );
        }
        for n in names {
            let mut base = at.clone();
            loop {
                let cand = if base.is_empty() {
                    format!("node_modules/{n}")
                } else {
                    format!("{base}/node_modules/{n}")
                };
                if let Some(hit) = pk.get(&cand) {
                    let link = hit.get("link").and_then(Value::as_bool).unwrap_or(false);
                    todo.push(if link {
                        hit.get("resolved")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string()
                    } else {
                        cand
                    });
                    break;
                }
                if base.is_empty() {
                    break;
                }
                base = base
                    .rfind("/node_modules/")
                    .map(|i| base[..i].to_string())
                    .unwrap_or_default();
            }
        }
    }
    out
}

/// The npm workspaces (folders) whose resolved packages differ between two root `package-lock.json` texts (lockfile v3; a
/// missing text is an empty lock), sorted.
pub fn lock_diff(before: Option<&str>, after: Option<&str>) -> Result<Vec<String>, GraphError> {
    let (a, b) = (packages(before)?, packages(after)?);
    let mut spaces: Vec<&String> = a
        .keys()
        .chain(b.keys())
        .filter(|k| !k.is_empty() && !k.contains("node_modules"))
        .collect();
    spaces.sort();
    spaces.dedup();
    Ok(spaces
        .into_iter()
        .filter(|ws| closure(&a, ws) != closure(&b, ws))
        .cloned()
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock(react: &str, extra: &str) -> String {
        format!(
            r#"{{"packages":{{
              "":{{"workspaces":["ui","apps/x"]}},
              "ui":{{"version":"1.0.0","dependencies":{{"react":"^1","l":"1","ghost":"1","dangling":"1"}}}},
              "apps/x":{{"version":"1.0.0","dependencies":{{"left":"1"}}}},
              "node_modules/react":{{"version":"{react}","integrity":"sha-r"}},
              "node_modules/left":{{"version":"1.0.0","resolved":"https://x/left"}},
              "node_modules/l":{{"link":true,"resolved":"ui/l"}},
              "ui/l":{{"version":"0.1.0","dependencies":{{"react":"1"}}}},
              "node_modules/dangling":{{"link":true,"resolved":"nowhere"}},
              "ui/node_modules/inner":{{"version":"1.0.0"}}
              {extra}
            }}}}"#
        )
    }

    #[test]
    fn only_workspaces_that_reach_a_changed_package_are_listed() {
        let a = lock("1.0.0", "");
        assert!(lock_diff(Some(&a), Some(&a)).unwrap().is_empty());
        assert_eq!(
            lock_diff(Some(&a), Some(&lock("1.1.0", ""))).unwrap(),
            ["ui", "ui/l"]
        );
    }

    #[test]
    fn a_new_or_missing_lock_marks_every_workspace_and_nested_lookup_works() {
        let a = lock("1.0.0", "");
        assert_eq!(lock_diff(None, Some(&a)).unwrap(), ["apps/x", "ui", "ui/l"]);
        assert_eq!(lock_diff(Some(&a), None).unwrap(), ["apps/x", "ui", "ui/l"]);
        assert!(lock_diff(None, None).unwrap().is_empty());
        // A nested copy shadows the root one for the workspace that contains it.
        let nested = r#","ui/node_modules/react":{"version":"9.0.0"}"#;
        assert_eq!(
            lock_diff(Some(&a), Some(&lock("1.0.0", nested))).unwrap(),
            ["ui"]
        );
        // A numeric version still compares.
        let weird = r#"{"packages":{"w":{"version":1}}}"#;
        assert!(lock_diff(Some(weird), Some(weird)).unwrap().is_empty());
        assert!(
            lock_diff(Some(r#"{"x":1}"#), Some(r#"{"packages":{"w":{}}}"#))
                .unwrap()
                .contains(&"w".to_string())
        );
    }

    #[test]
    fn broken_json_is_an_error() {
        assert!(matches!(
            lock_diff(Some("{"), None),
            Err(GraphError::Parse { .. })
        ));
    }
}
