//! A supervised server restarts itself when its own program file changed (an app update replaced it), but only when
//! nothing is in flight: it removes its discovery file and exits, and launchd (`KeepAlive`), systemd
//! (`Restart=always`) or the Windows service wrapper start the new program. The decision is
//! [`super::spec::should_restart`]; this module reads the file, asks the running server whether it is idle over its
//! own management API, and sleeps.

use super::spec::{should_restart, Activity, BinaryStamp};
use crate::paths::Dirs;
use pitwall_core::mgmt::DownloadState;
use serde_json::Value;
use std::future::Future;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

/// How often the program file is looked at.
pub const CHECK_EVERY: Duration = Duration::from_secs(60);
/// How long the idle question may take.
const ASK_TIMEOUT: Duration = Duration::from_secs(5);

/// The size and modification time of `path` (`None` while it cannot be read, e.g. during replacement).
pub fn stamp(path: &Path) -> Option<BinaryStamp> {
    let m = std::fs::metadata(path).ok()?;
    let modified_ms = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis();
    Some(BinaryStamp { len: m.len(), modified_ms })
}

/// Idle when no job runs or waits, no request is active and no download is in progress. Anything unexpected reads as
/// busy: a restart can wait, a dropped request cannot.
pub fn activity_from(overview: &Value, downloads: &Value) -> Activity {
    let empty = |v: Option<&Value>| v.and_then(Value::as_array).is_some_and(Vec::is_empty);
    let queue_idle = empty(overview.pointer("/queue/running")) && empty(overview.pointer("/queue/waiting"));
    let requests_idle = overview.pointer("/throughput/activeRequests").and_then(Value::as_u64) == Some(0);
    let active = |state: &str| {
        serde_json::from_value::<DownloadState>(Value::String(state.to_string())).map_or(true, |s| {
            !matches!(
                s,
                DownloadState::Done | DownloadState::Failed | DownloadState::Paused | DownloadState::Cancelled
            )
        })
    };
    let downloads_idle = downloads.as_array().is_some_and(|all| {
        all.iter()
            .all(|d| d.get("state").and_then(Value::as_str).is_some_and(|s| !active(s)))
    });
    if queue_idle && requests_idle && downloads_idle {
        Activity::Idle
    } else {
        Activity::Busy
    }
}

/// Asks the running server (over loopback with the admin key) whether it is idle; any failure is `Busy`.
pub async fn ask_activity(dirs: &Dirs) -> Activity {
    use pitwall_loopback::{Client, LoopbackPort};
    let port = std::fs::read_to_string(dirs.discovery())
        .ok()
        .and_then(|t| pitwall_core::discovery::Discovery::parse(&t).ok())
        .and_then(|d| LoopbackPort::new(d.port));
    let key = std::fs::read_to_string(dirs.admin_key()).ok().map(|k| k.trim().to_string());
    let (Some(port), Some(key)) = (port, key) else {
        return Activity::Busy;
    };
    let client = Client::new(port, Some(key));
    let ask = |method: &str| {
        let client = client.clone();
        let path = format!("/mgmt/v1/rpc/{method}");
        async move {
            match client.json("POST", &path, None, ASK_TIMEOUT).await {
                Ok((200, v)) => v,
                _ => Value::Null,
            }
        }
    };
    activity_from(&ask("overview").await, &ask("downloads").await)
}

/// The loop: every `every`, restart (call `on_restart`, then return) when the program file changed and the server is
/// idle. `stamp_now` and `activity` are injected so tests need neither a real file nor a real server.
pub async fn watch<S, A, Fut>(
    started: Option<BinaryStamp>,
    supervised: bool,
    every: Duration,
    stamp_now: S,
    activity: A,
    on_restart: impl FnOnce(),
) where
    S: Fn() -> Option<BinaryStamp>,
    A: Fn() -> Fut,
    Fut: Future<Output = Activity>,
{
    if !supervised || started.is_none() {
        return;
    }
    loop {
        tokio::time::sleep(every).await;
        let now = stamp_now();
        // Only ask the server when the file changed: the question costs two requests.
        if now.is_some() && now != started && should_restart(started, now, activity().await, supervised) {
            on_restart();
            return;
        }
    }
}

/// Starts the watcher for the running server (call from inside the Tokio runtime).
pub fn spawn(dirs: Dirs) {
    let Ok(exe) = std::env::current_exe() else { return };
    let started = stamp(&exe);
    let supervised = std::env::var_os(super::spec::SUPERVISED_ENV).is_some_and(|v| !v.is_empty());
    tokio::spawn(async move {
        let d = dirs.clone();
        watch(
            started,
            supervised,
            CHECK_EVERY,
            || stamp(&exe),
            || {
                let dirs = d.clone();
                async move { ask_activity(&dirs).await }
            },
            || {
                eprintln!("Pitwall: the program was updated and nothing is running; restarting");
                let _ = std::fs::remove_file(dirs.discovery());
                std::process::exit(0);
            },
        )
        .await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    fn overview(running: usize, waiting: usize, active: u64) -> Value {
        json!({"queue": {"running": vec![0; running], "waiting": vec![0; waiting]}, "throughput": {"activeRequests": active}})
    }

    #[test]
    fn idle_means_no_jobs_no_requests_and_no_downloads() {
        assert_eq!(activity_from(&overview(0, 0, 0), &json!([])), Activity::Idle);
        assert_eq!(
            activity_from(
                &overview(0, 0, 0),
                &json!([{"state": "done"}, {"state": "paused"}, {"state": "failed"}])
            ),
            Activity::Idle
        );
        assert_eq!(activity_from(&overview(1, 0, 0), &json!([])), Activity::Busy);
        assert_eq!(activity_from(&overview(0, 2, 0), &json!([])), Activity::Busy);
        assert_eq!(activity_from(&overview(0, 0, 1), &json!([])), Activity::Busy);
        assert_eq!(
            activity_from(&overview(0, 0, 0), &json!([{"state": "downloading"}])),
            Activity::Busy
        );
        assert_eq!(activity_from(&overview(0, 0, 0), &json!([{"state": "selfTest"}])), Activity::Busy);
        // Unknown shapes and unknown states are treated as busy.
        assert_eq!(activity_from(&Value::Null, &json!([])), Activity::Busy);
        assert_eq!(activity_from(&overview(0, 0, 0), &Value::Null), Activity::Busy);
        assert_eq!(activity_from(&overview(0, 0, 0), &json!([{"state": "weird"}])), Activity::Busy);
        assert_eq!(activity_from(&overview(0, 0, 0), &json!([{"nostate": 1}])), Activity::Busy);
    }

    #[test]
    fn the_stamp_changes_when_the_file_does_and_is_none_when_missing() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("pitwall-server");
        assert_eq!(stamp(&f), None);
        std::fs::write(&f, b"v1").unwrap();
        let a = stamp(&f).unwrap();
        std::fs::write(&f, b"version 2").unwrap();
        assert_ne!(stamp(&f).unwrap(), a);
    }

    #[tokio::test]
    async fn a_server_restarts_after_its_program_changed_but_only_once_idle() {
        let a = BinaryStamp { len: 1, modified_ms: 1 };
        let b = BinaryStamp { len: 2, modified_ms: 2 };
        let current = Arc::new(Mutex::new(a));
        let idle = Arc::new(AtomicBool::new(false));
        let asked = Arc::new(AtomicUsize::new(0));
        let restarted = Arc::new(AtomicBool::new(false));
        let (c2, i2, k2, r2) = (current.clone(), idle.clone(), asked.clone(), restarted.clone());
        let task = tokio::spawn(async move {
            watch(
                Some(a),
                true,
                Duration::from_millis(10),
                move || Some(*c2.lock().unwrap()),
                move || {
                    k2.fetch_add(1, Ordering::SeqCst);
                    let idle = i2.load(Ordering::SeqCst);
                    async move {
                        if idle {
                            Activity::Idle
                        } else {
                            Activity::Busy
                        }
                    }
                },
                move || r2.store(true, Ordering::SeqCst),
            )
            .await;
        });
        // Unchanged: no question is asked and nothing restarts.
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(asked.load(Ordering::SeqCst), 0);
        // Updated while busy: it asks but waits.
        *current.lock().unwrap() = b;
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(asked.load(Ordering::SeqCst) > 0);
        assert!(!restarted.load(Ordering::SeqCst));
        // Idle now: it restarts.
        idle.store(true, Ordering::SeqCst);
        tokio::time::timeout(Duration::from_secs(5), task).await.unwrap().unwrap();
        assert!(restarted.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn an_unsupervised_server_never_watches() {
        let a = BinaryStamp { len: 1, modified_ms: 1 };
        let done = tokio::time::timeout(
            Duration::from_secs(2),
            watch(
                Some(a),
                false,
                Duration::from_millis(5),
                || None,
                || async { Activity::Idle },
                || panic!("must not restart"),
            ),
        )
        .await;
        assert!(done.is_ok(), "returns at once");
        let done = tokio::time::timeout(
            Duration::from_secs(2),
            watch(
                None,
                true,
                Duration::from_millis(5),
                || None,
                || async { Activity::Idle },
                || panic!("must not restart"),
            ),
        )
        .await;
        assert!(done.is_ok());
    }
}
