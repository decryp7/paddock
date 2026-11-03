//! macOS: keeping a menu-bar item's place when its status item is removed (ADR 0028). AppKit stores the position under
//! `NSStatusItem Preferred Position <autosaveName>` in the app's user defaults and is believed to delete it when the item is
//! removed (on quit and when the layout changes), so Dyno keeps a copy of those keys under its own names and puts them back
//! before the item is made again. A copy never overwrites a key AppKit still holds.

//! The only unsafe code is `setObject:forKey:`, whose value must be a property-list object: each value is one just read from the
//! same defaults, so it is.
#![allow(unsafe_code)]

use crate::logging;
use crate::tray_identity::{appkit_defaults_keys, restored_keys, saved_key};
use objc2_foundation::{NSString, NSUserDefaults};

/// Copies AppKit's keys of the item `autosave_name` to Dyno's own, when AppKit has them. Returns how many were copied.
pub fn save(autosave_name: &str) -> usize {
    let defaults = NSUserDefaults::standardUserDefaults();
    let mut copied = 0;
    for key in appkit_defaults_keys(autosave_name) {
        if let Some(value) = defaults.objectForKey(&NSString::from_str(&key)) {
            // Safety: the value was just read from the defaults, so it is a property-list object.
            unsafe { defaults.setObject_forKey(Some(&value), &NSString::from_str(&saved_key(&key))) };
            copied += 1;
        }
    }
    copied
}

/// Puts Dyno's copy of the position back when AppKit no longer holds it (never the visibility: see [`restored_keys`]). Returns how
/// many keys were restored.
pub fn restore(autosave_name: &str) -> usize {
    let defaults = NSUserDefaults::standardUserDefaults();
    let mut restored = 0;
    for key in restored_keys(autosave_name) {
        let (appkit, saved) = (NSString::from_str(&key), NSString::from_str(&saved_key(&key)));
        if defaults.objectForKey(&appkit).is_some() {
            continue;
        }
        if let Some(value) = defaults.objectForKey(&saved) {
            // Safety: the value is a property-list object read from the defaults.
            unsafe { defaults.setObject_forKey(Some(&value), &appkit) };
            restored += 1;
        }
    }
    if restored > 0 {
        logging::info(&format!("tray position of {autosave_name}: {restored} saved key(s) put back"));
    }
    restored
}
