//! A supervised server restarts itself when its own program file changed (an app update replaced it), but only when nothing
//! is in flight: it removes its discovery file and exits, and launchd (`KeepAlive`), systemd (`Restart=always`) or the Windows
//! service host start the new program. The watch itself is the shared one ([`paddock_service::watch::ExeWatch`], rule 55);
//! what is Pitwall's is the question "is the server idle?", asked over its own management API.

use crate::paths::Dirs;
use paddock_service::watch::{is_supervised, ExeWatch, WatchStop};
use pitwall_core::mgmt::DownloadState;
use pitwall_service::SUPERVISED_ENV;
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;

/// How long the idle question may take.
const ASK_TIMEOUT: Duration = Duration::from_secs(5);

/// Whether anything is in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// Nothing runs: a restart drops nothing.
    Idle,
    /// A job, a request or a download is in flight, or the answer was unclear.
    Busy,
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

/// The running watch (kept for the life of the process: dropping a [`WatchStop`] stops the watch).
static WATCH: OnceLock<WatchStop> = OnceLock::new();

/// Starts the watch for the running server (call from inside the Tokio runtime): every minute, when the program file changed
/// and the server is idle, it removes the discovery file and ends the process so the supervisor starts the new program.
pub fn spawn(dirs: Dirs) {
    let Ok(exe) = std::env::current_exe() else { return };
    let handle = tokio::runtime::Handle::current();
    let ask_dirs = dirs.clone();
    let watch = ExeWatch::new(exe, is_supervised(SUPERVISED_ENV));
    let started = watch.spawn(
        // On the watch's own thread: a blocking question to the server, which runs on the runtime.
        move || handle.block_on(ask_activity(&ask_dirs)) != Activity::Idle,
        move || {
            eprintln!("Pitwall: the program was updated and nothing is running; restarting");
            let _ = std::fs::remove_file(dirs.discovery());
            std::process::exit(0);
        },
    );
    match started {
        Ok(stop) => {
            let _ = WATCH.set(stop);
        }
        Err(e) => log::warn!(target: "pitwall_server", "the update watch could not start: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
