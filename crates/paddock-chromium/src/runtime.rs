//! The pinned Chrome for Testing every app may download (`chromium.toml`), installed through `paddock-runtime` as one job
//! with phases (download hashed while written, unpack, self-test, install) into `<data>/runtimes/chromium`.
//!
//! ```ignore
//! let store = paddock_chromium::runtime::store(&dirs)?;
//! let ctx = jobs.register("runtime", "Downloading the browser", Some("Chrome for Testing"));
//! pool.spawn_job(ctx, move |ctx| Ok(paddock_chromium::runtime::install(ctx, &downloader, &store)?))?;
//! // later
//! let browser = paddock_chromium::discover::find(Some("APP_CHROMIUM_PATH"), paddock_chromium::runtime::installed(&store));
//! ```

use crate::discover::{Browser, this_os};
use paddock_download::Downloader;
use paddock_jobs::JobContext;
use paddock_proc::{Limits, Outcome};
use paddock_runtime::{Catalogue, OsKind, RuntimeKind, RuntimeStore};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The runtime's name: its store folder.
pub const NAME: &str = "chromium";
/// How long `--version` may take.
const SELF_TEST_LIMIT: Duration = Duration::from_secs(30);
const CATALOGUE: &str = include_str!("chromium.toml");

/// The pinned builds shipped with this crate.
pub fn catalogue() -> crate::Result<Catalogue> {
    Ok(Catalogue::parse(CATALOGUE)?)
}

/// The Chromium store in the app's data folder.
pub fn store(dirs: &paddock_env::AppDirs) -> crate::Result<RuntimeStore> {
    Ok(RuntimeStore::in_app_data(dirs, NAME)?)
}

/// Installs (or updates to) the pinned build for this platform as the job `ctx` and returns the browser.
pub fn install(
    ctx: &JobContext,
    downloader: &Downloader,
    store: &RuntimeStore,
) -> crate::Result<Browser> {
    install_from(&catalogue()?, ctx, downloader, store)
}

fn install_from(
    catalogue: &Catalogue,
    ctx: &JobContext,
    downloader: &Downloader,
    store: &RuntimeStore,
) -> crate::Result<Browser> {
    let pinned = catalogue.for_this_platform()?;
    let i = store.install(ctx, downloader, pinned, &ChromeForTesting::new(this_os()))?;
    Ok(Browser::chromium(i.executable))
}

/// The installed Chrome for Testing, if one passed its self-test.
pub fn installed(store: &RuntimeStore) -> Option<Browser> {
    store.active().map(|i| Browser::chromium(i.executable))
}

/// Where Chrome for Testing keeps its program, and its self-test (`--version` names the pinned version).
#[derive(Debug, Clone, Copy)]
pub struct ChromeForTesting {
    os: OsKind,
}

impl ChromeForTesting {
    /// The layout of `os`'s archive.
    pub fn new(os: OsKind) -> ChromeForTesting {
        ChromeForTesting { os }
    }
}

impl RuntimeKind for ChromeForTesting {
    fn locate(&self, root: &Path) -> Option<PathBuf> {
        locate(root, self.os)
    }

    fn self_test(&self, executable: &Path, version: &str) -> std::result::Result<String, String> {
        let mut cmd = paddock_proc::command(executable);
        cmd.arg("--version");
        let out = paddock_proc::run_bounded(cmd, Limits::new(SELF_TEST_LIMIT).max_output(4096))
            .map_err(|e| e.to_string())?;
        check_version(
            out.outcome == Outcome::TimedOut,
            out.success(),
            &out.stdout_text(),
            version,
        )
    }
}

/// The verdict on `--version`'s output.
fn check_version(
    timed_out: bool,
    success: bool,
    stdout: &str,
    version: &str,
) -> std::result::Result<String, String> {
    let line = stdout.trim().to_string();
    match () {
        _ if timed_out => Err("it did not answer in time".into()),
        _ if !success => Err(format!(
            "it ended with an error{}",
            if line.is_empty() {
                String::new()
            } else {
                format!(": {line}")
            }
        )),
        _ if !line.contains(version) => Err(format!("it reports another version ({line})")),
        _ => Ok(line),
    }
}

/// The program inside an unpacked Chrome for Testing archive, relative to `root`.
pub fn locate(root: &Path, os: OsKind) -> Option<PathBuf> {
    let want: &[&str] = match os {
        OsKind::Windows => &["chrome.exe"],
        OsKind::Macos => &["Google Chrome for Testing", "Chromium"],
        OsKind::Linux => &["chrome"],
    };
    let mut stack = vec![root.to_path_buf()];
    let mut found: Vec<PathBuf> = Vec::new();
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| want.contains(&n))
                && !p.to_string_lossy().contains("Helper")
            {
                found.push(p);
            }
        }
    }
    // The shortest path is the app's own program, not one inside a framework.
    found.sort_by_key(|p| (p.components().count(), p.clone()));
    found
        .into_iter()
        .next()
        .and_then(|p| p.strip_prefix(root).ok().map(Path::to_path_buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_runtime::RuntimeError as R;

    #[test]
    fn the_embedded_catalogue_pins_chrome_for_testing_for_every_platform() {
        let c = catalogue().unwrap();
        assert_eq!(c.name, NAME);
        assert!(
            c.version.split('.').count() == 4
                && c.version.split('.').all(|p| p.parse::<u32>().is_ok()),
            "{}",
            c.version
        );
        let keys: Vec<String> = c.entries().iter().map(|e| e.platform.key()).collect();
        for want in [
            "windows-x86_64",
            "macos-aarch64",
            "macos-x86_64",
            "linux-x86_64",
            "linux-aarch64",
        ] {
            assert!(keys.iter().any(|k| k == want), "{want} missing");
        }
        for e in c.entries() {
            assert!(
                e.url
                    .starts_with("https://storage.googleapis.com/chrome-for-testing-public/"),
                "{}",
                e.url
            );
            assert!(
                e.url.contains(&format!("/{}/", c.version)),
                "the URL names the pinned version: {}",
                e.url
            );
            assert!(e.size > 100 << 20, "{}", e.size);
        }
        let mut hashes: Vec<String> = c.entries().iter().map(|e| e.sha256.to_hex()).collect();
        hashes.sort();
        hashes.dedup();
        assert_eq!(
            hashes.len(),
            c.entries().len(),
            "every archive has its own hash"
        );
    }

    #[test]
    fn the_program_is_found_in_each_layout() {
        let d = tempfile::tempdir().unwrap();
        let put = |rel: &str| {
            let p = d.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        };
        put("chrome-linux64/chrome");
        put("chrome-win64/chrome.exe");
        let app = "chrome-mac-arm64/Google Chrome for Testing.app/Contents";
        put(&format!("{app}/MacOS/Google Chrome for Testing"));
        put(&format!(
            "{app}/Frameworks/X.framework/Helpers/Google Chrome for Testing Helper.app/Contents/MacOS/Google Chrome for Testing"
        ));
        assert_eq!(
            locate(d.path(), OsKind::Linux),
            Some(PathBuf::from("chrome-linux64/chrome"))
        );
        assert_eq!(
            locate(d.path(), OsKind::Windows),
            Some(PathBuf::from("chrome-win64/chrome.exe"))
        );
        assert_eq!(
            locate(d.path(), OsKind::Macos),
            Some(PathBuf::from(format!(
                "{app}/MacOS/Google Chrome for Testing"
            )))
        );
        assert_eq!(locate(&d.path().join("none"), OsKind::Linux), None);
        assert_eq!(
            ChromeForTesting::new(OsKind::Linux).locate(d.path()),
            locate(d.path(), OsKind::Linux)
        );
    }

    #[test]
    fn the_self_test_wants_the_pinned_version() {
        let v = "156.0.8078.47";
        assert_eq!(
            check_version(false, true, "Google Chrome for Testing 156.0.8078.47 \n", v),
            Ok("Google Chrome for Testing 156.0.8078.47".into())
        );
        assert!(
            check_version(false, true, "Chromium 1.0", v)
                .unwrap_err()
                .contains("another version")
        );
        assert!(
            check_version(false, false, "", v)
                .unwrap_err()
                .ends_with("an error")
        );
        assert!(
            check_version(false, false, "boom", v)
                .unwrap_err()
                .contains("boom")
        );
        assert!(
            check_version(true, false, "", v)
                .unwrap_err()
                .contains("in time")
        );
    }

    struct Offline;
    impl paddock_download::Transport for Offline {
        fn get(
            &self,
            _: &paddock_download::Get<'_>,
        ) -> std::result::Result<paddock_download::Response, paddock_download::TransportError>
        {
            Ok(paddock_download::Response {
                status: 404,
                headers: paddock_download::Headers::default(),
                body: Box::new(std::io::empty()),
            })
        }
    }

    struct NoSink;
    impl paddock_jobs::EventSink for NoSink {
        fn emit(&self, _: &paddock_jobs::JobSnapshot) {}
    }

    #[test]
    fn an_install_goes_through_the_store_and_reports_what_failed() {
        let d = tempfile::tempdir().unwrap();
        let vars = paddock_env::MapVars::new().with("DEMO_HOME", d.path().as_os_str());
        let app = paddock_env::AppId::new("demo", "Demo").unwrap();
        let dirs =
            paddock_env::AppDirs::resolve(&app, paddock_env::Os::current(), &vars, None, true)
                .unwrap();
        let store = store(&dirs).unwrap();
        assert_eq!(store.root(), dirs.data().join("runtimes").join(NAME));
        assert_eq!(installed(&store), None);
        let jobs = paddock_jobs::JobRegistry::new(std::sync::Arc::new(NoSink));
        let ctx = jobs.register("runtime", "Downloading the browser", None);
        let downloader =
            Downloader::new(std::sync::Arc::new(Offline)).with_free_space(|_| Ok(u64::MAX));
        let e = install(&ctx, &downloader, &store).unwrap_err();
        let download = matches!(e, crate::Error::Runtime(R::Download(_)));
        assert!(download, "{e}");
        assert_eq!(installed(&store), None);
    }

    /// A whole install for this platform from a local catalogue: a "browser" that answers `--version`.
    #[cfg(unix)]
    #[test]
    fn a_build_for_this_platform_is_installed_and_found() {
        use std::io::Write;
        let platform = paddock_runtime::Platform::current().expect("CI platforms are pinned");
        let layouts = [
            (
                OsKind::Macos,
                "chrome-mac/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
            ),
            (OsKind::Windows, "chrome-win64/chrome.exe"),
            (OsKind::Linux, "chrome-linux64/chrome"),
        ];
        let exe = layouts
            .iter()
            .find(|(os, _)| *os == this_os())
            .map(|l| l.1)
            .unwrap();
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let exec = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
        z.start_file(exe, exec).unwrap();
        z.write_all(b"#!/bin/sh\necho 'Google Chrome for Testing 1.2.3'\n")
            .unwrap();
        let bytes = z.finish().unwrap().into_inner();
        let mut h = paddock_jobs::HashingWriter::new(std::io::sink());
        h.write_all(&bytes).unwrap();
        let (_, digest, _) = h.finish().unwrap();
        let url = "https://dl.test/1.2.3/chrome.zip";
        let text = format!(
            "format = 1\nname = \"chromium\"\nversion = \"1.2.3\"\n[[platform]]\nkey = \"{platform}\"\nurl = \"{url}\"\nsize = {}\nsha256 = \"{}\"\n",
            bytes.len(),
            paddock_jobs::hex(&digest)
        );
        let c = Catalogue::parse(&text).unwrap();
        let d = tempfile::tempdir().unwrap();
        let store = RuntimeStore::new(d.path().join("chromium"), NAME).unwrap();
        let jobs = paddock_jobs::JobRegistry::new(std::sync::Arc::new(NoSink));
        let ctx = jobs.register("runtime", "Downloading the browser", None);
        let downloader =
            Downloader::new(std::sync::Arc::new(Serve(bytes))).with_free_space(|_| Ok(u64::MAX));
        let b = install_from(&c, &ctx, &downloader, &store).unwrap();
        assert!(b.path.ends_with(exe), "{}", b.path.display());
        assert_eq!(installed(&store), Some(b));
    }

    #[cfg(unix)]
    struct Serve(Vec<u8>);
    #[cfg(unix)]
    impl paddock_download::Transport for Serve {
        fn get(
            &self,
            _: &paddock_download::Get<'_>,
        ) -> std::result::Result<paddock_download::Response, paddock_download::TransportError>
        {
            Ok(paddock_download::Response {
                status: 200,
                headers: paddock_download::Headers {
                    content_length: Some(self.0.len().to_string()),
                    ..Default::default()
                },
                body: Box::new(std::io::Cursor::new(self.0.clone())),
            })
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_self_test_runs_the_program() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("chrome");
        std::fs::write(&exe, "#!/bin/sh\necho \"Chromium 9.9 $1\"\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let k = ChromeForTesting::new(OsKind::Linux);
        assert_eq!(
            k.self_test(&exe, "9.9"),
            Ok("Chromium 9.9 --version".into())
        );
        assert!(k.self_test(&d.path().join("missing"), "9.9").is_err());
    }
}
