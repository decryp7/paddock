//! `dev fetch chrome`: installs the Chrome for Testing build a `paddock-runtime` catalogue pins into a folder, so tests never
//! launch an installed Chrome (rule 7, check C15). Replaces `conformance/ci/install-test-browser.mjs --catalogue`.
//!
//! One job with phases on their own scales: the download through `paddock-download` (hashed while written, checked against
//! the pinned size and SHA-256 before the file appears), the unpack through `paddock-archive` (atomic: the folder is swapped in
//! only when everything was written) and the install (find the program, stamp the version). The browser is never started.

use super::error::FetchError;
use super::runner::Runner;
use paddock_archive::{Links, Options, extract_job};
use paddock_download::{Integrity, Request, phases};
use paddock_jobs::{JobError, Phase};
use paddock_runtime::{Catalogue, OsKind, Platform};
use std::path::{Path, PathBuf};

/// The file in the install folder that names the version installed.
pub const STAMP: &str = ".pinned-version";

/// What an install left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The browser program.
    pub executable: PathBuf,
    /// The pinned version.
    pub version: String,
    /// True when that version was already there and nothing was downloaded.
    pub cached: bool,
}

fn downloads_folder(dir: &Path) -> PathBuf {
    let name = dir
        .file_name()
        .map_or_else(|| "browser".to_string(), |n| n.to_string_lossy().into_owned());
    dir.with_file_name(format!("{name}.download"))
}

/// The installed program when `dir` already holds `version`.
pub fn installed(dir: &Path, version: &str, os: OsKind) -> Option<PathBuf> {
    let stamp = std::fs::read_to_string(dir.join(STAMP)).ok()?;
    if stamp.trim() != version {
        return None;
    }
    paddock_chromium::runtime::locate(dir, os).map(|relative| dir.join(relative))
}

/// Installs the build `catalogue` pins for `platform` into `dir` unless that version is already there.
pub fn install(runner: &Runner, catalogue: &Catalogue, platform: Platform, dir: &Path) -> Result<Installed, FetchError> {
    let pinned = catalogue.pinned(platform).map_err(|e| {
        FetchError::with_hint(
            format!("The catalogue pins no browser for {platform}: {e}."),
            "Pick another platform with --platform, or pin one with `./dev fetch pin`.",
        )
    })?;
    if let Some(executable) = installed(dir, &pinned.version, platform.os) {
        return Ok(Installed {
            executable,
            version: pinned.version.clone(),
            cached: true,
        });
    }
    // The unpack swaps the folder in whole, so a folder that is not ours must never be the target.
    let ours = dir.join(STAMP).is_file();
    if !ours && std::fs::read_dir(dir).is_ok_and(|mut d| d.next().is_some()) {
        return Err(FetchError::with_hint(
            format!(
                "{} exists, is not empty and holds no {STAMP}, so it is not a browser install of this tool.",
                dir.display()
            ),
            "Pick an empty or new folder with --dir; nothing was changed.",
        ));
    }
    let pinned = pinned.clone();
    let (dir, downloads) = (dir.to_path_buf(), downloads_folder(dir));
    let downloader = runner.downloader().clone();
    let subject = format!("{} {}", pinned.name, pinned.version);
    let os = platform.os;
    runner.run("fetch", "Fetching the test browser", Some(&subject), move |ctx| {
        let request = Request::new(&pinned.url, downloads.join(&pinned.file_name), Integrity::Sha256(pinned.sha256))?.size(pinned.size);
        let got = downloader.download(ctx, &request)?;
        if let Some(parent) = dir.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        // The macOS app bundle carries relative symbolic links; `Keep` makes only ones that cannot leave the tree.
        let options = Options {
            links: Links::Keep,
            ..Options::default()
        };
        extract_job(ctx, &got.path, &dir, &options)?;
        ctx.phase(Phase::items(phases::INSTALL, phases::INSTALL_LABEL, 1));
        let executable = paddock_chromium::runtime::locate(&dir, os)
            .map(|relative| dir.join(relative))
            .ok_or_else(|| JobError::Failed(format!("The archive holds no browser program (looked in {}).", dir.display())))?;
        std::fs::write(dir.join(STAMP), &pinned.version)?;
        ctx.add(1);
        let _ = std::fs::remove_dir_all(&downloads);
        Ok(Installed {
            executable,
            version: pinned.version.clone(),
            cached: false,
        })
    })
}

/// The lines a CI step exports: the browser path and, with a prefix, the switch that makes a missing browser fail the tests.
pub fn env_lines(executable: &Path, prefix: Option<&str>) -> Vec<String> {
    let mut lines = vec![format!("CHROMIUM_TEST_PATH={}", executable.display())];
    if let Some(p) = prefix {
        lines.push(format!("{p}_REQUIRE_TEST_BROWSER=1"));
    }
    lines
}

/// Whether `prefix` is a valid environment-variable prefix: upper-case letters, digits and underscores, starting with a letter.
pub fn prefix_ok(prefix: &str) -> bool {
    prefix.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && prefix.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::super::testing::{FakeNet, Route, runner, runner_with_lines, sha256_hex, zip_of};
    use super::*;

    const URL: &str = "https://dl.test/9.9.9/linux64/chrome-linux64.zip";

    fn archive() -> Vec<u8> {
        zip_of(&[
            ("chrome-linux64/chrome", b"#!/bin/sh\n", 0o755),
            ("chrome-linux64/locales/en.pak", b"pak", 0o644),
        ])
    }

    fn catalogue(zip: &[u8], sha: &str) -> Catalogue {
        Catalogue::parse(&format!(
            "format = 1\nname = \"chromium\"\nversion = \"9.9.9\"\n[[platform]]\nkey = \"linux-x86_64\"\nurl = \"{URL}\"\nsize = {}\nsha256 = \"{sha}\"\n",
            zip.len()
        ))
        .unwrap()
    }

    fn linux() -> Platform {
        Platform::parse("linux-x86_64").unwrap()
    }

    #[test]
    fn installs_the_pinned_build_once_and_reuses_it() {
        let zip = archive();
        let cat = catalogue(&zip, &sha256_hex(&zip));
        let net = FakeNet::new();
        net.serve(URL, zip);
        let work = tempfile::tempdir().unwrap();
        let dir = work.path().join("browser");
        let (r, lines) = runner_with_lines(&net, 1);
        let first = install(&r, &cat, linux(), &dir).unwrap();
        assert_eq!(first.executable, dir.join("chrome-linux64").join("chrome"));
        assert_eq!((first.version.as_str(), first.cached), ("9.9.9", false));
        assert_eq!(std::fs::read_to_string(dir.join(STAMP)).unwrap(), "9.9.9");
        assert!(!downloads_folder(&dir).exists(), "the archive is removed after the install");
        let text = String::from_utf8(lines.lock().unwrap().clone()).unwrap();
        for phase in ["Connecting", "Downloading", "Verifying", "Unpacking", "Installing", "Finished"] {
            assert!(text.contains(phase), "{phase} in {text}");
        }
        let asked = net.seen().len();
        let again = install(&r, &cat, linux(), &dir).unwrap();
        assert!(again.cached);
        assert_eq!(net.seen().len(), asked, "a cached install touches no network");
    }

    #[test]
    fn a_folder_that_is_not_an_install_is_never_replaced() {
        let zip = archive();
        let cat = catalogue(&zip, &sha256_hex(&zip));
        let net = FakeNet::new();
        net.serve(URL, zip);
        let work = tempfile::tempdir().unwrap();
        let dir = work.path().join("docs");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("precious.txt"), "mine").unwrap();
        let err = install(&runner(&net, 1), &cat, linux(), &dir).unwrap_err();
        assert!(matches!(&err, FetchError::Failed { hint: Some(_), .. }), "{err:?}");
        assert_eq!(std::fs::read_to_string(dir.join("precious.txt")).unwrap(), "mine");
        assert!(net.seen().is_empty());
        // An empty folder, and one with an older stamp of this tool, are fine.
        let empty = work.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(install(&runner(&net, 1), &cat, linux(), &empty).is_ok());
        std::fs::write(empty.join(STAMP), "0.0.1").unwrap();
        assert!(
            !install(&runner(&net, 1), &cat, linux(), &empty).unwrap().cached,
            "stamp of another version"
        );
    }

    #[test]
    fn framework_style_relative_links_are_kept_inside_the_tree() {
        let zip = crate::fetch::testing::zip_with_links(&[("chrome-linux64/chrome", b"x")], &[("chrome-linux64/Frameworks/Current", "1")]);
        let cat = catalogue(&zip, &sha256_hex(&zip));
        let net = FakeNet::new();
        net.serve(URL, zip);
        let work = tempfile::tempdir().unwrap();
        let dir = work.path().join("b");
        install(&runner(&net, 1), &cat, linux(), &dir).unwrap();
        #[cfg(unix)]
        assert_eq!(
            std::fs::read_link(dir.join("chrome-linux64/Frameworks/Current")).unwrap(),
            Path::new("1")
        );
        #[cfg(not(unix))]
        let _ = dir;
    }

    #[test]
    fn a_cancel_while_the_archive_downloads_leaves_no_install() {
        let zip = archive();
        let cat = catalogue(&zip, &sha256_hex(&zip));
        let net = FakeNet::new();
        let r = std::sync::Arc::new(runner(&net, 1));
        let r2 = r.clone();
        net.route(URL, Route::Endless(std::sync::Arc::new(move || r2.cancel_all())));
        let work = tempfile::tempdir().unwrap();
        let dir = work.path().join("b");
        assert_eq!(install(&r, &cat, linux(), &dir).unwrap_err(), FetchError::Cancelled);
        assert!(!dir.exists(), "no install folder");
    }

    #[test]
    fn a_wrong_hash_installs_nothing() {
        let zip = archive();
        let cat = catalogue(&zip, &"0".repeat(64));
        let net = FakeNet::new();
        net.serve(URL, zip);
        let work = tempfile::tempdir().unwrap();
        let dir = work.path().join("browser");
        let err = install(&runner(&net, 1), &cat, linux(), &dir).unwrap_err();
        assert!(
            matches!(&err, FetchError::Failed { message, .. } if message.to_lowercase().contains("checksum") || message.to_lowercase().contains("sha")),
            "{err:?}"
        );
        assert!(!dir.exists(), "nothing is unpacked from an archive that failed its check");
    }

    #[test]
    fn a_missing_platform_and_an_archive_without_a_program_are_refused() {
        let zip = archive();
        let cat = catalogue(&zip, &sha256_hex(&zip));
        let net = FakeNet::new();
        let work = tempfile::tempdir().unwrap();
        let mac = Platform::parse("macos-aarch64").unwrap();
        let err = install(&runner(&net, 1), &cat, mac, &work.path().join("b")).unwrap_err();
        assert!(matches!(&err, FetchError::Failed { hint: Some(_), .. }));
        assert!(net.seen().is_empty());

        let empty = zip_of(&[("chrome-linux64/readme.txt", b"x", 0o644)]);
        let cat = catalogue(&empty, &sha256_hex(&empty));
        net.serve(URL, empty);
        let err = install(&runner(&net, 1), &cat, linux(), &work.path().join("c")).unwrap_err();
        assert!(err.to_string().contains("no browser program"), "{err}");
    }

    #[test]
    fn a_host_error_is_a_failure_not_a_hang() {
        let zip = archive();
        let cat = catalogue(&zip, &sha256_hex(&zip));
        let net = FakeNet::new();
        net.route(URL, Route::Status(404));
        let work = tempfile::tempdir().unwrap();
        let err = install(&runner(&net, 1), &cat, linux(), &work.path().join("b")).unwrap_err();
        assert!(matches!(err, FetchError::Failed { .. }), "{err:?}");
    }

    #[test]
    fn the_exported_lines_and_prefixes() {
        let lines = env_lines(Path::new("/b/chrome"), Some("DEBRIEF"));
        assert_eq!(lines, ["CHROMIUM_TEST_PATH=/b/chrome", "DEBRIEF_REQUIRE_TEST_BROWSER=1"]);
        assert_eq!(env_lines(Path::new("/b/chrome"), None).len(), 1);
        assert!(prefix_ok("DEBRIEF") && prefix_ok("A1_B"));
        assert!(!prefix_ok("") && !prefix_ok("debrief") && !prefix_ok("1A") && !prefix_ok("A-B"));
    }

    #[test]
    fn an_old_stamp_or_no_program_is_not_an_install() {
        let work = tempfile::tempdir().unwrap();
        let dir = work.path();
        assert_eq!(installed(dir, "1", OsKind::Linux), None);
        std::fs::write(dir.join(STAMP), "1\n").unwrap();
        assert_eq!(installed(dir, "1", OsKind::Linux), None, "a stamp without a program");
        std::fs::create_dir_all(dir.join("chrome-linux64")).unwrap();
        std::fs::write(dir.join("chrome-linux64").join("chrome"), "x").unwrap();
        assert!(installed(dir, "1", OsKind::Linux).is_some());
        assert_eq!(installed(dir, "2", OsKind::Linux), None, "another version");
    }
}
