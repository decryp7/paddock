//! Runs one benchmark child and watches it (ADR 0030 section 2).
//!
//! The child (`dyno-bench`) writes JSON lines on stdout: progress, a heartbeat from a separate thread once a second, and one
//! result. [`drive`] reads them through a [`ChildProcess`] port and turns them into progress for the job, a liveness
//! ("Running", "Still working", "Not responding") and, at the end, one [`BenchStatus`]. Nothing here ends a child by itself
//! because it is slow or quiet: only the person's Cancel and a named safety stop do (ER-19). The real child is
//! [`ProcessChild`]; the tests drive a scripted one with a manual clock.

use dyno_core::bench::unit::{self, UnitRecord};
use dyno_core::bench::wire::{liveness, parse_line, Line, Liveness, PhaseProgress, WireError, MAX_LINE, MAX_LINES};
use dyno_core::bench::{BenchKind, BenchStatus};
use paddock_bench::Clock;
use paddock_proc::Supervised;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How often the loop looks up from the pipe to check Cancel, the power source and the heartbeat.
pub const TICK: Duration = Duration::from_millis(250);
/// The last bytes of a child's stderr kept for the log and the failure text.
const STDERR_TAIL: usize = 4096;

/// What to start.
#[derive(Debug, Clone)]
pub struct ChildRequest {
    /// The run's id, for the unit name and the logs.
    pub run_id: String,
    /// What to measure.
    pub kind: BenchKind,
    /// The scratch folder (disk only).
    pub scratch: PathBuf,
    /// The scratch file size (disk only), MiB.
    pub disk_mib: u32,
}

/// Why the loop should end the child now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// The person pressed Cancel.
    Cancelled,
    /// A safety stop, in the words the person sees (the battery, the heat).
    Safety(String),
}

/// What the loop tells its owner and asks of it.
pub trait ChildEvents {
    /// The child reported where it is.
    fn progress(&self, p: &PhaseProgress);
    /// The run's liveness changed.
    fn liveness(&self, l: Liveness);
    /// Whether to end the child now. Asked every tick.
    fn stop(&self) -> Option<Stop>;
}

/// One thing that arrived from the child.
#[derive(Debug)]
pub enum Msg {
    /// A line, or why it was refused.
    Line(Result<Line, WireError>),
    /// Nothing arrived in the time asked for.
    Tick,
    /// The child's output ended (it exited or closed stdout).
    Eof,
}

/// A running child, as the loop sees it. A port so the tests need no process.
pub trait ChildProcess {
    /// The next message, waiting up to `wait`.
    fn next(&mut self, wait: Duration) -> Msg;
    /// Ends the child and everything it started, and reaps it. Idempotent.
    fn end(&mut self);
    /// How it ended, for the failure text and the log: the exit code and the tail of stderr.
    fn exit_note(&mut self) -> String;
}

/// Reads the child until it answers, ends or has to be stopped.
pub fn drive(child: &mut dyn ChildProcess, events: &dyn ChildEvents, clock: &dyn Clock, bound: Duration, kind: BenchKind) -> BenchStatus {
    let started = clock.now();
    let mut last_beat = started;
    let mut state = Liveness::Running;
    let mut lines = 0_usize;
    let mut result = None;
    loop {
        if let Some(stop) = events.stop() {
            child.end();
            let why = match stop {
                Stop::Cancelled => "Cancelled.".to_string(),
                Stop::Safety(text) => text,
            };
            log::info!("dyno-agent: benchmark {}: stopped ({why})", kind.name());
            return BenchStatus::Stopped { reason: why };
        }
        match child.next(TICK) {
            Msg::Line(Ok(line)) => {
                lines += 1;
                if lines > MAX_LINES {
                    child.end();
                    return BenchStatus::Failed {
                        reason: format!("The benchmark runner wrote more than {MAX_LINES} lines and was stopped."),
                    };
                }
                last_beat = clock.now();
                match line {
                    Line::Progress(p) => events.progress(&p),
                    Line::Heartbeat => {}
                    Line::Result { status } => {
                        result = Some(status);
                        break;
                    }
                }
            }
            Msg::Line(Err(e)) => {
                child.end();
                log::warn!("dyno-agent: benchmark {}: refused a line from the runner: {e}", kind.name());
                return BenchStatus::Failed {
                    reason: format!("The benchmark runner sent something Dyno could not read ({e})."),
                };
            }
            Msg::Tick => {}
            Msg::Eof => break,
        }
        let now = liveness(clock.now(), started, last_beat, bound);
        if now != state {
            log::warn!(
                "dyno-agent: benchmark {}: {state:?} -> {now:?} (no cancel until the person asks)",
                kind.name()
            );
            state = now;
            events.liveness(now);
        }
    }
    let note = {
        child.end();
        child.exit_note()
    };
    match result {
        Some(status) => {
            log::info!("dyno-agent: benchmark {}: the runner answered ({note})", kind.name());
            status
        }
        None => {
            log::warn!("dyno-agent: benchmark {}: the runner ended without an answer ({note})", kind.name());
            BenchStatus::Failed {
                reason: format!("The benchmark runner ended without an answer ({note})."),
            }
        }
    }
}

/// Keeps the last `STDERR_TAIL` bytes written to it.
#[derive(Default)]
struct Tail(Mutex<Vec<u8>>);

impl Tail {
    fn push(&self, bytes: &[u8]) {
        let mut t = self.0.lock().unwrap_or_else(|p| p.into_inner());
        t.extend_from_slice(bytes);
        if t.len() > STDERR_TAIL {
            let cut = t.len() - STDERR_TAIL;
            t.drain(..cut);
        }
    }

    fn text(&self) -> String {
        let t = self.0.lock().unwrap_or_else(|p| p.into_inner());
        String::from_utf8_lossy(&t).trim().replace('\n', " | ")
    }
}

/// Reads one line of at most [`MAX_LINE`] bytes. `Ok(None)` at the end of the stream; a longer line is an error.
fn read_line(r: &mut impl BufRead, buf: &mut Vec<u8>) -> std::io::Result<Option<Result<(), ()>>> {
    buf.clear();
    let n = r.by_ref().take(MAX_LINE as u64 + 1).read_until(b'\n', buf)?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() != Some(&b'\n') && buf.len() > MAX_LINE {
        return Ok(Some(Err(())));
    }
    Ok(Some(Ok(())))
}

/// The real child: a supervised process (its whole tree ends with it), read by a thread that parses the lines.
pub struct ProcessChild {
    sup: Supervised,
    rx: Receiver<Msg>,
    stderr: Arc<Tail>,
    /// Linux: the unit that holds the child, stopped with `systemctl --user stop` on `end`.
    unit: Option<Box<dyn UnitHandle>>,
    stdin: Option<std::process::ChildStdin>,
}

/// A transient unit that must be stopped when the child is ended (Linux).
pub trait UnitHandle: Send {
    /// Stops the unit and waits for it to be inactive.
    fn stop(&mut self);
}

impl ProcessChild {
    /// Starts `cmd`. The caller set the arguments; the pipes and the supervision are set here.
    ///
    /// // rule 52 exception: ADR 0030. The child is deliberately NOT lowered (no `lower_priority`): a benchmark's result is
    /// the speed of the machine, and a lowered thread measures the scheduler. Only after a click, never scheduled.
    pub fn spawn(mut cmd: std::process::Command, unit: Option<Box<dyn UnitHandle>>) -> std::io::Result<ProcessChild> {
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut sup = Supervised::spawn(cmd)?;
        let stdout = sup.take_stdout();
        let stderr_pipe = sup.take_stderr();
        let stdin = sup.take_stdin();
        let (tx, rx) = mpsc::channel();
        let tail = Arc::new(Tail::default());
        if let Some(out) = stdout {
            std::thread::Builder::new().name("dyno-bench-read".into()).spawn(move || {
                let mut reader = std::io::BufReader::new(out);
                let mut buf = Vec::with_capacity(512);
                let mut n = 0_usize;
                loop {
                    match read_line(&mut reader, &mut buf) {
                        Ok(None) | Err(_) => break,
                        Ok(Some(Err(()))) => {
                            n += 1;
                            let _ = tx.send(Msg::Line(Err(WireError {
                                line: n,
                                reason: format!("longer than {MAX_LINE} bytes"),
                            })));
                            break;
                        }
                        Ok(Some(Ok(()))) => {
                            n += 1;
                            let text = String::from_utf8_lossy(&buf).into_owned();
                            if text.trim().is_empty() {
                                continue;
                            }
                            if tx.send(Msg::Line(parse_line(&text, n))).is_err() {
                                return;
                            }
                        }
                    }
                }
                let _ = tx.send(Msg::Eof);
            })?;
        } else {
            let _ = tx.send(Msg::Eof);
        }
        if let Some(mut err) = stderr_pipe {
            let tail = tail.clone();
            std::thread::Builder::new().name("dyno-bench-stderr".into()).spawn(move || {
                let mut chunk = [0_u8; 1024];
                while let Ok(n) = err.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    tail.push(&chunk[..n]);
                }
            })?;
        }
        Ok(ProcessChild {
            sup,
            rx,
            stderr: tail,
            unit,
            stdin,
        })
    }

    /// The child's process id (for the log).
    pub fn pid(&self) -> Option<u32> {
        self.sup.id()
    }

    /// Writes `go` to the child, which waits for it before it measures (so the unit's record is on disk first).
    pub fn send_go(&mut self) -> std::io::Result<()> {
        use std::io::Write;
        match self.stdin.as_mut() {
            Some(s) => {
                s.write_all(b"go\n")?;
                s.flush()
            }
            None => Ok(()),
        }
    }
}

impl ChildProcess for ProcessChild {
    fn next(&mut self, wait: Duration) -> Msg {
        match self.rx.recv_timeout(wait) {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => Msg::Tick,
            Err(RecvTimeoutError::Disconnected) => Msg::Eof,
        }
    }

    fn end(&mut self) {
        if let Some(u) = self.unit.as_mut() {
            u.stop();
        }
        self.stdin = None;
        self.sup.kill();
    }

    fn exit_note(&mut self) -> String {
        let code = self.sup.wait().map_or("no exit code".to_string(), |s| {
            s.code()
                .map_or("it was stopped by a signal".to_string(), |c| format!("exit code {c}"))
        });
        let tail = self.stderr.text();
        if tail.is_empty() {
            code
        } else {
            format!("{code}; stderr: {tail}")
        }
    }
}

/// The unit records file next to the history.
pub fn units_file(history: &Path) -> PathBuf {
    history.with_file_name("bench-units.json")
}

/// Reads the records; damage means none.
pub fn read_records(file: &Path) -> Vec<UnitRecord> {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Writes the records (removes the file when there are none).
pub fn write_records(file: &Path, records: &[UnitRecord]) -> std::io::Result<()> {
    if records.is_empty() {
        return match std::fs::remove_file(file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let text = serde_json::to_string(records).map_err(std::io::Error::other)?;
    std::fs::write(file, text)
}

/// What the agent needs of the user manager. A port: the real one runs `systemctl`, the tests a fake.
pub trait UnitControl: Send + Sync {
    /// The benchmark units that exist, with their InvocationIDs.
    fn live(&self) -> Vec<unit::LiveUnit>;
    /// Stops the unit.
    fn stop(&self, unit: &str);
}

/// The restart cleanup: stops the leftover units that this agent recorded (name and InvocationID both match), leaves any other
/// alone, and forgets the records. Returns the units it stopped and the ones it left.
pub fn cleanup_leftovers(control: &dyn UnitControl, records_file: &Path) -> unit::Cleanup {
    let records = read_records(records_file);
    let decision = unit::cleanup(&records, &control.live());
    for u in &decision.stop {
        log::warn!("dyno-agent: a benchmark unit {u} was left behind by an earlier agent; stopping it");
        control.stop(u);
    }
    for u in &decision.left_alone {
        log::info!("dyno-agent: the unit {u} is not one this agent started (no matching invocation id); left alone");
    }
    if let Err(e) = write_records(records_file, &[]) {
        log::warn!("dyno-agent: could not clear the benchmark unit records: {e}");
    }
    decision
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::bench::wire::HEARTBEAT_EVERY;
    use dyno_core::bench::BenchMetric;
    use paddock_bench::ManualClock;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    /// A child that plays a script and advances the clock by what was waited.
    struct Scripted<'a> {
        clock: &'a ManualClock,
        script: VecDeque<(Duration, Msg)>,
        ended: bool,
    }

    impl ChildProcess for Scripted<'_> {
        fn next(&mut self, wait: Duration) -> Msg {
            match self.script.pop_front() {
                Some((after, msg)) => {
                    self.clock.advance(after);
                    msg
                }
                None => {
                    // Silence: nothing arrives, time passes.
                    self.clock.advance(wait);
                    Msg::Tick
                }
            }
        }

        fn end(&mut self) {
            self.ended = true;
        }

        fn exit_note(&mut self) -> String {
            "exit code 0".into()
        }
    }

    #[derive(Default)]
    struct Events {
        progress: RefCell<Vec<String>>,
        live: RefCell<Vec<Liveness>>,
        stop_after_ticks: RefCell<Option<(u32, Stop)>>,
    }

    impl ChildEvents for Events {
        fn progress(&self, p: &PhaseProgress) {
            self.progress.borrow_mut().push(format!("{} {}/{}", p.phase, p.rep, p.of));
        }

        fn liveness(&self, l: Liveness) {
            self.live.borrow_mut().push(l);
        }

        fn stop(&self) -> Option<Stop> {
            let mut s = self.stop_after_ticks.borrow_mut();
            match s.as_mut() {
                Some((0, stop)) => Some(stop.clone()),
                Some((n, _)) => {
                    *n -= 1;
                    None
                }
                None => None,
            }
        }
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn beat() -> Msg {
        Msg::Line(Ok(Line::Heartbeat))
    }

    fn progress(rep: u32) -> Msg {
        Msg::Line(Ok(Line::Progress(PhaseProgress {
            phase: "cpu-single".into(),
            label: "Single core".into(),
            rep,
            of: 5,
            count: u64::from(rep),
            total: None,
        })))
    }

    fn done() -> Msg {
        Msg::Line(Ok(Line::Result {
            status: BenchStatus::Done {
                metrics: vec![BenchMetric::higher("a", "A", 1.0, "u")],
            },
        }))
    }

    fn run(script: Vec<(Duration, Msg)>, events: &Events, bound: Duration) -> (BenchStatus, bool) {
        let clock = ManualClock::new();
        let mut child = Scripted {
            clock: &clock,
            script: script.into(),
            ended: false,
        };
        let status = drive(&mut child, events, &clock, bound, BenchKind::Cpu);
        (status, child.ended)
    }

    #[test]
    fn progress_is_relayed_and_the_result_is_the_answer() {
        let ev = Events::default();
        let (s, ended) = run(
            vec![(secs(1), beat()), (secs(1), progress(1)), (secs(1), progress(2)), (secs(1), done())],
            &ev,
            secs(60),
        );
        assert!(matches!(s, BenchStatus::Done { .. }));
        assert_eq!(*ev.progress.borrow(), vec!["cpu-single 1/5", "cpu-single 2/5"]);
        assert!(ev.live.borrow().is_empty(), "a child that keeps talking stays Running");
        assert!(ended, "the child is ended and reaped after its answer");
    }

    #[test]
    fn a_child_that_beats_but_sends_no_progress_shows_still_working_past_the_bound_and_is_not_ended() {
        let ev = Events::default();
        // 80 heartbeats, one a second, with a bound of 60 s.
        let script: Vec<_> = (0..80).map(|_| (HEARTBEAT_EVERY, beat())).chain([(secs(1), done())]).collect();
        let (s, _) = run(script, &ev, secs(60));
        assert!(matches!(s, BenchStatus::Done { .. }), "nothing ended it: {s:?}");
        assert_eq!(*ev.live.borrow(), vec![Liveness::StillWorking]);
    }

    #[test]
    fn a_child_that_stops_beating_shows_not_responding_and_is_not_ended_without_cancel() {
        let ev = Events::default();
        *ev.stop_after_ticks.borrow_mut() = None;
        // One heartbeat, then silence for good: 40 s of ticks, then the person gives up (the test's Cancel).
        let clock = ManualClock::new();
        let mut child = Scripted {
            clock: &clock,
            script: vec![(secs(1), beat())].into(),
            ended: false,
        };
        // Cancel after 200 ticks (50 s of silence at 250 ms).
        *ev.stop_after_ticks.borrow_mut() = Some((200, Stop::Cancelled));
        let s = drive(&mut child, &ev, &clock, secs(600), BenchKind::Cpu);
        assert_eq!(*ev.live.borrow(), vec![Liveness::NotResponding], "after 30 s of silence, once");
        assert!(matches!(s, BenchStatus::Stopped { reason } if reason == "Cancelled."));
        assert!(child.ended, "Cancel ends the child");
    }

    #[test]
    fn cancel_in_any_phase_ends_the_child_at_once() {
        for ticks in [0, 1, 2] {
            let ev = Events::default();
            *ev.stop_after_ticks.borrow_mut() = Some((ticks, Stop::Cancelled));
            let script = vec![
                (secs(1), progress(1)),
                (secs(1), progress(2)),
                (secs(1), progress(3)),
                (secs(1), done()),
            ];
            let (s, ended) = run(script, &ev, secs(60));
            assert!(matches!(s, BenchStatus::Stopped { .. }), "{ticks}: {s:?}");
            assert!(ended);
        }
    }

    #[test]
    fn a_safety_stop_names_itself() {
        let ev = Events::default();
        *ev.stop_after_ticks.borrow_mut() = Some((1, Stop::Safety(dyno_core::bench::wire::BATTERY_STOP_REASON.into())));
        let (s, ended) = run(vec![(secs(1), progress(1)), (secs(1), done())], &ev, secs(60));
        assert!(matches!(s, BenchStatus::Stopped { reason } if reason.starts_with("Stopped for safety")));
        assert!(ended);
    }

    #[test]
    fn a_crash_or_a_missing_answer_is_one_failed_kind_with_the_exit_note() {
        let ev = Events::default();
        let (s, ended) = run(vec![(secs(1), progress(1)), (secs(1), Msg::Eof)], &ev, secs(60));
        assert!(matches!(s, BenchStatus::Failed { reason } if reason.contains("without an answer") && reason.contains("exit code 0")));
        assert!(ended);
    }

    #[test]
    fn a_malformed_line_ends_the_kind_with_its_number_and_never_panics() {
        let ev = Events::default();
        let bad = Msg::Line(Err(WireError {
            line: 4,
            reason: "not a benchmark event".into(),
        }));
        let (s, ended) = run(vec![(secs(1), progress(1)), (secs(1), bad)], &ev, secs(60));
        assert!(matches!(s, BenchStatus::Failed { reason } if reason.contains("line 4")));
        assert!(ended);
    }

    #[test]
    fn too_many_lines_end_the_kind() {
        let ev = Events::default();
        let script: Vec<_> = (0..=MAX_LINES).map(|_| (Duration::ZERO, beat())).collect();
        let (s, ended) = run(script, &ev, secs(60));
        assert!(matches!(s, BenchStatus::Failed { reason } if reason.contains("more than")));
        assert!(ended);
    }

    #[test]
    fn lines_are_read_with_a_size_limit() {
        let mut r = std::io::Cursor::new(b"abc\nxyz".to_vec());
        let mut buf = Vec::new();
        assert!(matches!(read_line(&mut r, &mut buf), Ok(Some(Ok(())))));
        assert_eq!(buf, b"abc\n");
        assert!(
            matches!(read_line(&mut r, &mut buf), Ok(Some(Ok(())))),
            "a last line without a newline is still a line"
        );
        assert!(matches!(read_line(&mut r, &mut buf), Ok(None)));
        let long = vec![b'x'; MAX_LINE + 10];
        let mut r = std::io::Cursor::new(long);
        assert!(matches!(read_line(&mut r, &mut buf), Ok(Some(Err(())))));
    }

    #[test]
    fn the_stderr_tail_keeps_only_the_end() {
        let t = Tail::default();
        t.push(&vec![b'a'; STDERR_TAIL]);
        t.push(b"\nthe end");
        let text = t.text();
        assert!(text.ends_with("the end") && text.len() <= STDERR_TAIL + 3, "{}", text.len());
    }

    struct FakeUnits {
        live: Vec<unit::LiveUnit>,
        stopped: Mutex<Vec<String>>,
    }

    impl UnitControl for FakeUnits {
        fn live(&self) -> Vec<unit::LiveUnit> {
            self.live.clone()
        }

        fn stop(&self, unit: &str) {
            self.stopped.lock().unwrap().push(unit.to_string());
        }
    }

    #[test]
    fn a_restart_stops_only_the_recorded_unit_and_forgets_the_records() {
        let dir = tempfile::tempdir().unwrap();
        let file = units_file(&dir.path().join("benchmarks.json"));
        let mine = "0123456789abcdef0123456789abcdef";
        write_records(
            &file,
            &[UnitRecord {
                unit: "dyno-bench-1-cpu".into(),
                invocation_id: mine.into(),
            }],
        )
        .unwrap();
        assert!(file.exists());
        let control = FakeUnits {
            live: vec![
                unit::LiveUnit {
                    unit: "dyno-bench-1-cpu".into(),
                    invocation_id: Some(mine.into()),
                },
                unit::LiveUnit {
                    unit: "dyno-bench-2-cpu".into(),
                    invocation_id: Some("ffffffffffffffffffffffffffffffff".into()),
                },
            ],
            stopped: Mutex::new(Vec::new()),
        };
        let d = cleanup_leftovers(&control, &file);
        assert_eq!(d.stop, vec!["dyno-bench-1-cpu"]);
        assert_eq!(*control.stopped.lock().unwrap(), vec!["dyno-bench-1-cpu"]);
        assert!(!file.exists(), "the records are forgotten");
        // A damaged file is no records: nothing is stopped.
        std::fs::write(&file, "{broken").unwrap();
        let control = FakeUnits {
            live: control.live.clone(),
            stopped: Mutex::new(Vec::new()),
        };
        assert!(cleanup_leftovers(&control, &file).stop.is_empty());
    }

    #[test]
    fn the_rule_52_exception_has_one_marker_and_the_child_is_never_lowered() {
        // ADR 0030: only the benchmark runner is started at normal priority for bulk work, and the one place says so.
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/bench_child.rs")).unwrap();
        let code: String = src
            .lines()
            .take_while(|l| !l.contains("#[cfg(test)]"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(code.matches("rule 52 exception: ADR 0030").count(), 1);
        assert!(
            !code.contains(&format!(".{}(", "lower_priority")),
            "the benchmark child runs at normal priority"
        );
    }
}
