//! How a command ended, in words: its exit code, the signal that killed it (Unix), or the NTSTATUS (Windows).

/// (the code `heavy` exits with, a description). `code` is the exit code when the process exited by itself, `signal` the
/// terminating signal (Unix).
pub fn describe_exit(code: Option<i32>, signal: Option<i32>) -> (i32, String) {
    if let Some(s) = signal {
        let name = match s {
            1 => "SIGHUP",
            2 => "SIGINT",
            6 => "SIGABRT",
            9 => "SIGKILL",
            11 => "SIGSEGV",
            13 => "SIGPIPE",
            15 => "SIGTERM",
            _ => "signal",
        };
        return (128 + s, format!("killed by signal {s} ({name})"));
    }
    match code {
        Some(0) => (0, "exit code 0".into()),
        // Windows reports an NTSTATUS crash as a 32-bit code with the high bits set.
        Some(c) if (c as u32) >= 0xC000_0000 => {
            let u = c as u32;
            let name = match u {
                0xC000_0005 => " (access violation)",
                0xC000_013A => " (Ctrl+C)",
                0xC000_0135 => " (a DLL was not found)",
                0xC000_0409 => " (stack buffer overrun, fast fail)",
                0xC000_00FD => " (stack overflow)",
                _ => "",
            };
            (c, format!("NTSTATUS 0x{u:08X}{name}"))
        }
        Some(c) => (c, format!("exit code {c}")),
        None => (1, "ended without an exit code".into()),
    }
}

/// The last `max_lines` lines of `bytes`, at most `max_bytes` bytes, lossily decoded.
pub fn tail_text(bytes: &[u8], max_lines: usize, max_bytes: usize) -> String {
    let cut = &bytes[bytes.len().saturating_sub(max_bytes)..];
    let text = String::from_utf8_lossy(cut);
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(max_lines)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes() {
        assert_eq!(describe_exit(Some(0), None).0, 0);
        assert_eq!(describe_exit(Some(3), None), (3, "exit code 3".into()));
        assert_eq!(describe_exit(None, Some(9)), (137, "killed by signal 9 (SIGKILL)".into()));
        let (c, t) = describe_exit(Some(0xC000_0005u32 as i32), None);
        assert_eq!(c, 0xC000_0005u32 as i32);
        assert!(t.contains("0xC0000005") && t.contains("access violation"), "{t}");
    }

    #[test]
    fn tail_is_bounded() {
        let big: Vec<u8> = (0..1000).flat_map(|i| format!("line {i}\n").into_bytes()).collect();
        let t = tail_text(&big, 3, 1000);
        assert_eq!(t.lines().count(), 3);
        assert!(t.ends_with("line 999"));
        assert_eq!(tail_text(b"", 3, 10), "");
    }
}
