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

/// Why the chat cannot take a picture when no model has been chosen yet.
pub const NO_MODEL_IMAGES: &str = "Choose a model that can read images first.";

/// A downloaded model that can use tools and read images: the one the pane offers when the chosen model cannot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionSuggestion {
    /// The model's id.
    pub id: String,
    /// Its name.
    pub name: String,
}

/// What the chosen model means for pictures (ADR 0023 Decision 4): whether it reads them, why not in words that say what to do,
/// the downloaded model the pane offers instead, and the pixel budget a picture is shrunk to. Rust decides; the pane shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatImages {
    /// The chosen model reads images (it has a vision projector).
    pub allowed: bool,
    /// When it does not: why, and what to do.
    pub reason: Option<String>,
    /// A downloaded model that can use tools and read images; `None` when there is none.
    pub suggest: Option<VisionSuggestion>,
    /// Most pixels of a picture sent to the chosen model (not sent to the pane).
    #[serde(skip)]
    pub max_pixels: u64,
}

impl Default for ChatImages {
    /// Before a model is chosen: no pictures.
    fn default() -> ChatImages {
        ChatImages {
            allowed: false,
            reason: Some(NO_MODEL_IMAGES.into()),
            suggest: None,
            max_pixels: super::pictures::DEFAULT_MAX_PIXELS,
        }
    }
}

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
    /// It has a vision projector: it reads pictures.
    pub vision: bool,
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
    /// What the chosen model means for pictures.
    pub images: ChatImages,
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
    /// Most pixels of a picture its projector takes (`None`: unknown, the default budget is used).
    pub image_max_pixels: Option<u64>,
}

impl Facts<'_> {
    fn vision(&self) -> bool {
        self.capabilities.contains(&Capability::Vision)
    }
}

impl<'a> From<&'a LocalModel> for Facts<'a> {
    fn from(m: &'a LocalModel) -> Facts<'a> {
        Facts {
            id: &m.id,
            name: &m.name,
            repo: &m.repo,
            loaded: m.loaded,
            capabilities: &m.capabilities,
            image_max_pixels: m.image_max_pixels,
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
            vision: m.vision(),
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
            vision: c.capabilities.contains(&Capability::Vision),
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
    let images = chat_images(&locals, selected.as_deref());
    ChatModels {
        models,
        selected,
        notes,
        images,
    }
}

/// What `selected` (a model id) means for pictures, among the downloaded `locals`: a model with a projector reads them, at its
/// projector's pixel budget; any other is told what to do, naming the downloaded vision model that can also use tools (a loaded
/// one first, then by name) when there is one.
pub fn chat_images(locals: &[Facts<'_>], selected: Option<&str>) -> ChatImages {
    let chosen = selected.and_then(|id| locals.iter().find(|m| m.id == id));
    let Some(chosen) = chosen else {
        return ChatImages::default();
    };
    if chosen.vision() {
        return ChatImages {
            allowed: true,
            reason: None,
            suggest: None,
            max_pixels: chosen.image_max_pixels.unwrap_or(super::pictures::DEFAULT_MAX_PIXELS),
        };
    }
    let mut seeing: Vec<&Facts<'_>> = locals.iter().filter(|m| reason(m).is_none() && m.vision()).collect();
    seeing.sort_by_key(|m| (!m.loaded, m.name.to_lowercase()));
    let suggest = seeing.first().map(|m| VisionSuggestion {
        id: m.id.to_string(),
        name: m.name.to_string(),
    });
    let words = match &suggest {
        Some(s) => format!("{} can't read images. Choose {} to ask about a picture.", chosen.name, s.name),
        None => format!(
            "{} can't read images. Download a model that can (one with a vision file, for example Gemma 3 or Qwen2.5-VL) in Models, then choose it here.",
            chosen.name
        ),
    };
    ChatImages {
        allowed: false,
        reason: Some(words),
        suggest,
        max_pixels: super::pictures::DEFAULT_MAX_PIXELS,
    }
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
            image_max_pixels: None,
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
                image_max_pixels: None,
            },
            Facts {
                id: "qwen-q4",
                name: "Qwen",
                repo: "org/qwen",
                loaded: false,
                capabilities: &[Chat, Tools],
                image_max_pixels: None,
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

    fn seeing(id: &str, loaded: bool, caps: &[Capability], px: Option<u64>) -> Facts<'static> {
        // The test facts live as long as the program: leak the small slice (a test only).
        let caps: &'static [Capability] = Box::leak(caps.to_vec().into_boxed_slice());
        let id: &'static str = Box::leak(id.to_string().into_boxed_str());
        Facts {
            id,
            name: id,
            repo: id,
            loaded,
            capabilities: caps,
            image_max_pixels: px,
        }
    }

    /// WP-5 (ADR 0023 Decision 4): only a model with a vision projector reads pictures, at its projector's budget; any other says
    /// so in words that name the downloaded vision model to choose, or what to download.
    #[test]
    fn images_are_for_models_with_a_projector_and_the_others_say_what_to_do() {
        let plain = seeing("plain", true, &[Chat, Tools], None);
        let eyes_b = seeing("b-eyes", false, &[Chat, Tools, Capability::Vision], Some(500_000));
        let eyes_a = seeing("a-eyes", false, &[Chat, Tools, Capability::Vision], Some(900_000));
        let eyes_notools = seeing("c-eyes", true, &[Chat, Capability::Vision], None);

        let ok = chat_images(&[plain, eyes_b, eyes_a], Some("b-eyes"));
        assert_eq!((ok.allowed, ok.reason, ok.suggest, ok.max_pixels), (true, None, None, 500_000));
        let default_budget = chat_images(&[eyes_notools], Some("c-eyes"));
        assert_eq!(
            default_budget.max_pixels,
            super::super::pictures::DEFAULT_MAX_PIXELS,
            "no projector header: the default budget"
        );

        let no = chat_images(&[plain, eyes_b, eyes_a, eyes_notools], Some("plain"));
        assert!(!no.allowed);
        // The first by name among those that can also use tools: a model without tools is never offered (the chat would refuse it).
        assert_eq!(no.suggest.as_ref().map(|s| s.id.as_str()), Some("a-eyes"));
        let words = no.reason.unwrap();
        assert!(
            words.contains("plain can't read images") && words.contains("Choose a-eyes"),
            "{words}"
        );

        let none = chat_images(&[plain], Some("plain"));
        assert!(none.suggest.is_none());
        let words = none.reason.unwrap();
        assert!(words.contains("Download a model") && words.contains("Models"), "{words}");

        // Nothing chosen yet, or a choice that is not downloaded: no pictures, in words.
        assert_eq!(chat_images(&[plain], None).reason.as_deref(), Some(NO_MODEL_IMAGES));
        assert!(!chat_images(&[plain], Some("gone")).allowed);
    }

    /// The picker carries each model's vision flag and the chosen model's image state.
    #[test]
    fn the_picker_says_which_models_read_images() {
        let eyes = seeing("eyes", true, &[Chat, Tools, Capability::Vision], None);
        let plain = seeing("plain", false, &[Chat, Tools], None);
        let p = chat_models([eyes, plain], &[], None);
        let flags: Vec<(&str, bool)> = p.models.iter().map(|m| (m.id.as_str(), m.vision)).collect();
        assert_eq!(flags, [("eyes", true), ("plain", false)]);
        assert!(p.images.allowed);
        let q = chat_models([eyes, plain], &[], Some("plain"));
        assert!(!q.images.allowed && q.images.suggest.as_ref().is_some_and(|s| s.id == "eyes"));
    }
}
