//! Where native menu clicks go and whose menus the menu bar shows: the focused top-level window (a dialog or a pop-out
//! stands for its host, `paddock_commands::labels::resolve`). Pure.

use paddock_commands::labels::{Label, resolve};

/// The top-level window the menu bar belongs to: the one that had the focus last (a dialog's or pop-out's focus counts
/// for its host). Fed from window events by the adapter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Focus {
    target: Option<String>,
}

impl Focus {
    /// No window had the focus yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The window `label` got the focus. Webviews outside the label scheme and tray popovers (`tray`, `tray-<item>`:
    /// they belong to no window) do not move the menu bar.
    pub fn focused(&mut self, label: &str) {
        if hosted(label) {
            self.target = Some(resolve(label).to_string());
        }
    }

    /// The window `label` was destroyed: if it was the target, there is none until another window gets the focus.
    pub fn destroyed(&mut self, label: &str) {
        if self.target.as_deref() == Some(label) {
            self.target = None;
        }
    }

    /// The top-level window clicks go to.
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }
}

/// Whether `label` is a top-level window or works for one (a dialog, a pop-out).
fn hosted(label: &str) -> bool {
    matches!(
        Label::parse(resolve(label)),
        Some(Label::Main | Label::Window(_))
    )
}

/// Where a click on item `id` goes: the target window, if the id is one of the shown menu's items (other menus' items,
/// such as a tray's, arrive at the same handler and are not ours).
pub fn route(id: &str, shown: &[&str], target: Option<&str>) -> Option<String> {
    if !shown.contains(&id) {
        return None;
    }
    target.map(|label| resolve(label).to_string())
}

/// Whether a click on item `id` is the host's to run: it is one of the shown items and no window is the target (the last
/// window was closed to the tray and the menu bar still shows its model), so no window can take it. The host runs it
/// ([`crate::tauri::NativeMenu::on_no_window`]): Quit quits, any other item brings the main window back. Pure.
pub fn runs_without_window(id: &str, shown: &[&str], target: Option<&str>) -> bool {
    target.is_none() && shown.contains(&id)
}

/// Whether a model sent by window `caller` is shown: the menu bar shows the focused window's menus, so a model is applied
/// when its window has the focus, is the target, or no window is the target yet (the first window at start).
pub fn applies(caller: &str, caller_focused: bool, target: Option<&str>) -> bool {
    hosted(caller) && (caller_focused || target.is_none_or(|t| t == resolve(caller)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_go_to_the_focused_window_or_its_host() {
        let ids = ["pw.view.compact", "pd.about"];
        assert_eq!(
            route("pw.view.compact", &ids, Some("main")).as_deref(),
            Some("main")
        );
        assert_eq!(route("pd.about", &ids, Some("w-2")).as_deref(), Some("w-2"));
        // A pop-out shares its host's menu bar: the host runs the command.
        assert_eq!(
            route("pw.view.compact", &ids, Some("p-w-2-1")).as_deref(),
            Some("w-2")
        );
        // Not ours (a tray's items), or no window.
        assert_eq!(route("quit", &ids, Some("main")), None);
        assert_eq!(route("pw.view.compact", &ids, None), None);
    }

    #[test]
    fn a_shown_item_is_the_hosts_only_when_no_window_is_the_target() {
        let ids = ["pd.app.quit", "pd.about"];
        assert!(runs_without_window("pd.app.quit", &ids, None));
        assert!(runs_without_window("pd.about", &ids, None));
        assert!(!runs_without_window("tray.quit", &ids, None), "not shown");
        assert!(
            !runs_without_window("pd.app.quit", &ids, Some("main")),
            "a window is the target: it gets the click"
        );
    }

    #[test]
    fn the_target_follows_the_focus_of_top_level_windows() {
        let mut f = Focus::new();
        assert_eq!(f.target(), None);
        f.focused("main");
        assert_eq!(f.target(), Some("main"));
        f.focused("d-w-3-1");
        assert_eq!(
            f.target(),
            Some("w-3"),
            "a dialog's focus counts for its host"
        );
        f.focused("devtools");
        assert_eq!(
            f.target(),
            Some("w-3"),
            "a webview outside the scheme does not move the menu bar"
        );
        f.focused("tray");
        assert_eq!(f.target(), Some("w-3"), "nor does a tray popover");
        f.destroyed("main");
        assert_eq!(f.target(), Some("w-3"));
        f.destroyed("w-3");
        assert_eq!(f.target(), None);
    }

    #[test]
    fn a_model_is_shown_for_the_focused_window_or_the_target() {
        assert!(applies("main", false, None), "the first window at start");
        assert!(applies("main", true, Some("w-2")));
        assert!(applies("w-2", false, Some("w-2")));
        assert!(
            applies("p-w-2-1", false, Some("w-2")),
            "a pop-out speaks for its host"
        );
        assert!(!applies("main", false, Some("w-2")));
        assert!(
            !applies("devtools", true, None),
            "a webview outside the scheme never sets the menus"
        );
        assert!(!applies("tray-cpu", true, None), "nor a tray popover");
    }
}
