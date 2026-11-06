//! `store.activity`: every ten minutes while connected, what the worker spent and what its background loops did since the last
//! line (ER-27). It answers "Debrief uses CPU while idle": a loop that wakes without work shows as idle turns and no step lines,
//! next to the worker's CPU milliseconds. Six lines an hour; reading the CPU time is one call.

use debrief_store_duck::activity::{take, Counts, Loop};
use std::time::Duration;

/// How often the line is written.
pub const EVERY: Duration = Duration::from_secs(10 * 60);

/// The properties of one line: CPU milliseconds of the worker process alone since the last line, its resident megabytes (the
/// same scope), and for each loop the turns that worked, found nothing, found the write lock busy and failed.
pub fn values(cpu_ms: u64, resident_mb: u64, loops: &[(Loop, Counts)]) -> Vec<(String, u64)> {
    let mut out = vec![("CpuMs".to_string(), cpu_ms), ("ResidentMb".to_string(), resident_mb)];
    for (l, c) in loops {
        let name = l.name();
        let name = format!("{}{}", name[..1].to_ascii_uppercase(), &name[1..]);
        out.push((format!("{name}Worked"), c.worked));
        out.push((format!("{name}Idle"), c.idle));
        out.push((format!("{name}Blocked"), c.blocked));
        out.push((format!("{name}Failed"), c.failed));
    }
    out
}

/// The CPU time of this process alone: monotonic (a plugin process that ends does not take its share away, as it does from
/// a tree's total), and the same scope as `ResidentMb`. `None` where the OS does not say.
fn cpu_time() -> Option<Duration> {
    paddock_proc::tree_cpu_time_of(&paddock_proc::OsSource::default(), &[std::process::id()])
}

/// The running activity thread; [`Activity::stop`] ends it (the worker keeps it for the life of the process).
pub struct Activity {
    stop: std::sync::mpsc::Sender<()>,
    thread: std::thread::JoinHandle<()>,
}

impl Activity {
    /// Ends the thread and waits for it.
    pub fn stop(self) {
        drop(self.stop);
        let _ = self.thread.join();
    }
}

/// Starts the thread that writes the line every `every` until it is stopped. The first line counts from the thread's start.
pub fn start(every: Duration) -> Option<Activity> {
    let (stop, stopped) = std::sync::mpsc::channel::<()>();
    let spawned = std::thread::Builder::new().name("debrief-store-activity".into()).spawn(move || {
        let mut last = cpu_time().unwrap_or(Duration::ZERO);
        // The wait ends early only when the sender is dropped.
        while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(every) {
            let now = cpu_time();
            let cpu_ms = now.map_or(0, |n| u64::try_from(n.saturating_sub(last).as_millis()).unwrap_or(u64::MAX));
            last = now.unwrap_or(last);
            emit(&values(cpu_ms, debrief_ipc::process::current_rss_bytes() >> 20, &take()), every);
        }
    });
    match spawned {
        Ok(thread) => Some(Activity { stop, thread }),
        Err(e) => {
            log::warn!(EventId = "store.activity.not_started", Why = e.kind().to_string().as_str(); "The activity line cannot start");
            None
        }
    }
}

fn emit(values: &[(String, u64)], every: Duration) {
    let mut kvs: Vec<(&str, log::kv::Value)> = vec![("EventId", log::kv::Value::from("store.activity"))];
    kvs.extend(values.iter().map(|(k, v)| (k.as_str(), log::kv::Value::from(*v))));
    log::logger().log(
        &log::Record::builder()
            .level(log::Level::Info)
            .target("debrief_store_worker::activity")
            .args(format_args!("Store worker activity over the last {} minutes", every.as_secs() / 60))
            .key_values(&kvs)
            .build(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use debrief_store_duck::activity::Counts;

    #[test]
    fn the_line_has_the_cpu_the_memory_and_every_loops_turns() {
        let loops = [
            (
                Loop::Fts,
                Counts {
                    worked: 0,
                    idle: 1198,
                    blocked: 2,
                    failed: 0,
                },
            ),
            (
                Loop::Catalogue,
                Counts {
                    worked: 3,
                    idle: 4,
                    blocked: 5,
                    failed: 0,
                },
            ),
            (Loop::Tail, Counts::default()),
            (Loop::Positions, Counts::default()),
        ];
        let v = values(1500, 310, &loops);
        let get = |k: &str| v.iter().find(|(n, _)| n == k).map(|(_, x)| *x);
        assert_eq!((get("CpuMs"), get("ResidentMb")), (Some(1500), Some(310)));
        assert_eq!(
            (get("FtsWorked"), get("FtsIdle"), get("FtsBlocked")),
            (Some(0), Some(1198), Some(2))
        );
        assert_eq!(
            (get("CatalogueWorked"), get("CatalogueIdle"), get("CatalogueBlocked")),
            (Some(3), Some(4), Some(5))
        );
        assert_eq!(get("TailWorked"), Some(0));
        assert_eq!(get("PositionsBlocked"), Some(0));
        assert_eq!(v.len(), 2 + 4 * 4);
    }

    #[test]
    fn the_line_is_written_with_every_value_and_the_thread_writes_it_on_its_own() {
        use debrief_store_duck::testlog;
        testlog::install();
        emit(
            &values(
                4242,
                17,
                &[(
                    Loop::Fts,
                    Counts {
                        worked: 1,
                        idle: 2,
                        blocked: 3,
                        failed: 0,
                    },
                )],
            ),
            EVERY,
        );
        let line = testlog::wait_for("store.activity", Duration::from_secs(5), |r| {
            r.props.get("CpuMs").and_then(|v| v.as_u64()) == Some(4242)
        })
        .expect("the line");
        assert_eq!(line.level, log::Level::Info);
        assert_eq!(
            (line.props["ResidentMb"].as_u64(), line.props["FtsIdle"].as_u64()),
            (Some(17), Some(2))
        );
        assert!(line.message.contains("10 minutes"), "{}", line.message);
        // The thread: a short period writes a line with the CPU of the period and the resident size.
        let activity = start(Duration::from_millis(30)).expect("the thread starts");
        let own = testlog::wait_for("store.activity", Duration::from_secs(10), |r| {
            r.props.contains_key("ResidentMb") && r.props.contains_key("PositionsIdle") && r.props["ResidentMb"].as_u64() != Some(17)
        })
        .expect("the thread's line");
        let resident = own.props["ResidentMb"].as_u64().unwrap();
        assert!((1..1 << 20).contains(&resident), "megabytes, not bytes: {own:?}");
        assert!(own.props["CpuMs"].is_u64(), "{own:?}");
        // Stopping ends the thread (no line from it after that).
        activity.stop();
    }

    #[test]
    fn the_cpu_time_of_this_process_can_be_read() {
        // Reading it twice around some work: it is real (not a constant) and does not go back.
        let first = cpu_time().expect("the OS reports the CPU time");
        assert!(first > Duration::ZERO);
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        let mut x = 0u64;
        while cpu_time().unwrap() <= first {
            assert!(std::time::Instant::now() < deadline, "the CPU time does not advance");
            x = std::hint::black_box(x.wrapping_add(1));
        }
        assert!(cpu_time().unwrap() > first);
    }
}
