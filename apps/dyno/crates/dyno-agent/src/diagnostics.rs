//! The diagnostics bundle: collects what helps a bug report (inventory, settings, alarms, the flight recorder, recent
//! activity), redacts it with [`dyno_core::diagnostics`] and writes the zip to the path the user chose. Nothing is
//! uploaded; the user decides what to do with the file.

use crate::agent::{lock, now, Shared};
use dyno_core::diagnostics::{BundleBuilder, DiagnosticsBundle, Redactor, SavedDiagnostics};
use dyno_core::protocol::{AgentError, ErrorCode};
use dyno_core::recorder::CaptureReason;
use serde::Serialize;
use std::io::Write;
use std::path::Path;

/// Most activity entries and alarm events included.
const RECENT: u32 = 200;

fn json<T: Serialize>(v: &T) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|e| format!("{{\"error\":\"could not serialise: {e}\"}}"))
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
    Ok(b.build())
}

const README: &str = "Dyno diagnostics bundle\n\nThis zip was made on your request and was not sent anywhere. User names, home folders, the machine name, \
e-mail addresses and IP addresses were replaced with placeholders (<user>, ~, <machine>, <email>, <ip>); manifest.json lists every file and \
what was removed. Look through the files before you share the bundle.\n";

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

    #[test]
    fn the_local_identity_redacts_what_the_environment_names() {
        let r = local_identity(Some("Workstation-77".into()));
        let (s, c) = r.apply("on Workstation-77 as nobody-here");
        assert_eq!(s, "on <machine> as nobody-here");
        assert_eq!(c.machine_names, 1);
    }
}
