//! A fake plugin for the host's supervision tests (never bundled): its importer misbehaves on demand.
//!
//! The text importer `fake` reads the file's first line: `crash` exits at once, `hang` never answers, `panic`
//! panics, `alloc <mb>` holds that many MB (touched) until the host stops it, `many <n>` returns n records, anything else one record per line. The tool `fake.tool` has `settings`
//! (the last applied value of `level`), `echo` (its arguments) and `pid` (its process id). The annotator
//! `fake.notes` marks the time range of the records it selects through the host's view.

use anyhow::{bail, Result};
use debrief_plugin_api::annotate::{Annotation, AnnotationView, Annotator, Evidence, RecordCursor, Severity, ViewFilter};
use debrief_plugin_api::importer::{ImportCtx, Imported, TextImporter};
use debrief_plugin_api::manifest::Manifest;
use debrief_plugin_api::model::Record;
use debrief_plugin_api::problems::{ProblemKind, Problems};
use debrief_plugin_api::settings::{Configurable, FieldKind, SectionOwner, SettingField, SettingsSection, ValidValues};
use debrief_plugin_api::tools::{OpSpec, PathAccess, PathKind, Schema, Tool, ToolCtx, ToolInfo, ToolTask};
use debrief_plugin_api::{Contributions, Plugin};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

static LEVEL: AtomicI64 = AtomicI64::new(-1);

struct Fake;

impl TextImporter for Fake {
    fn id(&self) -> &str {
        "fake"
    }

    fn identity_fields(&self) -> Vec<String> {
        vec!["Host".into(), "App".into()]
    }

    fn extensions(&self) -> Vec<String> {
        vec!["log".into()]
    }

    fn sniff(&self, _: &ImportCtx, head: &str) -> u8 {
        if head.starts_with("#fake") {
            100
        } else {
            0
        }
    }

    fn import(&self, _: &ImportCtx, content: &str) -> Result<Imported> {
        let body: Vec<&str> = content.lines().skip(1).collect();
        match body.first().copied().unwrap_or("") {
            "crash" => std::process::exit(9),
            "hang" => loop {
                std::thread::sleep(std::time::Duration::from_secs(3600));
            },
            "panic" => panic!("the fake importer panicked"),
            "fail" => bail!("a typed failure"),
            l if l.starts_with("alloc ") => {
                let mb: usize = l[6..].parse().unwrap_or(0);
                alloc_and_wait(mb)
            }
            l if l.starts_with("many ") => {
                let n: usize = l[5..].parse().unwrap_or(0);
                Ok(Imported {
                    records: (0..n)
                        .map(|i| {
                            let mut r = Record::new(i as u32 + 1);
                            r.message = format!("event {i}");
                            r.ts = Some(1_700_000_000_000 + i as i64);
                            r
                        })
                        .collect(),
                    skipped: 0,
                    ..Default::default()
                })
            }
            _ => {
                // Lines starting with `?` are problems, not events.
                let mut problems = Problems::default();
                let records = body
                    .iter()
                    .enumerate()
                    .filter_map(|(i, l)| {
                        if l.starts_with('?') {
                            problems.add(ProblemKind::LineNotParsed, i as u32 + 2);
                            return None;
                        }
                        let mut r = Record::new(i as u32 + 2);
                        r.message = l.to_string();
                        Some(r)
                    })
                    .collect();
                Ok(Imported {
                    records,
                    skipped: 0,
                    problems,
                })
            }
        }
    }
}

/// Parks this thread until the host stops the process (it never returns); `held` stays alive meanwhile.
fn hold_until_stopped(held: &[u8]) -> ! {
    loop {
        std::thread::park();
        std::hint::black_box(held);
    }
}

/// Touches `mb` megabytes so they count as used, then waits for the host's watcher to stop the process.
fn alloc_and_wait(mb: usize) -> ! {
    let mut held = vec![0u8; mb << 20];
    for i in (0..held.len()).step_by(4096) {
        held[i] = 1;
    }
    hold_until_stopped(&held)
}

/// The document importer `fake.doc`: a file starting with `alloc <mb>` holds that much memory until stopped; any
/// other file is answered with a typed error (the process is fine).
struct FakeDoc;

impl debrief_plugin_api::DocumentImporter for FakeDoc {
    fn id(&self) -> &str {
        "fake.doc"
    }

    fn produces(&self) -> &str {
        "dump"
    }

    fn sniff(&self, _: &str, _: &[u8]) -> u8 {
        0
    }

    fn import_encoded(&self, _: &str, file: &dyn debrief_plugin_api::host_file::RangeRead, _: &Value) -> Vec<u8> {
        let head = file.read_at(0, 64).unwrap_or_default();
        let text = String::from_utf8_lossy(&head);
        if let Some(mb) = text
            .strip_prefix("alloc ")
            .and_then(|r| r.split_whitespace().next())
            .and_then(|n| n.parse().ok())
        {
            alloc_and_wait(mb);
        }
        debrief_dump::wire::encode_result(&Err(debrief_dump::DumpError::invalid("the fake document reader refuses this file")))
    }
}

/// The profile importer `fake.prof`: like [`FakeDoc`], over the whole file.
struct FakeProf;

impl debrief_plugin_api::ProfileImporter for FakeProf {
    fn id(&self) -> &str {
        "fake.prof"
    }

    fn heap(&self) -> bool {
        false
    }

    fn sniff(&self, _: &str, _: &[u8]) -> u8 {
        0
    }

    fn import_encoded(&self, _: &str, bytes: &[u8], _: &Value) -> Vec<u8> {
        let text = String::from_utf8_lossy(&bytes[..bytes.len().min(64)]);
        if let Some(mb) = text
            .strip_prefix("alloc ")
            .and_then(|r| r.split_whitespace().next())
            .and_then(|n| n.parse().ok())
        {
            alloc_and_wait(mb);
        }
        debrief_profile::wire::encode_result(&Err(debrief_profile::ProfileError::invalid(
            "the fake profile reader refuses this file",
        )))
    }
}

struct FakeTool;

impl Tool for FakeTool {
    fn id(&self) -> &str {
        "fake.tool"
    }

    fn describe(&self) -> ToolInfo {
        ToolInfo::new("fake.tool", "Fake")
            .op(OpSpec::new("settings", "The applied level"))
            .op(OpSpec::new("echo", "Echo").args(Schema::object().open()))
            .op(OpSpec::new("pid", "Process id"))
            .op(OpSpec::new("write", "Write a file").args(Schema::object().field("path", Schema::path(PathKind::File, PathAccess::Write))))
            .op(OpSpec::new("admin", "Needs administrator rights").elevated())
            .op(OpSpec::new("admin-steps", "Reports progress in the elevated process").elevated())
            .op(OpSpec::new("via-admin", "Runs an operation in the elevated process").args(Schema::object().open()))
            .op(OpSpec::new("admin-save", "Streams an output file to the host")
                .elevated()
                .args(Schema::object().open()))
            .op(OpSpec::new("admin-save-crash", "Exits in the middle of an output file")
                .elevated()
                .args(Schema::object().open()))
            .op(OpSpec::new("admin-log", "Writes lines to standard error")
                .elevated()
                .args(Schema::object().open()))
    }

    fn invoke(&self, op: &str, args: Value, ctx: &ToolCtx) -> Result<Value> {
        match op {
            "settings" => Ok(json!(LEVEL.load(Ordering::SeqCst))),
            "echo" => Ok(args),
            "pid" => Ok(json!(std::process::id())),
            "write" => {
                let path = args["path"].as_str().unwrap_or_default();
                std::fs::write(path, b"x")?;
                Ok(json!("written"))
            }
            "admin" => Ok(json!({
                "pid": std::process::id(),
                "dataDir": ctx.data_dir().map(|d| d.to_string_lossy().into_owned()),
                "canElevate": ctx.elevated().is_some(),
            })),
            "admin-steps" => {
                // Progress while it works, then a pause before the answer: the host must see it before the reply.
                ctx.task_started(ToolTask {
                    id: "steps".into(),
                    label: "Steps".into(),
                    kind: "fake".into(),
                    stoppable: false,
                });
                ctx.task_progress("steps", Some(0.5));
                std::thread::sleep(std::time::Duration::from_millis(600));
                ctx.task_progress("steps", Some(1.0));
                ctx.task_finished("steps");
                Ok(json!("done"))
            }
            "admin-save" | "admin-save-crash" => {
                // `size` bytes of a pattern, written in odd-sized pieces, into `folder`/`name` through the host.
                let outputs = ctx.outputs().ok_or_else(|| anyhow::anyhow!("no output streams here"))?;
                let text = |k: &str, d: &str| args[k].as_str().unwrap_or(d).to_string();
                let size = args["size"].as_u64().unwrap_or(0) as usize;
                let max = args["max"].as_u64().unwrap_or(u64::MAX);
                let mut f = outputs.create(&text("folder", "fake"), &text("name", "out.bin"), max)?;
                let bytes: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
                let half = if op == "admin-save-crash" { size / 2 } else { size };
                for piece in bytes[..half].chunks(70_001) {
                    std::io::Write::write_all(&mut f, piece)?;
                }
                if op == "admin-save-crash" {
                    std::io::Write::flush(&mut f)?;
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    std::process::exit(7);
                }
                let saved = f.finish()?;
                Ok(json!({ "name": saved.name, "bytes": saved.bytes }))
            }
            "admin-log" => {
                let marker = args["marker"].as_str().unwrap_or("log");
                for i in 0..args["lines"].as_u64().unwrap_or(1) {
                    eprintln!("{marker} line {i}");
                }
                if let Some(n) = args["long"].as_u64() {
                    eprintln!("{marker} long {}", "x".repeat(n as usize));
                }
                Ok(json!("logged"))
            }
            "via-admin" => {
                let ops = ctx.elevated().ok_or_else(|| anyhow::anyhow!("no elevated process"))?;
                let target = args["op"].as_str().unwrap_or("admin");
                ops.invoke("fake.tool", target, json!({}), std::time::Duration::from_secs(10))
            }
            other => bail!("no op {other}"),
        }
    }
}

struct FakeSettings;

impl Configurable for FakeSettings {
    fn settings_section(&self) -> SettingsSection {
        SettingsSection::new("fake", "Fake", SectionOwner::Core).field(SettingField::new(
            "level",
            "Level",
            FieldKind::Int {
                min: 0,
                max: 10,
                step: 1,
                unit: None,
            },
            0,
        ))
    }

    fn apply_settings(&self, values: &ValidValues) -> Result<()> {
        LEVEL.store(values.int("level").unwrap_or(-1), Ordering::SeqCst);
        Ok(())
    }
}

struct Notes;

impl Annotator for Notes {
    fn id(&self) -> &str {
        "fake.notes"
    }

    fn annotate(&self, view: &dyn AnnotationView, _since: Option<RecordCursor>) -> Result<Vec<Annotation>> {
        let rows = view.select(&ViewFilter::default(), 1000)?;
        let times: Vec<i64> = rows.iter().filter_map(|r| r.record.ts).collect();
        let (Some(from), Some(to)) = (times.iter().min(), times.iter().max()) else {
            return Ok(Vec::new());
        };
        Ok(vec![Annotation {
            kind: "seen".into(),
            from: *from,
            to: *to,
            instance: None,
            on: None,
            label: format!("{} events, {} instances", rows.len(), view.instances().len()),
            severity: Severity::Info,
            evidence: Evidence::default(),
        }])
    }
}

struct FakePlugin;

impl Plugin for FakePlugin {
    fn id(&self) -> &str {
        "test.fake"
    }

    fn name(&self) -> &str {
        "Fake"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn permissions(&self) -> Vec<debrief_plugin_api::manifest::PermissionRequest> {
        vec![debrief_plugin_api::manifest::PermissionRequest {
            permission: debrief_plugin_api::manifest::Permission::Network,
            reason: "Tests revoking a permission.".into(),
        }]
    }

    fn register(&self, c: &mut Contributions) -> Result<()> {
        c.add_importer(Fake)
            .add_document_importer(FakeDoc)
            .add_profile_importer(FakeProf)
            .add_tool(Arc::new(FakeTool))
            .add_annotator(Notes)
            .set_settings(Arc::new(FakeSettings));
        Ok(())
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--manifests") {
        let mut c = Contributions::default();
        let _ = FakePlugin.register(&mut c);
        let m: Manifest = c.manifest(&FakePlugin);
        println!("{}", serde_json::to_string(&vec![m]).unwrap_or_default());
        return;
    }
    if let Err(e) = debrief_plugin_api::serve(Arc::new(FakePlugin)) {
        eprintln!("fake plugin: {e}");
        std::process::exit(1);
    }
}
