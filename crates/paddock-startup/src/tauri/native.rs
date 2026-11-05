//! The native question: on macOS and Linux a sheet or message dialog this crate shows itself (so it keeps the handle and can close it
//! when the page recovers, without touching another dialog), on Windows and elsewhere a message dialog through `tauri-plugin-dialog`
//! (rfd; on Windows found again by its class, owner and title). Custom button labels on every OS; the GTK and rfd Windows backends
//! show Yes/No/Cancel or Ok/Cancel, which map to the same choices in order.

#[cfg(target_os = "linux")]
mod gtk;
// Each OS uses part of it (the rest is pure, tested everywhere).
#[allow(dead_code)]
mod identity;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
mod win;

#[cfg(target_os = "linux")]
use gtk as own;
#[cfg(target_os = "macos")]
use mac as own;

use super::Asker;
use crate::{Choice, Prompt};
use ::tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_dialog::{
    DialogExt, MessageDialogBuilder, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};

/// Shows the watchdog's questions as native message dialogs. On Windows and other systems the app registers
/// `tauri_plugin_dialog::init()`; without it the question is logged and the window keeps waiting.
#[derive(Debug, Clone, Default)]
pub struct NativeAsker {
    /// The title of the question shown for each window (Windows finds the dialog by it).
    #[cfg(windows)]
    titles: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, (u64, String)>>>,
}

/// The buttons of `prompt` as a message dialog's buttons (the first is the default).
pub fn buttons(prompt: &Prompt) -> MessageDialogButtons {
    match prompt.buttons.as_slice() {
        [a, b, c] => MessageDialogButtons::YesNoCancelCustom((*a).into(), (*b).into(), (*c).into()),
        [a, b] => MessageDialogButtons::OkCancelCustom((*a).into(), (*b).into()),
        _ => MessageDialogButtons::Ok,
    }
}

/// The choice for a message dialog's result: a custom label by name, else the standard button in the prompt's order
/// (Yes/Ok: the first, No: the second of three, Cancel or closed: Keep Waiting).
pub fn choice_of(prompt: &Prompt, result: &MessageDialogResult) -> Choice {
    let nth = |i: usize| {
        prompt
            .buttons
            .get(i)
            .map_or(Choice::Wait, |l| prompt.choice(l))
    };
    match result {
        MessageDialogResult::Custom(label) => prompt.choice(label),
        MessageDialogResult::Yes | MessageDialogResult::Ok => nth(0),
        MessageDialogResult::No if prompt.buttons.len() == 3 => nth(1),
        _ => Choice::Wait,
    }
}

/// The message dialog for `prompt`, owned by window `label`; `None` when the app did not register the dialog plugin.
pub fn message<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    prompt: &Prompt,
) -> Option<MessageDialogBuilder<R>> {
    app.try_state::<tauri_plugin_dialog::Dialog<R>>()?;
    let dialog = app
        .dialog()
        .message(prompt.message.clone())
        .title(prompt.title.clone())
        .kind(MessageDialogKind::Warning)
        .buttons(buttons(prompt));
    Some(match app.get_webview_window(label) {
        Some(w) => dialog.parent(&w),
        None => dialog,
    })
}

impl NativeAsker {
    /// The message dialog through the dialog plugin (Windows and systems this crate has no dialog of its own for).
    #[cfg_attr(any(target_os = "macos", target_os = "linux"), allow(dead_code))]
    fn ask_with_plugin<R: Runtime>(
        app: &AppHandle<R>,
        label: &str,
        prompt: Prompt,
        answer: Box<dyn FnOnce(Choice) + Send>,
    ) {
        match message(app, label, &prompt) {
            Some(dialog) => {
                dialog.show_with_result(move |result| answer(choice_of(&prompt, &result)))
            }
            None => {
                log::warn!(Window = label; "tauri-plugin-dialog is not registered; window {label} keeps waiting");
                answer(Choice::Wait);
            }
        }
    }
}

impl<R: Runtime> Asker<R> for NativeAsker {
    fn ask(
        &self,
        app: &AppHandle<R>,
        label: &str,
        seq: u64,
        prompt: Prompt,
        answer: Box<dyn FnOnce(Choice) + Send>,
    ) {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        own::show(app, label, seq, &prompt, answer);
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            #[cfg(windows)]
            if let Ok(mut titles) = self.titles.lock() {
                titles.insert(label.to_string(), (seq, prompt.title.clone()));
            }
            let _ = seq;
            Self::ask_with_plugin(app, label, prompt, answer);
        }
    }

    fn dismiss(&self, app: &AppHandle<R>, label: &str, seq: u64) -> bool {
        #[cfg(target_os = "macos")]
        let found = own::dismiss(app, label, seq);
        #[cfg(target_os = "linux")]
        let found = {
            let _ = app;
            own::dismiss(label, seq)
        };
        #[cfg(windows)]
        let found = {
            let title = self.titles.lock().ok().and_then(|t| {
                t.get(label)
                    .filter(|(s, _)| *s == seq)
                    .map(|(_, t)| t.clone())
            });
            title.is_some_and(|title| win::dismiss(app, label, &title))
        };
        #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
        let found = {
            let _ = (app, seq);
            false
        };
        log::info!(Window = label, Seq = seq, Found = found; "Question {seq} dialog of window {label}: {}", if found { "asked to close" } else { "not found to close" });
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Question;
    use crate::ask::{RELOAD_LABEL, SAFE_MODE_LABEL, WAIT_LABEL};
    use MessageDialogResult as M;

    #[test]
    fn the_buttons_follow_the_prompt() {
        let three = Prompt::new(Question::Startup { safe_mode: true }, "App");
        assert!(
            matches!(buttons(&three), MessageDialogButtons::YesNoCancelCustom(a, b, c)
            if a == RELOAD_LABEL && b == SAFE_MODE_LABEL && c == WAIT_LABEL)
        );
        let two = Prompt::new(Question::Unresponsive, "App");
        assert!(
            matches!(buttons(&two), MessageDialogButtons::OkCancelCustom(a, b) if a == RELOAD_LABEL && b == WAIT_LABEL)
        );
        let none = Prompt {
            buttons: vec![],
            ..two
        };
        assert!(matches!(buttons(&none), MessageDialogButtons::Ok));
    }

    #[test]
    fn dialog_answers_map_to_choices_on_every_platform() {
        let three = Prompt::new(Question::Startup { safe_mode: true }, "App");
        assert_eq!(
            choice_of(&three, &M::Custom(RELOAD_LABEL.into())),
            Choice::Reload
        );
        assert_eq!(
            choice_of(&three, &M::Custom(SAFE_MODE_LABEL.into())),
            Choice::SafeMode
        );
        assert_eq!(
            choice_of(&three, &M::Custom(WAIT_LABEL.into())),
            Choice::Wait
        );
        assert_eq!(choice_of(&three, &M::Yes), Choice::Reload);
        assert_eq!(choice_of(&three, &M::No), Choice::SafeMode);
        assert_eq!(
            choice_of(&three, &M::Cancel),
            Choice::Wait,
            "closing the question waits"
        );
        let two = Prompt::new(Question::Unresponsive, "App");
        assert_eq!(choice_of(&two, &M::Ok), Choice::Reload);
        assert_eq!(choice_of(&two, &M::No), Choice::Wait);
        assert_eq!(choice_of(&two, &M::Cancel), Choice::Wait);
        let none = Prompt {
            buttons: vec![],
            ..two
        };
        assert_eq!(choice_of(&none, &M::Ok), Choice::Wait);
    }
}
