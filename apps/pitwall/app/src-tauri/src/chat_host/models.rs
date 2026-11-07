//! Which models the Chat tab offers (ADR 0023 D6): every downloaded chat model, those without the `tools` capability
//! listed but disabled with the reason, and a tool-capable one chosen by default (a loaded one first, then by name). Pure.

use pitwall_core::mgmt::LocalModel;
use pitwall_core::select::Capability;
use serde::Serialize;

/// One model as the picker shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatModel {
    /// The model's id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// It is in memory now (answers start at once).
    pub loaded: bool,
    /// Why it cannot be chosen; `None` when it can.
    pub disabled: Option<String>,
}

/// The picker: the models and the one the chat uses (`None`: no model can call tools).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatModels {
    /// Tool-capable models first (loaded first, then by name), then the others.
    pub models: Vec<ChatModel>,
    /// The chosen model.
    pub selected: Option<String>,
}

/// What the picker needs to know of a downloaded model.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    /// Its id.
    pub id: &'a str,
    /// Its name.
    pub name: &'a str,
    /// It is in memory now.
    pub loaded: bool,
    /// What it can do.
    pub capabilities: &'a [Capability],
}

impl<'a> From<&'a LocalModel> for Facts<'a> {
    fn from(m: &'a LocalModel) -> Facts<'a> {
        Facts {
            id: &m.id,
            name: &m.name,
            loaded: m.loaded,
            capabilities: &m.capabilities,
        }
    }
}

/// Why a model can't be the chat's: it cannot chat, or it cannot call tools.
fn reason(m: &Facts<'_>) -> Option<String> {
    if !m.capabilities.contains(&Capability::Chat) {
        Some("Not a chat model".into())
    } else if !m.capabilities.contains(&Capability::Tools) {
        Some("Can't use tools: its chat template has no tool calls".into())
    } else {
        None
    }
}

/// The picker for `locals`, keeping `wanted` when it is still a usable model.
pub fn chat_models<'a>(locals: impl IntoIterator<Item = Facts<'a>>, wanted: Option<&str>) -> ChatModels {
    let mut models: Vec<ChatModel> = locals
        .into_iter()
        .filter(|m| m.capabilities.contains(&Capability::Chat))
        .map(|m| ChatModel {
            id: m.id.into(),
            name: m.name.into(),
            loaded: m.loaded,
            disabled: reason(&m),
        })
        .collect();
    models.sort_by(|a, b| {
        (a.disabled.is_some(), !a.loaded, a.name.to_lowercase()).cmp(&(b.disabled.is_some(), !b.loaded, b.name.to_lowercase()))
    });
    let usable = |id: &str| models.iter().any(|m| m.id == id && m.disabled.is_none());
    let selected = wanted
        .filter(|w| usable(w))
        .map(str::to_string)
        .or_else(|| models.iter().find(|m| m.disabled.is_none()).map(|m| m.id.clone()));
    ChatModels { models, selected }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model<'a>(id: &'a str, loaded: bool, caps: &'a [Capability]) -> Facts<'a> {
        Facts {
            id,
            name: id,
            loaded,
            capabilities: caps,
        }
    }

    #[test]
    fn tool_models_come_first_others_are_disabled_with_the_reason_and_a_loaded_one_is_the_default() {
        use Capability::{Chat, Embeddings, Tools};
        let locals = [
            model("b-tools", false, &[Chat, Tools]),
            model("a-plain", true, &[Chat]),
            model("c-tools", true, &[Chat, Tools]),
            model("embed", true, &[Embeddings]),
        ];
        let p = chat_models(locals, None);
        let ids: Vec<&str> = p.models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            ["c-tools", "b-tools", "a-plain"],
            "no embedding model; tools first, loaded first"
        );
        assert_eq!(p.selected.as_deref(), Some("c-tools"));
        assert!(p.models[2].disabled.as_deref().unwrap().contains("Can't use tools"));
        assert_eq!(
            chat_models(locals, Some("b-tools")).selected.as_deref(),
            Some("b-tools"),
            "the user's choice is kept"
        );
        assert_eq!(
            chat_models(locals, Some("a-plain")).selected.as_deref(),
            Some("c-tools"),
            "a model without tools is never chosen"
        );
        assert_eq!(chat_models([locals[1]], None).selected, None, "no tool model: nothing chosen");
        assert_eq!(reason(&model("e", false, &[Embeddings])).as_deref(), Some("Not a chat model"));
    }
}
