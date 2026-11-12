//! The ledger's labels: `.gitea/labels.toml` parsed once into [`LabelSpec`]s, and the label arithmetic of `pr label`
//! (`+x -y`, with exclusive scopes).

/// One label the ledger knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelSpec {
    /// `state/approved`, `app/dyno`...
    pub name: String,
    /// `#rrggbb`.
    pub color: String,
    /// What it means.
    pub description: String,
    /// Whether its scope holds one label at a time.
    pub exclusive: bool,
}

impl LabelSpec {
    /// The scope of an exclusive label (the text before the last `/`), `None` for a label that is not exclusive or has none.
    pub fn exclusive_scope(&self) -> Option<&str> {
        if !self.exclusive {
            return None;
        }
        scope_of(&self.name)
    }
}

/// The text before the last `/` of a label name.
pub fn scope_of(name: &str) -> Option<&str> {
    name.rsplit_once('/').map(|(s, _)| s).filter(|s| !s.is_empty())
}

/// The longest label name the server stores (Gitea's column is 50 characters).
const MAX_NAME: usize = 50;

/// Parses the labels file; `apps` are the app folders the `[[template]]` entries expand over. Every error names the entry.
pub fn parse(text: &str, apps: &[String]) -> Result<Vec<LabelSpec>, String> {
    let doc: toml::Table = text.parse().map_err(|e| format!("not TOML: {e}"))?;
    for key in doc.keys() {
        if key != "label" && key != "template" {
            return Err(format!("unknown key `{key}` (only [[label]] and [[template]] entries)"));
        }
    }
    let entries = |key: &str| -> Result<Vec<LabelSpec>, String> {
        let Some(v) = doc.get(key) else { return Ok(Vec::new()) };
        let arr = v
            .as_array()
            .ok_or_else(|| format!("`{key}` must be an array of tables ([[{key}]])"))?;
        arr.iter()
            .enumerate()
            .map(|(i, e)| {
                let t = e.as_table().ok_or_else(|| format!("[[{key}]] #{} is not a table", i + 1))?;
                spec_of(t).map_err(|m| format!("[[{key}]] #{}: {m}", i + 1))
            })
            .collect()
    };
    let mut out = entries("label")?;
    for t in entries("template")? {
        if !t.name.contains("{app}") {
            return Err(format!("[[template]] `{}` has no {{app}}", t.name));
        }
        for app in apps {
            out.push(LabelSpec {
                name: t.name.replace("{app}", app),
                color: t.color.clone(),
                description: t.description.replace("{app}", app),
                exclusive: t.exclusive,
            });
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    for l in &out {
        if l.name.is_empty() || l.name.len() > MAX_NAME || l.name.chars().any(|c| c.is_whitespace() || c == ',') {
            return Err(format!(
                "`{}` is not a label name (1 to {MAX_NAME} characters, no spaces or commas)",
                l.name
            ));
        }
        if !seen.insert(l.name.clone()) {
            return Err(format!("`{}` is listed twice", l.name));
        }
    }
    Ok(out)
}

fn spec_of(t: &toml::Table) -> Result<LabelSpec, String> {
    for key in t.keys() {
        if !matches!(key.as_str(), "name" | "color" | "description" | "exclusive") {
            return Err(format!("unknown key `{key}`"));
        }
    }
    let text = |k: &str| t.get(k).and_then(toml::Value::as_str).map(str::to_string);
    let name = text("name").ok_or("no `name`")?;
    let color = text("color").ok_or_else(|| format!("`{name}` has no `color`"))?;
    let hex = color.strip_prefix('#').unwrap_or("");
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("`{name}`: color `{color}` is not #rrggbb"));
    }
    let exclusive = match t.get("exclusive") {
        None => false,
        Some(v) => v.as_bool().ok_or_else(|| format!("`{name}`: exclusive must be true or false"))?,
    };
    Ok(LabelSpec {
        name,
        color: color.to_ascii_lowercase(),
        description: text("description").unwrap_or_default(),
        exclusive,
    })
}

/// One change of `pr label`: `+name` adds, `-name` removes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// `+name`.
    Add(String),
    /// `-name`.
    Remove(String),
}

/// Parses `+x` / `-y` words.
pub fn parse_changes(words: &[String]) -> Result<Vec<Change>, String> {
    if words.is_empty() {
        return Err("no label changes (give +name to add, -name to remove)".into());
    }
    words
        .iter()
        .map(|w| match (w.strip_prefix('+'), w.strip_prefix('-')) {
            (Some(n), _) if !n.is_empty() => Ok(Change::Add(n.to_string())),
            (_, Some(n)) if !n.is_empty() => Ok(Change::Remove(n.to_string())),
            _ => Err(format!("`{w}` is not a change (give +name to add, -name to remove)")),
        })
        .collect()
}

/// The labels after `changes`, in the order of `known`: unknown names are refused, and adding an exclusive label drops the
/// others of its scope. Removing a label that is not there is fine; adding one that is there is too.
pub fn apply(current: &[String], changes: &[Change], known: &[LabelSpec]) -> Result<Vec<String>, String> {
    let find = |n: &str| {
        known
            .iter()
            .find(|l| l.name == n)
            .ok_or_else(|| format!("`{n}` is not a ledger label (.gitea/labels.toml; run `./dev pr sync` after adding one)"))
    };
    let mut set: Vec<String> = current.to_vec();
    let mut added_scopes: Vec<String> = Vec::new();
    for c in changes {
        match c {
            Change::Add(n) => {
                let spec = find(n)?;
                if let Some(scope) = spec.exclusive_scope() {
                    if added_scopes.iter().any(|s| s == scope) {
                        return Err(format!("two labels of the exclusive scope `{scope}/` in one change"));
                    }
                    added_scopes.push(scope.to_string());
                    set.retain(|l| scope_of(l) != Some(scope));
                }
                if !set.contains(n) {
                    set.push(n.clone());
                }
            }
            Change::Remove(n) => {
                find(n)?;
                set.retain(|l| l != n);
            }
        }
    }
    // Known labels in file order, then anything the server had that the file does not list (kept, never dropped silently).
    let mut out: Vec<String> = known.iter().filter(|k| set.contains(&k.name)).map(|k| k.name.clone()).collect();
    out.extend(set.into_iter().filter(|l| !known.iter().any(|k| &k.name == l)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r##"
[[label]]
name = "state/built"
color = "#C5DEF5"
exclusive = true

[[label]]
name = "state/approved"
color = "#0e8a16"
description = "ok"
exclusive = true

[[label]]
name = "owes/windows-tests"
color = "#fbca04"

[[template]]
name = "app/{app}"
color = "#ededed"
description = "Touches {app}"
"##;

    fn known() -> Vec<LabelSpec> {
        parse(FILE, &["debrief".into(), "dyno".into()]).unwrap()
    }

    #[test]
    fn templates_expand_per_app_and_colours_are_normalised() {
        let k = known();
        let names: Vec<&str> = k.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(
            names,
            ["state/built", "state/approved", "owes/windows-tests", "app/debrief", "app/dyno"]
        );
        assert_eq!(k[0].color, "#c5def5");
        assert_eq!(k[4].description, "Touches dyno");
        assert_eq!(k[0].exclusive_scope(), Some("state"));
        assert_eq!(k[2].exclusive_scope(), None);
    }

    #[test]
    fn a_bad_file_names_the_entry() {
        for (text, want) in [
            ("[[label]]\nname = \"a\"\ncolor = \"red\"\n", "not #rrggbb"),
            ("[[label]]\ncolor = \"#000000\"\n", "no `name`"),
            ("[[label]]\nname = \"a b\"\ncolor = \"#000000\"\n", "not a label name"),
            (
                "[[label]]\nname = \"a\"\ncolor = \"#000000\"\n[[label]]\nname = \"a\"\ncolor = \"#000000\"\n",
                "twice",
            ),
            ("[[template]]\nname = \"app\"\ncolor = \"#000000\"\n", "no {app}"),
            ("[[label]]\nname = \"a\"\ncolor = \"#000000\"\nkind = 1\n", "unknown key `kind`"),
            ("[other]\n", "unknown key `other`"),
        ] {
            let e = parse(text, &["x".into()]).unwrap_err();
            assert!(e.contains(want), "{text}: {e}");
        }
    }

    #[test]
    fn adding_an_exclusive_label_replaces_its_scope_and_keeps_the_rest() {
        let cur = vec!["owes/windows-tests".to_string(), "state/built".into(), "app/dyno".into()];
        let out = apply(
            &cur,
            &parse_changes(&["+state/approved".into(), "-owes/windows-tests".into()]).unwrap(),
            &known(),
        )
        .unwrap();
        assert_eq!(out, ["state/approved", "app/dyno"]);
    }

    #[test]
    fn unknown_labels_bad_words_and_two_of_one_scope_are_refused() {
        assert!(apply(&[], &[Change::Add("nope".into())], &known()).unwrap_err().contains("pr sync"));
        assert!(parse_changes(&["state/built".into()]).unwrap_err().contains("+name"));
        assert!(parse_changes(&["+".into()]).is_err());
        assert!(parse_changes(&[]).is_err());
        let two = parse_changes(&["+state/built".into(), "+state/approved".into()]).unwrap();
        assert!(apply(&[], &two, &known()).unwrap_err().contains("exclusive scope `state/`"));
    }

    #[test]
    fn a_label_the_file_does_not_list_is_kept() {
        let out = apply(&["legacy".into()], &[Change::Add("app/debrief".into())], &known()).unwrap();
        assert_eq!(out, ["app/debrief", "legacy"]);
    }

    #[test]
    fn the_repositorys_file_parses_and_has_the_ledger_labels() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.gitea/labels.toml")).unwrap();
        let k = parse(&text, &["dyno".into()]).unwrap();
        for want in [
            "state/built",
            "state/in-review",
            "state/fixes",
            "state/approved",
            "state/in-train",
            "state/landed",
            "owes/windows-tests",
            "owes/vm-rows",
            "owes/rule54-bundle",
            "security",
            "app/dyno",
            "review-round/1",
            "review-round/2",
        ] {
            assert!(k.iter().any(|l| l.name == want), "{want} is missing from .gitea/labels.toml");
        }
        assert!(k.iter().filter(|l| l.name.starts_with("state/")).all(|l| l.exclusive));
    }
}
