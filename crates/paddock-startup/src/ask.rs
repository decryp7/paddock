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
}

/// The Reload button.
pub const RELOAD_LABEL: &str = "Reload";
/// The Safe Mode button.
pub const SAFE_MODE_LABEL: &str = "Restart in Safe Mode";
/// The Keep Waiting button.
pub const WAIT_LABEL: &str = "Keep Waiting";

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

    /// The choice for the button labelled `label`; anything else (closing the question) waits. Safe Mode only counts
    /// when it was offered.
    pub fn choice(&self, label: &str) -> Choice {
        match label {
            RELOAD_LABEL => Choice::Reload,
            SAFE_MODE_LABEL if self.buttons.contains(&SAFE_MODE_LABEL) => Choice::SafeMode,
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
    fn the_unresponsive_question_offers_reload_and_keep_waiting() {
        let p = Prompt::new(Question::Unresponsive, "Debrief");
        assert!(p.message.contains("stopped responding"));
        assert_eq!(p.buttons, [RELOAD_LABEL, WAIT_LABEL]);
        assert_eq!(p.choice(RELOAD_LABEL), Choice::Reload);
        assert_eq!(p.choice(WAIT_LABEL), Choice::Wait);
        assert_eq!(p.choice(""), Choice::Wait, "closing the question waits");
    }
}
