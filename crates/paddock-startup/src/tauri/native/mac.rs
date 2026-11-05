//! macOS: the question is an `NSAlert` this crate shows itself as a sheet of the window, with the alert kept so that exactly its own
//! window can be ended when the page recovers. `rfd` keeps its alert to itself, and the window's attached sheet may be the user's save
//! panel instead. Shown and closed on the main thread.

use super::identity::{Open, is_own_attached_sheet};
use crate::{Choice, Prompt};
use ::tauri::{AppHandle, Manager, Runtime};
use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSModalResponse, NSWindow};
use objc2_foundation::NSString;
use std::sync::Mutex;

thread_local! {
    static OPEN: Open<Retained<NSAlert>> = Open::default();
}

/// The window's `NSWindow`, retained; `None` when it has gone.
fn parent_of<R: Runtime>(app: &AppHandle<R>, label: &str) -> Option<Retained<NSWindow>> {
    let ptr = app.get_webview_window(label)?.ns_window().ok()?;
    // SAFETY: Tauri hands out the window's own NSWindow, which is alive here; this runs on the main thread.
    unsafe { Retained::retain(ptr.cast::<NSWindow>()) }
}

/// Shows `prompt` as question `seq` about window `label`; `answer` runs once, when a button is pressed or the sheet is ended.
pub fn show<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    seq: u64,
    prompt: &Prompt,
    answer: Box<dyn FnOnce(Choice) + Send>,
) {
    let (Some(mtm), Some(parent)) = (MainThreadMarker::new(), parent_of(app, label)) else {
        log::warn!(Window = label, Seq = seq; "Window {label} has no window to ask through; it keeps waiting");
        answer(Choice::Wait);
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Warning);
    for button in &prompt.buttons {
        alert.addButtonWithTitle(&NSString::from_str(button));
    }
    alert.setMessageText(&NSString::from_str(&prompt.title));
    alert.setInformativeText(&NSString::from_str(&prompt.message));
    let (prompt, key, answer) = (prompt.clone(), label.to_string(), Mutex::new(Some(answer)));
    let done = RcBlock::new(move |response: NSModalResponse| {
        OPEN.with(|o| o.forget(&key, seq));
        let index = response
            .checked_sub(NSAlertFirstButtonReturn)
            .and_then(|i| usize::try_from(i).ok());
        let choice = index
            .and_then(|i| prompt.buttons.get(i))
            .map_or(Choice::Wait, |l| prompt.choice(l));
        if let Some(answer) = answer.lock().ok().and_then(|mut a| a.take()) {
            answer(choice);
        }
    });
    alert.beginSheetModalForWindow_completionHandler(&parent, Some(&done));
    OPEN.with(|o| o.put(label, seq, alert));
}

/// Ends question `seq`'s sheet of window `label` when it is this crate's alert and is the sheet attached to the window now. An alert
/// queued behind another sheet (a save panel) is not attached yet: the attached sheet is left alone and `false` is returned.
pub fn dismiss<R: Runtime>(app: &AppHandle<R>, label: &str, seq: u64) -> bool {
    let Some(parent) = parent_of(app, label) else {
        return false;
    };
    OPEN.with(|o| {
        o.with(label, seq, |alert| {
            let own = alert.window();
            let attached = parent.attachedSheet();
            let attached = attached.as_ref().map(Retained::as_ptr);
            let ours = is_own_attached_sheet(attached, Retained::as_ptr(&own));
            if ours {
                parent.endSheet(&own);
            }
            ours
        })
    })
    .unwrap_or(false)
}
