//! Live progress of DuckDB statements (DuckDB's query progress estimate, `duckdb_query_progress`), relayed to the
//! run's [`QueryControl`] so the job, console and analysis progress events show it (ER-18). A statement's own
//! stage is `Evaluating`; rows processed and rows expected are DuckDB's estimate and only reported once it has one.
//!
//! Polling happens on a small thread per borrowed connection, never on the statement's thread, and stops with the
//! connection. Cancelling is unchanged: the control's hook interrupts the statement.

use debrief_core::ql::{QueryControl, QueryProgress, QueryStage};
use duckdb::Connection;
use std::sync::mpsc;
use std::time::Duration;

/// How often the estimate is read.
pub(crate) const POLL: Duration = Duration::from_millis(50);

/// DuckDB's estimate as the control's progress, or `None` while DuckDB has none.
pub(crate) fn from_duckdb(p: &duckdb::QueryProgress) -> Option<QueryProgress> {
    p.is_available().then(|| QueryProgress {
        stage: QueryStage::Evaluating,
        done: p.rows_processed(),
        total: p.total_rows_to_process().max(p.rows_processed()),
    })
}

/// Whether the poller may write: the control is untouched or still shows what the poller wrote last, so a stage that
/// another reporter began (an event stream, a compaction) is never overwritten.
fn may_write(current: QueryProgress, last_written: Option<QueryProgress>) -> bool {
    current
        == last_written.unwrap_or(QueryProgress {
            stage: QueryStage::Matching,
            done: 0,
            total: 0,
        })
}

/// Reads the progress of the statement on `conn`'s connection every `every` until `stop` is dropped or signalled.
pub(crate) fn relay(handle: &duckdb::ProgressHandle, control: &QueryControl, stop: &mpsc::Receiver<()>, every: Duration) {
    let mut last = None;
    loop {
        match stop.recv_timeout(every) {
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            _ => return,
        }
        if let Some(p) = from_duckdb(&handle.query_progress()) {
            if may_write(control.progress(), last) {
                control.report(p);
                last = Some(p);
            }
        }
    }
}

/// A running relay; dropping it stops the thread and switches the tracking off again.
pub(crate) struct Relay {
    stop: Option<mpsc::Sender<()>>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Relay {
    /// Starts tracking on `conn` and relaying to `control`; `None` (and no tracking) when the thread cannot start.
    pub(crate) fn start(conn: &Connection, control: QueryControl, every: Duration) -> Option<Relay> {
        conn.set_query_progress_tracking(true).ok()?;
        let handle = conn.progress_handle();
        let (stop, rx) = mpsc::channel();
        let join = std::thread::Builder::new()
            .name("debrief-sql-progress".into())
            .spawn(move || relay(&handle, &control, &rx, every))
            .ok();
        if join.is_none() {
            let _ = conn.set_query_progress_tracking(false);
            return None;
        }
        Some(Relay { stop: Some(stop), join })
    }

    /// Stops the thread and the tracking on `conn` (a pooled connection goes back without tracking).
    pub(crate) fn finish(self, conn: &Connection) {
        drop(self);
        let _ = conn.set_query_progress_tracking(false);
    }
}

impl Drop for Relay {
    /// Stops the thread (tracking stays on: a dedicated connection keeps it for its next statement).
    fn drop(&mut self) {
        self.stop.take();
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(done: u64, total: u64) -> QueryProgress {
        QueryProgress {
            stage: QueryStage::Evaluating,
            done,
            total,
        }
    }

    #[test]
    fn the_relay_never_overwrites_another_reporters_stage() {
        let untouched = QueryProgress {
            stage: QueryStage::Matching,
            done: 0,
            total: 0,
        };
        assert!(may_write(untouched, None));
        assert!(may_write(progress(5, 10), Some(progress(5, 10))), "its own last value");
        assert!(!may_write(progress(1, 3), Some(progress(5, 10))), "a compaction's groups began");
        assert!(!may_write(progress(2, 100), None), "an event stream began");
    }

    #[test]
    fn a_long_statement_reports_rows_processed_of_rows_expected_and_the_relay_stops() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("SET threads = 2; CREATE TABLE big AS SELECT range AS i FROM range(60000000)")
            .unwrap();
        let control = QueryControl::new();
        let relay = Relay::start(&conn, control.clone(), Duration::from_millis(5)).expect("tracking starts");
        let sum: i64 = conn
            .query_row("SELECT sum(hash(i) % 7) FROM big WHERE i % 3 <> 5", [], |r| r.get(0))
            .unwrap();
        assert!(sum > 0);
        relay.finish(&conn);
        let p = control.progress();
        assert_eq!(p.stage, QueryStage::Evaluating);
        assert!(p.total >= 1 && p.done <= p.total, "{p:?}");
        // The relay is gone: nothing writes after finish.
        control.begin(QueryStage::Matching, 7);
        let after = conn.query_row("SELECT count(*) FROM big", [], |r| r.get::<_, i64>(0)).unwrap();
        assert!(after > 0);
        assert_eq!(control.progress().total, 7);
    }

    #[test]
    fn duckdbs_missing_estimate_reports_nothing() {
        let conn = Connection::open_in_memory().unwrap();
        // Tracking off: no estimate is available.
        assert!(from_duckdb(&conn.progress_handle().query_progress()).is_none());
    }

    #[test]
    fn a_statement_cancelled_once_progress_shows_stops_with_the_relay_running() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("SET threads = 2; CREATE TABLE big AS SELECT range AS i FROM range(200000000)")
            .unwrap();
        let control = QueryControl::new();
        let interrupt = conn.interrupt_handle();
        let _hook = control.on_cancel(Box::new(move || interrupt.interrupt()));
        let relay = Relay::start(&conn, control.clone(), Duration::from_millis(5)).unwrap();
        // Cancels as soon as the relay has shown the statement's progress (a condition, not a delay).
        let watcher = {
            let control = control.clone();
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                while control.progress().total == 0 && started.elapsed() < Duration::from_secs(60) {
                    std::thread::yield_now();
                }
                control.cancel();
            })
        };
        let r = conn.query_row("SELECT sum(hash(i) % 7) FROM big WHERE i % 3 <> 5", [], |r| r.get::<_, i64>(0));
        watcher.join().unwrap();
        assert!(control.is_cancelled());
        assert!(r.is_err(), "the statement was interrupted");
        relay.finish(&conn);
    }
}
