//! A debounced writer of the app's messages: the latest list scheduled is written once after a quiet delay, on its own
//! thread, so a burst of changes is one write and no write happens under the centre's locks. [`Debounced::flush`]
//! writes what is pending now (the app is closing; tests); dropping the writer writes it too.

use super::centre::Notice;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

type Value = Vec<Notice>;
type Write = Arc<dyn Fn(Value) + Send + Sync>;
/// Starts the writer thread (a parameter so a test can make it fail).
type Spawn = fn(&str, Box<dyn FnOnce() + Send>) -> std::io::Result<()>;

struct State {
    value: Option<Value>,
    due: Option<Instant>,
    writing: bool,
    closed: bool,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
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
    /// A writer calling `write` with each settled list.
    pub fn new(delay: Duration, write: impl Fn(Value) + Send + Sync + 'static) -> Debounced {
        Self::spawn_with(delay, Arc::new(write), spawn_thread)
    }

    fn spawn_with(delay: Duration, write: Write, spawn: Spawn) -> Debounced {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                value: None,
                due: None,
                writing: false,
                closed: false,
            }),
            changed: Condvar::new(),
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
        g.due = Some(Instant::now() + self.delay);
        self.shared.changed.notify_all();
    }

    /// Writes what is pending now and waits for it, at most `timeout`. True when nothing is pending any more.
    pub fn flush(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut g = self.shared.lock();
        if g.value.is_some() {
            g.due = Some(Instant::now());
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
        g.due = Some(Instant::now());
        self.shared.changed.notify_all();
    }
}

/// The writer thread: waits for a due list, writes it outside the lock, ends when closed with nothing pending.
fn run(shared: &Shared, write: &Write) {
    let mut g = shared.lock();
    loop {
        let due_now = g.due.is_some_and(|due| Instant::now() >= due) || g.closed;
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
                    due.saturating_duration_since(Instant::now())
                });
                g = shared
                    .changed
                    .wait_timeout(g, wait)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
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
        let w = Debounced::new(delay, move |v: Value| {
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
    fn a_short_delay_writes_by_itself() {
        let (w, rx) = counting(Duration::from_millis(1));
        w.schedule(list(2));
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)), Ok(2));
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
        let w = Debounced::new(Duration::ZERO, move |v: Value| {
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
