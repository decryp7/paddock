//! Which dialog is ours to close. A native dialog may only be taken down when it is the one this crate showed for that question: the
//! window may have another sheet or dialog up (a save panel, Settings), and closing that one would lose the user's work (rule 53: act on
//! a reusable OS name only after checking what it names). The checks are pure so they are tested on every OS.

use std::cell::RefCell;
use std::collections::HashMap;

/// The dialogs this crate showed itself (macOS, Linux), by window label and question identity. Only the main thread touches it
/// (the dialogs are shown and closed there), so it lives in a thread local and holds non-`Send` handles.
pub struct Open<T>(RefCell<HashMap<String, (u64, T)>>);

impl<T> Default for Open<T> {
    fn default() -> Self {
        Open(RefCell::new(HashMap::new()))
    }
}

impl<T> Open<T> {
    /// Keeps the dialog of question `seq` about `label`.
    pub fn put(&self, label: &str, seq: u64, dialog: T) {
        self.0.borrow_mut().insert(label.to_string(), (seq, dialog));
    }

    /// Runs `f` on the dialog of question `seq` about `label`; `None` when it is another question's or gone. `f` closes the dialog,
    /// which answers it at once and so [`Open::forget`]s it again in the same call: the registry is not borrowed while `f` runs.
    pub fn with<R>(&self, label: &str, seq: u64, f: impl FnOnce(&T) -> R) -> Option<R>
    where
        T: Clone,
    {
        let dialog = match self.0.borrow().get(label) {
            Some((s, d)) if *s == seq => d.clone(),
            _ => return None,
        };
        Some(f(&dialog))
    }

    /// Forgets the dialog of question `seq` about `label` (it was answered or closed). Another question's stays.
    pub fn forget(&self, label: &str, seq: u64) {
        let mut map = self.0.borrow_mut();
        if map.get(label).is_some_and(|(s, _)| *s == seq) {
            map.remove(label);
        }
    }
}

/// macOS: the sheet may be ended only when it is the alert's own window and is the one attached to the parent now (an alert queued
/// behind a save panel is not attached yet, and the attached sheet is the user's).
pub fn is_own_attached_sheet<T>(attached: Option<*const T>, own: *const T) -> bool {
    attached == Some(own)
}

/// Windows: a top-level window is the question's dialog when it is a dialog box (`#32770`, which the task dialog and message box use),
/// visible, owned by the stuck window, and titled exactly as the prompt (Settings and save dialogs are owned too, but differ).
pub fn is_question_dialog(
    class: &str,
    visible: bool,
    owned_by_parent: bool,
    title: &str,
    expected: &str,
) -> bool {
    class == "#32770" && visible && owned_by_parent && title == expected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dialog_is_found_and_forgotten_by_its_own_question_only() {
        let open = Open::default();
        open.put("main", 3, "alert");
        assert_eq!(open.with("main", 3, |d| *d), Some("alert"));
        assert_eq!(open.with("main", 4, |d| *d), None, "another question");
        assert_eq!(open.with("other", 3, |d| *d), None, "another window");
        open.forget("main", 4);
        assert_eq!(
            open.with("main", 3, |d| *d),
            Some("alert"),
            "a stale forget keeps the newer dialog"
        );
        open.put("main", 5, "newer");
        open.forget("main", 3);
        assert_eq!(open.with("main", 5, |d| *d), Some("newer"));
        open.forget("main", 5);
        assert_eq!(open.with("main", 5, |d| *d), None);
    }

    #[test]
    fn the_registry_is_not_borrowed_while_the_dialog_is_closed() {
        // Closing a dialog answers it at once, and the answer forgets it again: on the same thread, inside the closing call.
        let open = Open::default();
        open.put("main", 3, "alert");
        let closed = open.with("main", 3, |d| {
            open.forget("main", 3);
            *d
        });
        assert_eq!(closed, Some("alert"));
        assert_eq!(open.with("main", 3, |d| *d), None, "forgotten meanwhile");
        open.forget("main", 3); // already gone: harmless
    }

    #[test]
    fn only_the_alerts_own_attached_sheet_is_ended() {
        let (alert, panel) = (1u8, 2u8);
        assert!(is_own_attached_sheet(Some(&alert as *const u8), &alert));
        assert!(
            !is_own_attached_sheet(Some(&panel as *const u8), &alert),
            "the user's save panel"
        );
        assert!(
            !is_own_attached_sheet(None, &alert),
            "queued: nothing attached yet"
        );
    }

    #[test]
    fn only_the_visible_owned_dialog_box_with_the_prompt_title_is_closed() {
        let t = "Dyno is not responding";
        assert!(is_question_dialog("#32770", true, true, t, t));
        assert!(!is_question_dialog("#32770", false, true, t, t), "hidden");
        assert!(
            !is_question_dialog("#32770", true, false, t, t),
            "owned by another window"
        );
        assert!(
            !is_question_dialog("#32770", true, true, "Save a problem report", t),
            "a save dialog"
        );
        assert!(
            !is_question_dialog("ApplicationFrameWindow", true, true, t, t),
            "Settings"
        );
    }
}
