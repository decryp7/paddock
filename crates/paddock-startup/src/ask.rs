//! The native question the watchdog asks when a window does not recover by itself (ADR 0058; rule 17: a real OS window
//! owned by the stuck window; rule 19: the repair never depends on the broken webview). The texts and buttons are pure;
//! the Tauri adapter shows them (`tauri::Asker`).

/// Which question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Question {
    /// The page did not finish starting, even after a reload. `safe_mode`: the app offers Restart in Safe Mode.
    Startup { safe_mode: bool },
    /// The page stopped answering after it had started.
    Unresponsive,
}

/// What the user chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Reload the window's page.
    Reload,
    /// Restart the app in Safe Mode (only offered when the app has a Safe Mode).
    SafeMode,
    /// Keep waiting: another full timeout (closing the question waits too).
    Wait,
    /// Save a problem report without the window (only offered when the app can: [`Prompt::with_report`]).
    SaveReport,
}

/// The Reload button.
pub const RELOAD_LABEL: &str = "Reload";
/// The Safe Mode button.
pub const SAFE_MODE_LABEL: &str = "Restart in Safe Mode";
/// The Keep Waiting button.
pub const WAIT_LABEL: &str = "Keep Waiting";
/// The Save a problem report button: the report is written without the stuck window (rule 19: the repair never depends on the
/// broken webview), to a file the user picks in a native dialog.
pub const SAVE_REPORT_LABEL: &str = "Save a problem report…";

/// A question ready to show: title, message and buttons (the first is the default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub question: Question,
    pub title: String,
    pub message: String,
    pub buttons: Vec<&'static str>,
}

impl Prompt {
    /// The prompt for `question` in an app named `app` (the product name).
    pub fn new(question: Question, app: &str) -> Prompt {
        match question {
            Question::Startup { safe_mode } => Prompt {
                question,
                title: format!("{app} is not responding"),
                message: format!(
                    "This {app} window did not finish starting, even after a reload. Another app may be pausing {app} \
                     (for example a CPU limiter), or the window's content keeps failing. Your open files are kept."
                ),
                buttons: if safe_mode {
                    vec![RELOAD_LABEL, SAFE_MODE_LABEL, WAIT_LABEL]
                } else {
                    vec![RELOAD_LABEL, WAIT_LABEL]
                },
            },
            Question::Unresponsive => Prompt {
                question,
                title: format!("{app} is not responding"),
                message: format!(
                    "This {app} window stopped responding. It may be busy, or another app may be pausing it (for example \
                     a CPU limiter). Reload it, or keep waiting. Your open files are kept."
                ),
                buttons: vec![RELOAD_LABEL, WAIT_LABEL],
            },
        }
    }

    /// The prompt with the Save a problem report button. A native message dialog holds three buttons, so when the question already
    /// has three (Reload, Safe Mode, Keep Waiting) Keep Waiting becomes closing the question, which waits as it always did.
    pub fn with_report(mut self) -> Prompt {
        if self.buttons.len() >= 3 {
            self.buttons.retain(|b| *b != WAIT_LABEL);
            self.buttons.push(SAVE_REPORT_LABEL);
            self.message
                .push_str(" Close this question to keep waiting.");
        } else {
            let before_last = self.buttons.len().saturating_sub(1);
            self.buttons.insert(before_last, SAVE_REPORT_LABEL);
        }
        self
    }

    /// The choice for the button labelled `label`; anything else (closing the question) waits. Safe Mode only counts
    /// when it was offered.
    pub fn choice(&self, label: &str) -> Choice {
        match label {
            RELOAD_LABEL => Choice::Reload,
            SAFE_MODE_LABEL if self.buttons.contains(&SAFE_MODE_LABEL) => Choice::SafeMode,
            SAVE_REPORT_LABEL if self.buttons.contains(&SAVE_REPORT_LABEL) => Choice::SaveReport,
            _ => Choice::Wait,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_startup_question_names_the_app_and_offers_safe_mode_only_when_the_app_has_one() {
        let p = Prompt::new(Question::Startup { safe_mode: true }, "Pitwall");
        assert_eq!(p.title, "Pitwall is not responding");
        assert!(
            p.message
                .starts_with("This Pitwall window did not finish starting")
        );
        assert_eq!(p.buttons, [RELOAD_LABEL, SAFE_MODE_LABEL, WAIT_LABEL]);
        assert_eq!(p.choice(SAFE_MODE_LABEL), Choice::SafeMode);
        let p = Prompt::new(Question::Startup { safe_mode: false }, "Pitwall");
        assert_eq!(p.buttons, [RELOAD_LABEL, WAIT_LABEL]);
        assert_eq!(
            p.choice(SAFE_MODE_LABEL),
            Choice::Wait,
            "not offered: never acted on"
        );
    }

    #[test]
    fn the_report_button_fits_three_buttons_and_is_never_acted_on_unless_offered() {
        let two = Prompt::new(Question::Unresponsive, "Dyno").with_report();
        assert_eq!(two.buttons, [RELOAD_LABEL, SAVE_REPORT_LABEL, WAIT_LABEL]);
        assert_eq!(two.choice(SAVE_REPORT_LABEL), Choice::SaveReport);
        let three = Prompt::new(Question::Startup { safe_mode: true }, "Dyno").with_report();
        assert_eq!(
            three.buttons,
            [RELOAD_LABEL, SAFE_MODE_LABEL, SAVE_REPORT_LABEL]
        );
        assert!(
            three
                .message
                .ends_with("Close this question to keep waiting.")
        );
        assert_eq!(three.choice(WAIT_LABEL), Choice::Wait);
        let without = Prompt::new(Question::Unresponsive, "Dyno");
        assert_eq!(
            without.choice(SAVE_REPORT_LABEL),
            Choice::Wait,
            "not offered: never acted on"
        );
    }

    #[test]
    fn the_unresponsive_question_offers_reload_and_keep_waiting() {
        let p = Prompt::new(Question::Unresponsive, "Debrief");
        assert!(p.message.contains("stopped responding"));
        assert_eq!(p.buttons, [RELOAD_LABEL, WAIT_LABEL]);
        assert_eq!(p.choice(RELOAD_LABEL), Choice::Reload);
        assert_eq!(p.choice(WAIT_LABEL), Choice::Wait);
        assert_eq!(p.choice(""), Choice::Wait, "closing the question waits");
    }
}
