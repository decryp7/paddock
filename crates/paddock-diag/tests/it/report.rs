//! A report end to end over temporary folders: sections, redaction, preview, zip, cancel and limits. Nothing here touches
//! the user's folders or the network.

use chrono::{TimeZone, Utc};
use paddock_diag::*;
use paddock_env::{AppDirs, AppId, MapVars, Os};
use paddock_jobs::{CancelToken, Counter};
use serde_json::json;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

fn app_info() -> AppInfo {
    AppInfo::new("demo", "Demo", "1.2.3", "abc1234")
}

fn dirs(root: &Path) -> AppDirs {
    let id = AppId::new("demo", "Demo").unwrap();
    AppDirs::resolve(
        &id,
        Os::MacOs,
        &MapVars::new().with("DEMO_HOME", root),
        None,
        true,
    )
    .unwrap()
}

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 24, 10, 30, 0).unwrap()
}

fn redactor() -> Redactor {
    Redactor::for_machine(
        &MapVars::new()
            .with("USER", "alex")
            .with("HOME", "/Users/alex")
            .with("HOSTNAME", "ALEX-PC"),
    )
}

/// A machine with a log of three lines, a settings file with a secret, and a crash marker.
fn machine() -> (tempfile::TempDir, AppDirs) {
    let root = tempfile::tempdir().unwrap();
    let d = dirs(root.path());
    std::fs::create_dir_all(d.logs()).unwrap();
    std::fs::create_dir_all(d.config()).unwrap();
    std::fs::create_dir_all(d.data()).unwrap();
    let lines = [
        json!({"@t":"2026-10-24T09:00:00Z","@l":"Information","@m":"started for alex on ALEX-PC"}),
        json!({"@t":"2026-10-24T09:00:01Z","@l":"Error","@m":"open /Users/alex/Documents/a.log failed","Token":"abc","Window":"main"}),
        json!({"@t":"2026-10-24T09:00:02Z","@l":"Warning","@m":"retry with token=hunter2hunter2 to 10.1.2.3"}),
    ];
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(d.logs().join("demo.2026-10-24.log"), text).unwrap();
    std::fs::write(d.config().join("settings.json"), json!({"version":1,"sections":{"pd.appearance":{"theme":"dark"},"x":{"apiKey":"sk-secret-value","dir":"/Users/alex/work"}}}).to_string()).unwrap();
    std::fs::write(d.data().join("crash-app.json"), json!({"version":1,"process":"app","pid":7,"time":"2026-10-23T22:00:00Z","thread":"worker","message":"boom","location":"src/x.rs:1"}).to_string()).unwrap();
    (root, d)
}

fn entries(zip_bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            (f.name().unwrap().to_string(), data)
        })
        .collect()
}

struct CancelAfter {
    token: CancelToken,
    after: u64,
    seen: AtomicU64,
}

impl Counter for CancelAfter {
    fn add(&self, n: u64) {
        if self.seen.fetch_add(n, Ordering::SeqCst) + n >= self.after {
            self.token.cancel();
        }
    }
}

#[test]
fn the_preview_equals_the_zip() {
    let (_root, d) = machine();
    let collected = Collected::collect(&app_info(), &d, &builtin::all(), &redactor(), now());
    let report = collected.assemble("It crashed when I opened a file");
    let preview = report.preview();
    let bytes = report.zip_bytes(&CancelToken::new(), &()).unwrap();
    let zipped = entries(&bytes);
    let listed: Vec<(String, u64)> = preview
        .files
        .iter()
        .map(|f| (f.name.clone(), f.bytes))
        .collect();
    let packed: Vec<(String, u64)> = zipped
        .iter()
        .map(|(n, b)| (n.clone(), b.len() as u64))
        .collect();
    assert_eq!(listed, packed, "same files, same order, same sizes");
    assert_eq!(
        preview.total_bytes,
        packed.iter().map(|(_, n)| n).sum::<u64>()
    );
    let names: Vec<&str> = listed.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "manifest.json",
            "description.txt",
            "system-info.json",
            "settings.json",
            "crash.json",
            "logs/demo.2026-10-24.log"
        ]
    );
    // The manifest lists every other file with the same sizes and parses with the reader's function.
    let manifest = Manifest::parse(&zipped[0].1).unwrap();
    assert_eq!(
        manifest
            .files
            .iter()
            .map(|f| (f.name.clone(), f.bytes))
            .collect::<Vec<_>>(),
        packed[1..]
    );
    assert_eq!(manifest.app, app_info());
    assert_eq!(manifest.redactions, preview.redactions);
    assert_eq!(manifest.created_at, "2026-10-24T10:30:00.000Z");
}

#[test]
fn nothing_private_survives_in_any_file() {
    let (_root, d) = machine();
    let report =
        Collected::collect(&app_info(), &d, &builtin::all(), &redactor(), now()).assemble("");
    let bytes = report.zip_bytes(&CancelToken::new(), &()).unwrap();
    for (name, data) in entries(&bytes) {
        let text = String::from_utf8_lossy(&data);
        for secret in [
            "alex",
            "ALEX-PC",
            "hunter2hunter2",
            "sk-secret-value",
            "10.1.2.3",
            "\"abc\"",
        ] {
            assert!(!text.contains(secret), "{name} still has {secret}: {text}");
        }
    }
    let p = report.preview();
    assert!(p.redactions.total() >= 6, "{:?}", p.redactions);
    // Every log line is still one valid JSON object (a replacement never breaks the line).
    let log = String::from_utf8(
        entries(&bytes)
            .into_iter()
            .find(|(n, _)| n.starts_with("logs/"))
            .unwrap()
            .1,
    )
    .unwrap();
    for line in log.lines() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.is_object());
    }
    assert!(
        log.contains("<user>") && log.contains("~/Documents/a.log"),
        "{log}"
    );
}

#[test]
fn the_description_is_written_as_typed_and_capped() {
    let (_root, d) = machine();
    let collected = Collected::collect(&app_info(), &d, &builtin::all(), &redactor(), now());
    let bytes = collected
        .assemble("  contact me: me@example.com  ")
        .zip_bytes(&CancelToken::new(), &())
        .unwrap();
    let desc = entries(&bytes)
        .into_iter()
        .find(|(n, _)| n == "description.txt")
        .unwrap()
        .1;
    assert_eq!(
        String::from_utf8(desc).unwrap(),
        "contact me: me@example.com",
        "the user chose to write it, and sees it"
    );
    let long = "é".repeat(MAX_DESCRIPTION);
    let report = collected.assemble(&long);
    let size = report
        .files()
        .iter()
        .find(|f| f.name == DESCRIPTION_NAME)
        .unwrap()
        .data
        .len();
    assert!(size <= MAX_DESCRIPTION + 3, "{size}");
    assert!(
        collected
            .assemble("")
            .files()
            .iter()
            .any(|f| f.summary == "No description")
    );
    assert_eq!(cap_description("short"), "short");
}

#[test]
fn a_log_longer_than_the_window_is_cut_to_it() {
    // Four windows of real lines: the cut is the behaviour here; that the cost does not grow with the file is the seek
    // proven in `window::tests` (a 1 GiB file written for real costs a gigabyte of zeros on NTFS, which is not sparse).
    let (_root, d) = machine();
    let big = d.logs().join("demo.2026-10-23.log");
    let mut f = std::fs::File::create(&big).unwrap();
    let line = b"{\"@m\":\"an older line of the log, long enough to fill the window quickly\"}\n";
    for _ in 0..(4 * 64 * 1024) / line.len() {
        f.write_all(line).unwrap();
    }
    f.write_all(b"{\"@m\":\"the very last line\"}\n").unwrap();
    drop(f);
    let sections: Vec<Arc<dyn Section>> = vec![Arc::new(builtin::Logs::with_window(64 * 1024))];
    let collected = Collected::collect(&app_info(), &d, &sections, &redactor(), now());
    let total: usize = collected.files().iter().map(|f| f.data.len()).sum();
    assert!(total <= 64 * 1024 + 64, "{total}");
    let log = collected
        .files()
        .iter()
        .find(|f| f.name == "logs/demo.2026-10-23.log")
        .map(|f| String::from_utf8_lossy(&f.data).into_owned());
    assert!(log.is_some_and(|l| l.contains("the very last line")));
}

#[test]
fn cancel_leaves_no_partial_zip() {
    let (_root, d) = machine();
    let report =
        Collected::collect(&app_info(), &d, &builtin::all(), &redactor(), now()).assemble("x");
    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("report.zip");
    // Cancelled before it starts.
    let token = CancelToken::new();
    token.cancel();
    assert!(matches!(
        report.save(&dest, &token, &()),
        Err(DiagError::Cancelled)
    ));
    // Cancelled while packing: the counter cancels after the first bytes.
    let token = CancelToken::new();
    let progress = CancelAfter {
        token: token.clone(),
        after: 10,
        seen: AtomicU64::new(0),
    };
    let e = report.save(&dest, &token, &progress).unwrap_err();
    assert!(matches!(e, DiagError::Cancelled), "{e}");
    assert!(e.to_string().contains("nothing was saved"));
    assert_eq!(
        std::fs::read_dir(out.path()).unwrap().count(),
        0,
        "no zip, no temporary file"
    );
    // An existing file is not touched by a cancelled save.
    std::fs::write(&dest, b"old").unwrap();
    let token = CancelToken::new();
    token.cancel();
    let _ = report.save(&dest, &token, &());
    assert_eq!(std::fs::read(&dest).unwrap(), b"old");
}

#[test]
fn saving_writes_the_zip_and_counts_progress() {
    let (_root, d) = machine();
    let report =
        Collected::collect(&app_info(), &d, &builtin::all(), &redactor(), now()).assemble("x");
    let out = tempfile::tempdir().unwrap();
    let progress = AtomicU64::new(0);
    let saved = report
        .save(&out.path().join("r.zip"), &CancelToken::new(), &progress)
        .unwrap();
    assert_eq!(saved.files as usize, report.files().len());
    assert_eq!(
        progress.load(Ordering::SeqCst),
        report.total_bytes(),
        "every byte of content is counted once"
    );
    let on_disk = std::fs::read(out.path().join("r.zip")).unwrap();
    assert_eq!(saved.bytes, on_disk.len() as u64);
    assert_eq!(entries(&on_disk).len(), report.files().len());
    // The same inputs give the same bytes.
    assert_eq!(on_disk, report.zip_bytes(&CancelToken::new(), &()).unwrap());
    // Only one file was created in the folder.
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 1);
}

#[test]
fn destinations_are_validated() {
    let out = tempfile::tempdir().unwrap();
    let ok = out.path().join("a.ZIP");
    assert!(validate_destination(&ok).unwrap().ends_with("a.ZIP"));
    let sub = out.path().join("sub.zip");
    std::fs::create_dir(&sub).unwrap();
    let cases = [
        ("".into(), "Choose where"),
        ("relative.zip".into(), "full path"),
        (out.path().join("a.txt"), ".zip"),
        (out.path().join(".zip"), ".zip"),
        (out.path().join("missing/a.zip"), "does not exist"),
        (out.path().join("x/../a.zip"), ".."),
        (sub, "folder"),
        (out.path().join("a\0.zip"), "Choose where"),
        (out.path().join("a".repeat(2000) + ".zip"), "Choose where"),
    ];
    for (path, want) in cases {
        let e = validate_destination(&path).unwrap_err();
        assert!(
            matches!(e, DiagError::BadDestination(_)) && e.to_string().contains(want),
            "{path:?}: {e}"
        );
    }
    // A destination in a folder that vanished is a typed error from the save too.
    let report =
        Collected::collect(&app_info(), &machine().1, &[], &redactor(), now()).assemble("");
    assert!(
        report
            .save(&out.path().join("gone/r.zip"), &CancelToken::new(), &())
            .is_err()
    );
}

struct Fixed(SectionOutput, &'static str);

impl Section for Fixed {
    fn id(&self) -> &str {
        self.1
    }

    fn collect(&self, _: &Context<'_>) -> SectionOutput {
        self.0.clone()
    }
}

struct Panics;

impl Section for Panics {
    fn id(&self) -> &str {
        "panics"
    }

    fn collect(&self, _: &Context<'_>) -> SectionOutput {
        panic!("a bug in an app section")
    }
}

fn file(name: &str, text: &str) -> SectionFile {
    SectionFile::app(name, "test", Content::Text(text.into()))
}

#[test]
fn a_failing_section_costs_only_itself() {
    let (_root, d) = machine();
    let good = Fixed(
        SectionOutput::one(file("runner/log.txt", "ok token=abc123")),
        "runner",
    );
    let sections: Vec<Arc<dyn Section>> = vec![Arc::new(Panics), Arc::new(good)];
    let c = Collected::collect(&app_info(), &d, &sections, &redactor(), now());
    assert_eq!(c.files().len(), 1);
    assert_eq!(
        String::from_utf8_lossy(&c.files()[0].data),
        "ok token=[redacted]",
        "an app section's text is redacted too"
    );
    assert_eq!(c.notes(), ["The panics section failed and was left out."]);
}

#[test]
fn bad_names_duplicates_and_limits_are_noted_not_fatal() {
    let (_root, d) = machine();
    let mut output = SectionOutput::default();
    for name in [
        "../evil.txt",
        "/abs.txt",
        "a b.txt",
        "manifest.json",
        "description.txt",
        "",
        "ok.txt",
        "ok.txt",
    ] {
        output.files.push(file(name, "x"));
    }
    let sections: Vec<Arc<dyn Section>> = vec![Arc::new(Fixed(output, "odd"))];
    let c = Collected::collect(&app_info(), &d, &sections, &redactor(), now());
    assert_eq!(
        c.files()
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["ok.txt"]
    );
    assert_eq!(c.notes().len(), 7, "{:?}", c.notes());
    assert!(c.notes().iter().any(|n| n.contains("used twice")));
    assert!(
        valid_entry_name("logs/a-b_c.1.log")
            && !valid_entry_name("a//b")
            && !valid_entry_name("./a")
            && !valid_entry_name("naïve.txt")
    );
    assert!(!valid_entry_name(&"x".repeat(MAX_NAME_CHARS + 1)));

    // Too many files: two slots stay for the manifest and the description.
    let many = SectionOutput {
        files: (0..MAX_FILES + 5)
            .map(|i| file(&format!("f{i}.txt"), "x"))
            .collect(),
        notes: vec![],
    };
    let sections: Vec<Arc<dyn Section>> = vec![Arc::new(Fixed(many, "many"))];
    let c = Collected::collect(&app_info(), &d, &sections, &redactor(), now());
    assert_eq!(c.files().len(), MAX_FILES - 2);
    assert_eq!(c.assemble("").files().len(), MAX_FILES);
    assert!(
        c.notes().iter().all(|n| n.contains("at most 64 files")),
        "{:?}",
        c.notes()
    );
}

#[test]
fn a_file_over_the_limit_is_cut_and_the_total_is_bounded() {
    let (_root, d) = machine();
    let limits = Limits {
        max_files: MAX_FILES,
        max_file_bytes: 1000,
        max_total_bytes: 3500,
    };
    let big = "é".repeat(5000);
    let mut output = SectionOutput::default();
    for i in 0..6 {
        output.files.push(file(&format!("big{i}.txt"), &big));
    }
    let sections: Vec<Arc<dyn Section>> = vec![Arc::new(Fixed(output, "big"))];
    let c = Collected::collect_with(&app_info(), &d, &sections, &redactor(), now(), limits);
    let total: usize = c.files().iter().map(|f| f.data.len()).sum();
    assert!(total <= limits.max_total_bytes, "{total}");
    assert_eq!(
        c.files().len(),
        3,
        "three cut files fit; the others are left out and noted"
    );
    assert!(
        c.notes()
            .iter()
            .any(|n| n.contains("big3.txt") && n.contains("KB")),
        "{:?}",
        c.notes()
    );
    let first = String::from_utf8(c.files()[0].data.clone()).unwrap();
    assert!(
        first.ends_with(TRUNCATION_NOTE),
        "cut on a character boundary, and says so"
    );
    assert!(first.len() <= limits.max_file_bytes + TRUNCATION_NOTE.len());
    // The same holds for a log and for structured data handed over too long.
    let log = SectionFile {
        name: "logs/x.log".into(),
        kind: FileKind::Log,
        summary: String::new(),
        content: Content::Log("{\"@m\":\"line\"}\n".repeat(500).into_bytes()),
    };
    let sections: Vec<Arc<dyn Section>> = vec![Arc::new(Fixed(SectionOutput::one(log), "log"))];
    let c = Collected::collect_with(&app_info(), &d, &sections, &redactor(), now(), limits);
    assert!(c.files()[0].data.len() <= limits.max_file_bytes + TRUNCATION_NOTE.len());
    assert_eq!(
        Limits::default(),
        Limits {
            max_files: MAX_FILES,
            max_file_bytes: MAX_FILE_BYTES,
            max_total_bytes: MAX_TOTAL_BYTES
        }
    );
}

#[test]
fn missing_and_damaged_inputs_become_notes() {
    let root = tempfile::tempdir().unwrap();
    let d = dirs(root.path());
    let c = Collected::collect(&app_info(), &d, &builtin::all(), &redactor(), now());
    // Only the system facts exist on a fresh machine; the rest say why they are absent.
    assert_eq!(
        c.files()
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["system-info.json"]
    );
    assert!(c.notes().iter().any(|n| n.contains("no settings file")));
    assert!(c.notes().iter().any(|n| n.contains("no log files")));

    std::fs::create_dir_all(d.config()).unwrap();
    std::fs::create_dir_all(d.data()).unwrap();
    std::fs::write(d.config().join("settings.json"), b"{ not json").unwrap();
    std::fs::write(d.data().join("crash-app.json"), b"garbage").unwrap();
    let c = Collected::collect(&app_info(), &d, &builtin::all(), &redactor(), now());
    assert!(
        c.notes().iter().any(|n| n.contains("not valid JSON")),
        "{:?}",
        c.notes()
    );
    assert!(
        c.notes()
            .iter()
            .any(|n| n.contains("crash marker is damaged"))
    );

    std::fs::write(
        d.config().join("settings.json"),
        vec![b' '; (builtin::MAX_SETTINGS_FILE + 1) as usize],
    )
    .unwrap();
    let c = Collected::collect(&app_info(), &d, &builtin::all(), &redactor(), now());
    assert!(c.notes().iter().any(|n| n.contains("larger than")));
}

#[test]
fn the_crate_has_no_network_dependency() {
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    for banned in [
        "reqwest",
        "ureq",
        "hyper",
        "curl",
        "isahc",
        "surf",
        "attohttpc",
        "paddock-download",
        "paddock-updater",
        "tokio",
        "native-tls",
        "rustls",
    ] {
        assert!(
            !manifest.contains(banned),
            "paddock-diag must not depend on {banned}: a report is never sent"
        );
    }
}

#[test]
fn suggested_names_carry_the_app_and_the_time() {
    assert_eq!(
        suggested_name(&app_info(), now()),
        "demo-diagnostics-20261024-103000.zip"
    );
}

#[test]
fn a_cut_never_splits_a_character() {
    let (_root, d) = machine();
    // 999 bytes: the 1000-byte cut would land inside the 500th "é" (2 bytes each, after one ASCII byte).
    let limits = Limits {
        max_files: MAX_FILES,
        max_file_bytes: 1000,
        max_total_bytes: MAX_TOTAL_BYTES,
    };
    let text = format!("a{}", "é".repeat(2000));
    let sections: Vec<Arc<dyn Section>> = vec![Arc::new(Fixed(
        SectionOutput::one(file("wide.txt", &text)),
        "wide",
    ))];
    let c = Collected::collect_with(&app_info(), &d, &sections, &redactor(), now(), limits);
    let cut = String::from_utf8(c.files()[0].data.clone()).expect("valid UTF-8 after the cut");
    assert!(cut.ends_with(TRUNCATION_NOTE));
    // The description is cut the same way.
    let long = format!("a{}", "é".repeat(MAX_DESCRIPTION));
    let capped = cap_description(&long);
    assert!(
        capped.ends_with('…') && capped.len() <= MAX_DESCRIPTION + 3,
        "{}",
        capped.len()
    );
}

#[test]
fn a_destination_under_a_file_is_refused() {
    let out = tempfile::tempdir().unwrap();
    let plain = out.path().join("plain.txt");
    std::fs::write(&plain, "x").unwrap();
    let e = validate_destination(&plain.join("r.zip")).unwrap_err();
    assert!(
        matches!(e, DiagError::BadDestination(_)) && e.to_string().contains("does not exist"),
        "{e}"
    );
}

#[cfg(unix)]
#[test]
fn a_folder_that_cannot_be_written_is_a_typed_error_and_leaves_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let (_root, d) = machine();
    let report = Collected::collect(&app_info(), &d, &[], &redactor(), now()).assemble("");
    let out = tempfile::tempdir().unwrap();
    std::fs::set_permissions(out.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = report.save(&out.path().join("r.zip"), &CancelToken::new(), &());
    std::fs::set_permissions(out.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    // A process that may write anyway (root in a container) is not a failure of the code under test.
    if let Err(e) = result {
        assert!(
            matches!(
                e,
                DiagError::Io {
                    action: "write",
                    ..
                }
            ),
            "{e}"
        );
        assert!(e.to_string().contains("Choose another folder"));
        assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
    }
}

// Windows reports a path below a file as not found (the settings simply are not there), so the note exists on Unix only.
#[cfg(unix)]
#[test]
fn a_settings_file_that_cannot_be_inspected_is_a_note() {
    let root = tempfile::tempdir().unwrap();
    let d = dirs(root.path());
    // The config folder is a file: looking inside it fails with something other than "not found".
    std::fs::create_dir_all(root.path()).unwrap();
    std::fs::write(d.config(), b"not a folder").unwrap_or_default();
    let c = Collected::collect(&app_info(), &d, &builtin::all(), &redactor(), now());
    assert!(
        c.notes()
            .iter()
            .any(|n| n.contains("settings file cannot be read")),
        "{:?}",
        c.notes()
    );
}

/// An app that names the shell's own file gets that file in the report, masked, and its own `settings.json` (another
/// format, written by another owner) is not read as the shell's.
#[test]
fn a_named_settings_file_is_reported_and_the_default_one_is_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let d = dirs(root.path());
    std::fs::create_dir_all(d.config()).unwrap();
    let foreign = br#"{ "version": 1, "launchAtLogin": true }"#;
    std::fs::write(d.config().join("settings.json"), foreign).unwrap();
    std::fs::write(
        d.config().join("shell-settings.json"),
        json!({"version":1,"sections":{"x":{"apiKey":"sk-secret-value"}}}).to_string(),
    )
    .unwrap();
    let sections: Vec<std::sync::Arc<dyn paddock_diag::Section>> = vec![std::sync::Arc::new(
        builtin::Settings::file("shell-settings.json"),
    )];
    let c = Collected::collect(&app_info(), &d, &sections, &redactor(), now());
    let names: Vec<_> = c.files().iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["shell-settings.json"]);
    assert!(!format!("{:?}", c.files()).contains("sk-secret-value"));
    assert_eq!(
        std::fs::read(d.config().join("settings.json")).unwrap(),
        foreign
    );
}

/// A helper or agent crashes too (`crash-<process>.json`), not only the app: every marker is in the report, and a reported
/// crash is cleared once the user saved the report (it used to stay for ever).
#[test]
fn every_crash_marker_is_reported_once_and_cleared_when_saved() {
    let (_root, d) = machine();
    std::fs::write(
        d.data().join("crash-agent.json"),
        json!({"version":1,"process":"agent","pid":9,"time":"2026-10-23T23:00:00Z","thread":"poller","message":"boom","location":"src/y.rs:2"}).to_string(),
    )
    .unwrap();
    std::fs::write(d.data().join("crash-helper.json"), "not json").unwrap();
    let sections = builtin::all();
    let c = Collected::collect(&app_info(), &d, &sections, &redactor(), now());
    let names: Vec<String> = c
        .assemble("x")
        .preview()
        .files
        .iter()
        .map(|f| f.name.clone())
        .collect();
    assert!(
        names.contains(&"crash.json".to_string())
            && names.contains(&"crash-agent.json".to_string()),
        "{names:?}"
    );
    assert!(
        c.notes()
            .iter()
            .any(|n| n.contains("helper") && n.contains("damaged")),
        "{:?}",
        c.notes()
    );
    let cx = Context {
        app: &app_info(),
        dirs: &d,
        now: now(),
    };
    for s in &sections {
        s.reported(&cx);
    }
    assert!(
        !d.data().join("crash-app.json").exists() && !d.data().join("crash-agent.json").exists()
    );
    let again = Collected::collect(&app_info(), &d, &sections, &redactor(), now());
    assert!(
        !again
            .assemble("x")
            .preview()
            .files
            .iter()
            .any(|f| f.name.starts_with("crash"))
    );
}

/// A log folder that cannot be listed must not say "There are no log files yet".
#[test]
fn an_unreadable_log_folder_is_not_reported_as_empty() {
    let (_root, d) = machine();
    std::fs::remove_dir_all(d.logs()).unwrap();
    std::fs::write(d.logs(), "a file where the folder should be").unwrap();
    let c = Collected::collect(
        &app_info(),
        &d,
        &[Arc::new(builtin::Logs::new())],
        &redactor(),
        now(),
    );
    let notes = c.notes().join(" | ");
    assert!(
        notes.contains("cannot be read") && !notes.contains("no log files yet"),
        "{notes}"
    );
}

/// A VM run or a bug report binds to a build: the build commit is in `system-info.json`.
#[test]
fn the_system_info_names_the_build_commit() {
    let (_root, d) = machine();
    let c = Collected::collect(&app_info(), &d, &builtin::all(), &redactor(), now());
    let info = c
        .files()
        .iter()
        .find(|f| f.name == "system-info.json")
        .expect("system-info.json");
    let v: serde_json::Value = serde_json::from_slice(&info.data).unwrap();
    assert_eq!(v["app"]["commit"], "abc1234", "{v}");
}
