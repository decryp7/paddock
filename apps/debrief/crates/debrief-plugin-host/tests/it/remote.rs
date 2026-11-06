//! A plugin process supervised by the host, against the fake plugin: imports stream back with credits, a crash,
//! a panic or a hang is a failure attributed to the plugin (and quarantines it when it repeats), typed errors are
//! not, settings are restored after a restart, annotators read the events through callbacks, path arguments need
//! a grant, and the normal process refuses the operations of the elevated one.

use debrief_core::guard::Budgets;
use debrief_core::plugin::annotate::AnnotationsRequest;
use debrief_core::plugin::tools::{NoTasks, PathAccess, PathKind, PathPolicy, TaskSink, ToolCtx, ToolError, ToolTask};
use debrief_core::plugin::PluginStatus;
use debrief_core::{PluginRegistry, Profile, Workspace};
use debrief_ipc::IpcError;
use debrief_plugin_api::protocol::PluginRequest;
use debrief_plugin_host::{Builtins, PluginLimits};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ID: &str = "test.fake";

fn builtins() -> Builtins {
    let b = Builtins::load(Path::new(env!("CARGO_BIN_EXE_debrief-plugin-host-fake")));
    assert!(b.problem().is_none(), "{:?}", b.problem());
    b
}

fn registry(b: &Builtins) -> PluginRegistry {
    let mut r = PluginRegistry::new();
    for p in b.plugins() {
        r.install_shared(p).unwrap();
    }
    r.resolve();
    r
}

fn workspace(b: &Builtins) -> Workspace {
    Workspace::with_registry(Profile::default_profile(), registry(b)).unwrap()
}

/// A file whose first line makes the fake importer pick it.
fn file(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, format!("#fake\n{body}\n")).unwrap();
    p
}

fn pid(r: &PluginRegistry) -> u64 {
    r.tools()
        .invoke("fake.tool", "pid", json!({}), &ToolCtx::new(None, Arc::new(NoTasks)))
        .unwrap()
        .as_u64()
        .unwrap()
}

#[test]
fn imports_stream_back_and_annotators_read_through_the_host() {
    let b = builtins();
    let dir = tempfile::tempdir().unwrap();
    let mut ws = workspace(&b);
    ws.open(&[file(dir.path(), "a.log", "hello\nworld")]).unwrap();
    assert_eq!(ws.records().len(), 2);
    assert_eq!(ws.summary().sources[0].importer, "fake");
    // Identity fields and problems cross the contract (ADR 0020).
    assert_eq!(ws.registry().importers().find("fake").unwrap().identity_fields(), ["Host", "App"]);
    let mut ws = workspace(&b);
    ws.open(&[file(
        dir.path(),
        "p.log",
        "one
?bad
?worse
two",
    )])
    .unwrap();
    assert_eq!(ws.records().len(), 2);
    let problems = &ws.summary().sources[0].problems;
    let p = problems
        .iter()
        .find(|p| p.kind == debrief_plugin_api::problems::ProblemKind::LineNotParsed)
        .expect("the plugin's problems");
    assert_eq!((p.count, p.sample_lines.clone()), (2, vec![3, 4]));
    // More records than one item holds, over several credits.
    let mut ws = workspace(&b);
    ws.open(&[file(dir.path(), "many.log", "many 20000")]).unwrap();
    assert_eq!(ws.records().len(), 20_000);
    let notes = ws.annotations(&AnnotationsRequest::default());
    let seen = notes.iter().find(|a| a.kind == "test.fake/seen").expect("the annotator ran");
    assert!(seen.label.starts_with("1000 events"), "{}", seen.label);
}

#[test]
fn crashes_and_panics_count_against_the_plugin_and_restart_its_process() {
    let b = builtins();
    let dir = tempfile::tempdir().unwrap();
    let mut ws = workspace(&b);
    let r_pid = |ws: &Workspace| pid(ws.registry());
    let first = r_pid(&ws);
    // A typed failure is just an error.
    let _ = ws.open(&[file(dir.path(), "fail.log", "fail")]);
    let info = ws.summary().plugins.into_iter().find(|p| p.id == ID).unwrap();
    assert_eq!(info.failures, 0, "{info:?}");
    // A crash fails the import and counts; the next call runs in a fresh process.
    let _ = ws.open(&[file(dir.path(), "crash.log", "crash")]);
    let info = ws.summary().plugins.into_iter().find(|p| p.id == ID).unwrap();
    assert_eq!(info.failures, 1, "{info:?}");
    assert!(info.last_error.unwrap_or_default().contains("crashed"));
    let second = r_pid(&ws);
    assert_ne!(first, second, "restarted");
    // A panic inside the plugin's code is contained in its process, counted, and the process restarted.
    let _ = ws.open(&[file(dir.path(), "panic.log", "panic")]);
    let info = ws.summary().plugins.into_iter().find(|p| p.id == ID).unwrap();
    assert_eq!(info.failures, 2, "{info:?}");
    assert_ne!(r_pid(&ws), second);
    // A third failure quarantines the plugin.
    let _ = ws.open(&[file(dir.path(), "crash2.log", "crash")]);
    let info = ws.summary().plugins.into_iter().find(|p| p.id == ID).unwrap();
    assert_eq!(info.status, PluginStatus::Quarantined, "{info:?}");
}

/// The reasons of the files an import refused, by file name.
fn refused(ws: &Workspace) -> Vec<(String, String)> {
    let file_name = |n: String| Path::new(&n).file_name().map_or(n.clone(), |f| f.to_string_lossy().into_owned());
    ws.summary()
        .unrecognized
        .into_iter()
        .map(|u| (file_name(u.name), u.reason))
        .collect()
}

#[test]
fn a_stop_at_the_memory_cap_is_its_own_outcome_and_never_quarantines() {
    let b = builtins();
    b.set_limits(&PluginLimits {
        default_memory: Some(64 << 20),
        ..PluginLimits::default()
    });
    let dir = tempfile::tempdir().unwrap();
    let mut ws = workspace(&b);
    // Five over-cap files in a row: more than the crash quarantine allows.
    let big: Vec<PathBuf> = (0..5).map(|i| file(dir.path(), &format!("big{i}.log"), "alloc 300")).collect();
    let _ = ws.open(&big);
    let reasons = refused(&ws);
    assert_eq!(reasons.len(), 5, "{reasons:?}");
    for (name, reason) in &reasons {
        assert!(reason.contains("memory cap of 64 MB"), "{name}: names the cap: {reason}");
        assert!(reason.contains(name.as_str()), "{name}: names the file: {reason}");
        assert!(reason.contains("Plugin memory cap"), "{name}: says what to do: {reason}");
        assert!(!reason.contains("crashed"), "{name}: a cap stop is not a crash: {reason}");
        assert!(!reasons.iter().any(|(_, r)| r.contains("binary file")), "{reason}");
    }
    let info = ws.summary().plugins.into_iter().find(|p| p.id == ID).unwrap();
    assert_eq!((info.failures, info.status), (0, PluginStatus::Active), "{info:?}");
    // The next, smaller file still imports.
    let small = ws.open(&[file(dir.path(), "small.log", "hello")]);
    let info = ws.summary().plugins.into_iter().find(|p| p.id == ID).unwrap();
    small.unwrap_or_else(|e| panic!("{e}; plugin {info:?}; status {:?}", b.process(ID).unwrap().status()));
    assert_eq!(ws.records().len(), 1);
    // Real crashes still count and quarantine: the cap stops before them left no mark.
    for i in 0..3 {
        let _ = ws.open(&[file(dir.path(), &format!("crash{i}.log"), "crash")]);
    }
    let info = ws.summary().plugins.into_iter().find(|p| p.id == ID).unwrap();
    assert_eq!(info.status, PluginStatus::Quarantined, "{info:?}");
    // A file refused now says why: the plugin is quarantined.
    let _ = ws.open(&[file(dir.path(), "after.log", "hello")]);
    let after = refused(&ws).into_iter().find(|(n, _)| n == "after.log").expect("refused").1;
    assert!(after.contains("disabled after repeated crashes") && after.contains(ID), "{after}");
    assert!(!after.contains("binary file"), "{after}");
}

#[test]
fn a_file_the_cap_stopped_is_not_tried_again_until_the_cap_changes() {
    let b = builtins();
    let limits = |mb: u64| PluginLimits {
        default_memory: Some(mb << 20),
        ..PluginLimits::default()
    };
    b.set_limits(&limits(64));
    let dir = tempfile::tempdir().unwrap();
    let big = file(dir.path(), "big.log", "alloc 300");
    // A new workspace per import: one workspace never reads a refused file twice.
    let _ = workspace(&b).open(std::slice::from_ref(&big));
    let first = pid(workspace(&b).registry());
    let mut ws = workspace(&b);
    let _ = ws.open(std::slice::from_ref(&big));
    assert!(refused(&ws)[0].1.contains("memory cap of 64 MB"), "{:?}", refused(&ws));
    assert_eq!(
        pid(ws.registry()),
        first,
        "the plugin was not started again for the same file and cap"
    );
    // A raised cap is a new question: the file is tried again (and stopped at the new cap).
    b.set_limits(&limits(96));
    b.process(ID).unwrap().shutdown();
    let mut ws = workspace(&b);
    let _ = ws.open(&[big]);
    assert!(refused(&ws)[0].1.contains("memory cap of 96 MB"), "{:?}", refused(&ws));
}

/// A zip of `(name, bytes)` entries.
fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in entries {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

#[test]
fn a_member_stopped_at_the_cap_does_not_refuse_another_member_of_the_same_archive() {
    let b = builtins();
    b.set_limits(&PluginLimits {
        default_memory: Some(64 << 20),
        ..PluginLimits::default()
    });
    let dir = tempfile::tempdir().unwrap();
    // Both members have 16 bytes; the archive is one path for the plugin.
    let (big, small) = (b"#fake\nalloc 300\n".as_slice(), b"#fake\nhello1234\n".as_slice());
    assert_eq!(big.len(), small.len());
    let zip = dir.path().join("two.zip");
    std::fs::write(&zip, zip_of(&[("big.log", big), ("small.log", small)])).unwrap();
    let mut ws = workspace(&b);
    let _ = ws.open(&[zip]);
    let reasons = refused(&ws);
    assert_eq!(reasons.len(), 1, "only the big member is refused: {reasons:?}");
    assert!(
        reasons[0].0.contains("big.log") && reasons[0].1.contains("memory cap of 64 MB"),
        "{reasons:?}"
    );
    assert_eq!(ws.records().len(), 1, "the small member of the same size imported");
    assert_eq!(ws.records()[0].message.as_str(), "hello1234");
}

#[test]
fn document_and_profile_importers_stopped_at_the_cap_never_quarantine_the_plugin() {
    use debrief_dump::{DumpImporter, ImportOptions};
    use debrief_profile::CaptureImporter;
    let b = builtins();
    b.set_limits(&PluginLimits {
        default_memory: Some(64 << 20),
        ..PluginLimits::default()
    });
    let r = registry(&b);
    let docs = r.contributions::<dyn DumpImporter>(debrief_dump::IMPORTERS_POINT);
    let doc = docs
        .iter()
        .find(|i| i.call(|i| i.id() == "fake.doc").unwrap_or(false))
        .expect("fake document importer");
    let profs = r.contributions::<dyn CaptureImporter>(debrief_profile::IMPORTERS_POINT);
    let prof = profs
        .iter()
        .find(|i| i.call(|i| i.id() == "fake.prof").unwrap_or(false))
        .expect("fake profile importer");
    // More stops in a row than the crash quarantine allows.
    for _ in 0..4 {
        let e = doc
            .call(|i| i.import("a.dmp", Arc::new(b"alloc 300".to_vec()), &ImportOptions::default()))
            .unwrap()
            .expect_err("stopped at the cap");
        assert!(
            e.message.contains("memory cap of 64 MB") && e.message.contains("Plugin memory cap"),
            "{}",
            e.message
        );
        assert!(!e.message.contains("crashed"), "{}", e.message);
        let e = prof
            .call(|i| i.import("a.prof", b"alloc 300", &debrief_profile::Limits::default()))
            .unwrap()
            .expect_err("stopped at the cap");
        let text = e.to_string();
        assert!(text.contains("memory cap of 64 MB") && text.contains("Plugin memory cap"), "{text}");
    }
    let ws = Workspace::with_registry(Profile::default_profile(), r).unwrap();
    let info = ws.summary().plugins.into_iter().find(|p| p.id == ID).unwrap();
    assert_eq!((info.failures, info.status), (0, PluginStatus::Active), "{info:?}");
    // The next file is still read by the same plugin.
    let e = doc
        .call(|i| i.import("b.dmp", Arc::new(b"plain".to_vec()), &ImportOptions::default()))
        .unwrap()
        .expect_err("the fake reader refuses it");
    assert!(e.message.contains("fake document reader"), "{}", e.message);
}

#[test]
fn revoking_a_permission_restarts_the_plugin_with_fewer() {
    use debrief_plugin_api::manifest::Permission;
    let b = builtins();
    let ws = workspace(&b);
    let p = b.process(ID).unwrap();
    assert_eq!(p.granted_now(), [Permission::Network]);
    assert!(p.sandbox().network && p.granted(Permission::Network));
    let before = pid(ws.registry());
    p.set_granted(Vec::new());
    assert!(!p.sandbox().network && !p.granted(Permission::Network));
    let after = pid(ws.registry());
    assert_ne!(before, after, "the process was restarted with the new grants");
    assert!(!p.third_party());
    assert_eq!(
        p.tool_data_dir(Some(Path::new("/app"))).as_deref(),
        Some(Path::new("/app").to_str().unwrap())
    );
}

#[test]
fn a_hang_is_stopped_at_its_deadline() {
    let b = builtins();
    let dir = tempfile::tempdir().unwrap();
    let r = registry(&b);
    r.set_budgets(Budgets {
        import: Some(Duration::from_secs(2)),
        ..Budgets::default()
    });
    let mut ws = Workspace::with_registry(Profile::default_profile(), r).unwrap();
    let started = std::time::Instant::now();
    let _ = ws.open(&[file(dir.path(), "hang.log", "hang")]);
    assert!(started.elapsed() < Duration::from_secs(30), "{:?}", started.elapsed());
    let info = ws.summary().plugins.into_iter().find(|p| p.id == ID).unwrap();
    assert!(info.failures >= 1, "{info:?}");
}

#[test]
fn settings_are_restored_after_a_restart() {
    let b = builtins();
    let r = registry(&b);
    let section = r.plugin_settings().into_iter().find(|s| s.plugin.id == ID).unwrap();
    let valid = debrief_core::settings::validate(&section.configurable.settings_section(), &json!({"level": 7})).unwrap();
    section.apply(&valid).unwrap();
    let ctx = ToolCtx::new(None, Arc::new(NoTasks));
    assert_eq!(r.tools().invoke("fake.tool", "settings", json!({}), &ctx).unwrap(), json!(7));
    // Kill the process; the restore hook applies the settings again in the next one.
    b.process(ID).unwrap().shutdown();
    assert_eq!(r.tools().invoke("fake.tool", "settings", json!({}), &ctx).unwrap(), json!(7));
}

struct Under(PathBuf);

impl PathPolicy for Under {
    fn check(&self, path: &str, _: PathKind, _: PathAccess) -> Result<PathBuf, String> {
        if Path::new(path).starts_with(&self.0) {
            Ok(PathBuf::from(path))
        } else {
            Err("not chosen".into())
        }
    }
}

#[test]
fn path_arguments_need_a_grant_and_elevated_operations_stay_out_of_the_normal_process() {
    let b = builtins();
    let r = registry(&b);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("out.txt");
    let args = json!({"path": target.to_string_lossy()});
    let plain = ToolCtx::new(None, Arc::new(NoTasks));
    assert!(matches!(
        r.tools().invoke("fake.tool", "write", args.clone(), &plain),
        Err(ToolError::PathRefused { .. })
    ));
    assert!(!target.exists());
    // The tool runs locked down (low integrity), so writing a user folder may still be refused by the OS; the
    // host let the granted path through either way.
    let granted = plain.clone().with_paths(Arc::new(Under(dir.path().to_path_buf())));
    match r.tools().invoke("fake.tool", "write", args, &granted) {
        Ok(_) => assert!(target.exists()),
        Err(e) => assert!(!matches!(e, ToolError::PathRefused { .. }), "{e}"),
    }
    // The normal process refuses an elevated operation: only the elevated process serves it.
    let p = b.process(ID).unwrap();
    let req = PluginRequest::InvokeTool {
        tool: "fake.tool".into(),
        op: "admin".into(),
        args: json!({}).into(),
        data_dir: None,
    };
    let refused: Result<Result<debrief_ipc::Json, debrief_plugin_api::protocol::ToolFail>, IpcError> =
        p.call(&req, Some(Duration::from_secs(10)), false);
    match refused {
        Err(IpcError::Remote(e)) => assert!(e.message.contains("elevated process"), "{e:?}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_normal_process_runs_only_declared_elevated_operations_through_the_host() {
    // The elevated role as an ordinary process (the administrator prompt cannot run in tests).
    let exe = Path::new(env!("CARGO_BIN_EXE_debrief-plugin-host-fake"));
    let manifest = debrief_plugin_host::builtins::load_manifests(exe)
        .unwrap()
        .into_iter()
        .find(|m| &*m.id == ID)
        .unwrap();
    let mut spec = debrief_plugin_host::PluginSpec::new(exe, ID);
    spec.elevate = false;
    let process = debrief_plugin_host::PluginProcess::new(manifest, spec);
    let mut r = PluginRegistry::new();
    r.install_shared(Arc::new(debrief_plugin_host::RemotePlugin::new(process.clone())))
        .unwrap();
    r.resolve();
    let data = tempfile::tempdir().unwrap();
    let ctx = ToolCtx::new(Some(data.path().to_path_buf()), Arc::new(NoTasks));
    let normal = pid(&r);
    let out = r.tools().invoke("fake.tool", "via-admin", json!({}), &ctx).unwrap();
    // It ran in the other (elevated) process, with the host's data folder, and cannot elevate further.
    assert_ne!(out["pid"].as_u64().unwrap(), normal, "{out}");
    assert_eq!(out["dataDir"], json!(data.path().to_string_lossy()));
    assert_eq!(out["canElevate"], json!(false));
    // Only operations the manifest declares elevated: the host refuses anything else.
    let e = r
        .tools()
        .invoke("fake.tool", "via-admin", json!({"op": "pid"}), &ctx)
        .unwrap_err()
        .to_string();
    assert!(e.contains("not an elevated operation"), "{e}");
    process.shutdown();
}

/// Task reports with the time they arrived.
#[derive(Default)]
struct Reports(Mutex<Vec<(String, Instant)>>);

impl TaskSink for Reports {
    fn started(&self, tool: &str, task: &ToolTask) {
        self.0.lock().unwrap().push((format!("started {tool} {}", task.id), Instant::now()));
    }
    fn progress(&self, tool: &str, task: &str, fraction: Option<f32>) {
        self.0
            .lock()
            .unwrap()
            .push((format!("progress {tool} {task} {fraction:?}"), Instant::now()));
    }
    fn finished(&self, tool: &str, task: &str) {
        self.0.lock().unwrap().push((format!("finished {tool} {task}"), Instant::now()));
    }
}

#[test]
fn an_elevated_operation_reports_progress_while_it_runs_not_with_its_answer() {
    // The elevated role as an ordinary process (the administrator prompt cannot run in tests).
    let exe = Path::new(env!("CARGO_BIN_EXE_debrief-plugin-host-fake"));
    let manifest = debrief_plugin_host::builtins::load_manifests(exe)
        .unwrap()
        .into_iter()
        .find(|m| &*m.id == ID)
        .unwrap();
    let mut spec = debrief_plugin_host::PluginSpec::new(exe, ID);
    spec.elevate = false;
    let process = debrief_plugin_host::PluginProcess::new(manifest, spec);
    let mut r = PluginRegistry::new();
    r.install_shared(Arc::new(debrief_plugin_host::RemotePlugin::new(process.clone())))
        .unwrap();
    r.resolve();
    let reports = Arc::new(Reports::default());
    let ctx = ToolCtx::new(None, reports.clone());
    for op in [json!({"op": "admin-steps"}), json!(null)] {
        // Through the normal process (`ToolCtx::elevated`), and called directly in the elevated process.
        reports.0.lock().unwrap().clear();
        let out = if op.is_null() {
            r.tools().invoke("fake.tool", "admin-steps", json!({}), &ctx).unwrap()
        } else {
            r.tools().invoke("fake.tool", "via-admin", op, &ctx).unwrap()
        };
        let answered = Instant::now();
        assert_eq!(out, json!("done"));
        let got = reports.0.lock().unwrap().clone();
        let names: Vec<&str> = got.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "started fake.tool steps",
                "progress fake.tool steps Some(0.5)",
                "progress fake.tool steps Some(1.0)",
                "finished fake.tool steps"
            ]
        );
        // The first progress arrived while the operation was still working (it pauses 600 ms before answering).
        let early = answered.duration_since(got[1].1);
        assert!(early >= Duration::from_millis(400), "progress arrived {early:?} before the answer");
    }
    process.shutdown();
}

#[test]
fn a_quarantined_plugin_is_named_only_for_files_it_declares() {
    let b = builtins();
    let dir = tempfile::tempdir().unwrap();
    let mut ws = workspace(&b);
    for i in 0..3 {
        let _ = ws.open(&[file(dir.path(), &format!("crash{i}.log"), "crash")]);
    }
    let info = ws.summary().plugins.into_iter().find(|p| p.id == ID).unwrap();
    assert_eq!(info.status, PluginStatus::Quarantined, "{info:?}");
    // A file of the plugin's extension keeps its reason and says the plugin could not check it.
    let _ = ws.open(&[file(dir.path(), "after.log", "hello")]);
    let after = refused(&ws).into_iter().find(|(n, _)| n == "after.log").expect("refused").1;
    assert!(after.starts_with("no enabled parser reads this file"), "{after}");
    assert!(after.contains("disabled after repeated crashes") && after.contains(ID), "{after}");
    // An unrelated binary file is just a binary file: the plugin did not claim it.
    let other = dir.path().join("blob.bin");
    std::fs::write(&other, [0u8, 159, 146, 150, 0, 1, 2, 3, 255, 254, 0, 0]).unwrap();
    let _ = ws.open(&[other]);
    let reason = refused(&ws).into_iter().find(|(n, _)| n == "blob.bin").expect("refused").1;
    assert_eq!(reason, "binary file", "{reason}");
}

#[test]
fn a_binary_importer_of_a_plugin_imports_and_is_named_by_its_extension_when_quarantined() {
    let b = builtins();
    let dir = tempfile::tempdir().unwrap();
    let mut ws = workspace(&b);
    let fbin = dir.path().join("a.fbin");
    std::fs::write(&fbin, b"FAKEBIN\none\ntwo\n").unwrap();
    ws.open(&[fbin]).unwrap();
    assert_eq!(ws.records().len(), 2, "the plugin's binary importer read the file");
    for i in 0..3 {
        let _ = ws.open(&[file(dir.path(), &format!("crash{i}.log"), "crash")]);
    }
    let info = ws.summary().plugins.into_iter().find(|p| p.id == ID).unwrap();
    assert_eq!(info.status, PluginStatus::Quarantined, "{info:?}");
    // A binary file with the declared extension names the plugin; the same bytes under another name do not.
    let bytes = [0u8, 159, 146, 150, 0, 1, 2, 3, 255, 254, 0, 0];
    let (claimed, other) = (dir.path().join("b.fbin"), dir.path().join("b.xyz"));
    std::fs::write(&claimed, bytes).unwrap();
    std::fs::write(&other, bytes).unwrap();
    let _ = ws.open(&[claimed, other]);
    let reasons = refused(&ws);
    let reason = |n: &str| {
        reasons
            .iter()
            .find(|(f, _)| f == n)
            .unwrap_or_else(|| panic!("{n} refused: {reasons:?}"))
            .1
            .clone()
    };
    assert!(reason("b.fbin").contains("disabled after repeated crashes"), "{}", reason("b.fbin"));
    assert_eq!(reason("b.xyz"), "binary file");
}

#[test]
fn a_crash_of_a_document_or_profile_importer_counts_against_the_plugin() {
    use debrief_dump::{DumpImporter, ImportOptions};
    use debrief_profile::CaptureImporter;
    let b = builtins();
    let r = registry(&b);
    let docs = r.contributions::<dyn DumpImporter>(debrief_dump::IMPORTERS_POINT);
    let doc = docs
        .iter()
        .find(|i| i.call(|i| i.id() == "fake.doc").unwrap_or(false))
        .expect("fake document importer");
    let profs = r.contributions::<dyn CaptureImporter>(debrief_profile::IMPORTERS_POINT);
    let prof = profs
        .iter()
        .find(|i| i.call(|i| i.id() == "fake.prof").unwrap_or(false))
        .expect("fake profile importer");
    let ws = Workspace::with_registry(Profile::default_profile(), r).unwrap();
    let failures = |ws: &Workspace| ws.summary().plugins.into_iter().find(|p| p.id == ID).unwrap().failures;
    assert_eq!(failures(&ws), 0);
    let _ = doc.call(|i| i.import("a.dmp", Arc::new(b"crash".to_vec()), &ImportOptions::default()));
    assert_eq!(failures(&ws), 1, "a document reader that exits is a crash");
    let _ = prof.call(|i| i.import("a.prof", b"crash", &debrief_profile::Limits::default()));
    assert_eq!(failures(&ws), 2, "so is a profile reader that exits");
}
