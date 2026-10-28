//! Model presets and aliases (ADR 0014). An app asks for a name such as `debrief-explain`; the user decides which model
//! and which parameters serve it. A preset names a target (one installed model, or a capability selection such as
//! `auto:chat,fast`) and optional defaults: context, temperature, top-p, max tokens, a system prompt.
//!
//! Everything here is pure: parsing, validation, the pure part of resolution, applying defaults to a request, and the
//! versioned file format. The server supplies the installed models, carries out the selection and writes the file.
//!
//! Rules (also in the user guide):
//! - An alias never shadows a real model: creating one with an installed model's id is refused, and a model downloaded
//!   later that takes an alias's name wins (the preset is then listed as shadowed).
//! - Parameters the request carries win over the preset's defaults.
//! - The system prompt is added in front only when the request has no system message of its own.
//! - `context` applies when the model is loaded for the request; a model that is already loaded keeps the context it
//!   was loaded with until it is released. (An `engine` key from an older file is ignored: llama.cpp serves everything.)

use crate::api::{ApiError, ErrorKind};
use crate::chat::{ChatRequest, Message, Role};
use crate::ids::ModelId;
use crate::mgmt::ChosenBy;
use crate::select::{Choice, Requirements};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// The file's schema version.
pub const VERSION: u32 = 1;
/// The most presets kept.
pub const MAX_PRESETS: usize = 64;
/// The longest alias.
pub const MAX_ALIAS_LEN: usize = 48;
/// The longest system prompt (bytes).
pub const MAX_SYSTEM_PROMPT_BYTES: usize = 8_000;
/// The longest target text (an `auto:` selection).
pub const MAX_TARGET_LEN: usize = 160;
/// The largest context a preset may ask for.
pub const MAX_CONTEXT: u32 = 1 << 20;
/// The smallest context a preset may ask for.
pub const MIN_CONTEXT: u32 = 512;
/// The largest `max_tokens` default.
pub const MAX_MAX_TOKENS: u32 = 1 << 20;
/// The largest file read.
pub const MAX_BYTES: usize = 1024 * 1024;

/// Why a preset was refused, or could not be resolved. Every message says what to fix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PresetError {
    /// The alias is not a valid name.
    #[error("{0}")]
    InvalidAlias(String),
    /// The target is neither a model id nor an `auto:` selection.
    #[error("{0}")]
    InvalidTarget(String),
    /// A value is out of range or too long.
    #[error("{field}: {message}")]
    Invalid {
        /// The field.
        field: &'static str,
        /// What is wrong.
        message: String,
    },
    /// The limit on the number of presets.
    #[error("there are already {MAX_PRESETS} presets; delete one first")]
    TooMany,
    /// Another preset has this alias.
    #[error("a preset named \"{0}\" already exists")]
    Duplicate(String),
    /// No such preset.
    #[error("there is no preset named \"{0}\"")]
    NotFound(String),
    /// The alias is the id of an installed model.
    #[error("\"{alias}\" is the id of an installed model; choose another name so requests for the model are not redirected")]
    NameClash {
        /// The alias.
        alias: String,
    },
    /// The target model is not installed.
    #[error("the model \"{0}\" is not installed; pick an installed model or a capability selection such as auto:chat")]
    UnknownModel(String),
    /// The preset points to a model that was removed.
    #[error("the preset \"{alias}\" points to the model \"{model}\", which is no longer installed; edit the preset in Models > Presets")]
    StaleTarget {
        /// The alias.
        alias: String,
        /// The missing model.
        model: String,
    },
    /// The preset's capability selection matches no installed model.
    #[error("the preset \"{alias}\" matched no model: {reason}")]
    NoMatch {
        /// The alias.
        alias: String,
        /// Why nothing matched.
        reason: String,
    },
}

impl From<PresetError> for ApiError {
    fn from(e: PresetError) -> Self {
        let kind = match e {
            PresetError::StaleTarget { .. } | PresetError::NoMatch { .. } => ErrorKind::PresetTarget,
            _ => ErrorKind::InvalidRequest,
        };
        ApiError::new(kind, e.to_string())
    }
}

/// A preset's name: what apps put in the `model` field. Lowercase letters, digits and `.`, `_`, `-`, starting with a
/// letter or digit, up to [`MAX_ALIAS_LEN`]. Always a valid [`ModelId`] too, so it can be listed next to models.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AliasName(String);

impl AliasName {
    /// Parses a name: trimmed and lowercased. `auto` and `default` are reserved: they already mean Pitwall's own choice.
    pub fn parse(raw: &str) -> Result<AliasName, PresetError> {
        let s = raw.trim().to_ascii_lowercase();
        let ok = !s.is_empty()
            && s.len() <= MAX_ALIAS_LEN
            && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
            && !s.contains("..");
        if !ok {
            let shown: String = raw.chars().take(60).collect();
            return Err(PresetError::InvalidAlias(format!(
                "{shown:?} is not a valid preset name: use up to {MAX_ALIAS_LEN} letters, digits and the characters \"-_.\", starting with a letter or digit"
            )));
        }
        if s == "auto" || s == "default" {
            return Err(PresetError::InvalidAlias(format!(
                "\"{s}\" is reserved (it already means Pitwall's own choice of model); choose another name"
            )));
        }
        Ok(AliasName(s))
    }

    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// As a model id, for listings (an alias is always a valid one).
    pub fn as_model_id(&self) -> Option<ModelId> {
        ModelId::parse(&self.0).ok()
    }
}

impl fmt::Display for AliasName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for AliasName {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AliasName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        AliasName::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// What a preset resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// One installed model.
    Model(ModelId),
    /// Whatever fits these requirements best when the request arrives (`auto:chat,fast`).
    Auto(Requirements),
}

impl Target {
    /// Parses a model id, or `auto` / `auto:<capabilities>`.
    pub fn parse(raw: &str) -> Result<Target, PresetError> {
        let s = raw.trim().to_ascii_lowercase();
        if s.len() > MAX_TARGET_LEN {
            return Err(PresetError::InvalidTarget(format!(
                "the target is longer than {MAX_TARGET_LEN} characters"
            )));
        }
        if s == "auto" || s.starts_with("auto:") {
            let spec = s.strip_prefix("auto:").unwrap_or("");
            return Requirements::parse_auto(spec).map(Target::Auto).map_err(PresetError::InvalidTarget);
        }
        ModelId::parse(&s)
            .map(Target::Model)
            .map_err(|e| PresetError::InvalidTarget(e.to_string()))
    }

    /// The canonical text (`parse` of it gives the same target).
    pub fn to_text(&self) -> String {
        match self {
            Target::Model(id) => id.to_string(),
            Target::Auto(r) => {
                let mut parts: Vec<String> = r.capabilities.iter().map(|c| format!("{c:?}").to_ascii_lowercase()).collect();
                if let Some(n) = r.min_context {
                    parts.push(n.to_string());
                }
                let prefer = format!("{:?}", r.prefer).to_ascii_lowercase();
                if prefer != "balanced" {
                    parts.push(prefer);
                }
                if parts.is_empty() {
                    "auto".into()
                } else {
                    format!("auto:{}", parts.join(","))
                }
            }
        }
    }
}

impl Serialize for Target {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_text())
    }
}

impl<'de> Deserialize<'de> for Target {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Target::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// A named choice of model and defaults (`ModelPreset` in `api.types.ts`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPreset {
    /// The name apps send as `model`.
    pub alias: AliasName,
    /// The model or capability selection behind it.
    pub target: Target,
    /// Context the model loads with for this preset.
    #[serde(default)]
    pub context: Option<u32>,
    /// Default sampling temperature.
    #[serde(default)]
    pub temperature: Option<f32>,
    /// Default nucleus sampling.
    #[serde(default)]
    pub top_p: Option<f32>,
    /// Default most tokens to generate.
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// A system prompt added when the request has none.
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Answer identical deterministic requests from the response cache (ADR 0019).
    #[serde(default)]
    pub cache_responses: bool,
    /// Keep the prompt prefix (the system prompt) warm: saved to disk when the model unloads and restored on load.
    #[serde(default)]
    pub keep_warm: bool,
}

fn invalid(field: &'static str, message: impl Into<String>) -> PresetError {
    PresetError::Invalid {
        field,
        message: message.into(),
    }
}

impl ModelPreset {
    /// Checks everything that needs no knowledge of the installed models, and tidies the value (an empty system
    /// prompt is none).
    pub fn checked(mut self) -> Result<ModelPreset, PresetError> {
        if self.context.is_some_and(|c| !(MIN_CONTEXT..=MAX_CONTEXT).contains(&c)) {
            return Err(invalid("context", format!("use between {MIN_CONTEXT} and {MAX_CONTEXT} tokens")));
        }
        if self.temperature.is_some_and(|t| !(0.0..=2.0).contains(&t)) {
            return Err(invalid("temperature", "use a value between 0 and 2"));
        }
        if self.top_p.is_some_and(|p| !(0.0..=1.0).contains(&p)) {
            return Err(invalid("top-p", "use a value between 0 and 1"));
        }
        if self.max_tokens.is_some_and(|m| !(1..=MAX_MAX_TOKENS).contains(&m)) {
            return Err(invalid("max tokens", format!("use between 1 and {MAX_MAX_TOKENS}")));
        }
        self.system_prompt = self.system_prompt.filter(|p| !p.trim().is_empty());
        if self.system_prompt.as_ref().is_some_and(|p| p.len() > MAX_SYSTEM_PROMPT_BYTES) {
            return Err(invalid(
                "system prompt",
                format!(
                    "use at most {} of text",
                    paddock_units::format_whole(MAX_SYSTEM_PROMPT_BYTES as u64, paddock_units::Kind::Storage)
                ),
            ));
        }
        Ok(self)
    }
}

/// What the installed library offers, for the checks a draft needs.
#[derive(Debug, Clone, Copy)]
pub struct Known<'a> {
    /// Installed model ids.
    pub models: &'a [ModelId],
}

impl Known<'_> {
    fn check(&self, p: &ModelPreset) -> Result<(), PresetError> {
        if p.alias.as_model_id().is_some_and(|id| self.models.contains(&id)) {
            return Err(PresetError::NameClash {
                alias: p.alias.to_string(),
            });
        }
        if let Target::Model(id) = &p.target {
            if !self.models.contains(id) {
                return Err(PresetError::UnknownModel(id.to_string()));
            }
        }
        Ok(())
    }
}

/// The presets, sorted by alias.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Presets {
    list: Vec<ModelPreset>,
}

/// What loading found.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Loaded {
    /// The presets that were usable.
    pub presets: Presets,
    /// Entries that were damaged, duplicated or over the limit and were left out.
    pub skipped: usize,
    /// The primary file existed but could not be used (the backup, or nothing, was used instead).
    pub damaged: bool,
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    presets: Vec<serde_json::Value>,
}

fn parse_file(text: &str) -> Option<Loaded> {
    let f: File = serde_json::from_str(text).ok()?;
    // A file from a newer Pitwall is not guessed at.
    if f.version > VERSION {
        return None;
    }
    let mut out = Presets::default();
    let mut skipped = 0;
    for v in f.presets {
        let parsed = serde_json::from_value::<ModelPreset>(v).ok().and_then(|p| p.checked().ok());
        match parsed {
            Some(p) if out.list.len() < MAX_PRESETS && out.get(p.alias.as_str()).is_none() => out.insert_sorted(p),
            _ => skipped += 1,
        }
    }
    Some(Loaded {
        presets: out,
        skipped,
        damaged: false,
    })
}

/// The presets from the primary file, else the backup, else none. Damaged entries are skipped, not fatal.
pub fn load(primary: Option<&str>, backup: Option<&str>) -> Loaded {
    let first = primary.and_then(parse_file);
    let damaged = primary.is_some() && first.is_none();
    let mut loaded = first.or_else(|| backup.and_then(parse_file)).unwrap_or_default();
    loaded.damaged = damaged;
    loaded
}

impl Presets {
    /// All presets, sorted by alias.
    pub fn list(&self) -> &[ModelPreset] {
        &self.list
    }

    /// How many.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The preset an app's `model` value names (trimmed, case-insensitive).
    pub fn get(&self, name: &str) -> Option<&ModelPreset> {
        let n = name.trim().to_ascii_lowercase();
        self.list.iter().find(|p| p.alias.as_str() == n)
    }

    fn insert_sorted(&mut self, p: ModelPreset) {
        let at = self.list.partition_point(|x| x.alias < p.alias);
        self.list.insert(at, p);
    }

    /// Adds a preset after checking it against the library.
    pub fn create(&mut self, p: ModelPreset, known: &Known) -> Result<ModelPreset, PresetError> {
        let p = p.checked()?;
        known.check(&p)?;
        if self.get(p.alias.as_str()).is_some() {
            return Err(PresetError::Duplicate(p.alias.to_string()));
        }
        if self.list.len() >= MAX_PRESETS {
            return Err(PresetError::TooMany);
        }
        self.insert_sorted(p.clone());
        Ok(p)
    }

    /// Replaces the preset `alias` (which may be renamed by the new value's alias).
    pub fn update(&mut self, alias: &str, p: ModelPreset, known: &Known) -> Result<ModelPreset, PresetError> {
        let p = p.checked()?;
        known.check(&p)?;
        let old = alias.trim().to_ascii_lowercase();
        let Some(at) = self.list.iter().position(|x| x.alias.as_str() == old) else {
            return Err(PresetError::NotFound(old));
        };
        if p.alias.as_str() != old && self.get(p.alias.as_str()).is_some() {
            return Err(PresetError::Duplicate(p.alias.to_string()));
        }
        self.list.remove(at);
        self.insert_sorted(p.clone());
        Ok(p)
    }

    /// Removes a preset.
    pub fn delete(&mut self, alias: &str) -> Result<ModelPreset, PresetError> {
        let old = alias.trim().to_ascii_lowercase();
        match self.list.iter().position(|x| x.alias.as_str() == old) {
            Some(at) => Ok(self.list.remove(at)),
            None => Err(PresetError::NotFound(old)),
        }
    }

    /// The file's text.
    pub fn to_json(&self) -> String {
        let file = File {
            version: VERSION,
            presets: self.list.iter().filter_map(|p| serde_json::to_value(p).ok()).collect(),
        };
        serde_json::to_string(&file).unwrap_or_default()
    }
}

/// A preset resolved to a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// The model that serves it.
    pub model: ModelId,
    /// How it was chosen (`Explicit` for a fixed target, `Capabilities` for a selection).
    pub chosen_by: ChosenBy,
    /// One sentence for the inspector.
    pub reason: String,
}

/// Resolves a preset's target. `choose` carries out a capability selection against the installed models (the server
/// owns the candidates); a fixed target must be among `installed`, else the typed [`PresetError::StaleTarget`].
pub fn resolve(
    p: &ModelPreset,
    installed: &[ModelId],
    choose: impl FnOnce(&Requirements) -> Result<Choice, String>,
) -> Result<Resolution, PresetError> {
    match &p.target {
        Target::Model(id) if installed.contains(id) => Ok(Resolution {
            model: id.clone(),
            chosen_by: ChosenBy::Explicit,
            reason: format!("the preset {} names this model", p.alias),
        }),
        Target::Model(id) => Err(PresetError::StaleTarget {
            alias: p.alias.to_string(),
            model: id.to_string(),
        }),
        Target::Auto(req) => choose(req)
            .map(|c| Resolution {
                model: c.id,
                chosen_by: ChosenBy::Capabilities,
                reason: format!("the preset {}: {}", p.alias, c.reason),
            })
            .map_err(|reason| PresetError::NoMatch {
                alias: p.alias.to_string(),
                reason,
            }),
    }
}

/// Fills the request's gaps from the preset: sampling defaults only where the request set none, and the system prompt
/// in front only when the request has no system message.
pub fn apply(p: &ModelPreset, req: &mut ChatRequest) {
    req.temperature = req.temperature.or(p.temperature);
    req.top_p = req.top_p.or(p.top_p);
    req.max_tokens = req.max_tokens.or(p.max_tokens);
    if let Some(prompt) = &p.system_prompt {
        if !req.messages.iter().any(|m| m.role == Role::System) {
            req.messages.insert(0, Message::text(Role::System, prompt.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::select::{Capability, Prefer};

    fn mid(s: &str) -> ModelId {
        ModelId::parse(s).unwrap()
    }

    fn preset(alias: &str, target: &str) -> ModelPreset {
        ModelPreset {
            alias: AliasName::parse(alias).unwrap(),
            target: Target::parse(target).unwrap(),
            context: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            system_prompt: None,
            cache_responses: false,
            keep_warm: false,
        }
    }

    #[test]
    fn alias_parsing_is_strict() {
        assert_eq!(AliasName::parse("  Debrief-Explain ").unwrap().as_str(), "debrief-explain");
        assert_eq!(AliasName::parse("a.b_c-1").unwrap().to_string(), "a.b_c-1");
        for bad in [
            "",
            "   ",
            "-x",
            ".x",
            "a b",
            "a/b",
            "a:b",
            "a..b",
            "ünï",
            &"a".repeat(MAX_ALIAS_LEN + 1),
        ] {
            assert!(matches!(AliasName::parse(bad), Err(PresetError::InvalidAlias(_))), "{bad}");
        }
        assert!(AliasName::parse(&"a".repeat(MAX_ALIAS_LEN)).is_ok());
        for r in ["AUTO", "default"] {
            assert!(AliasName::parse(r).unwrap_err().to_string().contains("reserved"), "{r}");
        }
        assert_eq!(AliasName::parse("x").unwrap().as_model_id(), Some(mid("x")));
        assert!(serde_json::from_str::<AliasName>("\"bad name\"").is_err());
        assert_eq!(serde_json::to_value(AliasName::parse("ok").unwrap()).unwrap(), "ok");
    }

    #[test]
    fn targets_round_trip_through_text() {
        assert_eq!(Target::parse("Qwen3-8B").unwrap(), Target::Model(mid("qwen3-8b")));
        for text in [
            "auto",
            "auto:chat",
            "auto:vision,tools,8192,fast",
            "auto:chat,quality",
            "auto:embeddings,small",
            "auto:8192",
        ] {
            let t = Target::parse(text).unwrap();
            assert_eq!(t.to_text(), text);
            assert_eq!(Target::parse(&t.to_text()).unwrap(), t);
        }
        let t = Target::parse("auto: Chat balanced").unwrap();
        assert_eq!(t.to_text(), "auto:chat");
        assert!(matches!(t, Target::Auto(r) if r.prefer == Prefer::Balanced && r.capabilities == vec![Capability::Chat]));
        assert!(matches!(Target::parse("auto:telepathy"), Err(PresetError::InvalidTarget(m)) if m.contains("not a capability")));
        assert!(matches!(Target::parse("bad name"), Err(PresetError::InvalidTarget(_))));
        assert!(matches!(Target::parse(""), Err(PresetError::InvalidTarget(_))));
        assert!(matches!(Target::parse(&"a".repeat(MAX_TARGET_LEN + 1)), Err(PresetError::InvalidTarget(m)) if m.contains("longer")));
        assert_eq!(
            serde_json::to_value(Target::parse("auto:chat,fast").unwrap()).unwrap(),
            "auto:chat,fast"
        );
        assert!(serde_json::from_str::<Target>("\"auto:nope\"").is_err());
    }

    #[test]
    fn values_are_range_checked_and_tidied() {
        let ok = |f: &dyn Fn(&mut ModelPreset)| {
            let mut p = preset("a", "m");
            f(&mut p);
            p.checked()
        };
        assert!(ok(&|_| {}).is_ok());
        assert!(ok(&|p| p.context = Some(MIN_CONTEXT)).is_ok());
        assert!(ok(&|p| p.context = Some(MAX_CONTEXT)).is_ok());
        assert!(ok(&|p| p.temperature = Some(0.0)).is_ok());
        assert!(ok(&|p| p.top_p = Some(1.0)).is_ok());
        assert!(ok(&|p| p.max_tokens = Some(1)).is_ok());
        for (name, r) in [
            ("context", ok(&|p| p.context = Some(MIN_CONTEXT - 1))),
            ("context", ok(&|p| p.context = Some(MAX_CONTEXT + 1))),
            ("temperature", ok(&|p| p.temperature = Some(2.5))),
            ("temperature", ok(&|p| p.temperature = Some(f32::NAN))),
            ("top-p", ok(&|p| p.top_p = Some(-0.1))),
            ("max tokens", ok(&|p| p.max_tokens = Some(0))),
            ("max tokens", ok(&|p| p.max_tokens = Some(MAX_MAX_TOKENS + 1))),
            (
                "system prompt",
                ok(&|p| p.system_prompt = Some("x".repeat(MAX_SYSTEM_PROMPT_BYTES + 1))),
            ),
        ] {
            match r {
                Err(PresetError::Invalid { field, .. }) => assert_eq!(field, name),
                other => panic!("{name}: {other:?}"),
            }
        }
        let p = ok(&|p| {
            p.system_prompt = Some("   \n".into());
        })
        .unwrap();
        assert_eq!(p.system_prompt, None, "blank means none");
        let e = ok(&|p| p.temperature = Some(9.0)).unwrap_err();
        assert!(e.to_string().starts_with("temperature:"), "{e}");
    }

    #[test]
    fn create_update_delete_with_the_library_checks() {
        let models = vec![mid("tiny-q4"), mid("big-q4")];
        let known = Known { models: &models };
        let mut ps = Presets::default();
        assert!(ps.is_empty());
        ps.create(preset("zeta", "tiny-q4"), &known).unwrap();
        ps.create(preset("alpha", "auto:chat,fast"), &known).unwrap();
        ps.create(preset("mid", "big-q4"), &known).unwrap();
        assert_eq!(ps.len(), 3);
        let order: Vec<&str> = ps.list().iter().map(|p| p.alias.as_str()).collect();
        assert_eq!(order, ["alpha", "mid", "zeta"], "sorted by alias");
        assert!(ps.get("  ZETA ").is_some());
        assert!(ps.get("nope").is_none());

        assert_eq!(
            ps.create(preset("zeta", "big-q4"), &known),
            Err(PresetError::Duplicate("zeta".into()))
        );
        let clash = ps.create(preset("tiny-q4", "big-q4"), &known).unwrap_err();
        assert!(
            matches!(clash, PresetError::NameClash { .. }) && clash.to_string().contains("installed model"),
            "{clash}"
        );
        assert_eq!(
            ps.create(preset("new", "gone-q4"), &known),
            Err(PresetError::UnknownModel("gone-q4".into()))
        );
        let mut bad = preset("new", "tiny-q4");
        bad.temperature = Some(7.0);
        assert!(matches!(ps.create(bad, &known), Err(PresetError::Invalid { .. })));

        // Update in place and rename.
        let mut p = preset("zeta", "big-q4");
        p.temperature = Some(0.2);
        let saved = ps.update("zeta", p, &known).unwrap();
        assert_eq!(saved.temperature, Some(0.2));
        ps.update(" Zeta ", preset("omega", "tiny-q4"), &known).unwrap();
        assert!(ps.get("zeta").is_none() && ps.get("omega").is_some());
        assert_eq!(
            ps.update("nothing", preset("x", "tiny-q4"), &known),
            Err(PresetError::NotFound("nothing".into()))
        );
        assert_eq!(
            ps.update("omega", preset("alpha", "tiny-q4"), &known),
            Err(PresetError::Duplicate("alpha".into()))
        );
        assert!(ps.update("omega", preset("tiny-q4", "big-q4"), &known).is_err(), "clash on rename");
        assert!(ps.update("omega", preset("omega", "gone"), &known).is_err());
        assert_eq!(ps.len(), 3);

        assert_eq!(ps.delete("OMEGA").unwrap().alias.as_str(), "omega");
        assert_eq!(ps.delete("omega"), Err(PresetError::NotFound("omega".into())));
        assert_eq!(ps.len(), 2);
    }

    #[test]
    fn the_count_is_capped() {
        let models = vec![mid("m")];
        let known = Known { models: &models };
        let mut ps = Presets::default();
        for i in 0..MAX_PRESETS {
            ps.create(preset(&format!("p{i}"), "m"), &known).unwrap();
        }
        let e = ps.create(preset("one-more", "m"), &known).unwrap_err();
        assert_eq!(e, PresetError::TooMany);
        assert!(e.to_string().contains("64"), "{e}");
    }

    #[test]
    fn persistence_round_trips_and_survives_damage() {
        let models = vec![mid("m")];
        let known = Known { models: &models };
        let mut ps = Presets::default();
        let mut p = preset("debrief-explain", "m");
        p.system_prompt = Some("Explain plainly.".into());
        p.temperature = Some(0.3);
        p.context = Some(4096);
        ps.create(p, &known).unwrap();
        ps.create(preset("auto-ish", "auto:chat,fast"), &known).unwrap();
        let text = ps.to_json();
        assert!(text.contains("\"version\":1") && text.contains("\"systemPrompt\""), "{text}");
        let back = load(Some(&text), None);
        assert_eq!(back.presets, ps);
        assert_eq!((back.skipped, back.damaged), (0, false));
        assert!(!load(None, None).damaged, "a missing file is not damage");
        assert!(load(Some("{ damaged"), Some(&text)).damaged);
        assert!(load(Some("garbage"), None).damaged);
        // The backup is used when the primary is damaged, then nothing.
        assert_eq!(load(Some("{ damaged"), Some(&text)).presets, ps);
        assert_eq!(load(None, Some(&text)).presets, ps);
        assert!(load(Some("garbage"), Some("more")).presets.is_empty());
        assert!(load(None, None).presets.is_empty());
        // A newer file is not guessed at (the backup is tried).
        let newer = r#"{"version":99,"presets":[]}"#;
        assert!(load(Some(newer), None).presets.is_empty());
        assert_eq!(load(Some(newer), Some(&text)).presets, ps);
    }

    #[test]
    fn damaged_entries_are_skipped_one_by_one() {
        let text = r#"{"version":1,"presets":[
            {"alias":"good","target":"m"},
            {"alias":"Good","target":"m"},
            {"alias":"bad name","target":"m"},
            {"alias":"hot","target":"m","temperature":50},
            {"alias":"notarget"},
            42,
            {"alias":"fine","target":"auto:chat","maxTokens":256,"engine":"fakeoai"}]}"#;
        let l = load(Some(text), None);
        let names: Vec<&str> = l.presets.list().iter().map(|p| p.alias.as_str()).collect();
        assert_eq!(names, ["fine", "good"]);
        assert_eq!(l.skipped, 5, "duplicate, bad name, out of range, no target, not an object");
        // Over the cap: the rest are skipped.
        let many: Vec<String> = (0..MAX_PRESETS + 3)
            .map(|i| format!(r#"{{"alias":"p{i}","target":"m"}}"#))
            .collect();
        let l = load(Some(&format!(r#"{{"version":1,"presets":[{}]}}"#, many.join(","))), None);
        assert_eq!((l.presets.len(), l.skipped), (MAX_PRESETS, 3));
    }

    fn choice(id: &str) -> Result<Choice, String> {
        Ok(Choice {
            id: mid(id),
            reason: "chosen for chat (Fast)".into(),
        })
    }

    #[test]
    fn resolution_names_the_model_or_a_typed_error() {
        let installed = vec![mid("tiny"), mid("big")];
        let fixed = resolve(&preset("a", "tiny"), &installed, |_| panic!("no selection for a fixed target")).unwrap();
        assert_eq!((fixed.model, fixed.chosen_by), (mid("tiny"), ChosenBy::Explicit));
        assert!(fixed.reason.contains('a'));
        let auto = resolve(&preset("b", "auto:chat,fast"), &installed, |r| {
            assert_eq!(r.prefer, Prefer::Fast);
            choice("big")
        })
        .unwrap();
        assert_eq!((auto.model, auto.chosen_by), (mid("big"), ChosenBy::Capabilities));
        assert!(auto.reason.contains("preset b") && auto.reason.contains("Fast"), "{}", auto.reason);

        let stale = resolve(&preset("c", "removed"), &installed, |_| choice("tiny")).unwrap_err();
        assert_eq!(
            stale,
            PresetError::StaleTarget {
                alias: "c".into(),
                model: "removed".into()
            }
        );
        assert!(stale.to_string().contains("no longer installed") && stale.to_string().contains("Models > Presets"));
        let none = resolve(&preset("d", "auto:vision"), &installed, |_| {
            Err("no installed model offers vision".into())
        })
        .unwrap_err();
        assert!(
            matches!(&none, PresetError::NoMatch { reason, .. } if reason.contains("vision")),
            "{none}"
        );

        let api: ApiError = stale.into();
        assert_eq!((api.kind, api.code()), (ErrorKind::PresetTarget, "preset_target_unavailable"));
        let api: ApiError = none.into();
        assert_eq!(api.kind, ErrorKind::PresetTarget);
        let api: ApiError = PresetError::TooMany.into();
        assert_eq!(api.kind, ErrorKind::InvalidRequest);
    }

    fn req(messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            messages,
            ..ChatRequest::default()
        }
    }

    #[test]
    fn request_values_win_over_preset_defaults() {
        let mut p = preset("a", "m");
        p.temperature = Some(0.2);
        p.top_p = Some(0.9);
        p.max_tokens = Some(300);
        let mut r = req(vec![Message::text(Role::User, "hi")]);
        apply(&p, &mut r);
        assert_eq!((r.temperature, r.top_p, r.max_tokens), (Some(0.2), Some(0.9), Some(300)));
        let mut r = req(vec![Message::text(Role::User, "hi")]);
        r.temperature = Some(1.0);
        r.top_p = Some(0.5);
        r.max_tokens = Some(10);
        apply(&p, &mut r);
        assert_eq!((r.temperature, r.top_p, r.max_tokens), (Some(1.0), Some(0.5), Some(10)));
        // Even an explicit zero temperature is the request's choice.
        let mut r = req(vec![Message::text(Role::User, "hi")]);
        r.temperature = Some(0.0);
        apply(&p, &mut r);
        assert_eq!(r.temperature, Some(0.0));
        // A preset with no defaults changes nothing.
        let mut r = req(vec![Message::text(Role::User, "hi")]);
        let before = r.clone();
        apply(&preset("b", "m"), &mut r);
        assert_eq!(r, before);
    }

    #[test]
    fn the_system_prompt_is_added_only_when_the_request_has_none() {
        let mut p = preset("a", "m");
        p.system_prompt = Some("Be brief.".into());
        let mut r = req(vec![Message::text(Role::User, "hi")]);
        apply(&p, &mut r);
        assert_eq!(r.messages.len(), 2);
        assert_eq!(
            (r.messages[0].role, r.messages[0].text_content()),
            (Role::System, "Be brief.".to_string())
        );
        assert_eq!(r.messages[1].role, Role::User);
        let mut r = req(vec![Message::text(Role::System, "Mine."), Message::text(Role::User, "hi")]);
        apply(&p, &mut r);
        assert_eq!(r.messages.len(), 2, "the request's own system message is kept alone");
        assert_eq!(r.messages[0].text_content(), "Mine.");
        // A system message later in the conversation also counts.
        let mut r = req(vec![Message::text(Role::User, "hi"), Message::text(Role::System, "late")]);
        apply(&p, &mut r);
        assert_eq!(r.messages.len(), 2);
    }
}
