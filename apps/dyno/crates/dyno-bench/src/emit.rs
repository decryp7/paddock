//! What the runner writes on stdout (ADR 0030 section 2): JSON lines, one event each, flushed at once. The heartbeat comes from
//! a thread of its own that shares nothing with the measuring threads but the locked output, so a long kernel step or a slow
//! disk never looks like a hang, and a process that is really stuck stops beating.

use dyno_core::bench::wire::{Line, PhaseProgress};
use std::io::Write;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Takes where the measurement is. The kernels report through this; the real one writes a line, the tests collect.
pub trait Progress: Sync {
    /// The measurement reached this phase.
    fn report(&self, p: PhaseProgress);
    /// One second of a sustained run: the work done in it.
    fn sample(&self, _secs: u32, _throughput: f64) {}
    /// A fact about how the measurement was made.
    fn note(&self, _text: &str) {}
}

/// Reports nothing (unit tests of the kernels).
pub struct NoProgress;

impl Progress for NoProgress {
    fn report(&self, _: PhaseProgress) {}
}

/// Writes events as lines to a stream; safe to share between the measuring thread and the heartbeat.
pub struct Emitter<W: Write + Send> {
    out: Mutex<W>,
}

impl<W: Write + Send> Emitter<W> {
    /// An emitter on `out`.
    pub fn new(out: W) -> Emitter<W> {
        Emitter { out: Mutex::new(out) }
    }

    /// Writes one event and flushes. An error (the agent is gone: a closed pipe) is returned so the caller can stop.
    pub fn emit(&self, line: &Line) -> std::io::Result<()> {
        let mut out = self.out.lock().unwrap_or_else(|p| p.into_inner());
        writeln!(out, "{}", line.to_json())?;
        out.flush()
    }
}

impl<W: Write + Send> Progress for Emitter<W> {
    fn sample(&self, secs: u32, throughput: f64) {
        let _ = self.emit(&Line::Sample { secs, throughput });
    }

    fn note(&self, text: &str) {
        let _ = self.emit(&Line::Note {
            text: text.chars().take(dyno_core::bench::wire::MAX_NOTE).collect(),
        });
    }

    fn report(&self, p: PhaseProgress) {
        // A closed pipe is noticed by the heartbeat, which ends the process.
        let _ = self.emit(&Line::Progress(p));
    }
}

/// A phase report: `rep` is the repetitions finished (0 while warming up and during the first pass), `count` the same number on
/// the scale `total = of`. The label says what is happening now.
pub fn phase(phase: &str, label: &str, rep: u32, of: u32) -> PhaseProgress {
    PhaseProgress {
        phase: phase.into(),
        label: label.into(),
        rep,
        of,
        count: u64::from(rep),
        total: Some(u64::from(of)),
    }
}

/// The phase of a kind, steady for the whole kind (its id never changes), on the scale of repetitions finished: `done` counts a
/// repetition when it has ended, not when it starts. The label says what happens now, including the pause between passes.
pub fn kind_phase(kind: &str, label: &str, done: u32, of: u32) -> PhaseProgress {
    phase(kind, label, done, of)
}

/// The words for the warm-up.
pub const WARMING_UP: &str = "Warming up";

/// The words for a section of pass `n` of `of`.
pub fn pass_label(n: u32, of: u32, section: &str) -> String {
    format!("Pass {n} of {of} · {section}")
}

/// The words for the end of pass `n`.
pub fn pass_done_label(n: u32, of: u32) -> String {
    format!("Pass {n} of {of} done")
}

/// The words for the pause after pass `done`, before the next.
pub fn gap_label(done: u32, of: u32) -> String {
    format!("Cooling down before pass {} of {of}", done + 1)
}

/// Calls `beat` now and then every `every` on a thread of its own, until it is dropped or `beat` returns false.
pub struct Heartbeat {
    stop: Arc<(Mutex<bool>, Condvar)>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Heartbeat {
    /// Starts the thread. `beat` returns false when it can no longer write (then the thread ends).
    pub fn start(every: Duration, mut beat: impl FnMut() -> bool + Send + 'static) -> Heartbeat {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let s = stop.clone();
        let handle = std::thread::Builder::new()
            .name("dyno-bench-heartbeat".into())
            .spawn(move || {
                let (flag, cv) = &*s;
                loop {
                    if !beat() {
                        return;
                    }
                    let guard = flag.lock().unwrap_or_else(|p| p.into_inner());
                    let (g, _) = cv
                        .wait_timeout_while(guard, every, |stop| !*stop)
                        .unwrap_or_else(|p| p.into_inner());
                    if *g {
                        return;
                    }
                }
            })
            .ok();
        Heartbeat { stop, handle }
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        let (flag, cv) = &*self.stop;
        *flag.lock().unwrap_or_else(|p| p.into_inner()) = true;
        cv.notify_all();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::bench::wire::parse_line;
    use dyno_core::bench::BenchStatus;
    use std::sync::mpsc;

    /// A writer that hands every flushed chunk to a channel.
    struct ToChannel(mpsc::Sender<String>, Vec<u8>);

    impl Write for ToChannel {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.1.extend_from_slice(b);
            Ok(b.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            let _ = self.0.send(String::from_utf8_lossy(&std::mem::take(&mut self.1)).into_owned());
            Ok(())
        }
    }

    struct Broken;

    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn every_event_is_one_flushed_line_that_the_agent_can_parse() {
        let (tx, rx) = mpsc::channel();
        let e = Emitter::new(ToChannel(tx, Vec::new()));
        e.report(phase("cpu-single", "Single core", 2, 5));
        e.emit(&Line::Heartbeat).unwrap();
        e.emit(&Line::Result {
            status: BenchStatus::Unavailable { reason: "x".into() },
        })
        .unwrap();
        let lines: Vec<String> = rx.try_iter().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|l| l.ends_with('\n') && l.matches('\n').count() == 1));
        let parsed: Vec<Line> = lines.iter().enumerate().map(|(i, l)| parse_line(l, i + 1).unwrap()).collect();
        assert!(matches!(&parsed[0], Line::Progress(p) if p.phase == "cpu-single" && p.rep == 2 && p.of == 5 && p.total == Some(5)));
        assert_eq!(parsed[1], Line::Heartbeat);
        assert!(matches!(parsed[2], Line::Result { .. }));
    }

    #[test]
    fn a_closed_pipe_is_an_error_to_the_heartbeat_and_silent_for_progress() {
        let e = Emitter::new(Broken);
        assert!(e.emit(&Line::Heartbeat).is_err());
        e.report(phase("p", "P", 1, 1));
    }

    #[test]
    fn the_heartbeat_beats_on_its_own_until_it_is_dropped_and_stops_when_the_pipe_does() {
        let (tx, rx) = mpsc::channel();
        let hb = Heartbeat::start(Duration::from_millis(5), move || tx.send(()).is_ok());
        // Three beats arrive with nobody else doing anything: it needs no measuring thread.
        for _ in 0..3 {
            rx.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        drop(hb);
        // Dropping joined the thread, which dropped the sender: nothing more can arrive.
        assert!(rx.try_iter().count() < 1_000);
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
        // A beat that cannot write ends the thread by itself.
        let (tx2, rx2) = mpsc::channel::<()>();
        drop(rx2);
        let hb = Heartbeat::start(Duration::from_millis(1), move || tx2.send(()).is_ok());
        drop(hb);
    }

    #[test]
    fn the_first_beat_is_at_once() {
        let (tx, rx) = mpsc::channel();
        let _hb = Heartbeat::start(Duration::from_secs(3600), move || tx.send(()).is_ok());
        rx.recv_timeout(Duration::from_secs(10)).expect("a beat before the first interval");
    }
}
