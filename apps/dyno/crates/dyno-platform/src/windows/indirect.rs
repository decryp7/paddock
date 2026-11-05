//! `SHLoadIndirectString`, the OS's documented call that turns `@{package?ms-resource://...}` and `@file.dll,-id` strings into text
//! (the pure side is `parse::win_indirect`). Each lookup runs on its own short thread that enters a multithreaded COM apartment
//! like `firewall_com` does (the package resource lookup may use COM), and is abandoned after [`LOOKUP_LIMIT`], so a package that
//! never answers cannot hold the firewall list. At most [`MAX_STUCK`] abandoned lookups may still run; past that no new one starts.

use crate::parse::win_indirect::{IndirectLoader, Loaded};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::UI::Shell::SHLoadIndirectString;

/// How long one string may take.
const LOOKUP_LIMIT: Duration = Duration::from_secs(2);
/// Lookups abandoned and still running at which no new lookup starts.
const MAX_STUCK: usize = 4;
/// The longest text taken (the rule field is clipped further down).
const BUF_CHARS: usize = 512;

/// Lookup threads that have started and not finished.
static RUNNING: AtomicUsize = AtomicUsize::new(0);

struct Running;

impl Running {
    fn start() -> Running {
        RUNNING.fetch_add(1, Ordering::SeqCst);
        Running
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.fetch_sub(1, Ordering::SeqCst);
    }
}

fn load_now(source: &str) -> Loaded {
    // SAFETY: paired with `CoUninitialize` below only when it succeeded; this thread was created for this lookup.
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let wide: Vec<u16> = source.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = [0u16; BUF_CHARS];
    // SAFETY: `wide` is NUL-terminated and outlives the call; `buf` is a writable buffer of the length given; the reserved pointer is
    // None as documented.
    let result = unsafe { SHLoadIndirectString(PCWSTR(wide.as_ptr()), &mut buf, None) };
    let out = match result {
        Ok(()) => match buf.iter().position(|&c| c == 0) {
            Some(len) => Loaded::Text(String::from_utf16_lossy(&buf[..len])),
            None => Loaded::Unresolvable("no terminator in the answer".into()),
        },
        Err(e) => Loaded::Unresolvable(format!("{:#010x}", e.code().0)),
    };
    if com {
        // SAFETY: balances the successful `CoInitializeEx` above on this thread.
        unsafe { CoUninitialize() };
    }
    out
}

/// The OS loader.
pub struct ShellIndirect;

impl IndirectLoader for ShellIndirect {
    fn load(&self, source: &str) -> Loaded {
        let stuck = RUNNING.load(Ordering::SeqCst);
        if stuck >= MAX_STUCK {
            log::warn!(Running = stuck; "Firewall name lookups are stuck; not starting another");
            return Loaded::TimedOut;
        }
        let (tx, rx) = mpsc::channel();
        let owned = source.to_string();
        let running = Running::start();
        let spawned = std::thread::Builder::new().name("firewall-indirect".into()).spawn(move || {
            let _running = running;
            // The receiver may be gone after a timeout; the answer is then not needed.
            tx.send(load_now(&owned)).ok();
        });
        if let Err(e) = spawned {
            log::warn!(Error = e.to_string().as_str(); "A firewall name lookup thread could not start");
            return Loaded::Unresolvable(format!("thread: {e}"));
        }
        match rx.recv_timeout(LOOKUP_LIMIT) {
            Ok(loaded) => loaded,
            Err(_) => {
                log::warn!(LimitMs = LOOKUP_LIMIT.as_millis() as u64; "A firewall rule name did not resolve in time; its fallback is shown");
                Loaded::TimedOut
            }
        }
    }
}
