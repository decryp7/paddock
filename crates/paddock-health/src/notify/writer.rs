//! A debounced writer of the app's messages: the latest list scheduled is written once after a quiet delay, on its own
//! thread, so a burst of changes is one write and no write happens under the centre's locks. [`Debounced::flush`]
//! writes what is pending now (the app is closing; tests); dropping the writer writes it too.

use super::centre::Notice;
use paddock_jobs::CancelToken;
use paddock_jobs::time::Clock;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

type Value = Vec<Notice>;
type Write = Arc<dyn Fn(Value) + Send + Sync>;
/// Starts the writer thread (a parameter so a test can make it fail).
type Spawn = fn(&str, Box<dyn FnOnce() + Send>) -> std::io::Result<()>;

struct State {
    value: Option<Value>,
    /// On the writer's clock's monotonic reading.
    due: Option<Duration>,
    writing: bool,
    closed: bool,
    /// Cancelled (and replaced) on every change, so the writer's wait on the clock ends at once.
    poke: CancelToken,
}

impl State {
    fn poke(&mut self) {
        self.poke.cancel();
        self.poke = CancelToken::new();
    }
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    clock: Arc<dyn Clock>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Writes the latest scheduled list after `delay` without a newer one.
pub struct Debounced {
    shared: Arc<Shared>,
    delay: Duration,
    /// Set when the writer thread could not start: every list is written at once by the caller.
    inline: Option<Write>,
}

fn spawn_thread(name: &str, run: Box<dyn FnOnce() + Send>) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(run)
        .map(drop)
}

impl Debounced {
    /// A writer calling `write` with each settled list, the delay measured on `clock` (the notification centre's).
    pub fn on_clock(
        clock: Arc<dyn Clock>,
        delay: Duration,
        write: impl Fn(Value) + Send + Sync + 'static,
    ) -> Debounced {
        Self::spawn_with(clock, delay, Arc::new(write), spawn_thread)
    }

    fn spawn_with(clock: Arc<dyn Clock>, delay: Duration, write: Write, spawn: Spawn) -> Debounced {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                value: None,
                due: None,
                writing: false,
                closed: false,
                poke: CancelToken::new(),
            }),
            changed: Condvar::new(),
            clock,
        });
        let (s, w) = (shared.clone(), write.clone());
        let inline = match spawn(
            "paddock-notifications-writer",
            Box::new(move || run(&s, &w)),
        ) {
            Ok(()) => None,
            Err(e) => {
                log::warn!(target: "paddock_health", "The notifications writer could not start; writing at once: {e}");
                Some(write)
            }
        };
        Debounced {
            shared,
            delay,
            inline,
        }
    }

    /// Schedules `value` (replacing one not yet written).
    pub fn schedule(&self, value: Value) {
        if let Some(write) = &self.inline {
            write(value);
            return;
        }
        let mut g = self.shared.lock();
        g.value = Some(value);
        g.due = Some(self.shared.clock.monotonic().saturating_add(self.delay));
        g.poke();
        self.shared.changed.notify_all();
    }

    /// Writes what is pending now and waits for it, at most `timeout`. True when nothing is pending any more.
    pub fn flush(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut g = self.shared.lock();
        if g.value.is_some() {
            g.due = Some(Duration::ZERO);
            g.poke();
            self.shared.changed.notify_all();
        }
        while g.value.is_some() || g.writing {
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            g = self
                .shared
                .changed
                .wait_timeout(g, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }
}

impl Drop for Debounced {
    fn drop(&mut self) {
        let mut g = self.shared.lock();
        g.closed = true;
        g.due = Some(Duration::ZERO);
        g.poke();
        self.shared.changed.notify_all();
    }
}

/// The writer thread: waits for a due list, writes it outside the lock, ends when closed with nothing pending.
fn run(shared: &Shared, write: &Write) {
    let mut g = shared.lock();
    loop {
        let due_now = g.due.is_some_and(|due| shared.clock.monotonic() >= due) || g.closed;
        match g.value.take() {
            Some(value) if due_now => {
                g.writing = true;
                drop(g);
                write(value);
                g = shared.lock();
                g.writing = false;
                shared.changed.notify_all();
            }
            Some(value) => {
                g.value = Some(value);
                let wait = g.due.map_or(Duration::ZERO, |due| {
                    due.saturating_sub(shared.clock.monotonic())
                });
                let poke = g.poke.clone();
                drop(g);
                // Ends when the delay passes on the clock, or at once on a change (a newer list, a flush, close).
                let _ = shared.clock.wait(wait, &poke);
                g = shared.lock();
            }
            None if g.closed => return,
            None => {
                g = shared
                    .changed
                    .wait(g)
                    .unwrap_or_else(PoisonError::into_inner)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::centre::{Centre, NewNotice, NoticeId, NoticeScope, NoticeSeverity};
    use paddock_jobs::time::SystemClock;
    use std::sync::mpsc;

    /// A list of `n` messages.
    fn list(n: u64) -> Value {
        let mut c = Centre::new(100);
        for i in 0..n {
            c.post(
                NewNotice::new(
                    format!("c{i}"),
                    "x",
                    NoticeSeverity::Info,
                    "t",
                    NoticeScope::App,
                ),
                NoticeId(i + 1),
                0,
            );
        }
        c.notices().to_vec()
    }

    /// A writer that reports the length of each list it writes.
    fn counting(delay: Duration) -> (Debounced, mpsc::Receiver<usize>) {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let w = Debounced::on_clock(Arc::new(SystemClock), delay, move |v: Value| {
            let _ = tx.lock().map(|tx| tx.send(v.len()));
        });
        (w, rx)
    }

    #[test]
    fn a_burst_is_written_once_with_the_latest_value() {
        let (w, rx) = counting(Duration::from_secs(60));
        for n in 1..=5 {
            w.schedule(list(n));
        }
        assert!(rx.try_recv().is_err(), "not before the delay");
        assert!(w.flush(Duration::from_secs(10)));
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(5));
        assert!(rx.try_recv().is_err(), "once");
        assert!(w.flush(Duration::from_secs(1)), "nothing pending");
    }

    #[test]
    fn the_delay_runs_out_on_the_clock_and_a_newer_list_restarts_it() {
        let clock = paddock_jobs::time::FakeClock::new();
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let w = Debounced::on_clock(
            Arc::new(clock.clone()),
            Duration::from_secs(2),
            move |v: Value| {
                let _ = tx.lock().map(|tx| tx.send(v.len()));
            },
        );
        w.schedule(list(1));
        assert!(clock.wait_for_started(1, Duration::from_secs(60)));
        clock.advance(Duration::from_secs(1));
        // A newer list one second in: the wait starts again from its own two seconds.
        w.schedule(list(2));
        clock.advance(Duration::from_secs(1));
        assert!(
            rx.try_recv().is_err(),
            "not yet: one second of the newer two"
        );
        clock.advance(Duration::from_secs(1));
        assert_eq!(rx.recv_timeout(Duration::from_secs(60)), Ok(2));
        // The system clock writes by itself after a short delay too.
        let (w2, rx2) = counting(Duration::ZERO);
        w2.schedule(list(3));
        assert_eq!(rx2.recv_timeout(Duration::from_secs(60)), Ok(3));
        drop(w);
    }

    #[test]
    fn dropping_the_writer_writes_what_is_pending() {
        let (w, rx) = counting(Duration::from_secs(60));
        w.schedule(list(3));
        drop(w);
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(3));
    }

    #[test]
    fn a_flush_gives_up_after_its_timeout() {
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let gate = Mutex::new(gate_rx);
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let w = Debounced::on_clock(Arc::new(SystemClock), Duration::ZERO, move |v: Value| {
            let _ = tx.lock().map(|tx| tx.send(v.len()));
            let _ = gate.lock().map(|g| g.recv());
        });
        w.schedule(list(1));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(10)),
            Ok(1),
            "the write started"
        );
        assert!(!w.flush(Duration::from_millis(1)), "still writing");
        gate_tx.send(()).unwrap();
        assert!(w.flush(Duration::from_secs(10)));
    }

    #[test]
    fn without_a_thread_values_are_written_at_once() {
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let w = Debounced::spawn_with(
            Arc::new(SystemClock),
            Duration::from_secs(60),
            Arc::new(move |v: Value| {
                let _ = tx.lock().map(|tx| tx.send(v.len()));
            }),
            |_, _| Err(std::io::Error::other("no threads")),
        );
        w.schedule(list(4));
        assert_eq!(rx.try_recv(), Ok(4));
        assert!(w.flush(Duration::from_secs(1)));
    }

    #[test]
    fn a_poisoned_lock_is_recovered() {
        let (w, rx) = counting(Duration::from_secs(60));
        let shared = w.shared.clone();
        let _ = std::thread::spawn(move || {
            let _g = shared.lock();
            panic!("poison the writer's lock");
        })
        .join();
        assert!(w.shared.state.is_poisoned());
        w.schedule(list(1));
        assert!(w.flush(Duration::from_secs(10)));
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(1));
    }
}
