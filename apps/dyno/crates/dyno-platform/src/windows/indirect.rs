//! `SHLoadIndirectString`, the OS's documented call that turns `@{package?ms-resource://...}` and `@file.dll,-id` strings into text
//! (the pure side is `parse::win_indirect`). Each lookup runs on its own short thread and is abandoned after [`LOOKUP_LIMIT`], so a
//! package that never answers cannot hold the firewall list; the thread ends by itself.

use crate::parse::win_indirect::IndirectLoader;
use std::sync::mpsc;
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::UI::Shell::SHLoadIndirectString;

/// How long one string may take.
const LOOKUP_LIMIT: Duration = Duration::from_secs(2);
/// The longest text taken (the rule field is clipped further down).
const BUF_CHARS: usize = 512;

fn load_now(source: &str) -> Option<String> {
    let wide: Vec<u16> = source.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = [0u16; BUF_CHARS];
    // SAFETY: `wide` is NUL-terminated and outlives the call; `buf` is a writable buffer of the length given; the reserved pointer is
    // None as documented.
    unsafe { SHLoadIndirectString(PCWSTR(wide.as_ptr()), &mut buf, None) }.ok()?;
    let len = buf.iter().position(|&c| c == 0)?;
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// The OS loader.
pub struct ShellIndirect;

impl IndirectLoader for ShellIndirect {
    fn load(&self, source: &str) -> Option<String> {
        let (tx, rx) = mpsc::channel();
        let owned = source.to_string();
        std::thread::Builder::new()
            .name("firewall-indirect".into())
            .spawn(move || {
                // The receiver may be gone after a timeout; the answer is then not needed.
                tx.send(load_now(&owned)).ok();
            })
            .ok()?;
        match rx.recv_timeout(LOOKUP_LIMIT) {
            Ok(text) => text,
            Err(_) => {
                log::warn!("A firewall rule name did not resolve in time; its fallback is shown");
                None
            }
        }
    }
}
