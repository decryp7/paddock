//! What a function contributes: tools (also served over MCP), views the core renders, and settings.
//!
//! Mirrors `paddock-plugin-api`'s contributions in shape; package FN1 rebases it onto those types in stage 2.

use serde::{Deserialize, Serialize};

/// The most contributions of each kind.
pub const MAX_PER_KIND: usize = 64;

/// A tool: a named, checked entry point. Tools marked `read_only` may be served over MCP; the rest are UI and CLI only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSpec {
    /// The tool name, a slug (`find_symbol`).
    pub name: String,
    /// One sentence for the user and for MCP clients.
    pub description: String,
    /// True when the tool changes nothing (only such tools are exposed over MCP).
    pub read_only: bool,
    /// The JSON Schema of the arguments, as text (postcard cannot carry a JSON value).
    pub input_schema: String,
}

/// How the core renders a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewKind {
    /// Rows and columns.
    Table,
    /// A tree.
    Tree,
    /// A node-link graph.
    Graph,
    /// Formatted text.
    Text,
    /// A diagram.
    Diagram,
}

/// A declarative view. The function supplies data; a function never supplies web code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewSpec {
    /// The view id, a slug.
    pub id: String,
    /// The title users see.
    pub title: String,
    /// How to render it.
    pub kind: ViewKind,
    /// The tool that produces its data.
    pub tool: String,
}

/// A setting's value and default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingValue {
    /// On or off.
    Bool(bool),
    /// A whole number.
    Int(i64),
    /// Text.
    Text(String),
}

/// A setting a function adds to Settings (family rule 44: tunables are settings with defaults).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingSpec {
    /// The key, a slug.
    pub key: String,
    /// The label users see.
    pub title: String,
    /// The default.
    pub default: SettingValue,
}

/// Everything a function contributes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Contributions {
    /// Tools.
    pub tools: Vec<ToolSpec>,
    /// Views.
    pub views: Vec<ViewSpec>,
    /// Settings.
    pub settings: Vec<SettingSpec>,
}

fn is_slug(s: &str) -> bool {
    let mut chars = s.chars();
    s.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

impl Contributions {
    /// Checks the contributions once, when a function is registered: slugs, uniqueness, bounds, and that every view names a
    /// tool the function has. Returns what to fix.
    pub fn validate(&self) -> Result<(), String> {
        if self.tools.len() > MAX_PER_KIND || self.views.len() > MAX_PER_KIND || self.settings.len() > MAX_PER_KIND {
            return Err(format!("at most {MAX_PER_KIND} tools, views and settings each"));
        }
        let mut seen = std::collections::BTreeSet::new();
        for t in &self.tools {
            if !is_slug(&t.name) {
                return Err(format!(
                    "tool name {:?} is not a slug (lowercase letters, digits, `_` and `-`)",
                    t.name
                ));
            }
            if !seen.insert(t.name.as_str()) {
                return Err(format!("two tools are named {:?}", t.name));
            }
            if t.description.is_empty() || t.description.len() > 500 {
                return Err(format!("tool {:?} needs a description of at most 500 characters", t.name));
            }
            if t.input_schema.len() > 64 * 1024 {
                return Err(format!("tool {:?} has an input schema over 64 KiB", t.name));
            }
        }
        let mut views = std::collections::BTreeSet::new();
        for v in &self.views {
            if !is_slug(&v.id) || !views.insert(v.id.as_str()) {
                return Err(format!("view id {:?} is not a unique slug", v.id));
            }
            if !seen.contains(v.tool.as_str()) {
                return Err(format!(
                    "view {:?} names the tool {:?}, which this function does not have",
                    v.id, v.tool
                ));
            }
            if v.title.is_empty() || v.title.len() > 120 {
                return Err(format!("view {:?} needs a title of at most 120 characters", v.id));
            }
        }
        let mut keys = std::collections::BTreeSet::new();
        for s in &self.settings {
            if !is_slug(&s.key) || !keys.insert(s.key.as_str()) {
                return Err(format!("setting key {:?} is not a unique slug", s.key));
            }
            if s.title.is_empty() || s.title.len() > 120 {
                return Err(format!("setting {:?} needs a title of at most 120 characters", s.key));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> ToolSpec {
        ToolSpec {
            name: name.into(),
            description: "Finds a symbol.".into(),
            read_only: true,
            input_schema: "{}".into(),
        }
    }

    fn good() -> Contributions {
        Contributions {
            tools: vec![tool("find_symbol")],
            views: vec![ViewSpec {
                id: "symbols".into(),
                title: "Symbols".into(),
                kind: ViewKind::Table,
                tool: "find_symbol".into(),
            }],
            settings: vec![SettingSpec {
                key: "max_hits".into(),
                title: "Most hits".into(),
                default: SettingValue::Int(50),
            }],
        }
    }

    #[test]
    fn good_contributions_validate() {
        assert_eq!(good().validate(), Ok(()));
        assert_eq!(Contributions::default().validate(), Ok(()));
    }

    #[test]
    fn each_mistake_says_what_to_fix() {
        let mut c = good();
        c.tools.push(tool("find_symbol"));
        assert!(c.validate().unwrap_err().contains("two tools"));
        let mut c = good();
        c.tools[0].name = "Find Symbol".into();
        assert!(c.validate().unwrap_err().contains("not a slug"));
        let mut c = good();
        c.tools[0].description.clear();
        assert!(c.validate().unwrap_err().contains("description"));
        let mut c = good();
        c.tools[0].input_schema = "x".repeat(64 * 1024 + 1);
        assert!(c.validate().unwrap_err().contains("schema"));
        let mut c = good();
        c.views[0].tool = "nope".into();
        assert!(c.validate().unwrap_err().contains("does not have"));
        let mut c = good();
        c.views[0].title.clear();
        assert!(c.validate().unwrap_err().contains("title"));
        let mut c = good();
        c.views.push(c.views[0].clone());
        assert!(c.validate().unwrap_err().contains("unique slug"));
        let mut c = good();
        c.settings[0].key = "Bad Key".into();
        assert!(c.validate().unwrap_err().contains("setting key"));
        let mut c = good();
        c.settings[0].title.clear();
        assert!(c.validate().unwrap_err().contains("setting"));
        let mut c = good();
        c.tools = (0..=MAX_PER_KIND).map(|i| tool(&format!("t{i}"))).collect();
        assert!(c.validate().unwrap_err().contains("at most"));
    }
}
