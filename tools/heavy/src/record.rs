//! The on-disk records, identical to the shell `plock` (so shell and Rust holders and waiters interoperate):
//! - owner record, 6 lines: pid, process start text (informational), agent, epoch seconds written, cwd, masked command.
//!   Old versions wrote 3 lines (no epoch); the file's mtime stands in.
//! - ticket, 4 lines: pid, start text, agent, epoch.
//! - high marker, 3 lines: pid, start text, epoch (the pid is also in the file name).
//!
//! Parsing is total: any bytes give a value, never a panic (parse, don't validate).

/// Longest record read (bytes); a larger file is read up to this and the rest ignored.
pub const MAX_RECORD: usize = 64 * 1024;

/// A parsed owner record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    /// The holder's pid.
    pub pid: u32,
    /// The holder's start time as text (informational only).
    pub start_text: String,
    /// The agent name.
    pub name: String,
    /// Epoch seconds the record was written (the file mtime for an old 3-line record; None when that is unknown too).
    pub epoch: Option<u64>,
    /// The holder's working folder.
    pub cwd: String,
    /// The masked command line.
    pub cmd: String,
    /// The command heavy started (pid, OS start time in microseconds), written as a 7th line `child <pid> <micros>` once it
    /// runs. The shell plock reads six lines and ignores it. A lock whose heavy died but whose command still runs is not stale.
    pub child: Option<(u32, u64)>,
}

/// What an owner file said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerRead {
    /// A readable record.
    Ok(Owner),
    /// Missing or zero bytes: only the age of the lock dir may speak.
    Empty,
    /// Non-empty but not a record: never broken automatically.
    Unparsable,
}

fn lines(bytes: &[u8]) -> Vec<String> {
    let cut = &bytes[..bytes.len().min(MAX_RECORD)];
    String::from_utf8_lossy(cut)
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .collect()
}

fn digits(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Parses an owner record. `mtime` is the owner file's mtime in epoch seconds, used when the record has no epoch.
pub fn parse_owner(bytes: &[u8], mtime: Option<u64>) -> OwnerRead {
    if bytes.is_empty() {
        return OwnerRead::Empty;
    }
    let l = lines(bytes);
    let get = |i: usize| l.get(i).cloned().unwrap_or_default();
    let Some(pid) = digits(&get(0)).and_then(|p| u32::try_from(p).ok()) else {
        return OwnerRead::Unparsable;
    };
    OwnerRead::Ok(Owner {
        pid,
        start_text: get(1),
        name: get(2),
        epoch: digits(&get(3)).or(mtime),
        cwd: get(4),
        cmd: get(5),
        child: parse_child(&get(6)),
    })
}

fn parse_child(line: &str) -> Option<(u32, u64)> {
    let mut it = line.strip_prefix("child ")?.split(' ');
    let pid = u32::try_from(digits(it.next()?)?).ok()?;
    Some((pid, digits(it.next()?)?))
}

/// Formats an owner record (6 lines, each ending in a newline; a 7th `child` line when known). Fields are made single-line.
pub fn format_owner(o: &Owner, epoch: u64) -> String {
    let one = crate::timefmt::oneline;
    let child = o.child.map_or(String::new(), |(p, s)| format!("child {p} {s}\n"));
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{child}",
        o.pid,
        one(&o.start_text),
        one(&o.name),
        epoch,
        one(&o.cwd),
        one(&o.cmd)
    )
}

/// A parsed ticket (or the first lines of any pid-first record).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    /// The first line as written (may be empty or not a number).
    pub pid_text: String,
    /// The pid when the first line is a number that fits.
    pub pid: Option<u32>,
    /// The agent name (line 3).
    pub name: String,
    /// The epoch (line 4), when written.
    pub epoch: Option<u64>,
}

/// Parses a ticket.
pub fn parse_ticket(bytes: &[u8]) -> Ticket {
    let l = lines(bytes);
    let get = |i: usize| l.get(i).cloned().unwrap_or_default();
    let pid_text = get(0);
    Ticket {
        pid: digits(&pid_text).and_then(|p| u32::try_from(p).ok()),
        pid_text,
        name: get(2),
        epoch: digits(&get(3)),
    }
}

/// Formats a ticket.
pub fn format_ticket(pid: u32, start_text: &str, name: &str, epoch: u64) -> String {
    format!("{pid}\n{start_text}\n{}\n{epoch}\n", crate::timefmt::oneline(name))
}

/// The epoch of a high marker (line 3).
pub fn parse_marker_epoch(bytes: &[u8]) -> Option<u64> {
    digits(&lines(bytes).get(2).cloned().unwrap_or_default())
}

/// Formats a high marker.
pub fn format_marker(pid: u32, start_text: &str, epoch: u64) -> String {
    format!("{pid}\n{start_text}\n{epoch}\n")
}

/// The pid in a marker's file name `pitcrew-heavy.high.<pid>-<name>`: the digits before the first `-`, None if not a number.
pub fn marker_pid(suffix: &str) -> Option<u32> {
    digits(suffix.split('-').next().unwrap_or("")).and_then(|p| u32::try_from(p).ok())
}

/// A marker's name part: every byte outside `A-Za-z0-9._-` becomes `_` (as `tr -c` does, per byte).
pub fn sanitize_name(name: &str) -> String {
    name.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-') {
                b as char
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_line_roundtrip_and_old_three_line() {
        let o = Owner {
            pid: 42,
            start_text: "x".into(),
            name: "A\nB".into(),
            epoch: None,
            cwd: "/w".into(),
            cmd: "cargo test".into(),
            child: None,
        };
        let text = format_owner(&o, 1_700_000_000);
        assert_eq!(text.lines().count(), 6);
        let OwnerRead::Ok(back) = parse_owner(text.as_bytes(), Some(5)) else {
            panic!("not ok")
        };
        assert_eq!((back.pid, back.epoch, back.name.as_str()), (42, Some(1_700_000_000), "A B"));
        let OwnerRead::Ok(old) = parse_owner(b"7\nMon Jan  1\nghost\n", Some(99)) else {
            panic!("not ok")
        };
        assert_eq!((old.epoch, old.name.as_str()), (Some(99), "ghost"));
        let OwnerRead::Ok(unk) = parse_owner(b"7\n\ng\n", None) else {
            panic!("not ok")
        };
        assert_eq!(unk.epoch, None);
    }

    #[test]
    fn the_child_line_is_optional_and_total() {
        let mut o = Owner {
            pid: 1,
            start_text: "x".into(),
            name: "A".into(),
            epoch: None,
            cwd: "/".into(),
            cmd: "c".into(),
            child: Some((77, 123_456)),
        };
        let text = format_owner(&o, 10);
        assert_eq!(text.lines().count(), 7);
        let OwnerRead::Ok(back) = parse_owner(text.as_bytes(), None) else {
            panic!("not ok")
        };
        assert_eq!(back.child, Some((77, 123_456)));
        o.child = None;
        let OwnerRead::Ok(none) = parse_owner(format_owner(&o, 10).as_bytes(), None) else {
            panic!("not ok")
        };
        assert_eq!(none.child, None);
        for bad in ["child", "child x y", "child 1", "child 99999999999 1", "kid 1 2"] {
            let OwnerRead::Ok(r) = parse_owner(format!("1\nx\nA\n5\n/\nc\n{bad}\n").as_bytes(), None) else {
                panic!("not ok")
            };
            assert_eq!(r.child, None, "{bad}");
        }
    }

    #[test]
    fn empty_and_unparsable() {
        assert_eq!(parse_owner(b"", Some(1)), OwnerRead::Empty);
        assert_eq!(parse_owner(b"this is not a record\n", None), OwnerRead::Unparsable);
        assert_eq!(parse_owner(b"\n", None), OwnerRead::Unparsable);
        assert_eq!(parse_owner(b"99999999999999999999\nx\n", None), OwnerRead::Unparsable);
    }

    #[test]
    fn markers() {
        assert_eq!(marker_pid("123-agent"), Some(123));
        assert_eq!(marker_pid("99999999-x y z"), Some(99_999_999));
        assert_eq!(marker_pid("abc-x"), None);
        assert_eq!(sanitize_name("a b/é"), "a_b___");
        assert_eq!(parse_marker_epoch(b"1\nx\n55\n"), Some(55));
        assert_eq!(parse_marker_epoch(b"1\nx\n"), None);
    }
}
