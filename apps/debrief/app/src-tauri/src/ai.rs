//! The local AI assistant (`debrief-ai`): the use cases, chat, and the Settings → AI section. Inference runs in
//! Pitwall, the family's local model server (ADR 0070: it is the only engine; never a cloud service); each window has
//! its own chats.
//!
//! Everything is optional: in safe mode or while Pitwall serves no model for Debrief, every command fails with a typed
//! [`AiCmdError`] (`code` = `unavailable` / `noModel`) and the UI hides or greys its AI features.
//!
//! Events: `debrief://ai-status` (Pitwall or the model changed, all windows), `debrief://ai-chat` (chat stream, to the
//! window of the chat).

mod chats;
pub(crate) mod commands;
mod core;
pub(crate) mod error;
pub(crate) mod folder_setup;
pub(crate) mod knowledge;
pub(crate) mod parser_job;
pub(crate) mod pitwall;
pub(crate) mod requests;
mod settings_section;
#[cfg(test)]
mod tests;
pub(crate) mod vision;

#[cfg(test)]
pub use self::core::EngineStateDto;
pub use self::core::{AiCore, AiDeps, AiStatus};
pub use chats::WindowChats;
pub use settings_section::AiSettingsSection;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, Runtime};

pub const STATUS_EVENT: &str = "debrief://ai-status";
pub const CHAT_EVENT: &str = "debrief://ai-chat";
/// The app-wide AI runtime, managed as Tauri state.
pub struct AiState(pub Arc<AiCore>);

/// A window's AI state: its chats and the use cases it is running (to cancel them).
#[derive(Default)]
pub struct WindowAi {
    pub chats: WindowChats,
    ops: Mutex<Vec<Arc<AtomicBool>>>,
    /// Images the window opened for the AI (clean copies, bounded; ADR 0025).
    pub images: vision::ImageCache,
    /// The project's knowledge files (ADR 0029), saved with the project.
    pub knowledge: knowledge::ProjectKnowledge,
}

impl WindowAi {
    /// Registers the cancel flag of a running AI job, so `ai_cancel` reaches it too.
    pub fn track_op(&self, flag: Arc<AtomicBool>) {
        crate::sync::lock(&self.ops).push(flag);
    }

    pub fn end_op(&self, flag: &Arc<AtomicBool>) {
        crate::sync::lock(&self.ops).retain(|f| !Arc::ptr_eq(f, flag));
    }

    /// Cancels every running use case of the window; returns how many there were.
    pub fn cancel_ops(&self) -> usize {
        let ops = crate::sync::lock(&self.ops);
        for f in ops.iter() {
            f.store(true, Ordering::Relaxed);
        }
        ops.len()
    }
}

impl Drop for WindowAi {
    fn drop(&mut self) {
        self.cancel_ops();
        self.chats.cancel_all();
    }
}

/// Creates the AI runtime and registers Settings → AI. Call from `setup` after `settings::init`.
/// Never fails: problems leave the AI unavailable with a reason the UI shows.
pub fn init<R: Runtime>(app: &AppHandle<R>, safe_mode: bool) {
    let dir = crate::app_dirs::config(app).ok().map(|d| d.join("ai"));
    // Pitwall is looked for unless this is a safe-mode run (ADR 0059).
    let deps = AiDeps {
        pitwall: (!safe_mode).then(pitwall::system),
    };
    let core = Arc::new(AiCore::new(dir, safe_mode, deps));
    manage(app, core.clone());
    // Looks at Pitwall in the background.
    pitwall::start(app, core.clone());
    match core.status() {
        s if s.available => log::info!(State = format!("{:?}", s.state).as_str(); "AI assistant ready to use"),
        s => log::info!("AI assistant unavailable: {}", s.reason.unwrap_or_default()),
    }
}

/// Manages `core` and registers its settings section (separate from [`init`] so tests can pass fakes).
pub fn manage<R: Runtime>(app: &AppHandle<R>, core: Arc<AiCore>) {
    app.manage(AiState(core.clone()));
    if let Some(jobs) = app.try_state::<crate::jobs::Jobs>() {
        let handle = app.clone();
        jobs.set_retry(crate::jobs::JobKind::Ai, move |job| requests::retry(&handle, job));
    }
    if let Err(e) = crate::settings::register(app, Arc::new(AiSettingsSection { core }), crate::settings::SectionGroup::Ai) {
        log::warn!("AI settings section not registered: {e:#}");
    }
}
