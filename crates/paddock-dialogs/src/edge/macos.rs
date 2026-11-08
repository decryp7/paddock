//! The macOS edge of the dialog size state machine (ADR 0043 §3, D4): the user's resize is told by AppKit itself.
//! `NSWindowWillStartLiveResizeNotification` ("the user is about to resize the window", sent once per series, never for a
//! move or for our own `setContentSize:`) becomes [`crate::tauri::user_resize_started`]. Sizes are in points, which are
//! logical px, so the scale does not enter. The observer lives as long as the window and is removed when it is destroyed.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2_app_kit::{NSWindow, NSWindowWillStartLiveResizeNotification};
use objc2_foundation::{NSNotification, NSNotificationCenter};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;
use tauri::{AppHandle, Runtime, Window};

type Token = Retained<ProtocolObject<dyn NSObjectProtocol>>;

thread_local! {
    /// The observers of the dialog windows, by label: installed and removed on the main thread only.
    static OBSERVERS: RefCell<HashMap<String, Token>> = RefCell::new(HashMap::new());
}

/// Observes the live resize of `window` (on the main thread). Failure to find the native window is logged by the caller's
/// absence of any user-resize signal; the window then sizes itself by the state machine alone.
pub fn install<R: Runtime>(app: &AppHandle<R>, window: &Window<R>) {
    let w = window.clone();
    let _ = app.run_on_main_thread(move || {
        let Ok(ns) = w.ns_window() else {
            return;
        };
        let label = w.label().to_string();
        let target = w.clone();
        let block = RcBlock::new(move |_n: NonNull<NSNotification>| {
            crate::tauri::user_resize_started(&target);
        });
        // SAFETY: `ns` is the live NSWindow Tauri owns; this runs on the main thread, where AppKit objects may be used. The
        // observer token is kept until the window is destroyed (`remove`).
        let token = unsafe {
            let object = &*(ns as *const NSWindow as *const AnyObject);
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSWindowWillStartLiveResizeNotification),
                Some(object),
                None,
                &block,
            )
        };
        OBSERVERS.with(|o| o.borrow_mut().insert(label, token));
    });
}

/// Stops observing the window `label` (it was destroyed); main thread.
pub fn remove(label: &str) {
    if let Some(token) = OBSERVERS.with(|o| o.borrow_mut().remove(label)) {
        // SAFETY: the token came from `addObserverForName...` on this center.
        let object: &AnyObject = (*token).as_ref();
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(object) };
    }
}
