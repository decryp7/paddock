//! Linux: the question is a `GtkMessageDialog` this crate shows itself, transient for the window, with its handle kept so it can be
//! closed when the page recovers. `rfd` creates its dialog on its own thread with no parent and gives no handle, so it cannot be found
//! again. Shown and closed on the main thread (the GTK main loop's).

use super::identity::Open;
use crate::{Choice, Prompt};
use ::tauri::{AppHandle, Manager, Runtime};
use gtk::prelude::*;
use std::cell::RefCell;

thread_local! {
    static OPEN: Open<gtk::MessageDialog> = Open::default();
}

/// Shows `prompt` as question `seq` about window `label`; `answer` runs once, when a button is pressed or the dialog is closed.
pub fn show<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    seq: u64,
    prompt: &Prompt,
    answer: Box<dyn FnOnce(Choice) + Send>,
) {
    let Some(parent) = app
        .get_webview_window(label)
        .and_then(|w| w.gtk_window().ok())
    else {
        log::warn!(Window = label, Seq = seq; "Window {label} has no GTK window to ask through; it keeps waiting");
        answer(Choice::Wait);
        return;
    };
    let dialog = gtk::MessageDialog::new(
        Some(&parent),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        gtk::MessageType::Warning,
        gtk::ButtonsType::None,
        &prompt.message,
    );
    dialog.set_title(&prompt.title);
    for (i, button) in prompt.buttons.iter().enumerate() {
        dialog.add_button(button, gtk::ResponseType::Other(i as u16));
    }
    let (prompt, key, answer) = (
        prompt.clone(),
        label.to_string(),
        RefCell::new(Some(answer)),
    );
    dialog.connect_response(move |d, response| {
        // Closing the dialog below answers once more (delete event): only the first counts.
        let Some(answer) = answer.borrow_mut().take() else {
            return;
        };
        OPEN.with(|o| o.forget(&key, seq));
        let choice = match response {
            gtk::ResponseType::Other(i) => prompt
                .buttons
                .get(usize::from(i))
                .map_or(Choice::Wait, |l| prompt.choice(l)),
            _ => Choice::Wait,
        };
        answer(choice);
        d.close();
    });
    OPEN.with(|o| o.put(label, seq, dialog.clone()));
    dialog.show_all();
}

/// Closes question `seq` of window `label` if this crate's dialog for it is still up.
pub fn dismiss(label: &str, seq: u64) -> bool {
    OPEN.with(|o| o.with(label, seq, |d| d.response(gtk::ResponseType::DeleteEvent)))
        .is_some()
}
