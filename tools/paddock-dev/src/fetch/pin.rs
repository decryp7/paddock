//! `dev fetch pin`: re-pins a `paddock-runtime` catalogue (Chromium's `chromium.toml`, FFmpeg's, llama.cpp's). Every platform's
//! archive is downloaded once, its size and SHA-256 are taken while it is written (one pass, the file is removed at once), and
//! the catalogue's `version`, `url`, `size` and `sha256` are rewritten in place with `toml_edit`, so comments and order stay as
//! the author wrote them. The result must parse as a catalogue before anything is written. Run it by hand, review the diff,
//! commit: nothing is downloaded at build time. Replaces `conformance/ci/pin-runtime.mjs`.
//!
//! **Parallelism and cost** (rule 51, ER-20). The work is the download itself: network-bound, one archive per platform (five
//! for Chrome for Testing, about 200 MB each). Archives are measured up to `--jobs` at a time (default 3, at most 8); each
//! download already uses up to 4 connections and the downloader caps a host at 4 across all of them, so more jobs only queue
//! on the same host. Results are collected by platform order, so the output does not depend on which finished first. The cost
//! is already minimal: one pass per archive with no second read (the digest is computed while writing) and no archive kept on
//! disk after its digest is known. `--version` and `--chrome-for-testing` need no download of the old files.

use super::error::FetchError;
use super::runner::Runner;
use paddock_download::{Fetch, Integrity, Request};
use paddock_jobs::{JobError, Phase};
use paddock_runtime::{Catalogue, MAX_ARCHIVE_BYTES};
use serde_json::Value;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table, Value as TomlValue};

/// Chrome for Testing's channel list with download URLs.
pub const CFT_ENDPOINT: &str = "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";

/// Chrome for Testing's platform ids by catalogue key (Windows on ARM has no build).
pub fn cft_platform(key: &str) -> Option<&'static str> {
    match key {
        "windows-x86_64" => Some("win64"),
        "macos-aarch64" => Some("mac-arm64"),
        "macos-x86_64" => Some("mac-x64"),
        "linux-x86_64" => Some("linux64"),
        "linux-aarch64" => Some("linux-arm64"),
        _ => None,
    }
}

/// What to pin the catalogue to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The URLs as they are: only sizes and checksums are measured again.
    Same,
    /// Every URL moves from the pinned version to this one.
    Version(String),
    /// The version and URLs of a Chrome for Testing channel.
    ChromeForTesting {
        /// `Stable`, `Beta`, ...
        channel: String,
        /// The channel list.
        endpoint: String,
    },
}

/// One platform's pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformPin {
    /// `<os>-<arch>`.
    pub key: String,
    /// The archive.
    pub url: String,
    /// Bytes of the archive.
    pub size: u64,
    /// Lower-case hex SHA-256.
    pub sha256: String,
}

/// A re-pinned catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repinned {
    /// The new catalogue text.
    pub text: String,
    /// The pinned version.
    pub version: String,
    /// The pins, in catalogue order.
    pub platforms: Vec<PlatformPin>,
}

/// `url` with its `/<from>/` segment replaced by `/<to>/`; the old version must be in the URL exactly once.
pub fn move_url(url: &str, from: &str, to: &str) -> Result<String, FetchError> {
    let needle = format!("/{from}/");
    let parts: Vec<&str> = url.split(&needle).collect();
    if parts.len() != 2 {
        return Err(FetchError::failed(format!("{url} does not name version {from} once.")));
    }
    Ok(format!("{}/{to}/{}", parts[0], parts[1]))
}

/// The version and the per-key URLs of a Chrome for Testing channel in the channel list `json`.
pub fn cft_downloads(json: &Value, channel: &str, keys: &[String]) -> Result<(String, Vec<String>), FetchError> {
    let c = json
        .pointer(&format!("/channels/{channel}"))
        .ok_or_else(|| FetchError::failed(format!("Chrome for Testing has no channel {channel}.")))?;
    let version = c
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| FetchError::failed(format!("Chrome for Testing names no version for {channel}.")))?;
    let builds = c
        .pointer("/downloads/chrome")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut urls = Vec::new();
    for key in keys {
        let url = cft_platform(key)
            .and_then(|id| builds.iter().find(|d| d.get("platform").and_then(Value::as_str) == Some(id)))
            .and_then(|d| d.get("url").and_then(Value::as_str))
            .ok_or_else(|| FetchError::failed(format!("Chrome for Testing {version} has no build for {key}.")))?;
        urls.push(url.to_string());
    }
    Ok((version.to_string(), urls))
}

/// `https://...`, or `http://` to the loopback (local test servers).
pub fn url_ok(u: &str) -> bool {
    u.starts_with("https://")
        || ["127.0.0.1:", "localhost:", "[::1]:"]
            .iter()
            .any(|h| u.strip_prefix("http://").is_some_and(|r| r.starts_with(h)))
}

/// The version and the `(key, url)` of every platform in a catalogue text (read leniently: sizes and checksums are about to be
/// replaced, so a placeholder there is fine).
fn read_pins(doc: &DocumentMut) -> Result<(String, Vec<(String, String)>), FetchError> {
    let version = doc
        .get("version")
        .and_then(Item::as_str)
        .ok_or_else(|| FetchError::failed("The catalogue has no version."))?;
    let platforms = doc
        .get("platform")
        .and_then(Item::as_array_of_tables)
        .ok_or_else(|| FetchError::failed("The catalogue has no [[platform]] entries."))?;
    let mut out = Vec::new();
    for t in platforms {
        match (t.get("key").and_then(Item::as_str), t.get("url").and_then(Item::as_str)) {
            (Some(k), Some(u)) => out.push((k.to_string(), u.to_string())),
            _ => return Err(FetchError::failed("Every [[platform]] needs a key and a url.")),
        }
    }
    Ok((version.to_string(), out))
}

/// Sets `item` to `value`, keeping the comment and spacing around the old value.
fn set(item: &mut Item, mut value: TomlValue) {
    if let Some(old) = item.as_value() {
        *value.decor_mut() = old.decor().clone();
    }
    *item = Item::Value(value);
}

fn set_key(table: &mut Table, key: &str, value: TomlValue) {
    match table.get_mut(key) {
        Some(item) => set(item, value),
        None => {
            table.insert(key, Item::Value(value));
        }
    }
}

/// The catalogue text with `version` and, per platform, the new `url`, `size` and `sha256`. Every platform of the catalogue
/// must have a new pin.
pub fn rewrite(text: &str, version: &str, pins: &[PlatformPin]) -> Result<String, FetchError> {
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e| FetchError::failed(format!("The catalogue is not TOML: {e}")))?;
    set_key(doc.as_table_mut(), "version", TomlValue::from(version));
    let platforms = doc
        .get_mut("platform")
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| FetchError::failed("The catalogue has no [[platform]] entries."))?;
    for t in platforms.iter_mut() {
        let key = t.get("key").and_then(Item::as_str).unwrap_or_default().to_string();
        let pin = pins
            .iter()
            .find(|p| p.key == key)
            .ok_or_else(|| FetchError::failed(format!("There is no new pin for {key}.")))?;
        set_key(t, "url", TomlValue::from(pin.url.as_str()));
        set_key(t, "size", TomlValue::from(i64::try_from(pin.size).unwrap_or(i64::MAX)));
        set_key(t, "sha256", TomlValue::from(pin.sha256.as_str()));
    }
    Ok(doc.to_string())
}

/// Measures every `(key, url)` up to `runner.jobs()` at a time and returns the pins in the order given.
fn measure_all(runner: &Runner, items: Vec<(String, String)>, work: &Path) -> Result<Vec<PlatformPin>, FetchError> {
    if let Some((k, u)) = items.iter().find(|(_, u)| !url_ok(u)) {
        return Err(FetchError::failed(format!("The URL for {k} ({u}) is not an https address.")));
    }
    std::fs::create_dir_all(work).map_err(|e| FetchError::io(work.display(), &e))?;
    let mut pending = Vec::new();
    for (key, url) in items {
        let file = work.join(format!("{key}.download"));
        let downloader = runner.downloader().clone();
        let (k, u) = (key.clone(), url.clone());
        let job = runner.spawn("pin", "Measuring the archive", Some(&key), move |ctx| {
            let request = Request::new(&u, &file, Integrity::Unpinned)?.max_bytes(MAX_ARCHIVE_BYTES);
            let got = downloader.download(ctx, &request);
            let _ = std::fs::remove_file(&file);
            let got = got?;
            if got.bytes == 0 {
                return Err(JobError::Failed(format!("{u} is empty.")));
            }
            Ok(PlatformPin {
                key: k,
                url: u,
                size: got.bytes,
                sha256: got.sha256.to_hex(),
            })
        })?;
        pending.push(job);
    }
    // Wait for all of them (a failure still lets the others finish or stop), and report the first in catalogue order.
    let results: Vec<_> = pending.into_iter().map(|p| p.wait()).collect();
    results.into_iter().collect()
}

/// Reads the Chrome for Testing channel list as a job (so Cancel works while it loads).
fn read_channels(runner: &Runner, endpoint: &str) -> Result<Value, FetchError> {
    let downloader = runner.downloader().clone();
    let url = endpoint.to_string();
    runner.run("pin", "Reading the Chrome for Testing channels", Some("channel list"), move |ctx| {
        ctx.phase(Phase::items("fetch", "Reading the channel list", 1));
        let fetch = Fetch::new().cancel(ctx.cancel_token()).max_bytes(8 << 20);
        let json: Value = downloader.get_json(&url, &fetch)?;
        ctx.add(1);
        Ok(json)
    })
}

/// Re-pins the catalogue `text`. Nothing is written; the caller decides.
pub fn repin(runner: &Runner, text: &str, target: &Target, work: &Path) -> Result<Repinned, FetchError> {
    let doc: DocumentMut = text
        .parse()
        .map_err(|e| FetchError::failed(format!("The catalogue is not TOML: {e}")))?;
    let (old, entries) = read_pins(&doc)?;
    let (version, items) = match target {
        Target::Same => (old, entries),
        Target::Version(v) => {
            let moved = entries
                .into_iter()
                .map(|(k, u)| move_url(&u, &old, v).map(|u| (k, u)))
                .collect::<Result<Vec<_>, _>>()?;
            (v.clone(), moved)
        }
        Target::ChromeForTesting { channel, endpoint } => {
            let keys: Vec<String> = entries.into_iter().map(|(k, _)| k).collect();
            let (v, urls) = cft_downloads(&read_channels(runner, endpoint)?, channel, &keys)?;
            (v, keys.into_iter().zip(urls).collect())
        }
    };
    let platforms = measure_all(runner, items, work)?;
    let new = rewrite(text, &version, &platforms)?;
    Catalogue::parse(&new)
        .map_err(|e| FetchError::with_hint(format!("The re-pinned catalogue is not valid: {e}."), "Nothing was written."))?;
    Ok(Repinned {
        text: new,
        version,
        platforms,
    })
}

/// Writes `text` over `path` by writing a sibling file and renaming it.
pub fn write_atomic(path: &Path, text: &str) -> Result<(), FetchError> {
    let tmp: PathBuf = path.with_extension("toml.new");
    std::fs::write(&tmp, text).map_err(|e| FetchError::io(tmp.display(), &e))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        FetchError::io(path.display(), &e)
    })
}

#[cfg(test)]
mod tests {
    use super::super::testing::{FakeNet, Route, runner, runner_with_lines, sha256_hex};
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    const OLD: &str = "# Review the diff before committing a new pin.\nformat = 1\nname = \"chromium\"\nversion = \"1.0.0\" # the pinned version\nsource = \"s\"\nlicense = \"l\"\n\n[[platform]]\nkey = \"linux-x86_64\" # Linux\nurl = \"https://dl.test/1.0.0/linux64/chrome-linux64.zip\"\nsize = 1\nsha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"\n\n[[platform]]\nkey = \"macos-aarch64\"\nurl = \"https://dl.test/1.0.0/mac-arm64/chrome-mac-arm64.zip\"\n";

    fn linux2() -> String {
        "https://dl.test/2.0.0/linux64/chrome-linux64.zip".into()
    }
    fn mac2() -> String {
        "https://dl.test/2.0.0/mac-arm64/chrome-mac-arm64.zip".into()
    }

    #[test]
    fn urls_move_between_versions_exactly_once() {
        assert_eq!(
            move_url("https://h/1.0.0/a/b.zip", "1.0.0", "2.0.0").unwrap(),
            "https://h/2.0.0/a/b.zip"
        );
        assert!(move_url("https://h/9/a.zip", "1.0.0", "2").is_err());
        assert!(move_url("https://h/1.0.0/x/1.0.0/a.zip", "1.0.0", "2").is_err());
    }

    #[test]
    fn urls_must_be_https_or_loopback() {
        assert!(url_ok("https://a/b") && url_ok("http://127.0.0.1:80/x") && url_ok("http://localhost:1/x") && url_ok("http://[::1]:1/x"));
        assert!(!url_ok("http://example.com/x") && !url_ok("ftp://a") && !url_ok("http://127.0.0.1/x"));
    }

    fn channels() -> Value {
        serde_json::json!({"channels": {"Stable": {"version": "2.0.0", "downloads": {"chrome": [
            {"platform": "linux64", "url": linux2()},
            {"platform": "mac-arm64", "url": mac2()},
            {"platform": "win64", "url": "https://dl.test/2.0.0/win64/chrome-win64.zip"}]}}}})
    }

    #[test]
    fn chrome_for_testing_channels_give_version_and_urls() {
        let keys = vec!["linux-x86_64".to_string(), "macos-aarch64".to_string()];
        let (v, urls) = cft_downloads(&channels(), "Stable", &keys).unwrap();
        assert_eq!((v.as_str(), urls), ("2.0.0", vec![linux2(), mac2()]));
        assert!(
            cft_downloads(&channels(), "Beta", &keys)
                .unwrap_err()
                .to_string()
                .contains("no channel Beta")
        );
        let win_arm = vec!["windows-aarch64".to_string()];
        assert!(
            cft_downloads(&channels(), "Stable", &win_arm)
                .unwrap_err()
                .to_string()
                .contains("no build for windows-aarch64")
        );
        let missing = vec!["macos-x86_64".to_string()];
        assert!(cft_downloads(&channels(), "Stable", &missing).is_err());
        assert!(cft_downloads(&serde_json::json!({"channels": {"Stable": {}}}), "Stable", &keys).is_err());
        assert_eq!(cft_platform("linux-aarch64"), Some("linux-arm64"));
        assert_eq!(cft_platform("macos-x86_64"), Some("mac-x64"));
        assert_eq!(cft_platform("plan9-mips"), None);
    }

    #[test]
    fn rewriting_keeps_comments_and_order_and_adds_missing_keys() {
        let pins = [
            PlatformPin {
                key: "linux-x86_64".into(),
                url: linux2(),
                size: 42,
                sha256: "a".repeat(64),
            },
            PlatformPin {
                key: "macos-aarch64".into(),
                url: mac2(),
                size: 7,
                sha256: "b".repeat(64),
            },
        ];
        let out = rewrite(OLD, "2.0.0", &pins).unwrap();
        assert!(out.starts_with("# Review the diff before committing a new pin.\n"));
        assert!(out.contains("version = \"2.0.0\" # the pinned version\n"), "{out}");
        assert!(out.contains("key = \"linux-x86_64\" # Linux\n"));
        assert!(out.contains(&format!("url = \"{}\"\nsize = 42\nsha256 = \"{}\"\n", linux2(), "a".repeat(64))));
        assert!(
            out.ends_with(&format!("url = \"{}\"\nsize = 7\nsha256 = \"{}\"\n", mac2(), "b".repeat(64))),
            "{out}"
        );
        assert!(out.find("linux-x86_64").unwrap() < out.find("macos-aarch64").unwrap());
        assert!(
            rewrite(OLD, "2", &pins[..1])
                .unwrap_err()
                .to_string()
                .contains("no new pin for macos-aarch64")
        );
        assert!(rewrite("not toml [", "2", &pins).is_err());
        assert!(rewrite("version = \"1\"\n", "2", &pins).is_err());
        let no_version = rewrite("[[platform]]\nkey = \"linux-x86_64\"\nurl = \"u\"\n", "3", &pins[..1]).unwrap();
        assert!(no_version.contains("version = \"3\""));
    }

    fn net() -> Arc<FakeNet> {
        let n = FakeNet::new();
        n.serve(&linux2(), b"linux archive".to_vec());
        n.serve(&mac2(), b"mac archive!!".to_vec());
        n.serve("https://dl.test/1.0.0/linux64/chrome-linux64.zip", b"old linux".to_vec());
        n.serve("https://dl.test/1.0.0/mac-arm64/chrome-mac-arm64.zip", b"old mac".to_vec());
        n.serve("https://dl.test/channels.json", serde_json::to_vec(&channels()).unwrap());
        n
    }

    #[test]
    fn a_version_move_measures_the_new_archives_in_catalogue_order() {
        let n = net();
        let work = tempfile::tempdir().unwrap();
        let (r, lines) = runner_with_lines(&n, 2);
        let out = repin(&r, OLD, &Target::Version("2.0.0".into()), &work.path().join("w")).unwrap();
        assert_eq!(out.version, "2.0.0");
        assert_eq!(
            out.platforms[0],
            PlatformPin {
                key: "linux-x86_64".into(),
                url: linux2(),
                size: 13,
                sha256: sha256_hex(b"linux archive")
            }
        );
        assert_eq!(out.platforms[1].sha256, sha256_hex(b"mac archive!!"));
        let parsed = Catalogue::parse(&out.text).unwrap();
        assert_eq!(parsed.version, "2.0.0");
        assert!(
            std::fs::read_dir(work.path().join("w")).unwrap().next().is_none(),
            "no archive is kept"
        );
        let text = String::from_utf8(lines.lock().unwrap().clone()).unwrap();
        assert!(
            text.contains("Downloading") && text.contains("Finished: Measuring the archive"),
            "{text}"
        );
    }

    #[test]
    fn the_same_target_remeasures_and_the_channel_target_reads_the_list() {
        let n = net();
        let work = tempfile::tempdir().unwrap();
        let same = repin(&runner(&n, 3), OLD, &Target::Same, work.path()).unwrap();
        assert_eq!(same.version, "1.0.0");
        assert_eq!(same.platforms[0].size, 9);
        let cft = Target::ChromeForTesting {
            channel: "Stable".into(),
            endpoint: "https://dl.test/channels.json".into(),
        };
        let out = repin(&runner(&n, 1), OLD, &cft, work.path()).unwrap();
        assert_eq!((out.version.as_str(), out.platforms[1].url.as_str()), ("2.0.0", mac2().as_str()));
        assert_eq!(n.asked("https://dl.test/channels.json"), 1);
    }

    #[test]
    fn failures_write_nothing_and_name_the_cause() {
        let n = net();
        let work = tempfile::tempdir().unwrap();
        let r = runner(&n, 2);
        let v = |s: &str| Target::Version(s.into());
        assert!(repin(&r, OLD, &v("3.0.0"), work.path()).is_err(), "no route for the moved URLs");
        assert!(repin(&r, "x = [", &Target::Same, work.path()).is_err());
        assert!(
            repin(&r, "format = 1\n", &Target::Same, work.path())
                .unwrap_err()
                .to_string()
                .contains("no version")
        );
        assert!(
            repin(&r, "version = \"1\"\n", &Target::Same, work.path())
                .unwrap_err()
                .to_string()
                .contains("[[platform]]")
        );
        assert!(
            repin(&r, "version = \"1\"\n[[platform]]\nkey = \"a\"\n", &Target::Same, work.path())
                .unwrap_err()
                .to_string()
                .contains("key and a url")
        );
        let http = OLD.replace("https://dl.test/1.0.0/linux64", "http://example.com/1.0.0/linux64");
        assert!(
            repin(&r, &http, &Target::Same, work.path())
                .unwrap_err()
                .to_string()
                .contains("not an https address")
        );
        assert!(repin(&r, OLD, &v("9"), work.path()).is_err());
        n.serve(&linux2(), Vec::new());
        n.route(&mac2(), Route::Status(404));
        assert!(repin(&r, OLD, &v("2.0.0"), work.path()).is_err(), "an empty archive and a 404");
        let bad_work = work.path().join("file");
        std::fs::write(&bad_work, "x").unwrap();
        assert!(repin(&r, OLD, &Target::Same, &bad_work).is_err(), "a work folder that is a file");
        let cft = |e: &str| Target::ChromeForTesting {
            channel: "Stable".into(),
            endpoint: e.into(),
        };
        assert!(repin(&r, OLD, &cft("https://dl.test/none.json"), work.path()).is_err());
        n.serve("https://dl.test/bad.json", b"not json".to_vec());
        assert!(repin(&r, OLD, &cft("https://dl.test/bad.json"), work.path()).is_err());
        // A catalogue that would no longer parse after the pin (a URL that is not an archive) is refused.
        let odd = "version = \"1\"\nname=\"chromium\"\nformat = 1\n[[platform]]\nkey = \"linux-x86_64\"\nurl = \"https://dl.test/1/x\"\n";
        n.serve("https://dl.test/1/x", b"abc".to_vec());
        assert!(
            repin(&r, odd, &Target::Same, work.path())
                .unwrap_err()
                .to_string()
                .contains("not valid")
        );
    }

    #[test]
    fn a_cancel_while_bytes_flow_stops_every_download_and_keeps_nothing() {
        let n = net();
        let work = tempfile::tempdir().unwrap();
        let r = Arc::new(runner(&n, 2));
        let fired = Arc::new(AtomicBool::new(false));
        let (r2, f2) = (r.clone(), fired.clone());
        n.route(
            &linux2(),
            Route::Endless(Arc::new(move || {
                f2.store(true, Ordering::SeqCst);
                r2.cancel_all();
            })),
        );
        n.route(&mac2(), Route::Endless(Arc::new(|| {})));
        let err = repin(&r, OLD, &Target::Version("2.0.0".into()), work.path()).unwrap_err();
        assert_eq!(err, FetchError::Cancelled);
        assert!(fired.load(Ordering::SeqCst));
    }

    #[test]
    fn the_file_is_replaced_whole_or_not_at_all() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.toml");
        std::fs::write(&path, "old").unwrap();
        write_atomic(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert!(!dir.path().join("c.toml.new").exists());
        assert!(write_atomic(&dir.path().join("missing").join("c.toml"), "x").is_err());
        let target = dir.path().join("d.toml");
        std::fs::create_dir(&target).unwrap();
        assert!(write_atomic(&target, "x").is_err(), "a folder is not replaced by a file");
        assert!(!dir.path().join("d.toml.new").exists());
    }
}
