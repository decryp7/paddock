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

/// Why contributions are refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContributionError {
    /// More than [`MAX_PER_KIND`] tools, views or settings.
    #[error("at most {MAX_PER_KIND} tools, views and settings each")]
    TooMany,
    /// A name is not a slug (lowercase letters, digits, `_` and `-`).
    #[error("{kind} name {name:?} is not a slug (lowercase letters, digits, `_` and `-`)")]
    BadName {
        /// What it names.
        kind: &'static str,
        /// The name.
        name: String,
    },
    /// Two of the same kind share a name.
    #[error("two {kind}s are named {name:?}")]
    Duplicate {
        /// What it names.
        kind: &'static str,
        /// The name.
        name: String,
    },
    /// A text is empty or too long.
    #[error("{kind} {name:?} needs {what}")]
    BadText {
        /// What it belongs to.
        kind: &'static str,
        /// Its name.
        name: String,
        /// What is needed.
        what: &'static str,
    },
    /// A view names a tool the function does not have.
    #[error("view {view:?} names the tool {tool:?}, which this function does not have")]
    UnknownTool {
        /// The view.
        view: String,
        /// The tool.
        tool: String,
    },
}

/// The wire form of [`Contributions`].
#[derive(Serialize, Deserialize)]
pub(crate) struct ContributionsWire {
    tools: Vec<ToolSpec>,
    views: Vec<ViewSpec>,
    settings: Vec<SettingSpec>,
}

impl TryFrom<ContributionsWire> for Contributions {
    type Error = ContributionError;
    fn try_from(w: ContributionsWire) -> Result<Contributions, ContributionError> {
        Contributions::new(w.tools, w.views, w.settings)
    }
}

/// Everything a function contributes. Validated when built, and when read from the wire.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(try_from = "ContributionsWire")]
pub struct Contributions {
    tools: Vec<ToolSpec>,
    views: Vec<ViewSpec>,
    settings: Vec<SettingSpec>,
}

fn is_slug(s: &str) -> bool {
    let mut chars = s.chars();
    s.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

impl Contributions {
    /// Builds contributions once, when a function is registered: slugs, uniqueness, bounds, and that every view names a tool
    /// the function has.
    pub fn new(tools: Vec<ToolSpec>, views: Vec<ViewSpec>, settings: Vec<SettingSpec>) -> Result<Contributions, ContributionError> {
        use ContributionError::{BadName, BadText, Duplicate, TooMany, UnknownTool};
        if tools.len() > MAX_PER_KIND || views.len() > MAX_PER_KIND || settings.len() > MAX_PER_KIND {
            return Err(TooMany);
        }
        let mut seen = std::collections::BTreeSet::new();
        for t in &tools {
            if !is_slug(&t.name) {
                return Err(BadName {
                    kind: "tool",
                    name: t.name.clone(),
                });
            }
            if !seen.insert(t.name.as_str()) {
                return Err(Duplicate {
                    kind: "tool",
                    name: t.name.clone(),
                });
            }
            if t.description.is_empty() || t.description.len() > 500 {
                return Err(BadText {
                    kind: "tool",
                    name: t.name.clone(),
                    what: "a description of at most 500 characters",
                });
            }
            if t.input_schema.len() > 64 * 1024 {
                return Err(BadText {
                    kind: "tool",
                    name: t.name.clone(),
                    what: "an input schema of at most 64 KiB",
                });
            }
        }
        let mut ids = std::collections::BTreeSet::new();
        for v in &views {
            if !is_slug(&v.id) {
                return Err(BadName {
                    kind: "view",
                    name: v.id.clone(),
                });
            }
            if !ids.insert(v.id.as_str()) {
                return Err(Duplicate {
                    kind: "view",
                    name: v.id.clone(),
                });
            }
            if !seen.contains(v.tool.as_str()) {
                return Err(UnknownTool {
                    view: v.id.clone(),
                    tool: v.tool.clone(),
                });
            }
            if v.title.is_empty() || v.title.len() > 120 {
                return Err(BadText {
                    kind: "view",
                    name: v.id.clone(),
                    what: "a title of at most 120 characters",
                });
            }
        }
        let mut keys = std::collections::BTreeSet::new();
        for s in &settings {
            if !is_slug(&s.key) {
                return Err(BadName {
                    kind: "setting",
                    name: s.key.clone(),
                });
            }
            if !keys.insert(s.key.as_str()) {
                return Err(Duplicate {
                    kind: "setting",
                    name: s.key.clone(),
                });
            }
            if s.title.is_empty() || s.title.len() > 120 {
                return Err(BadText {
                    kind: "setting",
                    name: s.key.clone(),
                    what: "a title of at most 120 characters",
                });
            }
        }
        Ok(Contributions { tools, views, settings })
    }

    /// Tools.
    pub fn tools(&self) -> &[ToolSpec] {
        &self.tools
    }

    /// Views.
    pub fn views(&self) -> &[ViewSpec] {
        &self.views
    }

    /// Settings.
    pub fn settings(&self) -> &[SettingSpec] {
        &self.settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ContributionError::*;

    fn tool(name: &str) -> ToolSpec {
        ToolSpec {
            name: name.into(),
            description: "Finds a symbol.".into(),
            read_only: true,
            input_schema: "{}".into(),
        }
    }

    fn view() -> ViewSpec {
        ViewSpec {
            id: "symbols".into(),
            title: "Symbols".into(),
            kind: ViewKind::Table,
            tool: "find_symbol".into(),
        }
    }

    fn setting() -> SettingSpec {
        SettingSpec {
            key: "max_hits".into(),
            title: "Most hits".into(),
            default: SettingValue::Int(50),
        }
    }

    fn build(f: impl FnOnce(&mut Vec<ToolSpec>, &mut Vec<ViewSpec>, &mut Vec<SettingSpec>)) -> Result<Contributions, ContributionError> {
        let (mut t, mut v, mut s) = (vec![tool("find_symbol")], vec![view()], vec![setting()]);
        f(&mut t, &mut v, &mut s);
        Contributions::new(t, v, s)
    }

    #[test]
    fn good_contributions_build_and_expose_their_parts() {
        let c = build(|_, _, _| {}).unwrap();
        assert_eq!((c.tools().len(), c.views().len(), c.settings().len()), (1, 1, 1));
        assert_eq!(Contributions::default().tools().len(), 0);
    }

    #[test]
    fn each_mistake_is_a_typed_error_that_says_what_to_fix() {
        assert!(matches!(
            build(|t, _, _| t.push(tool("find_symbol"))),
            Err(Duplicate { kind: "tool", .. })
        ));
        assert!(matches!(
            build(|t, _, _| t[0].name = "Find Symbol".into()),
            Err(BadName { kind: "tool", .. })
        ));
        assert!(matches!(
            build(|t, _, _| t[0].description.clear()),
            Err(BadText { kind: "tool", .. })
        ));
        assert!(matches!(
            build(|t, _, _| t[0].input_schema = "x".repeat(64 * 1024 + 1)),
            Err(BadText { kind: "tool", .. })
        ));
        assert!(matches!(build(|_, v, _| v[0].tool = "nope".into()), Err(UnknownTool { .. })));
        assert!(matches!(build(|_, v, _| v[0].title.clear()), Err(BadText { kind: "view", .. })));
        assert!(matches!(build(|_, v, _| v.push(view())), Err(Duplicate { kind: "view", .. })));
        assert!(matches!(
            build(|_, v, _| v[0].id = "Bad Id".into()),
            Err(BadName { kind: "view", .. })
        ));
        assert!(matches!(
            build(|_, _, s| s[0].key = "Bad Key".into()),
            Err(BadName { kind: "setting", .. })
        ));
        assert!(matches!(build(|_, _, s| s.push(setting())), Err(Duplicate { kind: "setting", .. })));
        assert!(matches!(build(|_, _, s| s[0].title.clear()), Err(BadText { kind: "setting", .. })));
        assert!(matches!(
            build(|t, _, _| *t = (0..=MAX_PER_KIND).map(|i| tool(&format!("t{i}"))).collect()),
            Err(TooMany)
        ));
        assert!(
            build(|t, _, _| t[0].description.clear())
                .unwrap_err()
                .to_string()
                .contains("description")
        );
    }
}
