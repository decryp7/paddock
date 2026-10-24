//! The runtime store against an in-memory host: no network, temp folders only.

use paddock_download::{Downloader, Get, Headers, Response, Transport, TransportError};
use paddock_jobs::time::FakeClock;
use paddock_jobs::{EventSink, JobContext, JobRegistry, JobSnapshot, JobState};
use paddock_runtime::{
    ArchiveOptions, Catalogue, Installed, MARKER, Pinned, RuntimeError, RuntimeKind, RuntimeStore,
    STATE_FILE, Status,
};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Serves whole files by URL (no ranges), counting requests.
#[derive(Default)]
struct Host {
    files: Mutex<HashMap<String, Vec<u8>>>,
    gets: AtomicUsize,
}

impl Transport for Host {
    fn get(&self, g: &Get<'_>) -> Result<Response, TransportError> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        let body = self.files.lock().unwrap().get(g.url).cloned();
        Ok(match body {
            Some(b) => Response {
                status: 200,
                headers: Headers {
                    content_length: Some(b.len().to_string()),
                    ..Headers::default()
                },
                body: Box::new(std::io::Cursor::new(b)),
            },
            None => Response {
                status: 404,
                headers: Headers::default(),
                body: Box::new(std::io::empty()),
            },
        })
    }
}

#[derive(Default)]
struct Phases(Mutex<Vec<String>>);

impl EventSink for Phases {
    fn emit(&self, s: &JobSnapshot) {
        if let JobState::Running { phase: Some(p) } = &s.state {
            let mut v = self.0.lock().unwrap();
            if v.last() != Some(&p.phase.id) {
                v.push(p.phase.id.clone());
            }
        }
    }
}

/// Finds `bin/tool` and passes unless the tool's text is "broken".
struct Tool;

impl RuntimeKind for Tool {
    fn locate(&self, root: &Path) -> Option<PathBuf> {
        let rel = PathBuf::from("bin").join("tool");
        root.join(&rel).is_file().then_some(rel)
    }

    fn self_test(&self, exe: &Path, version: &str) -> Result<String, String> {
        match std::fs::read_to_string(exe)
            .map_err(|e| e.to_string())?
            .as_str()
        {
            "broken" => Err("it crashed".into()),
            text => Ok(format!("{text} {version}")),
        }
    }
}

/// A kind whose executable is a path outside the tree.
struct Outside;

impl RuntimeKind for Outside {
    fn locate(&self, _: &Path) -> Option<PathBuf> {
        Some(PathBuf::from("../x"))
    }
    fn self_test(&self, _: &Path, _: &str) -> Result<String, String> {
        Ok(String::new())
    }
}

/// A kind that names a file the archive does not have.
struct Missing;

impl RuntimeKind for Missing {
    fn locate(&self, _: &Path) -> Option<PathBuf> {
        Some(PathBuf::from("nope"))
    }
    fn self_test(&self, _: &Path, _: &str) -> Result<String, String> {
        Ok(String::new())
    }
}

fn zip_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, body) in files {
        z.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(body).unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn sha_hex(b: &[u8]) -> String {
    let mut w = paddock_jobs::HashingWriter::new(std::io::sink());
    w.write_all(b).unwrap();
    let (_, d, _) = w.finish().unwrap();
    paddock_jobs::hex(&d)
}

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    host: Arc<Host>,
    downloader: Downloader,
    phases: Arc<Phases>,
    jobs: JobRegistry,
    clock: FakeClock,
}

impl World {
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("runtimes").join("tool");
        let host = Arc::new(Host::default());
        let downloader = Downloader::new(host.clone()).with_free_space(|_: &Path| Ok(u64::MAX));
        let phases = Arc::new(Phases::default());
        let clock = FakeClock::at_wall_ms(1_000);
        let jobs = JobRegistry::with_clock(phases.clone(), Arc::new(clock.clone()), 0);
        World {
            _dir: dir,
            root,
            host,
            downloader,
            phases,
            jobs,
            clock,
        }
    }

    fn store(&self) -> RuntimeStore {
        RuntimeStore::new(&self.root, "tool")
            .unwrap()
            .with_clock(Arc::new(self.clock.clone()))
    }

    /// Publishes a build of `version` whose tool says `text`, and returns its pin.
    fn publish(&self, version: &str, text: &str) -> Pinned {
        self.publish_files(version, &[("bin/tool", text.as_bytes()), ("lib/x", b"lib")])
    }

    fn publish_files(&self, version: &str, files: &[(&str, &[u8])]) -> Pinned {
        let bytes = zip_bytes(files);
        let url = format!("https://dl.test/{version}/tool-{version}.zip");
        let toml = format!(
            "format = 1\nname = \"tool\"\nversion = \"{version}\"\n[[platform]]\nkey = \"linux-x86_64\"\nurl = \"{url}\"\nsize = {}\nsha256 = \"{}\"\n",
            bytes.len(),
            sha_hex(&bytes)
        );
        self.host.files.lock().unwrap().insert(url, bytes);
        Catalogue::parse(&toml).unwrap().entries()[0].clone()
    }

    /// Publishes a `.tar.gz` build of `version` whose tool says `text`.
    fn publish_tar(&self, version: &str, text: &str) -> Pinned {
        let mut gz = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut gz, flate2::Compression::fast());
            let mut t = tar::Builder::new(enc);
            let mut h = tar::Header::new_gnu();
            h.set_size(text.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            t.append_data(&mut h, "bin/tool", text.as_bytes()).unwrap();
            t.into_inner().unwrap().finish().unwrap();
        }
        let url = format!("https://dl.test/{version}/tool-{version}.tar.gz");
        let toml = format!(
            "format = 1\nname = \"tool\"\nversion = \"{version}\"\n[[platform]]\nkey = \"linux-x86_64\"\nurl = \"{url}\"\nsize = {}\nsha256 = \"{}\"\n",
            gz.len(),
            sha_hex(&gz)
        );
        self.host.files.lock().unwrap().insert(url, gz);
        Catalogue::parse(&toml).unwrap().entries()[0].clone()
    }

    fn ctx(&self) -> JobContext {
        self.jobs.register("runtime", "Installing", None)
    }

    fn install(
        &self,
        store: &RuntimeStore,
        pinned: &Pinned,
        kind: &dyn RuntimeKind,
    ) -> paddock_runtime::Result<Installed> {
        let ctx = self.ctx();
        let mut out = None;
        let _ = ctx.run(|c| {
            let r = store.install(c, &self.downloader, pinned, kind);
            let ok = r.is_ok();
            out = Some(r);
            if ok {
                Ok(())
            } else {
                Err(paddock_jobs::JobError::Failed("x".into()))
            }
        });
        out.unwrap()
    }

    fn phases(&self) -> Vec<String> {
        std::mem::take(&mut *self.phases.0.lock().unwrap())
    }
}

#[test]
fn a_fresh_install_runs_every_phase_and_is_used() {
    let w = World::new();
    let store = w.store();
    let v1 = w.publish("1.0.0", "tool");
    assert_eq!(store.status(&v1), Status::NotInstalled);
    let i = w.install(&store, &v1, &Tool).unwrap();
    assert_eq!(
        w.phases(),
        [
            "connect",
            "download",
            "verify",
            "unpack",
            "self-test",
            "install"
        ]
    );
    assert_eq!(
        (i.name.as_str(), i.version.as_str(), i.self_test.as_str()),
        ("tool", "1.0.0", "tool 1.0.0")
    );
    assert_eq!(i.executable, w.root.join("1.0.0").join("bin").join("tool"));
    assert_eq!(i.sha256, v1.sha256.to_hex());
    assert_eq!(store.status(&v1), Status::Current(i.clone()));
    assert_eq!(store.active(), Some(i));
    assert_eq!(store.installed_versions(), ["1.0.0"]);
    assert!(store.root().join(MARKER).exists() || store.root().join("1.0.0").join(MARKER).exists());
    assert!(!w.root.join(".staging-1.0.0").exists(), "no staging left");
    assert_eq!(
        std::fs::read_dir(w.root.join("downloads")).unwrap().count(),
        0,
        "the archive is removed"
    );
    assert_eq!(store.name(), "tool");
    // The version in use again: nothing is downloaded.
    let gets = w.host.gets.load(Ordering::SeqCst);
    w.install(&store, &v1, &Tool).unwrap();
    assert_eq!(w.host.gets.load(Ordering::SeqCst), gets);
}

#[test]
fn an_update_keeps_the_previous_release_for_a_rollback_and_removes_older_ones() {
    let w = World::new();
    let store = w.store();
    let (v1, v2, v3) = (
        w.publish("1.0.0", "a"),
        w.publish("2.0.0", "b"),
        w.publish("3.0.0", "c"),
    );
    w.install(&store, &v1, &Tool).unwrap();
    w.install(&store, &v2, &Tool).unwrap();
    assert!(matches!(store.status(&v1), Status::Outdated { pinned, .. } if pinned == "1.0.0"));
    assert_eq!(store.previous().map(|p| p.version), Some("1.0.0".into()));
    let back = store.rollback(&Tool).unwrap();
    assert_eq!(back.version, "1.0.0");
    assert_eq!(store.active().map(|a| a.version), Some("1.0.0".into()));
    assert_eq!(store.previous().map(|p| p.version), Some("2.0.0".into()));
    // 2.0.0 is complete but not in use: installing it again tests it and takes it without a download.
    let gets = w.host.gets.load(Ordering::SeqCst);
    w.phases();
    assert_eq!(w.install(&store, &v2, &Tool).unwrap().version, "2.0.0");
    assert_eq!(w.host.gets.load(Ordering::SeqCst), gets);
    assert_eq!(w.phases(), ["self-test", "install"]);
    w.install(&store, &v3, &Tool).unwrap();
    assert_eq!(
        store.installed_versions(),
        ["2.0.0", "3.0.0"],
        "only the one in use and the one before it stay"
    );
}

#[test]
fn a_release_that_fails_its_self_test_is_set_aside_and_the_old_one_stays() {
    let w = World::new();
    let store = w.store().keep_quarantined(2);
    let good = w.publish("1.0.0", "ok");
    w.install(&store, &good, &Tool).unwrap();
    for (n, v) in ["2.0.0", "2.0.1", "2.0.2"].iter().enumerate() {
        w.clock.advance_ms(10);
        let bad = w.publish(v, "broken");
        let e = w.install(&store, &bad, &Tool).unwrap_err();
        assert!(
            matches!(&e, RuntimeError::SelfTestFailed { version, reason } if version == v && reason == "it crashed"),
            "{e}"
        );
        assert!(e.to_string().contains("previous version stays"), "{e}");
        assert_eq!(
            store.active().map(|a| a.version),
            Some("1.0.0".into()),
            "attempt {n}"
        );
    }
    let q = store.quarantined();
    assert_eq!(
        q.iter().map(|r| r.version.as_str()).collect::<Vec<_>>(),
        ["2.0.1", "2.0.2"],
        "the newest two are kept"
    );
    assert_eq!(q[1].at_ms, 1_030);
    let folders: Vec<String> = std::fs::read_dir(w.root.join(".quarantine"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(folders.len(), 2, "{folders:?}");
    assert!(folders.contains(&q[1].folder));
}

#[test]
fn a_broken_release_in_use_is_quarantined_and_the_previous_one_takes_over() {
    let w = World::new();
    let store = w.store();
    w.install(&store, &w.publish("1.0.0", "a"), &Tool).unwrap();
    w.install(&store, &w.publish("2.0.0", "b"), &Tool).unwrap();
    let now = store.quarantine("2.0.0", "it crashed at start").unwrap();
    assert_eq!(now.map(|i| i.version), Some("1.0.0".into()));
    assert!(store.previous().is_none());
    assert_eq!(store.quarantined()[0].reason, "it crashed at start");
    // Twice at the same time: the folder names stay apart.
    w.install(&store, &w.publish("2.0.0", "b"), &Tool).unwrap();
    store.quarantine("2.0.0", "again").unwrap();
    let q = store.quarantined();
    assert_ne!(q[0].folder, q[1].folder);
    assert!(
        store
            .quarantine("9.9.9", "x")
            .unwrap_err()
            .to_string()
            .contains("not installed")
    );
    assert!(store.quarantine("../x", "x").is_err());
    // The previous release is quarantined: nothing to roll back to.
    w.install(&store, &w.publish("3.0.0", "c"), &Tool).unwrap();
    store.quarantine("1.0.0", "old").unwrap();
    assert!(matches!(
        store.rollback(&Tool),
        Err(RuntimeError::NotInstalled(_))
    ));
}

#[test]
fn a_rollback_to_a_release_that_no_longer_works_sets_it_aside() {
    let w = World::new();
    let store = w.store();
    w.install(&store, &w.publish("1.0.0", "a"), &Tool).unwrap();
    w.install(&store, &w.publish("2.0.0", "b"), &Tool).unwrap();
    std::fs::write(w.root.join("1.0.0/bin/tool"), "broken").unwrap();
    assert!(matches!(
        store.rollback(&Tool),
        Err(RuntimeError::SelfTestFailed { .. })
    ));
    assert_eq!(store.active().map(|a| a.version), Some("2.0.0".into()));
    assert!(store.previous().is_none());
    // A complete folder that no longer passes is set aside on install too.
    w.install(&store, &w.publish("3.0.0", "c"), &Tool).unwrap();
    std::fs::write(w.root.join("2.0.0/bin/tool"), "broken").unwrap();
    let e = w
        .install(&store, &w.publish("2.0.0", "b"), &Tool)
        .unwrap_err();
    assert!(matches!(e, RuntimeError::SelfTestFailed { .. }));
    assert_eq!(store.active().map(|a| a.version), Some("3.0.0".into()));
}

#[test]
fn bad_downloads_and_archives_install_nothing() {
    let w = World::new();
    let store = w.store();
    // Checksum mismatch: the host serves other bytes than pinned.
    let p = w.publish("1.0.0", "a");
    w.host
        .files
        .lock()
        .unwrap()
        .insert(p.url.clone(), zip_bytes(&[("bin/tool", b"evil")]));
    let e = w.install(&store, &p, &Tool).unwrap_err();
    assert!(matches!(e, RuntimeError::Download(_)), "{e}");
    assert!(e.to_string().contains("download failed"));
    assert!(std::error::Error::source(&e).is_some());
    assert!(store.active().is_none());
    // No executable in the archive.
    let none = w.publish_files("2.0.0", &[("readme", b"x")]);
    assert!(matches!(
        w.install(&store, &none, &Tool),
        Err(RuntimeError::NoExecutable)
    ));
    let p3 = w.publish("3.0.0", "a");
    assert!(matches!(
        w.install(&store, &p3, &Outside),
        Err(RuntimeError::NoExecutable)
    ));
    assert!(matches!(
        w.install(&store, &p3, &Missing),
        Err(RuntimeError::NoExecutable)
    ));
    // Over the unpack limit.
    let mut few = ArchiveOptions::default();
    few.limits.max_entries = 1;
    let small = w.store().with_archive_options(few);
    let e = w
        .install(&small, &w.publish("4.0.0", "a"), &Tool)
        .unwrap_err();
    assert!(matches!(e, RuntimeError::Archive(_)), "{e}");
    assert!(store.active().is_none());
    assert!(!w.root.join(".staging-4.0.0").exists());
    // A tar archive is unpacked too (FFmpeg's Linux builds).
    let tar = w.publish_tar("5.0.0", "t");
    assert_eq!(w.install(&store, &tar, &Tool).unwrap().self_test, "t 5.0.0");
    // A build of another runtime.
    let other = Catalogue::parse(&format!(
        "format = 1\nname = \"other\"\nversion = \"1\"\n[[platform]]\nkey = \"linux-x86_64\"\nurl = \"https://dl.test/o.zip\"\nsize = 1\nsha256 = \"{}\"\n",
        "a".repeat(64)
    ))
    .unwrap();
    assert!(
        w.install(&store, &other.entries()[0], &Tool)
            .unwrap_err()
            .to_string()
            .contains("for other, not for tool")
    );
}

#[test]
fn a_cancelled_install_keeps_nothing_in_use_and_resumes_later() {
    let w = World::new();
    let store = w.store();
    let p = w.publish("1.0.0", "a");
    let ctx = w.ctx();
    ctx.cancel_token().cancel();
    let e = store.install(&ctx, &w.downloader, &p, &Tool).unwrap_err();
    assert!(matches!(e, RuntimeError::Cancelled(_)), "{e}");
    assert!(e.to_string().contains("continues the download"));
    assert!(matches!(
        paddock_jobs::JobError::from(e),
        paddock_jobs::JobError::Cancelled(_)
    ));
    assert!(store.active().is_none());
    w.install(&store, &p, &Tool).unwrap();
}

#[test]
fn one_install_at_a_time_across_processes() {
    let w = World::new();
    let store = w.store();
    std::fs::create_dir_all(&w.root).unwrap();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(w.root.join(".lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let p = w.publish("1.0.0", "a");
    let e = w.install(&store, &p, &Tool).unwrap_err();
    assert!(matches!(e, RuntimeError::Busy), "{e}");
    assert!(e.to_string().contains("wait for it"));
    assert_eq!(
        store.sweep(None).unwrap(),
        0,
        "the clean-up waits for the install"
    );
    lock.unlock().unwrap();
    w.install(&store, &p, &Tool).unwrap();
}

#[test]
fn remove_refuses_the_release_in_use() {
    let w = World::new();
    let store = w.store();
    w.install(&store, &w.publish("1.0.0", "a"), &Tool).unwrap();
    w.install(&store, &w.publish("2.0.0", "b"), &Tool).unwrap();
    let e = store.remove("2.0.0").unwrap_err();
    assert!(
        matches!(e, RuntimeError::InUse(_)) && e.to_string().contains("in use"),
        "{e}"
    );
    store.remove("1.0.0").unwrap();
    assert!(store.previous().is_none());
    std::fs::create_dir_all(w.root.join("7.0.0")).unwrap();
    store.remove("7.0.0").unwrap();
    assert!(
        !w.root.join("7.0.0").exists(),
        "a folder that is neither in use nor kept is removed too"
    );
    assert!(matches!(
        store.remove("1.0.0"),
        Err(RuntimeError::NotInstalled(_))
    ));
    assert!(matches!(
        store.remove(".."),
        Err(RuntimeError::NotInstalled(_))
    ));
}

#[test]
fn a_state_that_cannot_be_saved_is_an_error_that_names_it() {
    let w = World::new();
    let store = w.store();
    w.install(&store, &w.publish("1.0.0", "a"), &Tool).unwrap();
    w.install(&store, &w.publish("2.0.0", "b"), &Tool).unwrap();
    std::fs::remove_file(w.root.join(STATE_FILE)).unwrap();
    std::fs::create_dir_all(w.root.join(STATE_FILE)).unwrap();
    let e = store.rollback(&Tool).unwrap_err();
    assert!(
        matches!(e, RuntimeError::NotInstalled(_) | RuntimeError::Io { .. }),
        "{e}"
    );
    let e = store.quarantine("2.0.0", "x").unwrap_err();
    assert!(matches!(&e, RuntimeError::Io { op: "save", .. }), "{e}");
}

#[test]
fn a_lost_or_damaged_state_falls_back_to_what_is_on_disk() {
    let w = World::new();
    let store = w.store();
    w.install(&store, &w.publish("1.0.0", "a"), &Tool).unwrap();
    w.install(&store, &w.publish("2.0.0", "b"), &Tool).unwrap();
    std::fs::write(w.root.join(STATE_FILE), "{ not json").unwrap();
    std::fs::remove_file(w.root.join(format!("{STATE_FILE}.bak"))).ok();
    assert_eq!(
        store.active().map(|a| a.version),
        Some("2.0.0".into()),
        "the newest complete release"
    );
    // A marker for another runtime, a damaged marker and a missing executable are not releases.
    std::fs::write(w.root.join("2.0.0").join(MARKER), "{}").unwrap();
    std::fs::write(
        w.root.join("1.0.0").join(MARKER),
        r#"{"format":1,"name":"other","version":"1.0.0","sha256":"","executable":"bin/tool","selfTest":""}"#,
    )
    .unwrap();
    assert!(
        store.installed_versions().is_empty(),
        "a marker of another runtime"
    );
    std::fs::write(
        w.root.join("1.0.0").join(MARKER),
        r#"{"format":1,"name":"tool","version":"1.0.0","sha256":"","executable":"bin/tool","selfTest":""}"#,
    )
    .unwrap();
    assert_eq!(store.installed_versions(), ["1.0.0"]);
    std::fs::remove_file(w.root.join("1.0.0/bin/tool")).unwrap();
    assert!(store.active().is_none());
    assert_eq!(store.status(&w.publish("1.0.0", "a")), Status::NotInstalled);
}

#[test]
fn sweep_removes_leftovers_but_keeps_a_resumable_download() {
    let w = World::new();
    let store = w.store();
    let p = w.publish("1.0.0", "a");
    w.install(&store, &p, &Tool).unwrap();
    for d in [".staging-2.0.0", "3.0.0", "downloads"] {
        std::fs::create_dir_all(w.root.join(d)).unwrap();
    }
    std::fs::write(w.root.join("downloads/old.zip.part"), b"x").unwrap();
    let next = w.publish("2.0.0", "b");
    std::fs::write(
        w.root
            .join("downloads")
            .join(format!("{}.part", next.file_name)),
        b"x",
    )
    .unwrap();
    assert_eq!(store.sweep(Some(&next)).unwrap(), 3);
    assert!(w.root.join("1.0.0").is_dir() && !w.root.join("3.0.0").exists());
    assert_eq!(
        std::fs::read_dir(w.root.join("downloads")).unwrap().count(),
        1
    );
    assert_eq!(store.sweep(None).unwrap(), 1);
}

#[test]
fn stores_live_in_the_app_data_folder_and_need_a_folder_name() {
    let d = tempfile::tempdir().unwrap();
    let vars = paddock_env::MapVars::new().with("HOME", d.path().as_os_str());
    let app = paddock_env::AppId::new("demo", "Demo").unwrap();
    let dirs =
        paddock_env::AppDirs::resolve(&app, paddock_env::Os::Linux, &vars, None, false).unwrap();
    let s = RuntimeStore::in_app_data(&dirs, "chromium").unwrap();
    assert_eq!(s.root(), dirs.data().join("runtimes").join("chromium"));
    assert!(format!("{s:?}").contains("chromium"));
    assert!(RuntimeStore::new(d.path(), "Bad Name").is_err());
}

/// Rule 40 (C36): no catalogue text, however broken, makes the parser panic.
#[test]
fn the_catalogue_parser_never_panics() {
    let seed = format!(
        "format = 1\nname = \"tool\"\nversion = \"1.0\"\n[[platform]]\nkey = \"linux-x86_64\"\nurl = \"https://dl.test/a.zip\"\nsize = 9\nsha256 = \"{}\"\n",
        "a".repeat(64)
    );
    let parse = |b: &[u8]| Catalogue::parse(&String::from_utf8_lossy(b)).is_ok();
    assert!(
        paddock_testkit::never_panics(&paddock_testkit::corpus(seed.as_bytes()), parse).is_empty()
    );
}
