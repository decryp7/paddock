//! Engine version pins, rollback and the capability report (ADR 0018): the management operations over
//! `pitwall_core::runtimes::{pins, capabilities}`. The decisions are pure and live in the core; this module reads the
//! installed builds and the loaded models, persists the pins atomically and shapes the answers for the UI.

use crate::app::{lock, AppState};
use pitwall_core::events::{EventKind, Severity};
use pitwall_core::ids::ModelId;
use pitwall_core::mgmt::{
    BuildCapabilities, BuildCapability, CapabilityStatus, CommandError, EnginePinState, ErrorCode, ModelEnginePin, PinChange,
    RollbackTarget,
};
use pitwall_core::runtimes::capabilities::Gate;
use pitwall_core::runtimes::pins::{self, EnginePins, PinError, RollbackError};
use pitwall_core::runtimes::BuildId;
use std::sync::Arc;

type R<T> = Result<T, CommandError>;

fn pin_err(e: PinError) -> CommandError {
    let code = match e {
        PinError::NotInstalled { .. } => ErrorCode::NotFound,
        PinError::FailedSelfTest { .. } | PinError::TooMany => ErrorCode::Conflict,
        PinError::NoGoodBuild => ErrorCode::NotAvailable,
    };
    CommandError::new(code, e.to_string())
}

fn parse_build(raw: Option<String>) -> R<Option<BuildId>> {
    raw.map(|r| BuildId::parse(&r).map_err(|m| CommandError::new(ErrorCode::BadRequest, m)))
        .transpose()
}

/// The pin state for the Engines page and the Models rows.
pub fn state(st: &AppState) -> EnginePinState {
    let pins = st.llama.pins();
    let facts = st.llama.facts();
    let tag_of = |id: &BuildId| facts.iter().find(|i| i.id == *id).map(|i| i.tag.clone());
    let selection = st.llama.select_for(None);
    let (active, active_source) = match &selection {
        Ok(s) => (Some(s.build.to_string()), s.source.words()),
        Err(e) => (None, e.to_string()),
    };
    let (rollback_to, rollback_blocked) = match selection.as_ref().ok().map(|s| &s.build) {
        None => (None, Some("No engine build is in use yet.".to_string())),
        Some(current) => match pins::rollback_target(pins.history(), current, &facts) {
            Ok(b) => (
                tag_of(&b).map(|tag| RollbackTarget {
                    build_id: b.to_string(),
                    tag,
                }),
                None,
            ),
            Err(e) => (None, Some(e.to_string())),
        },
    };
    EnginePinState {
        global: pins.global().map(ToString::to_string),
        models: pins
            .models()
            .iter()
            .map(|(m, b)| ModelEnginePin {
                model_id: m.clone(),
                build_id: b.to_string(),
                tag: tag_of(b),
            })
            .collect(),
        active,
        active_source,
        rollback_to,
        rollback_blocked,
        problems: pins.problems(&facts).iter().map(ToString::to_string).collect(),
    }
}

/// Models in memory that a change would move to another build: they keep their engine until their next load.
fn on_next_load(st: &AppState, pins: &EnginePins) -> Vec<String> {
    let loaded: Vec<String> = st.serving.loaded().into_iter().map(|m| m.model_id).collect();
    let running = st.llama.running_on(&loaded);
    pins::moved(&running, pins, &st.llama.facts(), &st.llama.choice())
}

fn finish(st: &AppState, pins: EnginePins, headline: String) -> R<PinChange> {
    st.save_pins(&pins)
        .map_err(|e| CommandError::new(ErrorCode::Internal, format!("could not save the engine pins: {e}")))?;
    st.llama.set_pins(pins.clone());
    st.invalidate_engine();
    let later = on_next_load(st, &pins);
    let message = if later.is_empty() {
        headline
    } else {
        format!(
            "{headline} Loaded models keep their current engine until they are loaded again: {}.",
            later.join(", ")
        )
    };
    Ok(PinChange {
        state: state(st),
        applies_on_next_load: later,
        message,
    })
}

/// If the build that serves models in general is no longer `before`, remember `before` for rollback. Called after
/// anything that can change it without going through the pins: the build choice and installs.
pub fn record_change(st: &AppState, before: Option<BuildId>) {
    let Some(before) = before else { return };
    if current(st).as_ref() == Some(&before) {
        return;
    }
    st.invalidate_engine();
    let _gate = lock(&st.pins_gate);
    let mut pins = st.llama.pins();
    pins.remember(before);
    if st.save_pins(&pins).is_ok() {
        st.llama.set_pins(pins);
    }
}

/// The build that serves models in general now (for [`record_change`]).
pub fn current(st: &AppState) -> Option<BuildId> {
    st.llama.select_for(None).ok().map(|s| s.build)
}

/// Pins (or, with `None`, unpins) all models.
pub fn set_global(st: &Arc<AppState>, build: Option<String>) -> R<PinChange> {
    let build = parse_build(build)?;
    let _gate = lock(&st.pins_gate);
    let facts = st.llama.facts();
    let before = current(st);
    let mut pins = st.llama.pins();
    pins.set_global(build.clone(), &facts).map_err(pin_err)?;
    let after = pins::select(&pins, None, &facts, &st.llama.choice()).ok().map(|s| s.build);
    if let (Some(before), true) = (before, after.is_some()) {
        if after.as_ref() != Some(&before) {
            pins.remember(before);
        }
    }
    let headline = match &build {
        Some(b) => format!("{b} is pinned for all models."),
        None => "All models follow the automatic engine choice again.".into(),
    };
    finish(st, pins, headline)
}

/// Pins (or unpins) one model.
pub fn set_model(st: &Arc<AppState>, model: &ModelId, build: Option<String>) -> R<PinChange> {
    let build = parse_build(build)?;
    let _gate = lock(&st.pins_gate);
    let mut pins = st.llama.pins();
    pins.set_model(model.as_str(), build.clone(), &st.llama.facts()).map_err(pin_err)?;
    let headline = match &build {
        Some(b) => format!("{model} is pinned to {b}."),
        None => format!("{model} follows the global choice again."),
    };
    finish(st, pins, headline)
}

/// One-click rollback: to the most recent earlier build that is installed and passed its self-test, pinned for all
/// models until the user unpins.
pub fn rollback(st: &Arc<AppState>) -> R<PinChange> {
    let _gate = lock(&st.pins_gate);
    let facts = st.llama.facts();
    let mut pins = st.llama.pins();
    let current = st.llama.select_for(None).map_err(pin_err)?.build;
    let target = pins::rollback_target(pins.history(), &current, &facts).map_err(|e| {
        CommandError::new(
            match e {
                RollbackError::NotInstalled | RollbackError::NothingToRollBackTo | RollbackError::OnlyFailedBuilds(_) => {
                    ErrorCode::Conflict
                }
            },
            e.to_string(),
        )
    })?;
    pins.set_global(Some(target.clone()), &facts).map_err(pin_err)?;
    pins.remember(current);
    let tag = facts.iter().find(|i| i.id == target).map(|i| i.tag.clone()).unwrap_or_default();
    st.event(
        EventKind::EngineStarted,
        Severity::Info,
        format!("Rolled back the engine to {target} {tag}"),
    );
    finish(
        st,
        pins,
        format!("Rolled back to {target} {tag} and pinned it for all models; unpin it to follow the automatic choice again."),
    )
}

/// What each installed build supports.
pub fn capabilities(st: &AppState) -> Vec<BuildCapabilities> {
    let matrix = st.llama.matrix();
    st.llama
        .facts()
        .into_iter()
        .map(|i| {
            let build = pitwall_core::runtimes::capabilities::BuildFacts::new(i.id.as_str(), &i.tag);
            BuildCapabilities {
                build_id: i.id.to_string(),
                tag: i.tag.clone(),
                capabilities: matrix
                    .report(&build)
                    .into_iter()
                    .map(|(capability, label, gate)| match gate {
                        Gate::Allowed => BuildCapability {
                            capability,
                            label,
                            status: CapabilityStatus::Supported,
                            min_build: None,
                            hint: None,
                        },
                        Gate::NeedsBuild { min_build, hint, .. } => BuildCapability {
                            capability,
                            label,
                            status: CapabilityStatus::NeedsBuild,
                            min_build: Some(min_build),
                            hint: Some(hint),
                        },
                        Gate::NotSupported { reason, .. } => BuildCapability {
                            capability,
                            label,
                            status: CapabilityStatus::NotSupported,
                            min_build: None,
                            hint: Some(reason),
                        },
                    })
                    .collect(),
            }
        })
        .collect()
}
