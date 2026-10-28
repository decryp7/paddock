//! The Windows process table in one call: `NtQuerySystemInformation(SystemProcessInformation)`, which lists every process
//! with its working set, CPU times, I/O transfer counts and create time, including the ones `OpenProcess` refuses.
//!
//! Microsoft documents `NtQuerySystemInformation` and the class, but only the first fields of the record (`winternl.h`);
//! the rest of the layout is the one every task manager reads. So the fields are checked at start against the documented
//! per-process APIs for Dyno's own process ([`self_check`]); when they disagree the undocumented values are not used.
//!
//! Everything here is pure and works on bytes, so the parser and the arithmetic are tested on any OS. The call itself
//! lives in `windows::nt_query`.

use std::collections::HashMap;

/// The information class `SystemProcessInformation`.
pub const SYSTEM_PROCESS_INFORMATION: u32 = 5;
/// `STATUS_INFO_LENGTH_MISMATCH`: the buffer was too small.
pub const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
/// The first buffer.
pub const INITIAL_BUFFER: usize = 1 << 20;
/// The largest buffer ever allocated for one snapshot.
pub const MAX_BUFFER: usize = 64 << 20;
/// How many times the buffer may grow before giving up (the table can grow between two calls).
pub const MAX_GROWTHS: usize = 8;
/// Most processes read from one snapshot.
pub const MAX_PROCESSES: usize = 100_000;

/// Bytes of one record up to and including `OtherTransferCount`: the fixed part this module reads.
pub const MIN_ENTRY: usize = 256;

const OFF_NEXT: usize = 0;
const OFF_PRIVATE: usize = 8;
const OFF_CREATE: usize = 32;
const OFF_USER: usize = 40;
const OFF_KERNEL: usize = 48;
const OFF_PID: usize = 80;
const OFF_PARENT: usize = 88;
const OFF_SESSION: usize = 100;
const OFF_WORKING_SET: usize = 144;
const OFF_READ_BYTES: usize = 232;
const OFF_WRITE_BYTES: usize = 240;

/// Seconds between 1601-01-01 (Windows file times) and 1970-01-01.
const FILETIME_UNIX_DIFF_SECS: u64 = 11_644_473_600;

/// One process as the snapshot lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NtProcess {
    /// Process id.
    pub pid: u32,
    /// Parent process id.
    pub parent: u32,
    /// Terminal-services session; 0 is the system's.
    pub session: u32,
    /// Create time as a Windows file time (100 ns since 1601).
    pub create_time: u64,
    /// CPU time in user mode, 100 ns units.
    pub user_time: u64,
    /// CPU time in kernel mode, 100 ns units.
    pub kernel_time: u64,
    /// Working set in bytes.
    pub working_set: u64,
    /// Private bytes (the working set that is not shared).
    pub private_bytes: u64,
    /// Bytes read by I/O operations (all devices).
    pub read_bytes: u64,
    /// Bytes written by I/O operations.
    pub write_bytes: u64,
}

impl NtProcess {
    /// User plus kernel CPU time, 100 ns units.
    pub fn cpu_time(&self) -> u64 {
        self.user_time.saturating_add(self.kernel_time)
    }

    /// The create time in seconds since the Unix epoch; 0 when it is not a plausible time.
    pub fn start_unix_secs(&self) -> u64 {
        (self.create_time / 10_000_000).saturating_sub(FILETIME_UNIX_DIFF_SECS)
    }
}

/// Why a snapshot could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NtError {
    /// The buffer does not hold the records it describes.
    Truncated,
    /// A record's size or the chain of records is not valid.
    Malformed,
    /// The snapshot lists more processes than the limit.
    TooMany,
    /// The buffer needed to outgrow the cap or kept growing.
    TooLarge,
    /// The call failed with this status.
    Status(i32),
    /// This build reads 64-bit records only.
    Unsupported,
}

impl std::fmt::Display for NtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NtError::Truncated => f.write_str("the process list ended in the middle of a record"),
            NtError::Malformed => f.write_str("the process list is not in the expected layout"),
            NtError::TooMany => f.write_str("the process list is longer than Dyno reads"),
            NtError::TooLarge => f.write_str("the process list did not fit in a buffer of the allowed size"),
            NtError::Status(s) => write!(f, "the system answered with status {s:#010x}"),
            NtError::Unsupported => f.write_str("this build cannot read the process list layout"),
        }
    }
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off.checked_add(4)?)?.try_into().ok()?))
}

fn u64_at(b: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(off..off.checked_add(8)?)?.try_into().ok()?))
}

/// Parses a `SystemProcessInformation` buffer. Every offset is checked against the buffer; a record that does not fit, a
/// chain that does not move forward or one that runs past the end is an error and nothing is returned for it.
pub fn parse_processes(buf: &[u8]) -> Result<Vec<NtProcess>, NtError> {
    if usize::BITS != 64 {
        return Err(NtError::Unsupported);
    }
    let mut out = Vec::new();
    let mut at = 0usize;
    loop {
        let rec = buf.get(at..).filter(|r| r.len() >= MIN_ENTRY).ok_or(NtError::Truncated)?;
        let next = u32_at(rec, OFF_NEXT).ok_or(NtError::Truncated)? as usize;
        // Records of pids that do not fit 32 bits are not processes; skipped without failing the snapshot.
        let pid = u64_at(rec, OFF_PID).and_then(|p| u32::try_from(p).ok());
        let parent = u64_at(rec, OFF_PARENT).and_then(|p| u32::try_from(p).ok()).unwrap_or(0);
        if let Some(pid) = pid {
            if out.len() >= MAX_PROCESSES {
                return Err(NtError::TooMany);
            }
            out.push(NtProcess {
                pid,
                parent,
                session: u32_at(rec, OFF_SESSION).ok_or(NtError::Truncated)?,
                create_time: u64_at(rec, OFF_CREATE).ok_or(NtError::Truncated)?,
                user_time: u64_at(rec, OFF_USER).ok_or(NtError::Truncated)?,
                kernel_time: u64_at(rec, OFF_KERNEL).ok_or(NtError::Truncated)?,
                working_set: u64_at(rec, OFF_WORKING_SET).ok_or(NtError::Truncated)?,
                private_bytes: u64_at(rec, OFF_PRIVATE).ok_or(NtError::Truncated)?,
                read_bytes: u64_at(rec, OFF_READ_BYTES).ok_or(NtError::Truncated)?,
                write_bytes: u64_at(rec, OFF_WRITE_BYTES).ok_or(NtError::Truncated)?,
            });
        }
        if next == 0 {
            return Ok(out);
        }
        if next < MIN_ENTRY {
            return Err(NtError::Malformed);
        }
        at = at.checked_add(next).filter(|a| *a < buf.len()).ok_or(NtError::Truncated)?;
    }
}

/// What one call with a buffer answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryOutcome {
    /// Success: this many bytes of the buffer are filled.
    Done(usize),
    /// The buffer was too small; this many bytes are needed (0 when the system did not say).
    TooSmall(usize),
    /// The call failed with this status.
    Failed(i32),
}

/// Calls `query` with a buffer that grows until the answer fits: from [`INITIAL_BUFFER`], to what the system asked for plus
/// headroom for processes started meanwhile, at most [`MAX_GROWTHS`] times and never beyond `cap` bytes.
pub fn read_with_growth(mut query: impl FnMut(&mut [u8]) -> QueryOutcome, initial: usize, cap: usize) -> Result<Vec<u8>, NtError> {
    let mut size = initial.clamp(MIN_ENTRY, cap);
    for _ in 0..=MAX_GROWTHS {
        let mut buf = vec![0u8; size];
        match query(&mut buf) {
            QueryOutcome::Done(n) => {
                buf.truncate(n.min(size));
                return Ok(buf);
            }
            QueryOutcome::Failed(status) => return Err(NtError::Status(status)),
            QueryOutcome::TooSmall(needed) => {
                let wanted = needed.max(size.saturating_mul(2)).saturating_add(64 << 10);
                if size >= cap {
                    return Err(NtError::TooLarge);
                }
                size = wanted.min(cap);
            }
        }
    }
    Err(NtError::TooLarge)
}

/// Dyno's own process as the documented APIs report it (`GetProcessTimes`, `GetProcessMemoryInfo`,
/// `GetProcessIoCounters`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiCounters {
    /// Kernel plus user CPU time, 100 ns units.
    pub cpu_time: u64,
    /// Working set in bytes.
    pub working_set: u64,
    /// Bytes read.
    pub read_bytes: u64,
    /// Bytes written.
    pub write_bytes: u64,
}

/// Compares the snapshot's record of Dyno's own process with the documented APIs read right after it. The two are read
/// moments apart, so the tolerance allows what a running process changes in that time. `Err` names the first field that
/// disagrees.
pub fn self_check(nt: &NtProcess, api: &ApiCounters) -> Result<(), &'static str> {
    // CPU time only grows, and a few hundred milliseconds pass between the two calls.
    if nt.cpu_time() > api.cpu_time.saturating_add(CPU_SLACK) || api.cpu_time > nt.cpu_time().saturating_add(CPU_SLACK) {
        return Err("CPU time");
    }
    if !within(nt.working_set, api.working_set, MEMORY_SLACK) {
        return Err("working set");
    }
    // Counts only grow between the calls; a record that is ahead of the API is wrong.
    if nt.read_bytes > api.read_bytes.saturating_add(IO_SLACK) || api.read_bytes > nt.read_bytes.saturating_add(IO_SLACK) {
        return Err("bytes read");
    }
    if nt.write_bytes > api.write_bytes.saturating_add(IO_SLACK) || api.write_bytes > nt.write_bytes.saturating_add(IO_SLACK) {
        return Err("bytes written");
    }
    Ok(())
}

/// Two seconds of CPU time, in 100 ns units.
const CPU_SLACK: u64 = 20_000_000;
/// 64 MiB of working set.
const MEMORY_SLACK: u64 = 64 << 20;
/// 16 MiB of I/O.
const IO_SLACK: u64 = 16 << 20;

fn within(a: u64, b: u64, slack: u64) -> bool {
    a.abs_diff(b) <= slack.max(a.max(b) / 4)
}

/// What the tracker derived for one process.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NtReading {
    /// CPU in percent of the whole machine.
    pub cpu: f32,
    /// Working set in bytes.
    pub memory: u64,
    /// Bytes read per second.
    pub read_bps: f64,
    /// Bytes written per second.
    pub write_bps: f64,
    /// Start time, seconds since the Unix epoch.
    pub start_secs: u64,
    /// The terminal-services session.
    pub session: u32,
}

/// Whether the undocumented fields are used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NtState {
    /// No snapshot has been checked yet.
    Unchecked,
    /// The self-check passed.
    Active,
    /// The self-check failed or the snapshot cannot be read; the fields stay unavailable for the session.
    Disabled(String),
}

/// Turns consecutive snapshots into readings: CPU and I/O rates are differences between two snapshots of the same process
/// (same pid and create time, so a reused pid starts over).
#[derive(Debug)]
pub struct NtTracker {
    state: NtState,
    previous: HashMap<(u32, u64), NtProcess>,
    current: HashMap<u32, NtReading>,
    own_pid: u32,
    own_session: Option<u32>,
}

impl NtTracker {
    /// A tracker for the process `own_pid`.
    pub fn new(own_pid: u32) -> Self {
        NtTracker {
            state: NtState::Unchecked,
            previous: HashMap::new(),
            current: HashMap::new(),
            own_pid,
            own_session: None,
        }
    }

    /// The state.
    pub fn state(&self) -> &NtState {
        &self.state
    }

    /// Whether the readings may be used.
    pub fn usable(&self) -> bool {
        self.state == NtState::Active
    }

    /// Dyno's own session, once a snapshot listed Dyno.
    pub fn own_session(&self) -> Option<u32> {
        self.own_session
    }

    /// The reading for `pid`, when the tracker is usable and the snapshot listed it.
    pub fn reading(&self, pid: u32) -> Option<&NtReading> {
        if self.usable() {
            self.current.get(&pid)
        } else {
            None
        }
    }

    /// The session of `pid` from the latest snapshot, usable or not (the session number is a documented field).
    pub fn session_of(&self, pid: u32) -> Option<u32> {
        self.current.get(&pid).map(|r| r.session)
    }

    /// Takes the next snapshot. `secs` is the time since the previous one, `cores` the logical CPUs, `api` the documented
    /// counters of Dyno's own process read right after the snapshot (checked on the first one only). Returns the message
    /// to log when the snapshot disables the tracker, once.
    pub fn update(&mut self, snapshot: Result<Vec<NtProcess>, NtError>, secs: f64, cores: f32, api: Option<ApiCounters>) -> Option<String> {
        let list = match snapshot {
            Ok(l) => l,
            Err(e) => {
                return self.disable(format!(
                    "Dyno cannot read the system process list ({e}); other users' values stay unavailable."
                ))
            }
        };
        if self.state == NtState::Unchecked {
            let own = list.iter().find(|p| p.pid == self.own_pid);
            let verdict = match (own, api) {
                (Some(n), Some(a)) => self_check(n, &a).map_err(|field| format!("the {field} of Dyno's own process")),
                _ => Err("Dyno's own process".to_string()),
            };
            match verdict {
                Ok(()) => self.state = NtState::Active,
                Err(what) => {
                    return self.disable(format!(
                        "The system process list disagrees with the documented counters on {what}; its values stay unavailable."
                    ))
                }
            }
        }
        if !self.usable() {
            return None;
        }
        self.own_session = list.iter().find(|p| p.pid == self.own_pid).map(|p| p.session);
        let mut current = HashMap::with_capacity(list.len());
        let mut previous = HashMap::with_capacity(list.len());
        let cpu_scale = if secs > 0.0 && cores > 0.0 {
            100.0 / (secs * 1e7 * f64::from(cores))
        } else {
            0.0
        };
        for p in &list {
            let before = self.previous.get(&(p.pid, p.create_time));
            let (cpu, read_bps, write_bps) = match before {
                Some(b) if secs > 0.0 => (
                    (p.cpu_time().saturating_sub(b.cpu_time()) as f64 * cpu_scale).clamp(0.0, 100.0) as f32,
                    p.read_bytes.saturating_sub(b.read_bytes) as f64 / secs,
                    p.write_bytes.saturating_sub(b.write_bytes) as f64 / secs,
                ),
                _ => (0.0, 0.0, 0.0),
            };
            current.insert(
                p.pid,
                NtReading {
                    cpu,
                    memory: p.working_set,
                    read_bps,
                    write_bps,
                    start_secs: p.start_unix_secs(),
                    session: p.session,
                },
            );
            previous.insert((p.pid, p.create_time), *p);
        }
        self.current = current;
        self.previous = previous;
        None
    }

    fn disable(&mut self, message: String) -> Option<String> {
        let first = !matches!(self.state, NtState::Disabled(_));
        self.state = NtState::Disabled(message.clone());
        self.current.clear();
        self.previous.clear();
        first.then_some(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One invented record whose NextEntryOffset is 0 ([`table`] chains them).
    fn record(pid: u64, session: u32, user: u64, kernel: u64, ws: u64, read: u64, write: u64) -> Vec<u8> {
        let mut r = vec![0u8; MIN_ENTRY];
        let put32 = |r: &mut Vec<u8>, off: usize, v: u32| r[off..off + 4].copy_from_slice(&v.to_le_bytes());
        let put64 = |r: &mut Vec<u8>, off: usize, v: u64| r[off..off + 8].copy_from_slice(&v.to_le_bytes());
        put64(&mut r, OFF_PRIVATE, ws / 2);
        put64(&mut r, OFF_CREATE, (FILETIME_UNIX_DIFF_SECS + 1_000) * 10_000_000);
        put64(&mut r, OFF_USER, user);
        put64(&mut r, OFF_KERNEL, kernel);
        put64(&mut r, OFF_PID, pid);
        put64(&mut r, OFF_PARENT, 4);
        put32(&mut r, OFF_SESSION, session);
        put64(&mut r, OFF_WORKING_SET, ws);
        put64(&mut r, OFF_READ_BYTES, read);
        put64(&mut r, OFF_WRITE_BYTES, write);
        r
    }

    fn table(recs: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for (i, r) in recs.iter().enumerate() {
            let mut r = r.clone();
            let next = if i + 1 == recs.len() { 0 } else { r.len() as u32 };
            r[0..4].copy_from_slice(&next.to_le_bytes());
            out.extend(r);
        }
        out
    }

    #[test]
    fn a_table_of_records_is_parsed_field_by_field() {
        let buf = table(&[record(4, 0, 10, 20, 1 << 20, 100, 200), record(812, 2, 30, 40, 5 << 20, 7, 9)]);
        let p = parse_processes(&buf).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(
            p[1],
            NtProcess {
                pid: 812,
                parent: 4,
                session: 2,
                create_time: (FILETIME_UNIX_DIFF_SECS + 1_000) * 10_000_000,
                user_time: 30,
                kernel_time: 40,
                working_set: 5 << 20,
                private_bytes: (5 << 20) / 2,
                read_bytes: 7,
                write_bytes: 9,
            }
        );
        assert_eq!(p[1].cpu_time(), 70);
        assert_eq!(p[1].start_unix_secs(), 1_000);
        assert_eq!(
            NtProcess { create_time: 5, ..p[1] }.start_unix_secs(),
            0,
            "an implausible create time is no time"
        );
    }

    #[test]
    fn truncated_malformed_and_oversized_buffers_are_errors_not_panics() {
        let ok = table(&[record(4, 0, 0, 0, 0, 0, 0), record(8, 0, 0, 0, 0, 0, 0)]);
        assert_eq!(parse_processes(&[]), Err(NtError::Truncated));
        assert_eq!(parse_processes(&ok[..MIN_ENTRY - 1]), Err(NtError::Truncated));
        assert_eq!(
            parse_processes(&ok[..MIN_ENTRY + 10]),
            Err(NtError::Truncated),
            "the next record is cut off"
        );
        // NextEntryOffset smaller than a record, pointing back into itself.
        let mut small = ok.clone();
        small[0..4].copy_from_slice(&8u32.to_le_bytes());
        assert_eq!(parse_processes(&small), Err(NtError::Malformed));
        // NextEntryOffset far beyond the buffer.
        let mut huge = ok.clone();
        huge[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse_processes(&huge), Err(NtError::Truncated));
        // NextEntryOffset exactly at the end of the buffer: nothing follows.
        let mut end = record(4, 0, 0, 0, 0, 0, 0);
        end[0..4].copy_from_slice(&(MIN_ENTRY as u32).to_le_bytes());
        assert_eq!(parse_processes(&end), Err(NtError::Truncated));
    }

    #[test]
    fn records_with_a_pid_beyond_32_bits_are_skipped_and_the_count_is_capped() {
        let buf = table(&[record(u64::MAX, 0, 0, 0, 0, 0, 0), record(9, 0, 0, 0, 0, 0, 0)]);
        assert_eq!(parse_processes(&buf).unwrap().iter().map(|p| p.pid).collect::<Vec<_>>(), [9]);
        let one = record(1, 0, 0, 0, 0, 0, 0);
        let many = table(&vec![one; MAX_PROCESSES + 1]);
        assert_eq!(parse_processes(&many), Err(NtError::TooMany));
    }

    #[test]
    fn the_buffer_grows_to_what_the_system_asks_for_and_stops_at_the_cap() {
        let mut sizes = Vec::new();
        let out = read_with_growth(
            |b| {
                sizes.push(b.len());
                if b.len() < 5000 {
                    QueryOutcome::TooSmall(5000)
                } else {
                    b[0] = 7;
                    QueryOutcome::Done(3)
                }
            },
            1024,
            1 << 20,
        )
        .unwrap();
        assert_eq!(out, [7, 0, 0]);
        assert_eq!(sizes.len(), 2);
        assert!(sizes[1] >= 5000);
        assert_eq!(
            read_with_growth(|_| QueryOutcome::TooSmall(usize::MAX), 1024, 4096),
            Err(NtError::TooLarge),
            "the cap holds"
        );
        assert_eq!(
            read_with_growth(|_| QueryOutcome::TooSmall(0), 1024, usize::MAX),
            Err(NtError::TooLarge),
            "growth is bounded"
        );
        assert_eq!(read_with_growth(|_| QueryOutcome::Failed(-5), 1024, 4096), Err(NtError::Status(-5)));
        assert!(NtError::Status(-5).to_string().contains("status"));
        for e in [
            NtError::Truncated,
            NtError::Malformed,
            NtError::TooMany,
            NtError::TooLarge,
            NtError::Unsupported,
        ] {
            assert!(!e.to_string().is_empty());
        }
    }

    fn own(cpu: u64, ws: u64, read: u64, write: u64) -> (NtProcess, ApiCounters) {
        let nt = parse_processes(&table(&[record(100, 1, cpu, 0, ws, read, write)])).unwrap()[0];
        (
            nt,
            ApiCounters {
                cpu_time: cpu,
                working_set: ws,
                read_bytes: read,
                write_bytes: write,
            },
        )
    }

    #[test]
    fn the_self_check_accepts_matching_counters_and_names_the_field_that_disagrees() {
        let (nt, api) = own(50_000_000, 40 << 20, 1 << 20, 2 << 20);
        assert_eq!(self_check(&nt, &api), Ok(()));
        let moved = ApiCounters {
            cpu_time: api.cpu_time + 1_000_000,
            working_set: api.working_set + (1 << 20),
            read_bytes: api.read_bytes + 4096,
            ..api
        };
        assert_eq!(self_check(&nt, &moved), Ok(()), "what changes between the two calls is tolerated");
        assert_eq!(
            self_check(
                &nt,
                &ApiCounters {
                    cpu_time: api.cpu_time * 10,
                    ..api
                }
            ),
            Err("CPU time")
        );
        assert_eq!(self_check(&nt, &ApiCounters { cpu_time: 0, ..api }), Err("CPU time"));
        assert_eq!(
            self_check(
                &nt,
                &ApiCounters {
                    working_set: 1 << 30,
                    ..api
                }
            ),
            Err("working set")
        );
        assert_eq!(
            self_check(
                &nt,
                &ApiCounters {
                    read_bytes: 1 << 40,
                    ..api
                }
            ),
            Err("bytes read")
        );
        assert_eq!(
            self_check(
                &nt,
                &ApiCounters {
                    write_bytes: 1 << 40,
                    ..api
                }
            ),
            Err("bytes written")
        );
    }

    fn snap(cpu: u64, read: u64) -> Vec<NtProcess> {
        parse_processes(&table(&[
            record(100, 1, cpu, 0, 40 << 20, read, 0),
            record(200, 3, cpu * 2, 0, 9 << 20, read, 0),
        ]))
        .unwrap()
    }

    fn api() -> Option<ApiCounters> {
        Some(ApiCounters {
            cpu_time: 0,
            working_set: 40 << 20,
            read_bytes: 0,
            write_bytes: 0,
        })
    }

    #[test]
    fn readings_are_rates_between_two_snapshots_of_the_same_process() {
        let mut t = NtTracker::new(100);
        assert!(!t.usable() && t.reading(200).is_none());
        assert_eq!(t.update(Ok(snap(0, 0)), 0.0, 4.0, api()), None);
        assert!(t.usable());
        assert_eq!(t.own_session(), Some(1));
        assert_eq!(t.reading(200).unwrap().cpu, 0.0, "the first snapshot has nothing to compare with");
        // One second later pid 200 used 2 s of CPU on a 4-core machine: 50 percent; it read 1 MiB.
        t.update(Ok(snap(10_000_000, 1 << 20)), 1.0, 4.0, None);
        let r = *t.reading(200).unwrap();
        assert!((r.cpu - 50.0).abs() < 0.01, "{}", r.cpu);
        assert_eq!((r.memory, r.session, r.start_secs), (9 << 20, 3, 1_000));
        assert!((r.read_bps - f64::from(1 << 20)).abs() < 1.0);
        assert_eq!(r.write_bps, 0.0);
        assert_eq!(t.session_of(100), Some(1));
        assert_eq!(t.reading(999), None);
        // A reused pid (different create time) starts over instead of subtracting another process's counters.
        let mut reused = snap(10_000_000, 1 << 20);
        reused[1].create_time += 1;
        reused[1].user_time = 5;
        t.update(Ok(reused), 1.0, 4.0, None);
        assert_eq!(t.reading(200).unwrap().cpu, 0.0);
    }

    #[test]
    fn a_failing_self_check_or_snapshot_disables_the_fields_for_the_session_and_logs_once() {
        let mut t = NtTracker::new(100);
        let bad = Some(ApiCounters {
            working_set: 1 << 40,
            ..api().unwrap()
        });
        let msg = t.update(Ok(snap(0, 0)), 0.0, 4.0, bad).expect("logged");
        assert!(msg.contains("working set"), "{msg}");
        assert!(!t.usable() && t.reading(200).is_none());
        assert_eq!(t.session_of(200), None, "nothing is kept from an untrusted snapshot");
        assert_eq!(
            t.update(Ok(snap(0, 0)), 1.0, 4.0, api()),
            None,
            "disabled stays disabled and says nothing again"
        );
        assert!(matches!(t.state(), NtState::Disabled(_)));
        // Not listing Dyno at all, a missing API answer and a failed call are all disabling.
        for (snapshot, a) in [(Ok(Vec::new()), api()), (Ok(snap(0, 0)), None), (Err(NtError::Malformed), api())] {
            let mut t = NtTracker::new(100);
            assert!(t.update(snapshot, 0.0, 4.0, a).is_some());
            assert!(!t.usable());
        }
    }
}
