//! The choice between Pitwall and an app's own engine (Debrief ADR 0059): [`EngineChoice`] is the setting (Automatic, Pitwall,
//! Built-in), [`select`] is the pure decision, [`Session`] holds what a session knows, and [`run_or_fallback`] is the family
//! rule "Pitwall is always optional" in code: the Pitwall path or the app's own, never both for one request, and a plain
//! "needs local AI" view when neither may run.
//!
//! **Never silently mix.** One session uses one engine at a time. When Pitwall served this session and then goes away,
//! Automatic does not continue on the other engine by itself: [`Routed::Unavailable`] says so, and the app offers the
//! repair "Use the built-in engine" ([`Session::use_built_in_for_session`]), which does not depend on Pitwall (rule 19).
//! The app keeps its own engine trait; this module only decides.

use crate::service::Link;
use crate::status::{AiState, NeedsAiView};
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Which engine the user wants (Settings > AI > AI engine).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineChoice {
    /// Pitwall when it is usable, else the app's own engine.
    #[default]
    Auto,
    /// Pitwall only.
    Pitwall,
    /// The app's own engine only.
    BuiltIn,
}

impl EngineChoice {
    /// The setting value.
    pub fn as_str(self) -> &'static str {
        match self {
            EngineChoice::Auto => "auto",
            EngineChoice::Pitwall => "pitwall",
            EngineChoice::BuiltIn => "builtIn",
        }
    }

    /// Parses a setting value; anything unknown is Automatic (a damaged value never loses the other settings).
    pub fn parse(s: &str) -> EngineChoice {
        match s.trim().to_ascii_lowercase().as_str() {
            "pitwall" => EngineChoice::Pitwall,
            "builtin" | "built-in" | "built_in" => EngineChoice::BuiltIn,
            _ => EngineChoice::Auto,
        }
    }
}

impl<'de> Deserialize<'de> for EngineChoice {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match serde_json::Value::deserialize(d)? {
            serde_json::Value::String(s) => EngineChoice::parse(&s),
            _ => EngineChoice::Auto,
        })
    }
}

/// The engine kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineKind {
    /// The app's own engine.
    BuiltIn,
    /// Pitwall.
    Pitwall,
}

/// What the choice depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SelectInput {
    /// The user's setting.
    pub choice: EngineChoice,
    /// Pitwall is running, compatible, paired and has a model for the app.
    pub pitwall_usable: bool,
    /// Pitwall answered a request of this session.
    pub pitwall_served: bool,
    /// The user switched the session to the app's own engine (the repair "Use the built-in engine").
    pub built_in_for_session: bool,
}

/// The engine requests go to; `None`: the wanted engine (Pitwall) is not usable and nothing replaces it silently.
pub fn select(i: SelectInput) -> Option<EngineKind> {
    if i.built_in_for_session || i.choice == EngineChoice::BuiltIn {
        return Some(EngineKind::BuiltIn);
    }
    match (i.choice, i.pitwall_usable, i.pitwall_served) {
        (_, true, _) => Some(EngineKind::Pitwall),
        (EngineChoice::Pitwall, false, _) => None,
        // Automatic: Pitwall served this session and went away: never continue on the other engine silently.
        (_, false, true) => None,
        _ => Some(EngineKind::BuiltIn),
    }
}

/// The user's setting and the session switch of one app.
#[derive(Debug, Default)]
pub struct Session {
    choice: Mutex<EngineChoice>,
    built_in: AtomicBool,
    disabled: AtomicBool,
}

impl Session {
    /// A session with this setting.
    pub fn new(choice: EngineChoice) -> Session {
        Session {
            choice: Mutex::new(choice),
            ..Session::default()
        }
    }

    /// The setting.
    pub fn choice(&self) -> EngineChoice {
        *self.choice.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The user changed the setting; the session switch is lifted (a new choice is a new decision).
    pub fn set_choice(&self, c: EngineChoice) {
        *self.choice.lock().unwrap_or_else(|p| p.into_inner()) = c;
        self.built_in.store(false, Ordering::Relaxed);
    }

    /// The repair "Use the built-in engine": for this session only, the setting stays.
    pub fn use_built_in_for_session(&self) {
        self.built_in.store(true, Ordering::Relaxed);
    }

    /// Whether the session was switched to the app's own engine.
    pub fn built_in_for_session(&self) -> bool {
        self.built_in.load(Ordering::Relaxed)
    }

    /// The app's own switch for AI features (everything off).
    pub fn set_disabled(&self, disabled: bool) {
        self.disabled.store(disabled, Ordering::Relaxed);
    }

    /// The inputs of [`select`] now.
    pub fn input(&self, link: &Link) -> SelectInput {
        SelectInput {
            choice: self.choice(),
            pitwall_usable: link.view().usable,
            pitwall_served: link.served(),
            built_in_for_session: self.built_in_for_session(),
        }
    }

    /// The engine requests go to now.
    pub fn engine(&self, link: &Link) -> Option<EngineKind> {
        select(self.input(link))
    }

    /// What the app can do with Pitwall now, for a status line or [`NeedsAiView`].
    pub fn state(&self, link: &Link) -> AiState {
        AiState::from_view(
            &link.view(),
            link.needs(),
            self.disabled.load(Ordering::Relaxed),
        )
    }
}

/// How a routed call went.
#[derive(Debug)]
pub enum Routed<T> {
    /// It ran on this engine.
    Ran(EngineKind, T),
    /// Neither engine may run it; the view says why and what fixes it (rendered by the shared needs-AI component).
    Unavailable(NeedsAiView),
}

/// Runs `feature` on the engine the session selects: `pitwall` for Pitwall, `built_in` for the app's own engine. Exactly one
/// of them runs; when [`select`] finds none (Pitwall wanted or lost, no silent mixing) or AI is off, nothing runs and the
/// answer is the [`NeedsAiView`] for `feature`.
pub fn run_or_fallback<T>(
    session: &Session,
    link: &Link,
    feature: &str,
    app: &str,
    pitwall: impl FnOnce() -> T,
    built_in: impl FnOnce() -> T,
) -> Routed<T> {
    if session.disabled.load(Ordering::Relaxed) {
        let state = AiState::Disabled;
        return Routed::Unavailable(NeedsAiView {
            feature: feature.into(),
            message: state.message(app),
            actions: state.actions(),
            reason: state,
        });
    }
    match session.engine(link) {
        Some(EngineKind::Pitwall) => Routed::Ran(EngineKind::Pitwall, pitwall()),
        Some(EngineKind::BuiltIn) => Routed::Ran(EngineKind::BuiltIn, built_in()),
        None => {
            let state = session.state(link);
            Routed::Unavailable(NeedsAiView {
                feature: feature.into(),
                message: state.message(app),
                actions: state.actions(),
                reason: state,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_parses_leniently_and_round_trips() {
        for c in [
            EngineChoice::Auto,
            EngineChoice::Pitwall,
            EngineChoice::BuiltIn,
        ] {
            assert_eq!(EngineChoice::parse(c.as_str()), c);
            let json = serde_json::to_string(&c).unwrap();
            assert_eq!(serde_json::from_str::<EngineChoice>(&json).unwrap(), c);
        }
        assert_eq!(EngineChoice::parse("built-in"), EngineChoice::BuiltIn);
        assert_eq!(EngineChoice::parse(" BUILT_IN "), EngineChoice::BuiltIn);
        assert_eq!(EngineChoice::parse("cloud"), EngineChoice::Auto);
        assert_eq!(
            serde_json::from_str::<EngineChoice>("3").unwrap(),
            EngineChoice::Auto
        );
    }

    #[test]
    fn selection_never_mixes_engines_silently() {
        use EngineChoice::*;
        use EngineKind as K;
        let s = |choice, usable, served, session| {
            select(SelectInput {
                choice,
                pitwall_usable: usable,
                pitwall_served: served,
                built_in_for_session: session,
            })
        };
        assert_eq!(s(BuiltIn, true, true, false), Some(K::BuiltIn));
        assert_eq!(s(Pitwall, true, false, false), Some(K::Pitwall));
        assert_eq!(s(Pitwall, false, false, false), None);
        assert_eq!(s(Auto, true, false, false), Some(K::Pitwall));
        assert_eq!(s(Auto, false, false, false), Some(K::BuiltIn));
        assert_eq!(
            s(Auto, false, true, false),
            None,
            "Pitwall went away mid-session"
        );
        assert_eq!(s(Auto, true, true, false), Some(K::Pitwall), "it came back");
        assert_eq!(
            s(Pitwall, false, true, true),
            Some(K::BuiltIn),
            "the user chose the fallback"
        );
        assert_eq!(s(Auto, true, true, true), Some(K::BuiltIn));
    }

    #[test]
    fn the_session_holds_the_setting_and_the_switch() {
        let s = Session::new(EngineChoice::Pitwall);
        assert_eq!(
            (s.choice(), s.built_in_for_session()),
            (EngineChoice::Pitwall, false)
        );
        s.use_built_in_for_session();
        assert!(s.built_in_for_session());
        s.set_choice(EngineChoice::Auto);
        assert!(!s.built_in_for_session(), "a new choice lifts the switch");
        assert_eq!(Session::default().choice(), EngineChoice::Auto);
    }
}
