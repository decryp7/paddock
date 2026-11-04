//! The diagnostics bundle: collects what helps a bug report (inventory, settings, alarms, the flight recorder, recent
//! activity), redacts it with [`dyno_core::diagnostics`] and writes the zip to the path the user chose. Nothing is
//! uploaded; the user decides what to do with the file.

use crate::agent::{lock, now, Shared};
use crate::helper_first::{HelperCalls, HelperReading, ReadClass, Sourced};
use crate::helper_link::CallError;
use dyno_core::diagnostics::{BundleBuilder, DiagnosticsBundle, Redactor, SavedDiagnostics};
use dyno_core::metrics::Reading;
use dyno_core::protocol::{AgentError, ErrorCode};
use dyno_core::recorder::CaptureReason;
use serde::Serialize;
use std::io::Write;
use std::path::Path;

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

/// Most of one log file a bundle holds (its recent part).
pub const LOG_BYTES_PER_FILE: usize = 256 * 1024;
/// Most log files a bundle holds (the newest of each process).
const MAX_LOG_FILES_PER_PROCESS: usize = 2;

/// The newest dated logs of Dyno's processes in `dir` (`dyno.<day>.log` for the app, `dyno-agent.<day>.log` for the agent):
/// `(path in the bundle, recent text)`, each cut to [`LOG_BYTES_PER_FILE`] at a line start. Read from disk, so they are there with
/// the app's own process stopped or crashed; nothing is asked of anything. A missing folder gives nothing.
pub fn recent_logs(dir: &Path) -> Vec<(String, String)> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> = entries
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            (n.starts_with("dyno.") || n.starts_with("dyno-agent.")) && n.ends_with(".log") && e.path().is_file()
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    let (mut app_seen, mut agent_seen) = (0, 0);
    files
        .into_iter()
        .filter(|(_, path)| {
            let agent = path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("dyno-agent."));
            let seen = if agent { &mut agent_seen } else { &mut app_seen };
            *seen += 1;
            *seen <= MAX_LOG_FILES_PER_PROCESS
        })
        .filter_map(|(_, path)| {
            let mut file = std::fs::File::open(&path).ok()?;
            let len = file.metadata().ok()?.len();
            let start = len.saturating_sub(LOG_BYTES_PER_FILE as u64);
            file.seek(SeekFrom::Start(start)).ok()?;
            let mut bytes = Vec::new();
            file.take(LOG_BYTES_PER_FILE as u64).read_to_end(&mut bytes).ok()?;
            if start > 0 {
                // Began inside a line: drop up to the first newline.
                match bytes.iter().position(|b| *b == b'\n') {
                    Some(i) => {
                        bytes.drain(..=i);
                    }
                    None => bytes.clear(),
                }
            }
            let name = path.file_name()?.to_string_lossy().into_owned();
            (!bytes.is_empty()).then(|| (format!("logs/{name}"), String::from_utf8_lossy(&bytes).into_owned()))
        })
        .collect()
}

/// The user names, home folders and machine names of this machine, for the redactor.
pub fn local_identity(host_name: Option<String>) -> Redactor {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let users: Vec<String> = ["USER", "USERNAME", "LOGNAME"].iter().filter_map(|k| env(k)).collect();
    let homes: Vec<String> = ["HOME", "USERPROFILE"].iter().filter_map(|k| env(k)).collect();
    let machines: Vec<String> = ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .filter_map(|k| env(k))
        .chain(host_name)
        .collect();
    Redactor::new(&users, &homes, &machines)
}

/// Collects the bundle (redacted and bounded). Slow only the first time (the inventory is read then).
pub fn collect(shared: &Shared) -> Result<DiagnosticsBundle, String> {
    collect_with_logs(shared, &shared.dirs.log_folder())
}

/// [`collect`] with the folder the recent logs are read from named (the tests name a temporary one).
pub fn collect_with_logs(shared: &Shared, log_dir: &Path) -> Result<DiagnosticsBundle, String> {
    let inventory = crate::inventory::get(shared);
    let t = now();
    let mut b = BundleBuilder::new(
        local_identity(inventory.host_name.clone()),
        t,
        env!("CARGO_PKG_VERSION"),
        &inventory.os,
    );
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
    let network = shared
        .network
        .snapshot(
            shared,
            dyno_core::network::NetworkQuery {
                show_loopback: true,
                ..Default::default()
            },
        )
        .ok();
    let mut add = |path: &str, what: &str, text: String| b.add(path, what, &text).map_err(|e| e.to_string());
    add("README.txt", "What this is", README.into())?;
    add(
        "system/inventory.json",
        "Operating system, processor, memory slots and disks",
        json(&inventory),
    )?;
    add("settings.json", "Dyno's settings", json(&settings))?;
    add("system/time-zone.json", "The time zone local days are counted in", json(&zone))?;
    add("alarms/open.json", "Alarms", json(&alarms))?;
    add("alarms/stats.json", "Alarm statistics", json(&stats))?;
    add("alarms/history.json", "Recent alarm changes", json(&events))?;
    if let Some(n) = network {
        add(
            "network/sockets.json",
            "Listening ports and connections (addresses redacted)",
            json(&n),
        )?;
    }
    add("activity.json", "What Dyno recorded recently", json(&activity))?;
    add("alerts.json", "Recent alerts", json(&alerts))?;
    if let Some(r) = recorder {
        add(
            "recorder/last-10-minutes.json",
            "The last ten minutes at one-second resolution",
            json(&r),
        )?;
    }
    // The helper's own log, or why it is missing; best effort like the logs below.
    let (path, what, text) = helper_log_file(lock(&shared.helper_log).read());
    let _ = add(path, what, text);
    // Last and best effort: a log that does not fit the bundle's total is left out, it never fails the save.
    for (path, text) in recent_logs(log_dir) {
        let _ = add(
            &path,
            "The recent part of a log; file contents are never logged, user names and addresses are replaced",
            text,
        );
    }
    Ok(b.build())
}

const README: &str = "Dyno diagnostics bundle\n\nThis zip was made on your request and was not sent anywhere. User names, home folders, the machine name, \
e-mail addresses and IP addresses were replaced with placeholders (<user>, ~, <machine>, <email>, <ip>); manifest.json lists every file and \
what was removed. logs/ holds the recent part of the app's and the agent's own logs. Look through the files before you share the bundle.\n";

fn refuse(message: impl Into<String>) -> AgentError {
    AgentError::new(ErrorCode::BadRequest, message)
}

/// Writes `bundle` as a zip at `dest` (already validated by [`dyno_core::diagnostics::validate_destination`]). The
/// parent folder must exist; the file is written next to the destination and renamed into place so a failure never
/// leaves a half-written zip, and an existing directory is never replaced.
pub fn write_zip(dest: &Path, bundle: &DiagnosticsBundle) -> Result<SavedDiagnostics, AgentError> {
    let parent = dest.parent().ok_or_else(|| refuse("The save path has no folder."))?;
    if !parent.is_dir() {
        return Err(refuse(format!(
            "The folder {} does not exist; choose an existing folder.",
            parent.display()
        )));
    }
    if dest.is_dir() {
        return Err(refuse("That path is a folder; choose a file name ending in .zip."));
    }
    let name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let part = parent.join(format!(".{name}.part"));
    let manifest = bundle
        .manifest_json()
        .map_err(|e| AgentError::new(ErrorCode::Internal, e.to_string()))?;
    let result = (|| -> Result<u32, String> {
        let file = std::fs::File::create(&part).map_err(|e| format!("Could not create the file: {e}"))?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut count = 0u32;
        for f in &bundle.files {
            zip.start_file(f.path.as_str(), opts).map_err(|e| e.to_string())?;
            zip.write_all(f.text.as_bytes()).map_err(|e| e.to_string())?;
            count += 1;
        }
        zip.start_file("manifest.json", opts).map_err(|e| e.to_string())?;
        zip.write_all(manifest.as_bytes()).map_err(|e| e.to_string())?;
        zip.finish().map_err(|e| e.to_string())?.sync_all().map_err(|e| e.to_string())?;
        Ok(count + 1)
    })();
    match result {
        Ok(files) => {
            std::fs::rename(&part, dest).map_err(|e| {
                let _ = std::fs::remove_file(&part);
                AgentError::new(ErrorCode::AccessDenied, format!("Could not save to {}: {e}", dest.display()))
            })?;
            let bytes = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
            Ok(SavedDiagnostics {
                path: dest.to_string_lossy().into_owned(),
                bytes,
                files,
            })
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(AgentError::new(ErrorCode::AccessDenied, format!("Could not write the bundle: {e}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::time::Timestamp;
    use std::io::Read;

    fn bundle() -> DiagnosticsBundle {
        let mut b = BundleBuilder::new(Redactor::new(&["alex".into()], &[], &[]), Timestamp(1), "0.1.0", "TestOS");
        b.add("a.txt", "A", "hello alex").unwrap();
        b.add("dir/b.json", "B", "{}").unwrap();
        b.build()
    }

    #[test]
    fn writes_a_zip_with_the_files_and_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.zip");
        let saved = write_zip(&dest, &bundle()).unwrap();
        assert_eq!((saved.files, saved.bytes > 0), (3, true));
        let mut z = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
        let names: Vec<String> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().unwrap().to_string()).collect();
        assert_eq!(names, ["a.txt", "dir/b.json", "manifest.json"]);
        let mut text = String::new();
        z.by_name("a.txt").unwrap().read_to_string(&mut text).unwrap();
        assert_eq!(text, "hello <user>");
        let mut m = String::new();
        z.by_name("manifest.json").unwrap().read_to_string(&mut m).unwrap();
        assert!(m.contains("\"userNames\": 1"));
        assert!(!dir.path().join(".out.zip.part").exists());
        // Overwriting an existing file works.
        write_zip(&dest, &bundle()).unwrap();
    }

    #[test]
    fn bad_destinations_are_refused_with_a_message() {
        let dir = tempfile::tempdir().unwrap();
        let e = write_zip(&dir.path().join("missing").join("out.zip"), &bundle()).unwrap_err();
        assert!(e.message.contains("does not exist"), "{}", e.message);
        std::fs::create_dir(dir.path().join("taken.zip")).unwrap();
        let e = write_zip(&dir.path().join("taken.zip"), &bundle()).unwrap_err();
        assert!(e.message.contains("folder"));
        assert!(write_zip(Path::new("/"), &bundle()).is_err());
        let e = write_zip(dir.path(), &bundle()).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest);
    }

    /// "Save bundle" on Dyno's Diagnostics page held 12 data files and no log: the logs of both processes are what a bug report is
    /// read from. The newest dated logs of the app and the agent are included, each cut to its recent part at a line.
    #[test]
    fn the_newest_dated_logs_of_both_processes_are_read_cut_at_a_line_and_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let line = "{\"@m\":\"Agent running\"}\n";
        std::fs::write(dir.path().join("dyno.2026-10-31.log"), line.repeat(40_000)).unwrap(); // about 1 MB
        std::fs::write(dir.path().join("dyno-agent.2026-10-31.log"), format!("first\n{line}")).unwrap();
        std::fs::write(dir.path().join("other.2026-10-31.log"), "not ours\n").unwrap();
        std::fs::write(dir.path().join("dyno.notes.txt"), "no\n").unwrap();
        let logs = recent_logs(dir.path());
        let names: Vec<&str> = logs.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(
            names.contains(&"logs/dyno.2026-10-31.log") && names.contains(&"logs/dyno-agent.2026-10-31.log"),
            "{names:?}"
        );
        let app = &logs.iter().find(|(n, _)| n == "logs/dyno.2026-10-31.log").unwrap().1;
        assert!(
            app.len() <= LOG_BYTES_PER_FILE && app.starts_with("{\"@m\""),
            "cut to the window at a line: {}",
            app.len()
        );
        let agent = &logs.iter().find(|(n, _)| n.contains("agent")).unwrap().1;
        assert!(agent.starts_with("first\n"), "a small log is whole");
        assert!(
            recent_logs(&dir.path().join("missing")).is_empty(),
            "no log folder: nothing, not an error"
        );
    }

    /// A few days of app logs must not push the agent's last log out of the bundle: the newest files are taken per process.
    #[test]
    fn the_newest_logs_are_taken_per_process_not_across_both() {
        let dir = tempfile::tempdir().unwrap();
        let base = std::time::SystemTime::now();
        let put = |name: &str, age_s: u64| {
            let f = std::fs::File::create(dir.path().join(name)).unwrap();
            std::io::Write::write_all(&mut &f, b"x\n").unwrap();
            f.set_modified(base - std::time::Duration::from_secs(age_s)).unwrap();
        };
        for (i, day) in ["01", "02", "03", "04", "05"].iter().enumerate() {
            put(&format!("dyno.2026-10-{day}.log"), 10 + i as u64);
        }
        put("dyno-agent.2026-09-01.log", 100_000);
        let names: Vec<String> = recent_logs(dir.path()).into_iter().map(|(n, _)| n).collect();
        assert!(names.contains(&"logs/dyno-agent.2026-09-01.log".to_string()), "{names:?}");
        assert_eq!(
            names.iter().filter(|n| n.starts_with("logs/dyno.")).count(),
            MAX_LOG_FILES_PER_PROCESS,
            "{names:?}"
        );
    }

    #[test]
    fn the_local_identity_redacts_what_the_environment_names() {
        let r = local_identity(Some("Workstation-77".into()));
        let (s, c) = r.apply("on Workstation-77 as nobody-here");
        assert_eq!(s, "on <machine> as nobody-here");
        assert_eq!(c.machine_names, 1);
    }

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
    fn the_helpers_log_goes_into_the_bundle_and_is_redacted_with_the_rest() {
        let lines = vec!["2026-11-04 helper started".to_string(), "alex connected from 10.0.0.7".to_string()];
        let got = log_reading(true, vec![Ok(HelperReply::Log(lines))]);
        assert_eq!(got.via, ReadVia::Helper);
        let (path, _, text) = helper_log_file(got);
        assert_eq!(path, "helper/helper.log");
        let mut b = BundleBuilder::new(Redactor::new(&["alex".into()], &[], &[]), Timestamp(1), "0.1.0", "TestOS");
        b.add(path, "log", &text).unwrap();
        let out = b.build();
        let body = out.files.iter().find(|f| f.path == path).map(|f| f.text.clone()).unwrap();
        assert!(
            body.contains("helper started") && !body.contains("alex") && !body.contains("10.0.0.7"),
            "{body}"
        );
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
