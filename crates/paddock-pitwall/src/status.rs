//! "Pitwall is optional" as a standard state (ADR 0004): [`AiState`] says what an app can do with Pitwall right now, and
//! [`NeedsAiView`] is what every app renders (the shared `<pd-needs-ai>` component) when a feature needs local AI: one
//! plain sentence and the actions that fix it. No app shows an error or a dead button for a missing Pitwall.

use crate::error::NextStep;
use crate::model::Caps;
use crate::service::{FoundKind, LinkView, PairingView};
use serde::Serialize;

/// What an app can do with Pitwall right now.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AiState {
    /// The user turned AI features off in this app.
    Disabled,
    /// No Pitwall on this computer: offer to set it up.
    NotInstalled,
    /// Installed but not running: offer to start it.
    NotRunning,
    /// Running, but another connection version.
    #[serde(rename_all = "camelCase")]
    Incompatible {
        /// Pitwall's reason, with whom to update.
        message: String,
        /// The app is the older side.
        update_app: bool,
    },
    /// Running; the app is not connected yet: offer the pairing code flow.
    NeedsPairing,
    /// Running but not serving (paused or stopped by the user).
    #[serde(rename_all = "camelCase")]
    Paused {
        /// Stopped rather than paused.
        stopped: bool,
    },
    /// Connected, but no installed model fits what the app needs.
    #[serde(rename_all = "camelCase")]
    NoSuitableModel {
        /// What the app asked for.
        needs: Caps,
    },
    /// Connected and something else is wrong; the text says what.
    #[serde(rename_all = "camelCase")]
    Unavailable {
        /// The reason, one sentence.
        message: String,
    },
    /// Ready to answer.
    #[serde(rename_all = "camelCase")]
    Ready {
        /// The model Pitwall picked.
        model: String,
        /// It accepts images.
        vision: bool,
        /// The context the app can plan prompts for.
        context_tokens: usize,
    },
}

impl AiState {
    /// The state a [`LinkView`] means. `disabled`: the app's own switch is off. `needs`: what the app asked of models.
    pub fn from_view(view: &LinkView, needs: Caps, disabled: bool) -> AiState {
        if disabled {
            return AiState::Disabled;
        }
        match view.found {
            FoundKind::Absent => AiState::NotInstalled,
            FoundKind::Installed => AiState::NotRunning,
            FoundKind::Incompatible => AiState::Incompatible {
                message: view
                    .problem
                    .clone()
                    .unwrap_or_else(|| "Pitwall speaks another connection version.".into()),
                update_app: view.problem.as_deref().is_some_and(|p| p.contains("newer")),
            },
            FoundKind::Running if !view.paired => AiState::NeedsPairing,
            FoundKind::Running if view.usable => AiState::Ready {
                model: view.model.clone().unwrap_or_default(),
                vision: view.vision,
                context_tokens: view.context_tokens,
            },
            FoundKind::Running => match (&view.paused, view.no_model) {
                (Some(p), _) => AiState::Paused {
                    stopped: p == "stopped",
                },
                (None, true) => AiState::NoSuitableModel { needs },
                _ => AiState::Unavailable {
                    message: view
                        .problem
                        .clone()
                        .unwrap_or_else(|| "Pitwall is not ready.".into()),
                },
            },
        }
    }

    /// The buttons that fix it, best first; empty when nothing needs fixing.
    pub fn actions(&self) -> Vec<NextStep> {
        match self {
            AiState::Disabled | AiState::Ready { .. } => vec![],
            AiState::NotInstalled => vec![NextStep::InstallPitwall],
            AiState::NotRunning => vec![NextStep::StartPitwall],
            AiState::Incompatible { update_app, .. } => vec![if *update_app {
                NextStep::UpdateApp
            } else {
                NextStep::UpdatePitwall
            }],
            AiState::NeedsPairing => vec![NextStep::Pair],
            AiState::Paused { .. } => vec![NextStep::StartPitwall],
            AiState::NoSuitableModel { .. } => vec![NextStep::InstallModel],
            AiState::Unavailable { .. } => vec![NextStep::Wait],
        }
    }

    /// Whether requests can go to Pitwall.
    pub fn is_ready(&self) -> bool {
        matches!(self, AiState::Ready { .. })
    }

    /// One plain sentence for the user, naming `app`.
    pub fn message(&self, app: &str) -> String {
        match self {
            AiState::Disabled => format!("AI features are turned off in {app}."),
            AiState::NotInstalled => format!(
                "{app} can use Pitwall, the local AI server, which is not installed on this computer."
            ),
            AiState::NotRunning => "Pitwall is installed but not running.".into(),
            AiState::Incompatible { message, .. } => message.clone(),
            AiState::NeedsPairing => format!("{app} is not connected to Pitwall yet."),
            AiState::Paused { stopped: true } => {
                "Pitwall is stopped. Start serving in Pitwall.".into()
            }
            AiState::Paused { stopped: false } => {
                "Pitwall is paused. Resume serving in Pitwall.".into()
            }
            AiState::NoSuitableModel { needs } => {
                let vision = if needs.vision { " with vision" } else { "" };
                format!("Pitwall has no model{vision} that suits {app}. Download one in Pitwall.")
            }
            AiState::Unavailable { message } => message.clone(),
            AiState::Ready { model, .. } => format!("Pitwall answers with {model}."),
        }
    }
}

/// What the shared `<pd-needs-ai>` component shows for a feature that needs local AI.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NeedsAiView {
    /// The feature, as the user knows it ("Ask about this log").
    pub feature: String,
    /// Why it is not available.
    pub reason: AiState,
    /// The sentence to show.
    pub message: String,
    /// The buttons, best first.
    pub actions: Vec<NextStep>,
}

impl NeedsAiView {
    /// The view for `feature` in `state`; `None` when the feature can run.
    pub fn for_state(feature: &str, state: &AiState, app: &str) -> Option<NeedsAiView> {
        if state.is_ready() {
            return None;
        }
        Some(NeedsAiView {
            feature: feature.into(),
            reason: state.clone(),
            message: state.message(app),
            actions: state.actions(),
        })
    }
}

/// Everything the shared `<pd-needs-ai>` component draws, in one value the app's `pitwall_state` command returns and its
/// `paddock://pitwall` event carries.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSnapshot {
    /// What the app can do with Pitwall now.
    pub state: AiState,
    /// The app's name.
    pub app: String,
    /// One plain sentence for the user (the engine's wording, naming `app`).
    pub message: String,
    /// The buttons that fix it, best first; empty when nothing needs fixing.
    pub actions: Vec<NextStep>,
    /// The pairing code while one waits for the user's approval in Pitwall.
    pub pairing: Option<PairingView>,
    /// The app has an engine of its own, so "Use the built-in engine" is a working repair (rule 19).
    pub built_in_available: bool,
    /// The user already switched this session to the app's own engine.
    pub built_in_for_session: bool,
}

/// The event every window hears when the [`AiSnapshot`] changed; the payload is an `AiSnapshot`.
pub const EVENT: &str = "paddock://pitwall";

/// The commands an app registers for the shared component (the names the UI's Tauri backend invokes): the snapshot, start
/// pairing, cancel the waiting pairing, use the app's own engine for this session.
pub const COMMANDS: [&str; 4] = [
    "pitwall_state",
    "pitwall_connect",
    "pitwall_cancel_pairing",
    "pitwall_use_builtin",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn view(found: FoundKind) -> LinkView {
        LinkView {
            found,
            ..LinkView::default()
        }
    }

    #[test]
    fn every_view_maps_to_one_state_with_its_fix() {
        let needs = Caps::chat(8192).with_vision();
        let cases: Vec<(LinkView, bool, AiState, Vec<NextStep>)> = vec![
            (view(FoundKind::Running), true, AiState::Disabled, vec![]),
            (
                view(FoundKind::Absent),
                false,
                AiState::NotInstalled,
                vec![NextStep::InstallPitwall],
            ),
            (
                view(FoundKind::Installed),
                false,
                AiState::NotRunning,
                vec![NextStep::StartPitwall],
            ),
            (
                view(FoundKind::Running),
                false,
                AiState::NeedsPairing,
                vec![NextStep::Pair],
            ),
            (
                LinkView {
                    paired: true,
                    paused: Some("paused".into()),
                    ..view(FoundKind::Running)
                },
                false,
                AiState::Paused { stopped: false },
                vec![NextStep::StartPitwall],
            ),
            (
                LinkView {
                    paired: true,
                    paused: Some("stopped".into()),
                    ..view(FoundKind::Running)
                },
                false,
                AiState::Paused { stopped: true },
                vec![NextStep::StartPitwall],
            ),
            (
                LinkView {
                    paired: true,
                    no_model: true,
                    ..view(FoundKind::Running)
                },
                false,
                AiState::NoSuitableModel { needs },
                vec![NextStep::InstallModel],
            ),
            (
                LinkView {
                    paired: true,
                    problem: Some("Odd.".into()),
                    ..view(FoundKind::Running)
                },
                false,
                AiState::Unavailable {
                    message: "Odd.".into(),
                },
                vec![NextStep::Wait],
            ),
            (
                LinkView {
                    paired: true,
                    usable: true,
                    model: Some("m".into()),
                    vision: true,
                    context_tokens: 8192,
                    ..view(FoundKind::Running)
                },
                false,
                AiState::Ready {
                    model: "m".into(),
                    vision: true,
                    context_tokens: 8192,
                },
                vec![],
            ),
            (
                LinkView {
                    problem: Some("Pitwall 3 speaks a newer connection version".into()),
                    ..view(FoundKind::Incompatible)
                },
                false,
                AiState::Incompatible {
                    message: "Pitwall 3 speaks a newer connection version".into(),
                    update_app: true,
                },
                vec![NextStep::UpdateApp],
            ),
            (
                LinkView {
                    problem: Some("Pitwall 0 speaks an older connection version".into()),
                    ..view(FoundKind::Incompatible)
                },
                false,
                AiState::Incompatible {
                    message: "Pitwall 0 speaks an older connection version".into(),
                    update_app: false,
                },
                vec![NextStep::UpdatePitwall],
            ),
        ];
        for (v, disabled, state, actions) in cases {
            let got = AiState::from_view(&v, needs, disabled);
            assert_eq!((&got, got.actions()), (&state, actions), "{v:?}");
            assert_eq!(got.is_ready(), matches!(state, AiState::Ready { .. }));
            assert!(!got.message("Debrief").is_empty());
        }
        assert!(
            AiState::from_view(&view(FoundKind::Incompatible), needs, false)
                .message("Debrief")
                .contains("connection version")
        );
        assert!(
            AiState::from_view(&view(FoundKind::Running), needs, true)
                .message("Debrief")
                .contains("Debrief")
        );
        let unknown = LinkView {
            paired: true,
            ..view(FoundKind::Running)
        };
        assert_eq!(
            AiState::from_view(&unknown, needs, false).message("Debrief"),
            "Pitwall is not ready."
        );
    }

    #[test]
    fn the_needs_ai_view_is_only_for_features_that_cannot_run() {
        let ready = AiState::Ready {
            model: "m".into(),
            vision: false,
            context_tokens: 4096,
        };
        assert!(NeedsAiView::for_state("Ask", &ready, "Debrief").is_none());
        let v = NeedsAiView::for_state("Ask about this log", &AiState::NeedsPairing, "Debrief")
            .unwrap();
        assert_eq!(
            (v.feature.as_str(), v.actions.clone()),
            ("Ask about this log", vec![NextStep::Pair])
        );
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["reason"]["kind"], "needsPairing");
        assert_eq!(json["actions"][0], "pair");
        assert_eq!(
            serde_json::to_value(AiState::NoSuitableModel {
                needs: Caps::chat(1024)
            })
            .unwrap()["needs"]["contextTokens"],
            1024
        );
        assert!(
            NeedsAiView::for_state(
                "x",
                &AiState::NoSuitableModel {
                    needs: Caps::default().with_vision()
                },
                "Debrief"
            )
            .unwrap()
            .message
            .contains("with vision")
        );
        assert!(
            AiState::Paused { stopped: true }
                .message("A")
                .contains("stopped")
        );
        assert!(
            AiState::Paused { stopped: false }
                .message("A")
                .contains("paused")
        );
        assert!(
            AiState::NotInstalled.message("A").contains("not installed")
                && AiState::NotRunning.message("A").contains("not running")
        );
        assert!(
            AiState::NeedsPairing.message("A").contains('A') && ready.message("A").contains("m")
        );
    }
}
