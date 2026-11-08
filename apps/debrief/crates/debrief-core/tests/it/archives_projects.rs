//! Compressed input, the context view, and saving/restoring workspace state.

use debrief_core::api::{ContextRequest, WorkspaceState};
use debrief_core::project::ProjectFile;
use debrief_core::{LogStore, Profile, Workspace};
use std::io::Write;
use std::path::{Path, PathBuf};

struct Dir(PathBuf);
impl Dir {
    fn new(_tag: &str) -> Dir {
        let p = tempfile::Builder::new().prefix("debrief-test-").tempdir().unwrap().keep();
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Dir(std::fs::canonicalize(p).unwrap())
    }
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, bytes).unwrap();
        p
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn log(machine: &str, n: usize) -> String {
    (0..n)
        .map(|i| format!("2026-10-14 10:00:{i:02}.000 +00:00 [INF] {machine} event {i} {{\"MachineName\": \"{machine}\"}}\n"))
        .collect()
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for (name, data) in files {
        w.start_file(*name, opts).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn names(ws: &Workspace) -> Vec<String> {
    ws.summary().sources.iter().map(|s| s.name.clone()).collect()
}

#[test]
fn reads_gzip_and_zip_transparently() {
    let d = Dir::new("archives");
    d.write("logs/app.log.1.gz", &gzip(log("WEB01", 3).as_bytes()));
    d.write(
        "logs/bundle.zip",
        &zip_of(&[
            ("web02/app.log", log("WEB02", 4).as_bytes()),
            ("worker/w.log", log("WORKER", 2).as_bytes()),
            ("readme.bin", &[0, 1, 2, 3, 0, 0, 0, 1, 2, 3, 4, 5]),
        ]),
    );
    d.write("logs/broken.zip", b"PK\x03\x04not really");
    let mut ws = Workspace::new(Profile::default_profile()).unwrap();
    ws.open(std::slice::from_ref(&d.0)).unwrap();

    assert_eq!(
        names(&ws),
        ["logs/app.log.1.gz", "logs/bundle.zip/web02/app.log", "logs/bundle.zip/worker/w.log"]
    );
    assert_eq!(ws.summary().total, 9);
    let unrecognized: Vec<(String, String)> = ws
        .summary()
        .unrecognized
        .iter()
        .map(|u| (u.name.clone(), u.reason.clone()))
        .collect();
    assert!(
        unrecognized
            .iter()
            .any(|(n, r)| n == "logs/bundle.zip/readme.bin" && r == "binary file"),
        "{unrecognized:?}"
    );
    assert!(
        unrecognized
            .iter()
            .any(|(n, r)| n == "logs/broken.zip" && r.contains("invalid zip")),
        "{unrecognized:?}"
    );

    // Opening again adds nothing; a removed archive entry stays out after reload.
    ws.open(std::slice::from_ref(&d.0)).unwrap();
    assert_eq!(ws.summary().total, 9);
    let worker = ws.summary().sources.iter().find(|s| s.name.ends_with("w.log")).unwrap().id;
    ws.remove_source(worker);
    ws.reload().unwrap();
    assert_eq!(names(&ws), ["logs/app.log.1.gz", "logs/bundle.zip/web02/app.log"]);
}

#[test]
fn context_view_reads_plain_compressed_and_archived_sources() {
    let d = Dir::new("context");
    let mut text = log("WEB01", 30);
    text.push_str("System.Exception: boom\n   at X()\n");
    d.write("plain.log", text.as_bytes());
    d.write("gz.log.gz", &gzip(log("GZ", 5).as_bytes()));
    d.write("z.zip", &zip_of(&[("in/zip.log", log("ZIP", 5).as_bytes())]));
    let mut ws = Workspace::new(Profile::default_profile()).unwrap();
    ws.open(std::slice::from_ref(&d.0)).unwrap();
    let id_of = |ws: &Workspace, m: &str, i: usize| {
        ws.records()
            .iter()
            .find(|r| r.message == format!("{m} event {i}"))
            .unwrap_or_else(|| panic!("{m} {i}"))
            .id
    };

    let c = ws
        .source_context(&ContextRequest {
            record_id: id_of(&ws, "WEB01", 15),
            before: 2,
            after: 3,
        })
        .unwrap();
    assert_eq!(c.source_name, "plain.log");
    assert_eq!(c.line, 16);
    assert_eq!(c.lines.iter().map(|l| l.line).collect::<Vec<_>>(), [14, 15, 16, 17, 18, 19]);
    assert!(c.lines[2].text.contains("WEB01 event 15"));

    let first = ws
        .source_context(&ContextRequest {
            record_id: id_of(&ws, "WEB01", 0),
            before: 10,
            after: 0,
        })
        .unwrap();
    assert_eq!(first.lines.len(), 1);
    let last = ws
        .source_context(&ContextRequest {
            record_id: id_of(&ws, "WEB01", 29),
            before: 0,
            after: 100,
        })
        .unwrap();
    assert_eq!(last.lines.last().unwrap().text, "   at X()");

    let gz = ws
        .source_context(&ContextRequest {
            record_id: id_of(&ws, "GZ", 2),
            before: 1,
            after: 1,
        })
        .unwrap();
    assert_eq!(gz.lines.len(), 3);
    let z = ws
        .source_context(&ContextRequest {
            record_id: id_of(&ws, "ZIP", 4),
            before: 1,
            after: 5,
        })
        .unwrap();
    assert_eq!(z.source_name, "z.zip/in/zip.log");
    assert_eq!(z.lines.len(), 2);

    let req: ContextRequest = serde_json::from_str(r#"{"recordId": 0}"#).unwrap();
    assert_eq!((req.before, req.after), (10, 10));
    assert!(ws
        .source_context(&ContextRequest {
            record_id: 9_999,
            before: 1,
            after: 1
        })
        .is_err());

    // The file changed or vanished since import.
    let plain_id = id_of(&ws, "WEB01", 1);
    std::fs::remove_file(d.0.join("plain.log")).unwrap();
    assert!(ws
        .source_context(&ContextRequest {
            record_id: plain_id,
            before: 1,
            after: 1
        })
        .unwrap_err()
        .to_string()
        .contains("cannot read"));
    let zip_id = id_of(&ws, "ZIP", 1);
    d.write("z.zip", &zip_of(&[("other.log", b"x")]));
    assert!(ws
        .source_context(&ContextRequest {
            record_id: zip_id,
            before: 1,
            after: 1
        })
        .unwrap_err()
        .to_string()
        .contains("no longer in"));
    let gz_id = id_of(&ws, "GZ", 1);
    d.write("gz.log.gz", &gzip(&[0, 1, 2, 3, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6]));
    assert!(ws
        .source_context(&ContextRequest {
            record_id: gz_id,
            before: 1,
            after: 1
        })
        .unwrap_err()
        .to_string()
        .contains("no longer a text file"));
}

#[test]
fn saves_and_restores_projects() {
    let d = Dir::new("project");
    d.write("logs/a.log", log("A", 3).as_bytes());
    d.write("logs/b.log", log("B", 3).as_bytes());
    d.write("solo.log", log("S", 2).as_bytes());
    let mut ws = Workspace::new(Profile::default_profile()).unwrap();
    ws.open(&[d.0.join("logs"), d.0.join("solo.log")]).unwrap();
    let b = ws.summary().sources.iter().find(|s| s.name == "b.log").unwrap().id;
    ws.remove_source(b);

    let state = ws.state();
    assert_eq!(state.roots.len(), 2);
    assert_eq!(state.removed, [d.0.join("logs/b.log").to_string_lossy().into_owned()]);
    let file = d.0.join("incident.debriefproj");
    let project = ProjectFile {
        version: 1,
        name: "Incident".into(),
        sources: state.roots.clone(),
        removed: state.removed.clone(),
        profile: "name = 'Saved profile'".into(),
        ui: serde_json::json!({ "bookmarks": [1, 2] }),
        ..Default::default()
    };
    project.save(&file).unwrap();

    // Restore into a fresh workspace through the store abstraction.
    let loaded = ProjectFile::load(&file).unwrap();
    let mut store: Box<dyn LogStore> = Box::new(Workspace::new(Profile::default_profile()).unwrap());
    let profile = Profile::from_toml(&loaded.profile).unwrap();
    let missing = store
        .restore(
            profile,
            &WorkspaceState {
                roots: loaded.sources.clone(),
                removed: loaded.removed.clone(),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(missing.is_empty());
    let s = store.summary();
    assert_eq!(s.profile_name, "Saved profile");
    assert_eq!(s.total, 5, "a.log + solo.log, b.log stays removed");
    assert_eq!(
        store.state(),
        WorkspaceState {
            roots: loaded.sources.clone(),
            removed: loaded.removed.clone(),
            ..Default::default()
        }
    );

    // Missing paths are reported, not fatal; an invalid profile is.
    std::fs::remove_file(d.0.join("solo.log")).unwrap();
    let missing = store
        .restore(
            Profile::default_profile(),
            &WorkspaceState {
                roots: loaded.sources.clone(),
                removed: vec![],
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(missing, [d.0.join("solo.log").to_string_lossy().into_owned()]);
    assert_eq!(store.summary().total, 6);
    let bad = Profile::from_toml("[[extract]]\nname = 'x'\nregex = '('").unwrap();
    assert!(store.restore(bad, &WorkspaceState::default()).is_err());
    assert!(Path::new(&file).exists());
}

#[test]
fn refreshes_changed_new_and_deleted_files() {
    let d = Dir::new("refresh");
    let app = d.write("logs/app.log", log("A", 2).as_bytes());
    d.write("logs/skip.gz.tmp", b"x");
    d.write("solo.log", log("S", 1).as_bytes());
    let outside = Dir::new("refresh-outside");
    let stranger = outside.write("x.log", log("X", 1).as_bytes());
    let mut ws = Workspace::new(Profile::default_profile()).unwrap();
    ws.open(&[d.0.join("logs"), d.0.join("solo.log")]).unwrap();
    assert_eq!(ws.summary().total, 3);

    // Appended lines, a rotated file, a file outside the workspace, a directory.
    std::fs::write(&app, log("A", 5)).unwrap();
    let rotated = d.write("logs/app.log.1", log("R", 3).as_bytes());
    let n = ws
        .refresh_files(&[app.clone(), rotated.clone(), stranger, d.0.join("logs")])
        .unwrap();
    assert_eq!(n, 2);
    assert_eq!(ws.summary().total, 5 + 3 + 1);
    assert_eq!(ws.summary().sources.len(), 3);

    // Removed sources stay out; excluded names are ignored. Deleted files keep
    // their history (live tail no longer drops what was already imported;
    // `TailOptions::deleted_files = Drop` restores the old behaviour).
    let rid = ws.summary().sources.iter().find(|s| s.name == "app.log.1").unwrap().id;
    ws.remove_source(rid);
    assert_eq!(ws.refresh_files(std::slice::from_ref(&rotated)).unwrap(), 0);
    std::fs::remove_file(&app).unwrap();
    let hidden = d.write("logs/.hidden.log", log("H", 1).as_bytes());
    ws.refresh_files(&[app.clone(), hidden.clone()]).unwrap();
    let left: Vec<String> = ws.summary().sources.iter().map(|s| s.name.clone()).collect();
    assert_eq!(left.len(), 2, "{left:?}");
    assert_eq!(ws.summary().total, 5 + 1);

    // An explicitly opened file refreshes too.
    std::fs::write(d.0.join("solo.log"), log("S", 4)).unwrap();
    let mut store: Box<dyn LogStore> = Box::new(ws);
    assert_eq!(store.refresh_files(&[d.0.join("solo.log")]).unwrap(), 1);
    assert_eq!(store.summary().total, 5 + 4);
}

#[test]
fn imports_report_progress_and_can_be_cancelled() {
    let d = Dir::new("progress");
    for i in 0..20 {
        d.write(&format!("f{i:02}.log"), log("P", 2).as_bytes());
    }
    let mut ws = Workspace::new(Profile::default_profile()).unwrap();
    let progress = ws.progress();
    assert!(!progress.snapshot().running);
    ws.open(std::slice::from_ref(&d.0)).unwrap();
    let snap = progress.snapshot();
    assert_eq!((snap.running, snap.done, snap.total, snap.cancelled), (false, 20, 20, false));
    assert!(snap.bytes_total > 0 && snap.bytes_done == snap.bytes_total);
    assert_eq!((snap.current, snap.eta_ms), (None, None), "nothing in flight after the import");

    // Cancel while importing: an importer pulls the brake on the first file it reads.
    struct Brake(std::sync::Mutex<Option<std::sync::Arc<debrief_core::workspace::ImportProgress>>>);
    impl debrief_core::importers::Importer for Brake {
        fn id(&self) -> &'static str {
            "brake"
        }
        fn sniff(&self, _: &debrief_core::importers::ImportCtx, head: &str) -> u8 {
            if head.starts_with("BRAKE") {
                100
            } else {
                0
            }
        }
        fn import(&self, _: &debrief_core::importers::ImportCtx, _: &str) -> anyhow::Result<debrief_core::importers::Imported> {
            if let Some(p) = self.0.lock().unwrap().as_ref() {
                p.cancel();
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            let mut r = debrief_core::model::Record::new(1);
            r.message = "x".into();
            Ok(debrief_core::importers::Imported {
                records: vec![r],
                skipped: 0,
                ..Default::default()
            })
        }
    }
    let brake = std::sync::Arc::new(Brake(std::sync::Mutex::new(None)));
    let mut importers = debrief_core::importers::Importers::new();
    struct Shared(std::sync::Arc<Brake>);
    impl debrief_core::importers::Importer for Shared {
        fn id(&self) -> &str {
            self.0.id()
        }
        fn sniff(&self, c: &debrief_core::importers::ImportCtx, h: &str) -> u8 {
            self.0.sniff(c, h)
        }
        fn import(&self, c: &debrief_core::importers::ImportCtx, t: &str) -> anyhow::Result<debrief_core::importers::Imported> {
            self.0.import(c, t)
        }
    }
    importers.register(Box::new(Shared(brake.clone())));
    let mut ws = Workspace::with_importers(Profile::default_profile(), importers).unwrap();
    *brake.0.lock().unwrap() = Some(ws.progress());
    let d2 = Dir::new("progress2");
    for i in 0..200 {
        d2.write(&format!("g{i:03}.log"), b"BRAKE\n");
    }
    ws.open(std::slice::from_ref(&d2.0)).unwrap();
    let snap = ws.progress().snapshot();
    assert!(snap.cancelled && !snap.running && snap.done == 200);
    let imported = ws.summary().total;
    assert!((1..200).contains(&imported), "cancel stopped the import early ({imported} of 200)");
    // The files not read are listed, so the user sees what is missing.
    let not_read = ws
        .summary()
        .unrecognized
        .iter()
        .filter(|u| u.reason == debrief_core::workspace::NOT_IMPORTED)
        .count();
    assert_eq!(not_read, 200 - imported);
    // Opening again imports the files that were skipped.
    *brake.0.lock().unwrap() = None;
    ws.open(std::slice::from_ref(&d2.0)).unwrap();
    assert_eq!(ws.summary().total, 200);
    assert!(ws.summary().unrecognized.is_empty());
}

// ---------------------------------------------------------------------------
// Repeated names in a zip (the user's decision of 2026-10-29): every entry is kept; the second of a full path becomes
// `<stem> (2)<.ext>` in the same folder, then `(3)` and so on, never the name of a real entry.
// ---------------------------------------------------------------------------

fn entries_of(files: &[(&str, &[u8])]) -> Vec<(String, Vec<u8>)> {
    let files: Vec<(String, Vec<u8>)> = files.iter().map(|(n, b)| (n.to_string(), b.to_vec())).collect();
    debrief_core::archive::expand("dup.zip", crate::common::stored_zip(&files))
        .expect("the archive expands")
        .into_iter()
        .map(|e| (e.name.unwrap(), e.bytes))
        .collect()
}

#[test]
fn two_entries_with_one_path_are_both_kept_with_their_contents() {
    let got = entries_of(&[("logs/app.log", b"first"), ("logs/app.log", b"second")]);
    assert_eq!(
        got,
        [
            ("logs/app.log".to_string(), b"first".to_vec()),
            ("logs/app (2).log".to_string(), b"second".to_vec())
        ]
    );
}

#[test]
fn a_third_entry_with_the_same_path_is_numbered_three() {
    let got = entries_of(&[("x.log", b"1"), ("x.log", b"2"), ("x.log", b"3"), ("noext", b"a"), ("noext", b"b")]);
    let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["x.log", "x (2).log", "x (3).log", "noext", "noext (2)"]);
    assert_eq!(got[2].1, b"3");
}

#[test]
fn a_generated_name_never_takes_the_name_of_a_real_entry() {
    // The real `x (2).log` comes before or after the repeated `x.log`: either way the copy becomes `x (3).log`.
    for files in [
        vec![("x.log", &b"real"[..]), ("x (2).log", &b"own"[..]), ("x.log", &b"copy"[..])],
        vec![("x.log", &b"real"[..]), ("x.log", &b"copy"[..]), ("x (2).log", &b"own"[..])],
    ] {
        let got = entries_of(&files);
        let find = |n: &str| got.iter().find(|(name, _)| name == n).map(|(_, b)| b.clone());
        assert_eq!(got.len(), 3, "{got:?}");
        assert_eq!(find("x.log").as_deref(), Some(&b"real"[..]));
        assert_eq!(find("x (2).log").as_deref(), Some(&b"own"[..]));
        assert_eq!(find("x (3).log").as_deref(), Some(&b"copy"[..]));
    }
}

#[test]
fn a_renamed_entry_is_read_again_by_its_new_name() {
    let z = crate::common::stored_zip(&[("a.log".into(), b"one".to_vec()), ("a.log".into(), b"two".to_vec())]);
    assert_eq!(debrief_core::archive::read_entry(z.clone(), "dup.zip", "a.log").unwrap(), b"one");
    assert_eq!(debrief_core::archive::read_entry(z, "dup.zip", "a (2).log").unwrap(), b"two");
}

/// Regression (review of c9f283344): entries written with a data descriptor (flag bit 3) hold no CRC-32 or size in their
/// local header, and the repeated-path reader took the CRC from there, so the whole archive failed with "Invalid
/// checksum". The central directory's values are the ones to use.
#[test]
fn repeated_paths_written_with_data_descriptors_are_all_kept() {
    let files = [
        ("logs/app.log".to_string(), b"first".to_vec()),
        ("logs/app.log".to_string(), b"second one".to_vec()),
        ("logs/app.log".to_string(), Vec::new()),
    ];
    let z = crate::common::stored_zip_flagged(&files, true);
    let got: Vec<(String, Vec<u8>)> = debrief_core::archive::expand("dup.zip", z)
        .expect("the archive expands")
        .into_iter()
        .map(|e| (e.name.unwrap(), e.bytes))
        .collect();
    assert_eq!(
        got,
        [
            ("logs/app.log".to_string(), b"first".to_vec()),
            ("logs/app (2).log".to_string(), b"second one".to_vec()),
            ("logs/app (3).log".to_string(), Vec::new())
        ]
    );
}

/// Regression (review of c9f283344): renaming probed `(2)`, `(3)`, ... from 2 for every copy, so n copies of one path took
/// n squared steps (40,000 copies took a minute). The count is the assertion; the run must also end.
#[test]
fn many_copies_of_one_path_are_all_kept_under_distinct_names() {
    let files: Vec<(String, Vec<u8>)> = (0..2_000u32)
        .map(|i| ("logs/app.log".to_string(), i.to_le_bytes().to_vec()))
        .collect();
    let entries = debrief_core::archive::expand("dup.zip", crate::common::stored_zip(&files)).expect("the archive expands");
    assert_eq!(entries.len(), files.len());
    let names: std::collections::HashSet<_> = entries.iter().map(|e| e.name.clone().unwrap()).collect();
    assert_eq!(names.len(), files.len());
    assert_eq!(entries.iter().filter(|e| e.renamed_from.is_some()).count(), files.len() - 1);
    assert_eq!(entries[1_999].name.as_deref(), Some("logs/app (2000).log"));
    assert_eq!(entries[1_999].bytes, 1_999u32.to_le_bytes());
}

/// Regression (review round 2): a repeated UTF-8 name in an archive that does not set the UTF-8 flag keeps its letters.
#[test]
fn a_repeated_utf8_name_without_the_flag_keeps_its_letters() {
    let got = entries_of(&[("caf\u{e9}.log", b"one"), ("caf\u{e9}.log", b"two")]);
    let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["caf\u{e9}.log", "caf\u{e9} (2).log"]);
}

/// More entries than `MAX_ENTRIES` is refused with a message, on the normal path and the repeated-path one.
#[test]
fn an_archive_with_too_many_entries_is_refused() {
    let n = debrief_core::archive::MAX_ENTRIES + 1;
    let distinct: Vec<(String, Vec<u8>)> = (0..n).map(|i| (format!("f{i}.log"), Vec::new())).collect();
    let same: Vec<(String, Vec<u8>)> = (0..n).map(|_| ("f.log".to_string(), Vec::new())).collect();
    for files in [distinct, same] {
        let e = debrief_core::archive::expand("big.zip", crate::common::stored_zip(&files))
            .err()
            .expect("refused");
        assert!(format!("{e:#}").contains("entries"), "{e:#}");
    }
}

/// `d/a.log` and `d\a.log` are one path once `\` is read as `/`: the second becomes `d/a (2).log` whether or not the
/// archive repeats a raw name.
#[test]
fn a_backslash_twin_of_a_path_is_renamed() {
    let got = entries_of(&[("d/a.log", b"one"), ("d\\a.log", b"two")]);
    assert_eq!(
        got,
        [
            ("d/a.log".to_string(), b"one".to_vec()),
            ("d/a (2).log".to_string(), b"two".to_vec())
        ]
    );
}

/// Regression (review of c9f283344): the output name came from the local header, so a local name equal to a generated
/// one could collide. The central directory's name is the entry's name.
#[test]
fn the_central_directory_name_is_the_entrys_name() {
    let mut z = crate::common::stored_zip(&[
        ("a.log".to_string(), b"one".to_vec()),
        ("a.log".to_string(), b"two".to_vec()),
        ("abcdefghi".to_string(), b"three".to_vec()),
    ]);
    let at = z.windows(9).position(|w| w == b"abcdefghi").expect("the local header comes first");
    z[at..at + 9].copy_from_slice(b"a (2).log");
    let entries = debrief_core::archive::expand("dup.zip", z).expect("the archive expands");
    let got: Vec<(&str, &[u8])> = entries.iter().map(|e| (e.name.as_deref().unwrap(), e.bytes.as_slice())).collect();
    assert_eq!(got, [("a.log", &b"one"[..]), ("a (2).log", b"two"), ("abcdefghi", b"three")]);
}

/// The fuzz corpus' duplicate seeds (stored, streamed deflate with data descriptors, zip64 sizes) all expand to the same
/// two entries.
#[test]
fn the_fuzz_corpus_duplicate_seeds_expand_to_both_entries() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/fuzz_archive");
    for seed in ["dup-stored.zip", "dup-bit3-deflate.zip", "dup-zip64.zip"] {
        let bytes = std::fs::read(dir.join(seed)).unwrap_or_else(|e| panic!("{seed}: {e}"));
        let got = debrief_core::archive::expand(seed, bytes).unwrap_or_else(|e| panic!("{seed}: {e:#}"));
        let names: Vec<_> = got.iter().map(|e| e.name.as_deref().unwrap()).collect();
        assert_eq!(names, ["logs/a.log", "logs/a (2).log"], "{seed}");
        assert_eq!(got[1].bytes, b"second line\n".repeat(5), "{seed}");
    }
}
