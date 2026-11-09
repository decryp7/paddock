//! Which models the Chat tab offers (ADR 0023 D6): every downloaded chat model and the starter catalog's tool-capable ones.
//! A model that cannot be chosen is listed disabled with a short reason ("Can't use tools", "Not downloaded"), and each
//! reason has its full note for the list. The default is a downloaded starter model that can use tools, else any
//! downloaded one that can (a loaded one first, then by name). Pure.

use pitwall_core::mgmt::LocalModel;
use pitwall_core::packs::CatalogModel;
use pitwall_core::select::Capability;
use serde::Serialize;
use std::collections::BTreeMap;

/// The short reason of a model without tool calls.
pub const NO_TOOLS: &str = "Can't use tools";
/// The short reason of a catalog model that is not downloaded.
pub const NOT_DOWNLOADED: &str = "Not downloaded";
/// The note of [`NO_TOOLS`] (the list's group label).
pub const TOOLS_NOTE: &str = "\"Can't use tools\": the model's chat template has no tool calls, so it can't use a connected app's tools.";
/// The note of [`NOT_DOWNLOADED`].
pub const DOWNLOAD_NOTE: &str = "Starter models that can use tools: download one in Models, then choose it here.";

/// One model as the picker shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatModel {
    /// The model's id (a catalog id for a model not downloaded).
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
    /// The models it can use first (loaded first, then by name), then the others grouped by their reason.
    pub models: Vec<ChatModel>,
    /// The chosen model.
    pub selected: Option<String>,
    /// For each reason in `models`, its full note (the list's group label).
    pub notes: BTreeMap<String, String>,
}

/// What the picker needs to know of a downloaded model.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    /// Its id.
    pub id: &'a str,
    /// Its name.
    pub name: &'a str,
    /// Its Hugging Face repository (to tell a starter model).
    pub repo: &'a str,
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
            repo: &m.repo,
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
        Some(NO_TOOLS.into())
    } else {
        None
    }
}

fn tool_capable(caps: &[Capability]) -> bool {
    caps.contains(&Capability::Chat) && caps.contains(&Capability::Tools)
}

/// The picker for the downloaded `locals` and the `catalog`, keeping `wanted` when it is still a usable model.
pub fn chat_models<'a>(locals: impl IntoIterator<Item = Facts<'a>>, catalog: &[CatalogModel], wanted: Option<&str>) -> ChatModels {
    let locals: Vec<Facts<'a>> = locals.into_iter().filter(|m| m.capabilities.contains(&Capability::Chat)).collect();
    let starter = |repo: &str| catalog.iter().any(|c| c.repo == repo && tool_capable(&c.capabilities));
    let mut models: Vec<ChatModel> = locals
        .iter()
        .map(|m| ChatModel {
            id: m.id.into(),
            name: m.name.into(),
            loaded: m.loaded,
            disabled: reason(m),
        })
        .collect();
    models.sort_by(|a, b| {
        (a.disabled.is_some(), !a.loaded, a.name.to_lowercase()).cmp(&(b.disabled.is_some(), !b.loaded, b.name.to_lowercase()))
    });
    let mut missing: Vec<ChatModel> = catalog
        .iter()
        .filter(|c| tool_capable(&c.capabilities) && !locals.iter().any(|l| l.repo == c.repo))
        .map(|c| ChatModel {
            id: c.id.clone(),
            name: c.name.clone(),
            loaded: false,
            disabled: Some(NOT_DOWNLOADED.into()),
        })
        .collect();
    missing.sort_by_key(|m| m.name.to_lowercase());
    models.extend(missing);
    let usable = |id: &str| models.iter().any(|m| m.id == id && m.disabled.is_none());
    let preferred = || {
        let mut ok: Vec<&Facts<'a>> = locals.iter().filter(|m| reason(m).is_none() && starter(m.repo)).collect();
        ok.sort_by_key(|m| (!m.loaded, m.name.to_lowercase()));
        ok.first().map(|m| m.id.to_string())
    };
    let selected = wanted
        .filter(|w| usable(w))
        .map(str::to_string)
        .or_else(preferred)
        .or_else(|| models.iter().find(|m| m.disabled.is_none()).map(|m| m.id.clone()));
    let mut notes = BTreeMap::new();
    for m in &models {
        match m.disabled.as_deref() {
            Some(NO_TOOLS) => notes.insert(NO_TOOLS.into(), TOOLS_NOTE.into()),
            Some(NOT_DOWNLOADED) => notes.insert(NOT_DOWNLOADED.into(), DOWNLOAD_NOTE.into()),
            _ => None,
        };
    }
    ChatModels { models, selected, notes }
}

/// The chat's window in tokens (ADR 0023 WP-4b): what one request may hold, so the meter and the check before sending use it.
/// llama-server gives each slot `--ctx-size / --parallel` tokens, so it is the context per slot, never more than the model was
/// trained for; 0 when neither is known (the engine then says what overflowed).
pub fn chat_window(context_max: Option<u64>, context_per_slot: Option<u32>) -> usize {
    let tokens = match (context_max, context_per_slot.map(u64::from)) {
        (Some(max), Some(slot)) => max.min(slot),
        (max, slot) => max.or(slot).unwrap_or(0),
    };
    usize::try_from(tokens).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Capability::{Chat, Embeddings, Tools};

    #[test]
    fn the_window_is_one_slots_context_never_more_than_the_model_was_trained_for() {
        assert_eq!(chat_window(Some(131_072), Some(8192)), 8192);
        assert_eq!(chat_window(Some(4096), Some(8192)), 4096);
        assert_eq!(chat_window(None, Some(8192)), 8192);
        assert_eq!(chat_window(Some(32_768), None), 32_768);
        assert_eq!(chat_window(None, None), 0, "unknown: the engine then says what overflowed");
        assert_eq!(chat_window(Some(u64::MAX), None), usize::MAX);
        // The room the meter and the check before sending use: the slot's context less the answer's reserve.
        let reserve = paddock_assistant::Budgets::default().max_answer_tokens as u64;
        let room = paddock_assistant::ContextUse::new(0, chat_window(Some(131_072), Some(8192)) as u64, reserve, false);
        assert_eq!(room.limit_tokens, 8192 - reserve);
    }

    fn model<'a>(id: &'a str, loaded: bool, caps: &'a [Capability]) -> Facts<'a> {
        Facts {
            id,
            name: id,
            repo: id,
            loaded,
            capabilities: caps,
        }
    }

    fn catalog(id: &str, repo: &str, caps: &[Capability]) -> CatalogModel {
        CatalogModel {
            id: id.into(),
            name: id.to_uppercase(),
            repo: repo.into(),
            context: 8192,
            capabilities: caps.to_vec(),
            tags: vec![],
            variants: vec![],
        }
    }

    #[test]
    fn tool_models_come_first_others_are_disabled_with_the_reason_and_a_loaded_one_is_the_default() {
        let locals = [
            model("b-tools", false, &[Chat, Tools]),
            model("a-plain", true, &[Chat]),
            model("c-tools", true, &[Chat, Tools]),
            model("embed", true, &[Embeddings]),
        ];
        let p = chat_models(locals, &[], None);
        let ids: Vec<&str> = p.models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(
            ids,
            ["c-tools", "b-tools", "a-plain"],
            "no embedding model; tools first, loaded first"
        );
        assert_eq!(p.selected.as_deref(), Some("c-tools"));
        assert_eq!(p.models[2].disabled.as_deref(), Some(NO_TOOLS), "short, for the option");
        assert_eq!(
            p.notes.get(NO_TOOLS).map(String::as_str),
            Some(TOOLS_NOTE),
            "the detail is the group's label"
        );
        assert!(
            chat_models(locals[2..3].to_vec(), &[], None).notes.is_empty(),
            "no disabled model, no note"
        );
        assert_eq!(
            chat_models(locals, &[], Some("b-tools")).selected.as_deref(),
            Some("b-tools"),
            "the user's choice is kept"
        );
        assert_eq!(
            chat_models(locals, &[], Some("a-plain")).selected.as_deref(),
            Some("c-tools"),
            "a model without tools is never chosen"
        );
        assert_eq!(chat_models([locals[1]], &[], None).selected, None, "no tool model: nothing chosen");
        assert_eq!(reason(&model("e", false, &[Embeddings])).as_deref(), Some("Not a chat model"));
    }

    #[test]
    fn the_catalogs_tool_models_are_listed_until_downloaded_and_a_downloaded_starter_model_is_the_default() {
        let cat = [
            catalog("qwen", "org/qwen", &[Chat, Tools]),
            catalog("gemma", "org/gemma", &[Chat]),
            catalog("big", "org/big", &[Chat, Tools]),
        ];
        let locals = [
            Facts {
                id: "my-tools",
                name: "A tool model",
                repo: "me/x",
                loaded: true,
                capabilities: &[Chat, Tools],
            },
            Facts {
                id: "qwen-q4",
                name: "Qwen",
                repo: "org/qwen",
                loaded: false,
                capabilities: &[Chat, Tools],
            },
        ];
        let p = chat_models(locals, &cat, None);
        assert_eq!(
            p.selected.as_deref(),
            Some("qwen-q4"),
            "the starter model is the default, even when another is loaded"
        );
        let rows: Vec<(&str, Option<&str>)> = p.models.iter().map(|m| (m.id.as_str(), m.disabled.as_deref())).collect();
        assert_eq!(
            rows,
            [("my-tools", None), ("qwen-q4", None), ("big", Some(NOT_DOWNLOADED))],
            "gemma can't use tools: not offered"
        );
        assert_eq!(p.notes.get(NOT_DOWNLOADED).map(String::as_str), Some(DOWNLOAD_NOTE));
        // Nothing downloaded: the catalog shows what to get, and nothing is chosen.
        let none = chat_models([], &cat, None);
        assert_eq!(none.selected, None);
        assert_eq!(none.models.len(), 2);
    }
}
