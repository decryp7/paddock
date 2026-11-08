//! The thread behind a notification subscription, and how it ends. The OS side (a D-Bus signal stream) decides when its iterator
//! returns, and on Linux a graceful shutdown of the connection never returns while the iterator holds a clone of it. So dropping a
//! subscription (1) says "stop" first, so nothing more is delivered to the sink, (2) closes the source through the closer it was
//! given, (3) waits a bounded time for the thread to end, and when it does not, logs it and lets the thread go. The agent's stop
//! therefore never depends on D-Bus (Dyno ADR 0031 class S).
//!
//! The stop flag is checked before each delivery, not atomically with it: an event that has passed the check when the drop begins
//! is still delivered once, so a sink must tolerate one late event (and must not block, as the port says). Every later event is
//! dropped.

use crate::{EventSink, PowerEvent, Subscription};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread::JoinHandle;
use std::time::Duration;

/// How long dropping a subscription waits for its thread to end.
#[cfg_attr(
    not(target_os = "linux"),
    allow(
        dead_code,
        reason = "only the Linux adapter uses it; the tests run everywhere"
    )
)]
pub(crate) const JOIN_WAIT: Duration = Duration::from_secs(2);

/// Closes the source of the events (the D-Bus connection), so the thread's loop ends. Called once, on drop.
pub(crate) type Closer = Box<dyn FnOnce() + Send>;

/// A running subscription thread.
pub(crate) struct EventThread {
    stop: Arc<AtomicBool>,
    done: Receiver<()>,
    thread: Option<JoinHandle<()>>,
    closer: Option<Closer>,
    wait: Duration,
}

impl EventThread {
    /// Starts `name` running `body`, which calls the function it is given for each event; events arriving after the subscription
    /// was dropped are not passed to `sink`. `closer` makes `body` return.
    ///
    /// # Errors
    /// The thread could not be started.
    pub(crate) fn start(
        name: &str,
        sink: EventSink,
        closer: Closer,
        wait: Duration,
        body: impl FnOnce(&dyn Fn(PowerEvent)) + Send + 'static,
    ) -> std::io::Result<EventThread> {
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, done) = channel();
        let flag = stop.clone();
        let thread = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                body(&|event| {
                    if !flag.load(Ordering::SeqCst) {
                        sink(event);
                    }
                });
                let _ = tx.send(());
            })?;
        Ok(EventThread {
            stop,
            done,
            thread: Some(thread),
            closer: Some(closer),
            wait,
        })
    }
}

impl Subscription for EventThread {}

impl Drop for EventThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // The closer runs on a thread of its own, so the one deadline below covers it too: a close that never returns (D-Bus not
        // answering) cannot hold the drop. If it hangs, that thread is let go with the events thread.
        if let Some(close) = self.closer.take() {
            let spawned = std::thread::Builder::new()
                .name("paddock-power-close".into())
                .spawn(close);
            if let Err(e) = spawned {
                log::warn!("the power subscription could not be closed on its own thread: {e}");
            }
        }
        match self.done.recv_timeout(self.wait) {
            // A thread that panicked dropped its sender without sending: it has ended all the same.
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                if let Some(t) = self.thread.take() {
                    let _ = t.join();
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                // Detached: it ends when the OS source does, and by then it delivers nothing.
                log::warn!(
                    "the power events thread did not end within {} ms of closing its connection; letting it go",
                    self.wait.as_millis()
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::mpsc::{Sender, channel};
    use std::time::Instant;

    type Seen = Arc<Mutex<Vec<PowerEvent>>>;
    type Body = Box<dyn FnOnce(&dyn Fn(PowerEvent)) + Send>;

    /// A sink that records events and tells the test each time one arrived.
    fn sink() -> (EventSink, Seen, Receiver<()>) {
        let seen: Seen = Arc::default();
        let s = seen.clone();
        let (arrived, rx) = channel();
        (
            Arc::new(move |e| {
                s.lock().unwrap().push(e);
                let _ = arrived.send(());
            }),
            seen,
            rx,
        )
    }

    /// A fake source: events come from a channel, the loop ends when every sender is gone (what closing the connection does).
    fn fake_source() -> (Sender<PowerEvent>, Body) {
        let (tx, rx) = channel::<PowerEvent>();
        (
            tx,
            Box::new(move |deliver: &dyn Fn(PowerEvent)| {
                for event in rx {
                    deliver(event);
                }
            }),
        )
    }

    #[test]
    fn events_reach_the_sink_and_dropping_closes_the_source_and_joins_the_thread() {
        let (sink, seen, arrived) = sink();
        let (tx, body) = fake_source();
        let tx = Arc::new(Mutex::new(Some(tx)));
        let closing = tx.clone();
        let closer: Closer = Box::new(move || {
            closing.lock().unwrap().take();
        });
        let thread = EventThread::start("t", sink, closer, Duration::from_secs(5), body).unwrap();
        tx.lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .send(PowerEvent::Sleeping)
            .unwrap();
        arrived
            .recv_timeout(Duration::from_secs(10))
            .expect("the event reached the sink (a hang guard)");
        // Dropping takes the sender, which ends the loop, and returns once the thread has ended.
        drop(thread);
        assert!(tx.lock().unwrap().is_none(), "the closer ran");
        assert_eq!(*seen.lock().unwrap(), [PowerEvent::Sleeping]);
    }

    #[test]
    fn nothing_is_delivered_after_the_drop_began() {
        let (sink, seen, _arrived) = sink();
        let (tx, body) = fake_source();
        let late = tx.clone();
        // The closer sends one more event, as a signal that was already in flight would, then lets the loop end.
        let closer: Closer = Box::new(move || {
            late.send(PowerEvent::Woke).unwrap();
            drop(tx);
            drop(late);
        });
        let thread = EventThread::start("t", sink, closer, Duration::from_secs(5), body).unwrap();
        drop(thread);
        assert!(
            seen.lock().unwrap().is_empty(),
            "{:?}",
            seen.lock().unwrap()
        );
    }

    #[test]
    fn a_thread_that_does_not_end_is_waited_for_a_bounded_time_and_let_go() {
        let (sink, seen, _arrived) = sink();
        let (tx, body) = fake_source();
        // The closer does nothing, as zbus' graceful shutdown did: the loop never ends by itself.
        let thread =
            EventThread::start("t", sink, Box::new(|| {}), Duration::from_millis(100), body)
                .unwrap();
        let began = Instant::now();
        drop(thread);
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "the drop waited {:?}",
            began.elapsed()
        );
        // The detached thread still ends when the source does, and delivers nothing then.
        tx.send(PowerEvent::Woke).unwrap();
        drop(tx);
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn a_closer_that_never_returns_cannot_hold_the_drop_either() {
        let (sink, _seen, _arrived) = sink();
        let (tx, body) = fake_source();
        let (hold_tx, hold_rx) = channel::<()>();
        // The closer blocks (D-Bus never answering the close): it runs on a thread of its own, inside the one deadline.
        let closer: Closer = Box::new(move || {
            let _ = hold_rx.recv();
        });
        let thread =
            EventThread::start("t", sink, closer, Duration::from_millis(100), body).unwrap();
        let began = Instant::now();
        drop(thread);
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "the drop waited {:?}",
            began.elapsed()
        );
        drop(hold_tx);
        drop(tx);
    }

    #[test]
    fn a_thread_that_panicked_is_ended_and_joined_at_once() {
        let (sink, _seen, _arrived) = sink();
        let body: Body = Box::new(|_| panic!("the source failed (this panic is the test)"));
        let thread =
            EventThread::start("t", sink, Box::new(|| {}), Duration::from_secs(30), body).unwrap();
        let began = Instant::now();
        drop(thread);
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "the drop waited {:?} for a thread that was already gone",
            began.elapsed()
        );
    }
}
