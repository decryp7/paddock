//! The app-wide AI runtime: the settings, the Pitwall connection, the knowledge files and the assistants made on the
//! model Pitwall serves for Debrief (ADR 0070: Pitwall is the only engine). Plain Rust, no Tauri, so it is testable
//! with the fakes of `debrief-ai` (a scripted provider, the fake Pitwall).

use super::error::{AiCmdError, AiResult};
use debrief_ai::assistant::{Assistant, AssistantConfig};
use debrief_ai::engine::pitwall::{Needs, Pitwall, PitwallView, Priority};
use debrief_ai::knowledge::{KnowledgeFile, KnowledgeProvider};
use debrief_ai::llm::LlmProvider;
use debrief_ai::settings::AiSettings;
use debrief_ai::AiError;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

const SETTINGS_FILE: &str = "ai.json";
const MAX_TOOL_ROUNDS: usize = 6;

/// The adapters the runtime works with: Pitwall, when Debrief may look for it. Tests pass the fake Pitwall or none.
pub struct AiDeps {
    pub pitwall: Option<Arc<Pitwall>>,
}

/// What the AI part is doing, for the status bar and to hide or grey AI features.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineStateDto {
    /// AI is off in this run (safe mode).
    Unavailable,
    /// Pitwall serves no model for Debrief (not installed, not running, not paired, no model fits).
    NoModel,
    Ready,
}

/// Wire contract: `AiStatus` in `ai.types.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub available: bool,
    pub state: EngineStateDto,
    pub reason: Option<String>,
    /// The model Pitwall serves for Debrief.
    pub model: Option<String>,
    pub safe_mode: bool,
    /// The model reads images ("Text + images", ADR 0025).
    pub vision: bool,
    /// What Debrief knows about Pitwall (ADR 0059).
    pub pitwall: PitwallView,
}

use crate::sync::lock;

/// The app-wide AI state. Managed once; every window shares Pitwall.
pub struct AiCore {
    dir: Option<PathBuf>,
    safe_mode: bool,
    pub(super) deps: AiDeps,
    settings: RwLock<AiSettings>,
    /// The global knowledge files (`<dir>/knowledge`, ADR 0029).
    knowledge: super::knowledge::GlobalKnowledge,
    /// A provider given directly (tests).
    fixed: Mutex<Option<Arc<dyn LlmProvider>>>,
}

impl AiCore {
    /// `dir` holds `ai.json` and the knowledge files.
    pub fn new(dir: Option<PathBuf>, safe_mode: bool, deps: AiDeps) -> AiCore {
        let (settings, warning) = match &dir {
            Some(d) => AiSettings::load(&d.join(SETTINGS_FILE)),
            None => (AiSettings::default(), None),
        };
        if let Some(w) = warning {
            log::warn!("AI: {w}");
        }
        let knowledge = super::knowledge::GlobalKnowledge::open(dir.as_ref().map(|d| d.join("knowledge")));
        let core = AiCore {
            knowledge,
            dir,
            safe_mode,
            settings: RwLock::new(settings),
            deps,
            fixed: Mutex::new(None),
        };
        core.pitwall_needs();
        core
    }

    /// Ok when AI can be used in this run; otherwise why not (safe mode turns it off).
    pub fn availability(&self) -> AiResult<()> {
        if self.safe_mode {
            return Err(AiCmdError::unavailable(
                "Debrief is running in safe mode, so the AI assistant is off. Restart normally to use it.",
            ));
        }
        Ok(())
    }

    pub fn settings(&self) -> AiSettings {
        crate::sync::read(&self.settings).clone()
    }

    fn settings_path(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(SETTINGS_FILE))
    }

    /// Uses `provider` as the model (tests).
    #[cfg(test)]
    pub fn set_fixed_provider(&self, provider: Arc<dyn LlmProvider>) {
        *lock(&self.fixed) = Some(provider);
    }

    /// Pitwall, when Debrief looks for it.
    pub fn pitwall(&self) -> Option<&Arc<Pitwall>> {
        self.deps.pitwall.as_ref()
    }

    /// Why no model answers (Pitwall not usable).
    pub fn engine_lost_reason(&self) -> String {
        self.pitwall()
            .and_then(|p| p.view().problem)
            .unwrap_or_else(|| "Pitwall is not ready for Debrief. Open Pitwall and connect Debrief to it (Settings ▸ AI).".into())
    }

    /// The model Pitwall serves, with `priority` for Pitwall's queue.
    fn model(&self, priority: Priority) -> AiResult<Arc<dyn LlmProvider>> {
        self.availability()?;
        if let Some(p) = &*lock(&self.fixed) {
            return Ok(p.clone());
        }
        let Some(pw) = self.pitwall() else {
            return Err(AiError::NoModel.into());
        };
        if !pw.view().usable {
            return Err(AiCmdError::from(AiError::Engine(self.engine_lost_reason())).in_outage_of(crate::health::hub::AI));
        }
        match pw.provider(priority) {
            Ok(p) => Ok(Arc::new(p)),
            Err(e) => Err(AiCmdError::from(AiError::Engine(e.to_string())).in_outage_of(crate::health::hub::AI)),
        }
    }

    /// The model, for a use case or a chat turn.
    pub fn provider(&self) -> AiResult<Arc<dyn LlmProvider>> {
        self.model(Priority::Interactive)
    }

    /// The global knowledge files.
    pub fn knowledge(&self) -> &super::knowledge::GlobalKnowledge {
        &self.knowledge
    }

    /// The model reading the enabled knowledge files: the window's `project` ones, then the global ones
    /// (ADR 0029). Every AI feature's assistant is made on it.
    pub fn provider_with_knowledge(&self, project: &[KnowledgeFile]) -> AiResult<Arc<dyn LlmProvider>> {
        let provider = self.provider()?;
        let global = self.knowledge.snapshot();
        Ok(Arc::new(KnowledgeProvider::new(provider, project, global.files())))
    }

    /// An assistant on the model with the current settings, the knowledge files (`project`: the window's) and its own
    /// cancel flag.
    pub fn assistant(&self, project: &[KnowledgeFile], cancel: Arc<AtomicBool>) -> AiResult<Assistant<Arc<dyn LlmProvider>>> {
        self.assistant_with_rounds(project, cancel, MAX_TOOL_ROUNDS)
    }

    /// Like [`Self::assistant`] with more tool rounds (agent mode takes a round per browser step).
    pub fn assistant_with_rounds(
        &self,
        project: &[KnowledgeFile],
        cancel: Arc<AtomicBool>,
        rounds: usize,
    ) -> AiResult<Assistant<Arc<dyn LlmProvider>>> {
        let provider = self.provider_with_knowledge(project)?;
        let s = self.settings();
        Ok(Assistant::new(
            provider,
            AssistantConfig {
                max_tokens: s.max_tokens,
                temperature: s.temperature,
                max_tool_rounds: rounds.clamp(1, 120),
                cancel,
            },
        ))
    }

    pub fn status(&self) -> AiStatus {
        let pitwall = self.pitwall().map(|p| p.view()).unwrap_or_default();
        let mut s = AiStatus {
            available: true,
            state: EngineStateDto::NoModel,
            reason: None,
            model: None,
            safe_mode: self.safe_mode,
            vision: false,
            pitwall,
        };
        if let Err(e) = self.availability() {
            s.available = false;
            s.state = EngineStateDto::Unavailable;
            s.reason = Some(e.message);
            return s;
        }
        if let Some(p) = &*lock(&self.fixed) {
            s.state = EngineStateDto::Ready;
            s.model = Some(p.display_name());
            s.vision = p.vision();
            return s;
        }
        if s.pitwall.usable {
            s.state = EngineStateDto::Ready;
            s.model = s.pitwall.model.clone();
            s.vision = self
                .pitwall()
                .and_then(|pw| pw.provider(Priority::Interactive).ok())
                .is_some_and(|p| p.vision());
        } else {
            s.reason = Some(self.engine_lost_reason());
        }
        s
    }

    // ---- settings ----

    pub(super) fn update_settings(&self, f: impl FnOnce(&mut AiSettings)) -> AiResult<AiSettings> {
        let mut s = self.settings();
        f(&mut s);
        let s = s.clamp();
        if let Some(p) = self.settings_path() {
            s.save(&p)?;
        }
        *crate::sync::write(&self.settings) = s.clone();
        Ok(s)
    }

    /// Applies new settings from the Settings screen. Pitwall is told what Debrief's prompts need.
    pub fn apply_settings(&self, next: AiSettings) -> AiResult<()> {
        let next = next.clamp();
        self.update_settings(|s| *s = next.clone())?;
        self.pitwall_needs();
        Ok(())
    }

    /// Tells Pitwall what Debrief's prompts need (the context length setting).
    pub fn pitwall_needs(&self) {
        if let Some(pw) = self.pitwall() {
            pw.set_needs(Needs {
                context_tokens: self.settings().context_length,
            });
        }
    }
}
