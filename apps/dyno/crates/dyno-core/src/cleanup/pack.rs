//! Declarative cleanup packs (ADR 0007, tier 2): TOML that names categories, parsed once into typed values.
//!
//! The built-in pack ships inside the app (`builtin.toml`); a user may drop more into the data folder's `packs/`
//! folder. A pack cannot widen what Dyno may delete: user packs name folders by path template only, may use only the
//! `children` and `installers` selectors, cannot reach personal folders, and every path they produce still passes
//! [`super::policy::admit`]. A pack that does not parse or validate is refused as a whole, with a message naming the
//! problem (and the line, for syntax errors).

use super::catalog::{CategorySpec, CleanupGroup, CleanupRisk, Os, Root, RootRef, Selector};
use super::policy::PERSONAL_DIRS;
use serde::Deserialize;

/// The pack format this build reads.
pub const FORMAT: u32 = 1;
/// The largest pack file read.
pub const MAX_PACK_BYTES: usize = 128 * 1024;
/// The most categories in one user pack.
pub const MAX_USER_CATEGORIES: usize = 64;
/// The most roots and paths in one category.
pub const MAX_ROOTS: usize = 16;
/// The longest path template.
pub const MAX_TEMPLATE_LEN: usize = 240;
/// The deepest path template.
pub const MAX_TEMPLATE_DEPTH: usize = 12;

/// The variables a path template may start with; the agent expands them per OS.
pub const TEMPLATE_VARS: &[&str] = &["{home}", "{temp}", "{cache}", "{data}", "{localAppData}"];

/// Who wrote a pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// Shipped inside the app.
    Builtin,
    /// Dropped into the user's `packs/` folder.
    User,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPack {
    format: u32,
    #[serde(default)]
    category: Vec<RawCategory>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCategory {
    id: String,
    name: String,
    description: String,
    group: CleanupGroup,
    risk: CleanupRisk,
    #[serde(default)]
    os: Vec<Os>,
    #[serde(default)]
    roots: Vec<Root>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default = "children")]
    selector: Selector,
    #[serde(default)]
    min_age_days: u32,
}

fn children() -> Selector {
    Selector::Children
}

/// The built-in categories. The embedded pack is checked by a test, so a failure here can only mean no categories.
pub fn builtin() -> Vec<CategorySpec> {
    parse(include_str!("builtin.toml"), Trust::Builtin).unwrap_or_default()
}

fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 48 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Whether `t` is a path template: a known variable, then plain folder names (`*` stands for one whole level).
pub fn valid_template(t: &str) -> Result<(), String> {
    if t.len() > MAX_TEMPLATE_LEN {
        return Err(format!("the path is longer than {MAX_TEMPLATE_LEN} characters"));
    }
    let mut parts = t.split('/');
    let var = parts.next().unwrap_or_default();
    if !TEMPLATE_VARS.contains(&var) {
        return Err(format!("`{t}` must start with one of {}", TEMPLATE_VARS.join(", ")));
    }
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() {
        return Err(format!("`{t}` names a whole base folder; add the folder inside it"));
    }
    if rest.len() > MAX_TEMPLATE_DEPTH {
        return Err(format!("`{t}` is more than {MAX_TEMPLATE_DEPTH} levels deep"));
    }
    for seg in &rest {
        let bad = seg.is_empty()
            || *seg == "."
            || *seg == ".."
            || seg.contains(['\\', ':', '\0', '{', '}'])
            || (seg.contains('*') && *seg != "*");
        if bad {
            return Err(format!("`{t}` has a segment Dyno does not accept (`{seg}`)"));
        }
    }
    Ok(())
}

fn personal_first_segment(t: &str) -> Option<&'static str> {
    let mut parts = t.split('/');
    if parts.next() != Some("{home}") {
        return None;
    }
    let rest: Vec<&str> = parts.collect();
    PERSONAL_DIRS.iter().copied().find(|d| {
        let segs: Vec<&str> = d.split('/').collect();
        rest.len() >= segs.len() && segs.iter().zip(&rest).all(|(a, b)| a.eq_ignore_ascii_case(b))
    })
}

fn convert(raw: RawCategory, trust: Trust) -> Result<CategorySpec, String> {
    let id = raw.id.clone();
    let at = move |m: String| format!("category `{id}`: {m}");
    if !id_ok(&raw.id) {
        return Err(format!("category id `{}` must be 1-48 letters, digits, - or _", raw.id));
    }
    if raw.name.trim().is_empty() || raw.name.chars().count() > 80 {
        return Err(at("the name must be 1-80 characters".into()));
    }
    if raw.description.trim().is_empty() || raw.description.chars().count() > 300 {
        return Err(at("the description must be 1-300 characters".into()));
    }
    if raw.min_age_days > 3650 {
        return Err(at("min_age_days is more than ten years".into()));
    }
    if raw.roots.len() + raw.paths.len() > MAX_ROOTS {
        return Err(at(format!("more than {MAX_ROOTS} roots and paths")));
    }
    if raw.roots.is_empty() && raw.paths.is_empty() {
        return Err(at("name at least one root or path".into()));
    }
    if trust == Trust::User {
        if !raw.roots.is_empty() {
            return Err(at("a user pack names folders with `paths`; named roots are built in".into()));
        }
        if !matches!(raw.selector, Selector::Children | Selector::Installers) {
            return Err(at("a user pack may only use the `children` and `installers` selectors".into()));
        }
        if raw.min_age_days == 0 && raw.risk == CleanupRisk::Safe {
            return Err(at(
                "a `safe` category must leave files younger than a day alone (min_age_days >= 1)".into(),
            ));
        }
    }
    let mut roots: Vec<RootRef> = raw.roots.iter().map(|r| RootRef::Named(*r)).collect();
    for p in &raw.paths {
        if let Err(m) = valid_template(p) {
            return Err(at(m));
        }
        if let Some(d) = personal_first_segment(p) {
            return Err(at(format!("`{p}` is inside your personal folder {d}")));
        }
        roots.push(RootRef::Template(p.clone()));
    }
    Ok(CategorySpec {
        id: raw.id,
        name: raw.name,
        description: raw.description,
        group: raw.group,
        risk: raw.risk,
        os: raw.os,
        roots,
        selector: raw.selector,
        min_age_days: raw.min_age_days,
        builtin: trust == Trust::Builtin,
    })
}

/// Parses and validates a pack. Any problem refuses the whole pack.
pub fn parse(text: &str, trust: Trust) -> Result<Vec<CategorySpec>, String> {
    if text.len() > MAX_PACK_BYTES {
        return Err(format!("the pack is larger than {} KiB", MAX_PACK_BYTES / 1024));
    }
    let raw: RawPack = toml::from_str(text).map_err(|e| e.to_string())?;
    if raw.format != FORMAT {
        return Err(format!("format {} is not supported (this Dyno reads format {FORMAT})", raw.format));
    }
    if trust == Trust::User && raw.category.len() > MAX_USER_CATEGORIES {
        return Err(format!("more than {MAX_USER_CATEGORIES} categories"));
    }
    let mut out: Vec<CategorySpec> = Vec::with_capacity(raw.category.len());
    for c in raw.category {
        let spec = convert(c, trust)?;
        if out.iter().any(|o| o.id == spec.id) {
            return Err(format!("category id `{}` appears twice", spec.id));
        }
        out.push(spec);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(body: &str) -> Result<Vec<CategorySpec>, String> {
        parse(&format!("format = 1\n{body}"), Trust::User)
    }

    const OK: &str = "[[category]]\nid = \"gradle\"\nname = \"Gradle caches\"\ndescription = \"Gradle's download cache.\"\ngroup = \"developer\"\nrisk = \"review\"\npaths = [\"{home}/.gradle/caches\"]\nmin_age_days = 30\n";

    #[test]
    fn the_embedded_pack_parses_and_is_consistent() {
        let all = parse(include_str!("builtin.toml"), Trust::Builtin).unwrap();
        assert!(all.len() >= 19);
        assert_eq!(builtin().len(), all.len());
        assert!(all.iter().all(|c| c.builtin));
    }

    #[test]
    fn a_good_user_pack_becomes_typed_categories() {
        let got = user(OK).unwrap();
        assert_eq!(got.len(), 1);
        assert!(!got[0].builtin);
        assert_eq!(got[0].roots, vec![RootRef::Template("{home}/.gradle/caches".into())]);
        assert_eq!(got[0].selector, Selector::Children);
    }

    #[test]
    fn bad_packs_are_refused_with_a_reason() {
        let cases = [
            ("not toml at all [", "TOML"),
            ("[[category]]\nid = \"x\"\n", "missing field"),
            (&format!("{OK}unknown_key = 1\n"), "unknown field"),
            (&OK.replace("{home}/.gradle/caches", "/etc/passwd"), "must start with"),
            (&OK.replace("{home}/.gradle/caches", "{home}/../x"), "segment"),
            (&OK.replace("{home}/.gradle/caches", "{home}"), "whole base folder"),
            (&OK.replace("{home}/.gradle/caches", "{home}/a\\\\b"), "segment"),
            (&OK.replace("{home}/.gradle/caches", "{home}/Documents/x"), "personal folder"),
            (&OK.replace("{home}/.gradle/caches", "{home}/documents"), "personal folder"),
            (&OK.replace("{home}/.gradle/caches", "{home}/a*b"), "segment"),
            (&OK.replace("id = \"gradle\"", "id = \"bad id!\""), "category id"),
            (&OK.replace("Gradle caches", ""), "name"),
            (
                &OK.replace("paths = [\"{home}/.gradle/caches\"]", "roots = [\"userTemp\"]"),
                "named roots",
            ),
            (&OK.replace("paths = [\"{home}/.gradle/caches\"]", ""), "at least one"),
            (&format!("{OK}selector = \"devArtifacts\"\n"), "selectors"),
            (
                &OK.replace("review", "safe").replace("min_age_days = 30", "min_age_days = 0"),
                "younger than a day",
            ),
            (&OK.replace("min_age_days = 30", "min_age_days = 99999"), "ten years"),
        ];
        for (body, want) in cases {
            let err = user(body).unwrap_err();
            assert!(err.contains(want), "{body:?} -> {err}");
        }
        assert!(parse("format = 2\n", Trust::User).unwrap_err().contains("format 2"));
        assert!(user(&format!("{OK}{OK}")).unwrap_err().contains("twice"));
        assert!(parse(&"#".repeat(MAX_PACK_BYTES + 1), Trust::User).unwrap_err().contains("larger"));
        let many = OK.replace("gradle", "g");
        let mut big = String::new();
        for i in 0..=MAX_USER_CATEGORIES {
            big.push_str(&many.replace("\"g\"", &format!("\"g{i}\"")));
        }
        assert!(user(&big).unwrap_err().contains("more than"));
        let roots = (0..=MAX_ROOTS).map(|i| format!("\"{{home}}/p{i}\"")).collect::<Vec<_>>().join(",");
        let body = OK.replace("[\"{home}/.gradle/caches\"]", &format!("[{roots}]"));
        assert!(user(&body).unwrap_err().contains("roots and paths"));
    }

    #[test]
    fn syntax_errors_name_the_line() {
        let err = user("[[category]]\nid = \n").unwrap_err();
        assert!(err.contains("line 3") || err.contains("3:"), "{err}");
    }

    #[test]
    fn templates() {
        assert!(valid_template("{cache}/Google/Chrome/*/Cache").is_ok());
        assert!(valid_template("{temp}/x").is_ok());
        assert!(valid_template(&format!("{{home}}/{}", "a/".repeat(MAX_TEMPLATE_DEPTH + 1))).is_err());
        assert!(valid_template(&format!("{{home}}/{}", "a".repeat(MAX_TEMPLATE_LEN))).is_err());
        assert!(valid_template("{home}//x").is_err());
        assert!(valid_template("{home}/{data}").is_err());
        assert!(valid_template("{nope}/x").is_err());
    }
}
