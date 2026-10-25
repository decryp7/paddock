//! Toast buttons and click-through: a notification about a watch carries Open and Mark as seen, and a click on the
//! toast itself opens the watch. Which buttons a system shows is up to it (`tauri-plugin-notification` 2.7):
//!
//! - **Windows:** both buttons; a click is only reported while the toast is on screen, not once it sits in the Action
//!   Center (the toast then still shows, only without a reaction).
//! - **macOS:** two or more buttons are folded into an "Options" menu, and a click on the toast opens the watch.
//! - **Linux:** buttons and clicks as far as the notification server reports them (GNOME and KDE do, minimal servers
//!   may not).
//!
//! Where the system reports nothing the toast is a plain message, as before; the watch is one click away in the tray.

use crate::state::Shared;
use crate::tray;
use spotter_engine::notifier::NotificationKind;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_notification::{Action, ActionPerformed, ActionType, NotificationExt};

/// The action type of a toast about one release: Open and Mark as seen.
pub const RELEASE_ACTIONS: &str = "release";
/// The action type of any other toast about a watch: Open.
pub const WATCH_ACTIONS: &str = "watch";
/// The extra payload key that carries the watch id.
pub const WATCH_KEY: &str = "watchId";

const OPEN: &str = "open";
const SEEN: &str = "seen";
/// The id the plugin reports for a click on the toast itself.
const TAP: &str = "tap";

/// What the user asked for on a toast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToastAction {
    /// Show the watch in the main window (a validated route).
    Open(String),
    /// Mark the watch's latest release as seen.
    MarkSeen(String),
}

/// The action type for a notification, or `None` for one that is not about a single watch.
pub fn action_type_for(kind: NotificationKind, watch_id: Option<&str>) -> Option<&'static str> {
    watch_id?;
    Some(match kind {
        NotificationKind::NewRelease => RELEASE_ACTIONS,
        NotificationKind::Digest | NotificationKind::NeedsYou => WATCH_ACTIONS,
    })
}

/// What a reported click means. `watch` comes from the toast's payload, which the OS hands back: it is checked like any
/// route before it is used.
pub fn interpret(action_id: &str, watch: Option<&str>) -> Option<ToastAction> {
    let watch = watch.filter(|w| !w.is_empty() && w.len() <= 100)?;
    let route = format!("/watches/{watch}");
    if !tray::valid_route(&route) {
        return None;
    }
    match action_id {
        OPEN | TAP => Some(ToastAction::Open(route)),
        SEEN => Some(ToastAction::MarkSeen(watch.to_string())),
        _ => None,
    }
}

fn action_types() -> Vec<ActionType> {
    let open = Action::builder(OPEN, "Open").build();
    let seen = Action::builder(SEEN, "Mark as seen").build();
    vec![
        ActionType::builder(RELEASE_ACTIONS).actions(vec![open.clone(), seen]).build(),
        ActionType::builder(WATCH_ACTIONS).actions(vec![open]).build(),
    ]
}

/// Registers the buttons and the handler for clicks. Called once at start-up, before the first toast: the plugin only
/// shows buttons (and reports clicks) for toasts shown after a handler exists.
pub fn register<R: Runtime>(app: &AppHandle<R>) {
    let n = app.notification();
    if let Err(e) = n.register_action_types(action_types()) {
        crate::applog::write(&format!("toast buttons are not available: {e}"));
        return;
    }
    let handle = app.clone();
    if let Err(e) = n.on_action(move |performed| on_action(&handle, performed)) {
        crate::applog::write(&format!("toast clicks are not reported: {e}"));
    }
}

fn watch_of(performed: &ActionPerformed) -> Option<String> {
    performed.notification()?.extra().get(WATCH_KEY)?.as_str().map(str::to_string)
}

fn on_action<R: Runtime>(app: &AppHandle<R>, performed: &ActionPerformed) {
    match interpret(performed.action_id(), watch_of(performed).as_deref()) {
        Some(ToastAction::Open(route)) => {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || tray::show_main(&handle, Some(&route)));
        }
        Some(ToastAction::MarkSeen(id)) => {
            if let Some(state) = app.try_state::<Shared>() {
                if let Err(e) = state.engine.acknowledge(&id, None) {
                    crate::applog::write(&format!("a toast could not mark a release as seen: {e}"));
                }
            }
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_toast_about_one_watch_has_buttons() {
        assert_eq!(action_type_for(NotificationKind::NewRelease, Some("w1")), Some(RELEASE_ACTIONS));
        assert_eq!(action_type_for(NotificationKind::NeedsYou, Some("w1")), Some(WATCH_ACTIONS));
        assert_eq!(action_type_for(NotificationKind::Digest, None), None);
        assert_eq!(action_type_for(NotificationKind::NewRelease, None), None);
    }

    #[test]
    fn a_click_or_the_open_button_opens_the_watch_and_the_other_button_marks_it_seen() {
        assert_eq!(interpret("tap", Some("ab-12")), Some(ToastAction::Open("/watches/ab-12".into())));
        assert_eq!(interpret("open", Some("ab-12")), Some(ToastAction::Open("/watches/ab-12".into())));
        assert_eq!(interpret("seen", Some("ab-12")), Some(ToastAction::MarkSeen("ab-12".into())));
    }

    #[test]
    fn unknown_actions_and_bad_payloads_do_nothing() {
        assert_eq!(interpret("dismiss", Some("ab")), None);
        assert_eq!(interpret("open", None), None);
        assert_eq!(interpret("open", Some("")), None);
        assert_eq!(
            interpret("open", Some("../x")),
            None,
            "the payload comes back from the OS and is checked like a route"
        );
        assert_eq!(interpret("seen", Some("a b")), None);
        assert_eq!(interpret("open", Some(&"a".repeat(101))), None);
    }

    #[test]
    fn both_action_types_are_defined() {
        let types = action_types();
        assert_eq!(types.len(), 2);
        assert_eq!(types[0].id(), RELEASE_ACTIONS);
        assert_eq!(types[0].actions().len(), 2);
        assert_eq!(types[1].actions().len(), 1);
    }
}
