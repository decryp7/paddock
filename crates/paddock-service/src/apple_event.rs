//! What the open-application Apple Event says about this start (macOS), for [`crate::launch::launch_reason`]. Moved from
//! Dyno's `login.rs`; the Objective-C calls are the only unsafe code here, each a plain message send.
#![allow(unsafe_code)]

use crate::launch::{OpenEvent, is_loginwindow};

/// What the open-application Apple Event says about this start: `keyAELaunchedAsLogInItem` (`'lgit'`) - Apple's
/// `AERegistry.h` (Xcode SDK, read 2026-10-31): "application was launched as a login item and probably shouldn't open
/// up untitled documents" - as a parameter of the event or the value of its `keyAEPropData` (`'prdt'`); and
/// `keyAERestoreAppState` (`'rsto'`) with the value yes (`AppleEvents.h`), which loginwindow sends when it reopens
/// the apps that were running at logout ("Reopen windows"), together with whether the sender (`keySenderPIDAttr`,
/// `'spid'`) is the login window, judged by the sender pid's executable path read right here (rule 53: not a pid
/// remembered from earlier). Only valid while the event is being handled: call it from `applicationDidFinishLaunching`,
/// which is where Tauri runs its setup (`RuntimeRunEvent::Ready`). Any miss gives the default (all false): the window shows.
pub fn open_event() -> OpenEvent {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    const OAPP: u32 = u32::from_be_bytes(*b"oapp");
    const PROP_DATA: u32 = u32::from_be_bytes(*b"prdt");
    const LOGIN_ITEM: u32 = u32::from_be_bytes(*b"lgit");
    const RESTORE: u32 = u32::from_be_bytes(*b"rsto");
    const YES: u32 = u32::from_be_bytes(*b"yes ");
    const SENDER_PID: u32 = u32::from_be_bytes(*b"spid");
    // SAFETY: plain message sends on Foundation objects; the types of the replies match the documented
    // NSAppleEventManager / NSAppleEventDescriptor signatures (FourCharCode is u32, a pid is int32).
    unsafe {
        let manager: Option<objc2::rc::Retained<AnyObject>> =
            msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
        let Some(manager) = manager else {
            return OpenEvent::default();
        };
        let event: Option<objc2::rc::Retained<AnyObject>> = msg_send![&*manager, currentAppleEvent];
        let Some(event) = event else {
            return OpenEvent::default();
        };
        let id: u32 = msg_send![&*event, eventID];
        if id != OAPP {
            return OpenEvent::default();
        }
        let direct: Option<objc2::rc::Retained<AnyObject>> =
            msg_send![&*event, paramDescriptorForKeyword: LOGIN_ITEM];
        let props: Option<objc2::rc::Retained<AnyObject>> =
            msg_send![&*event, paramDescriptorForKeyword: PROP_DATA];
        let by_props = match props {
            Some(p) => {
                let code: u32 = msg_send![&*p, enumCodeValue];
                code == LOGIN_ITEM
            }
            None => false,
        };
        let restore: Option<objc2::rc::Retained<AnyObject>> =
            msg_send![&*event, paramDescriptorForKeyword: RESTORE];
        let restore = match restore {
            Some(r) => {
                let code: u32 = msg_send![&*r, enumCodeValue];
                code == YES
            }
            None => false,
        };
        let sender_is_loginwindow = restore && {
            let sender: Option<objc2::rc::Retained<AnyObject>> =
                msg_send![&*event, attributeDescriptorForKeyword: SENDER_PID];
            match sender {
                Some(d) => {
                    let pid: i32 = msg_send![&*d, int32Value];
                    pid_path(pid).is_some_and(|p| is_loginwindow(&p))
                }
                None => false,
            }
        };
        OpenEvent {
            login_item: direct.is_some() || by_props,
            restore,
            sender_is_loginwindow,
        }
    }
}

/// The executable path of process `pid` now (`proc_pidpath`), `None` when it is gone or unreadable.
fn pid_path(pid: i32) -> Option<String> {
    unsafe extern "C" {
        fn proc_pidpath(pid: i32, buffer: *mut std::ffi::c_void, size: u32) -> i32;
    }
    if pid <= 0 {
        return None;
    }
    let mut buf = vec![0u8; 4096];
    let size = u32::try_from(buf.len()).unwrap_or(4096);
    // SAFETY: `buf` is valid for `size` bytes; the call writes at most that many and returns the length written.
    let n = unsafe { proc_pidpath(pid, buf.as_mut_ptr().cast(), size) };
    let n = usize::try_from(n).ok().filter(|n| *n > 0)?;
    buf.truncate(n);
    String::from_utf8(buf).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn outside_an_event_handler_the_event_is_the_default_so_the_window_shows() {
        // Not inside applicationDidFinishLaunching: there is no current Apple Event (or it is not an open event).
        assert_eq!(super::open_event(), crate::launch::OpenEvent::default());
    }

    #[test]
    fn a_pid_that_does_not_exist_has_no_path() {
        assert_eq!(super::pid_path(0), None);
        assert_eq!(super::pid_path(-1), None);
        assert!(super::pid_path(i32::try_from(std::process::id()).unwrap()).is_some());
    }
}
