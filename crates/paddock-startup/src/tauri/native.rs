//! The native question through `tauri-plugin-dialog` (feature `dialog`): a warning message dialog owned by the stuck
//! window, with custom button labels (macOS and Windows; the GTK backend shows Yes/No/Cancel or Ok/Cancel, which map to
//! the same choices in order).

use super::Asker;
use crate::{Choice, Prompt};
use ::tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_dialog::{
    DialogExt, MessageDialogBuilder, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};

/// Shows the watchdog's questions as native message dialogs. The app registers `tauri_plugin_dialog::init()`; without it
/// the question is logged and the window keeps waiting.
#[derive(Debug, Clone, Copy, Default)]
pub struct NativeAsker;

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

impl<R: Runtime> Asker<R> for NativeAsker {
    fn ask(
        &self,
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
