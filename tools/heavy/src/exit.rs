//! How a command ended, in words: its exit code, the signal that killed it (Unix), or the NTSTATUS (Windows).

/// Exit codes of heavy's own failures, chosen so they do not look like a usual command failure (1, 2): the wrapper rule is that
/// 64, 75 and 78 mean heavy itself refused or failed, 126/127 the command could not start, 128+n a signal; everything else is
/// the command's own code, passed through. The log and the failure line say which.
pub const USAGE: i32 = 64;
/// The lock could not be taken (nothing could be written; see the message).
pub const LOCK_NOT_TAKEN: i32 = 75;
/// Refused before anything ran (the home guard).
pub const REFUSED: i32 = 78;
/// No slot was taken within `--queue-max`: the command never ran.
pub const QUEUE_LIMIT: i32 = 79;
/// The command ran longer than `--run-max` and was ended.
pub const RUN_LIMIT: i32 = 80;
/// The command exists but cannot be run.
pub const NOT_RUNNABLE: i32 = 126;
/// The command was not found.
pub const NOT_FOUND: i32 = 127;
/// Stopped by SIGHUP.
pub const HUP: i32 = 129;
/// Stopped by SIGINT / Ctrl+C.
pub const INT: i32 = 130;
/// Stopped by SIGTERM.
pub const TERM: i32 = 143;

/// The status the process exits with for `code`. Windows takes the full 32 bits (NTSTATUS crashes); Unix keeps the low byte,
/// and a non-zero code whose low byte is 0 becomes 1: never a false success.
pub fn process_exit(code: i32, windows: bool) -> i32 {
    if windows {
        return code;
    }
    match code & 0xff {
        0 if code != 0 => 1,
        low => low,
    }
}

/// Which stop request wins: terminate, then hangup, then interrupt.
pub fn stop_code(int: bool, term: bool, hup: bool) -> Option<i32> {
    if term {
        Some(TERM)
    } else if hup {
        Some(HUP)
    } else if int {
        Some(INT)
    } else {
        None
    }
}

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
    fn exit_mapping_never_turns_a_failure_into_success() {
        assert_eq!(process_exit(3, false), 3);
        assert_eq!(process_exit(256, false), 1);
        assert_eq!(process_exit(0, false), 0);
        assert_eq!(process_exit(0xC000_0005u32 as i32, true), 0xC000_0005u32 as i32);
        assert_eq!(process_exit(256, true), 256);
        assert_eq!(process_exit(-1, false), 255);
        for c in [1, 2, 64, 75, 78, 126, 127, 129, 130, 143, 255, 256, 512, 1024, i32::MIN, i32::MAX] {
            assert_ne!(process_exit(c, false), 0, "{c}");
            assert_ne!(process_exit(c, true), 0, "{c}");
        }
    }

    #[test]
    fn stop_requests_map_to_128_plus_the_signal() {
        assert_eq!(stop_code(false, false, false), None);
        assert_eq!(stop_code(true, false, false), Some(130));
        assert_eq!(stop_code(false, false, true), Some(129));
        assert_eq!(stop_code(false, true, false), Some(143));
        assert_eq!(stop_code(true, true, true), Some(143));
    }

    #[test]
    fn heavys_own_codes_are_distinct_from_the_usual_command_failures() {
        for c in [USAGE, LOCK_NOT_TAKEN, REFUSED] {
            assert!(c > 2 && c < 126);
        }
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
