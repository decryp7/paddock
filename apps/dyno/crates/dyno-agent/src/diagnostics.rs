//! The agent's part of a problem report: its data files (inventory, settings, alarms, sockets, activity, alerts, the flight
//! recorder, the helper's log) as plain text. The app's Report a problem dialog (`paddock-diag`) redacts every file with the one
//! redactor, previews it and writes the zip; this module redacts and zips nothing. Nothing is uploaded.

use crate::agent::{lock, now, Shared};
use crate::helper_first::{HelperCalls, HelperReading, ReadClass, Sourced};
use crate::helper_link::CallError;
use dyno_core::diagnostics::{DiagnosticsFile, DiagnosticsFiles};
use dyno_core::metrics::Reading;
use dyno_core::recorder::CaptureReason;
use serde::Serialize;

/// Most activity entries and alarm events included.
const RECENT: u32 = 200;
/// The helper's log lines a bundle holds (the helper clamps its reply to its own cap).
pub const HELPER_LOG_LINES: u32 = 200;

/// The helper's own log (`LogTail`), for the bundle: ER-27 wants a helper failure explained from the bundle alone. Only the helper can
/// read it (it is root's), and only the owner or an administrator may ask.
pub struct HelperLog;

impl HelperReading for HelperLog {
    type Value = Vec<String>;

    fn name(&self) -> &'static str {
        "helper log"
    }

    fn class(&self) -> ReadClass {
        // A person saves a bundle and waits; the log is read once, not every few seconds.
        ReadClass::Bulk
    }

    fn via_helper(&self, calls: &dyn HelperCalls) -> Result<Vec<String>, CallError> {
        calls.log_tail(HELPER_LOG_LINES)
    }

    fn local(&mut self) -> Reading<Vec<String>> {
        Reading::unavailable("the helper's log is readable only through the helper")
    }
}

/// The bundle file for the helper's log: its lines when the helper gave them, else a note saying why not (so a bundle from a computer
/// with a broken helper still explains it).
pub fn helper_log_file(got: Sourced<Vec<String>>) -> (&'static str, &'static str, String) {
    match got.value {
        Reading::Available(lines) => (
            "helper/helper.log",
            "The last lines of the administrator helper's own log",
            lines.join("\n") + "\n",
        ),
        Reading::Unavailable(why) => (
            "helper/helper-log.txt",
            "Why the administrator helper's log is not in this bundle",
            format!("{}\n", got.via.reason().unwrap_or(&why)),
        ),
    }
}

fn json<T: Serialize>(v: &T) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|e| format!("{{\"error\":\"could not serialise: {e}\"}}"))
}

/// Collects the agent's files. Slow only the first time (the inventory is read then). A part that cannot be collected becomes a
/// note, never a failure of the rest.
pub fn collect(shared: &Shared) -> DiagnosticsFiles {
    let inventory = crate::inventory::get(shared);
    let t = now();
    let (zone, settings, alarms, stats, events, activity, alerts, recorder) = {
        let core = lock(&shared.core);
        (
            serde_json::json!({
                "zone": core.zone.describe(),
                "offsetNow": dyno_core::time::offset_label(t, core.zone.as_ref()),
            }),
            core.settings.clone(),
            core.alarms.list(
                &dyno_core::alarms::AlarmFilter {
                    limit: 500,
                    include_shelved: true,
                    ..Default::default()
                },
                t,
            ),
            core.alarms.stats(t),
            core.store.alarm_events(None, RECENT).unwrap_or_default(),
            core.store.activity(RECENT).unwrap_or_default().into_iter().collect::<Vec<_>>(),
            core.store.alerts(RECENT).unwrap_or_default(),
            core.recorder.snapshot(CaptureReason::Manual, t),
        )
    };
    let network = shared.network.snapshot(
        shared,
        dyno_core::network::NetworkQuery {
            show_loopback: true,
            ..Default::default()
        },
    );
    let mut out = DiagnosticsFiles::default();
    let mut add = |name: &str, what: &str, text: String| out.files.push(DiagnosticsFile::new(name, what, text));
    add(
        "system/inventory.json",
        "Operating system, processor, memory slots and disks",
        json(&inventory),
    );
    add("agent-settings.json", "Dyno's settings as the agent holds them", json(&settings));
    add("system/time-zone.json", "The time zone local days are counted in", json(&zone));
    add("alarms/open.json", "Alarms", json(&alarms));
    add("alarms/stats.json", "Alarm statistics", json(&stats));
    add("alarms/history.json", "Recent alarm changes", json(&events));
    add("activity.json", "What Dyno recorded recently", json(&activity));
    add("alerts.json", "Recent alerts", json(&alerts));
    let mut notes = Vec::new();
    match network {
        Ok(n) => add(
            "network/sockets.json",
            "Listening ports and connections (private addresses kept, others redacted)",
            json(&n),
        ),
        Err(e) => notes.push(format!("The listening ports and connections could not be read ({e:?}).")),
    }
    match recorder {
        Some(r) => add(
            "recorder/last-10-minutes.json",
            "The last ten minutes at one-second resolution",
            json(&r),
        ),
        None => notes.push("The flight recorder has nothing recorded yet.".into()),
    }
    // The helper's own log (and the end of the rotated one), or why it is missing: a computer with a broken helper still explains it.
    let (name, what, text) = helper_log_file(lock(&shared.helper_log).read());
    add(name, what, text);
    out.notes = notes;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::helper_first::testkit::{centre, port, Net};
    use crate::helper_first::HelperFirst;
    use crate::helper_link::TransportError;
    use dyno_core::helper_wire::HelperReply;
    use dyno_core::read_via::ReadVia;
    use paddock_jobs::time::FakeClock;

    fn log_reading(up: bool, answers: Vec<Result<HelperReply, TransportError>>) -> Sourced<Vec<String>> {
        let c = centre(&Net::new(up, answers), &FakeClock::new());
        HelperFirst::new(HelperLog, port(&c)).read()
    }

    #[test]
    fn the_helpers_log_goes_into_the_report_files() {
        let lines = vec!["2026-11-04 helper started".to_string(), "second line".to_string()];
        let got = log_reading(true, vec![Ok(HelperReply::Log(lines))]);
        assert_eq!(got.via, ReadVia::Helper);
        let (path, _, text) = helper_log_file(got);
        assert_eq!(path, "helper/helper.log");
        assert_eq!(text, "2026-11-04 helper started\nsecond line\n");
    }

    #[test]
    fn without_the_helper_the_bundle_says_why_its_log_is_missing() {
        let (path, _, text) = helper_log_file(log_reading(false, vec![]));
        assert_eq!(path, "helper/helper-log.txt");
        assert!(text.contains("readable only through the helper"), "{text}");
        let refused = log_reading(
            true,
            vec![Err(TransportError::Remote {
                code: "denied".into(),
                message: "Only the owner or an administrator may read the helper's log.".into(),
            })],
        );
        let (path, _, text) = helper_log_file(refused);
        assert_eq!(path, "helper/helper-log.txt");
        assert!(text.contains("owner or an administrator"), "{text}");
    }
}
