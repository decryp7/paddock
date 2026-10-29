//! `dev fetch ffmpeg`: pins the FFmpeg builds Debrief downloads for its video support (Debrief ADR 0033). Writes
//! `ffmpeg-runtimes.json`: one month-end autobuild of BtbN/FFmpeg-Builds (those are kept for about two years; daily builds are
//! deleted within weeks) on the release branch given, and per platform its `lgpl-shared` archive with URL, size and SHA-256.
//! Only LGPL builds are ever pinned (no GPL components such as x264/x265). There is no macOS entry: no publisher offers LGPL
//! macOS builds with stable URLs and checksums. Replaces `apps/debrief/scripts/update-ffmpeg-runtimes.py`.

use super::error::FetchError;
use super::github::{self, Check};
use super::runner::Runner;
use paddock_jobs::{JobError, Phase};
use regex::Regex;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// The repository the builds come from.
pub const REPO: &str = "BtbN/FFmpeg-Builds";
/// The default output, relative to the repository root.
pub const DEFAULT_OUT: &str = "apps/debrief/crates/debrief-video/src/runtime/ffmpeg-runtimes.json";
/// The release branch the builds follow unless another is asked for.
pub const DEFAULT_BRANCH: &str = "9.0";
/// How many pages of 100 releases are searched for a month-end build.
const MAX_PAGES: u32 = 10;

/// platform, then the archive name pattern (`{version}` is the FFmpeg version, `{branch}` the release branch).
const PLATFORMS: &[(&str, &str)] = &[
    ("windows-x64", r"ffmpeg-{version}-win64-lgpl-shared-{branch}\.zip"),
    ("windows-arm64", r"ffmpeg-{version}-winarm64-lgpl-shared-{branch}\.zip"),
    ("linux-x64", r"ffmpeg-{version}-linux64-lgpl-shared-{branch}\.tar\.xz"),
    ("linux-arm64", r"ffmpeg-{version}-linuxarm64-lgpl-shared-{branch}\.tar\.xz"),
];

fn tag_rx() -> Regex {
    Regex::new(r"^autobuild-(\d{4})-(\d{2})-(\d{2})-\d{2}-\d{2}$").unwrap_or_else(|_| unreachable!("the pattern is a literal"))
}

/// Whether an autobuild tag was made on the last day of its month (those are the ones kept long).
pub fn month_end(tag: &str) -> bool {
    let Some(c) = tag_rx().captures(tag) else {
        return false;
    };
    let n = |i: usize| c[i].parse::<u32>().unwrap_or(0);
    let (y, m, d) = (n(1), n(2), n(3));
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => return false,
    };
    d == days
}

/// The tag of the newest (by `published_at`) month-end autobuild among `releases`.
pub fn newest_month_end(releases: &[Value]) -> Option<String> {
    let mut best: Option<&Value> = None;
    for r in releases {
        let tag = r.get("tag_name").and_then(Value::as_str).unwrap_or("");
        if month_end(tag) && best.is_none_or(|b| field(r, "published_at") > field(b, "published_at")) {
            best = Some(r);
        }
    }
    best.map(|b| field(b, "tag_name").to_string())
}

fn field<'a>(v: &'a Value, name: &str) -> &'a str {
    github::field(v, name)
}

/// The FFmpeg version of the release branch's builds (`n9.0.2-17-g2a571b6068` in `ffmpeg-n9.0.2-17-g2a571b6068-win64-...`);
/// the last matching asset wins.
pub fn version_of(assets: &[&Value], branch: &str) -> Result<Option<String>, FetchError> {
    let b = regex::escape(branch);
    let probe = Regex::new(&format!("^ffmpeg-(n{b}[0-9.]*-[0-9]+-g[0-9a-f]+)-win64-lgpl-shared-{b}\\.zip$"))
        .map_err(|e| FetchError::failed(format!("The branch '{branch}' makes no valid pattern: {e}.")))?;
    Ok(assets
        .iter()
        .rev()
        .find_map(|a| probe.captures(field(a, "name")).map(|c| c[1].to_string())))
}

/// The manifest for autobuild `tag` from its release object `rel`.
pub fn build_manifest(tag: &str, branch: &str, rel: &Value) -> Result<(Value, Vec<String>, Vec<Check>), FetchError> {
    if !tag_rx().is_match(tag) {
        return Err(FetchError::failed(format!("{tag} is not an autobuild tag.")));
    }
    let assets = github::assets(rel);
    let version = version_of(&assets, branch)?
        .ok_or_else(|| FetchError::failed(format!("{tag} has no LGPL shared build of the {branch} branch.")))?;
    let (mut platforms, mut notes, mut checks) = (Map::new(), Vec::new(), Vec::new());
    // In platform-name order, as the committed manifest is (the Python updater listed Windows first).
    let mut table: Vec<&(&str, &str)> = PLATFORMS.iter().collect();
    table.sort_by_key(|(platform, _)| *platform);
    for (platform, pattern) in table {
        let pattern = pattern
            .replace("{version}", &regex::escape(&version))
            .replace("{branch}", &regex::escape(branch));
        let Some(a) = github::pick(&assets, &pattern)? else {
            notes.push(format!("{platform} is not published for {tag}."));
            continue;
        };
        let sha = github::digest(a)?;
        checks.push(Check {
            name: field(a, "name").to_string(),
            url: field(a, "browser_download_url").to_string(),
            sha256: sha.clone(),
        });
        platforms.insert(
            (*platform).to_string(),
            json!({"name": a.get("name"), "url": a.get("browser_download_url"), "size": a.get("size"), "sha256": sha}),
        );
    }
    let manifest = json!({
        "version": 1,
        "source": REPO,
        "tag": tag,
        "release": version,
        "license": "LGPL-2.1-or-later",
        "platforms": Value::Object(platforms),
    });
    Ok((manifest, notes, checks))
}

/// What to pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// An autobuild tag; `None` is the newest month-end autobuild.
    pub tag: Option<String>,
    /// The FFmpeg release branch the builds follow.
    pub branch: String,
    /// Download every chosen asset and check its SHA-256.
    pub verify: bool,
    /// The manifests to write (the same content in each).
    pub out: Vec<PathBuf>,
    /// A `GITHUB_TOKEN`, to lift the anonymous rate limit.
    pub token: Option<String>,
    /// Where `--verify` puts the downloads while it checks them.
    pub work: PathBuf,
}

/// What was pinned.
#[derive(Debug, Clone, PartialEq)]
pub struct Pinned {
    /// The manifest written.
    pub manifest: Value,
    /// Platforms the build does not publish.
    pub notes: Vec<String>,
    /// Assets checked by `--verify`.
    pub verified: usize,
}

fn api() -> String {
    format!("https://api.github.com/repos/{REPO}/releases")
}

/// Pins the build, checks it when asked and writes the manifests.
pub fn pin(runner: &Runner, opts: &Options) -> Result<Pinned, FetchError> {
    let (downloader, tag, branch, token) = (
        runner.downloader().clone(),
        opts.tag.clone(),
        opts.branch.clone(),
        opts.token.clone(),
    );
    let (manifest, notes, checks) = runner.run("pin", "Pinning FFmpeg", Some(REPO), move |ctx| {
        ctx.phase(Phase::discovering("find", "Finding the month-end build"));
        let tag = match tag {
            Some(t) => t,
            None => {
                let mut found = None;
                for page in 1..=MAX_PAGES {
                    let releases = github::api_json(&downloader, ctx, &format!("{}?per_page=100&page={page}", api()), token.as_deref())?;
                    ctx.add(1);
                    let list = releases.as_array().cloned().unwrap_or_default();
                    if list.is_empty() {
                        break;
                    }
                    found = newest_month_end(&list);
                    if found.is_some() {
                        break;
                    }
                }
                found.ok_or_else(|| JobError::Failed("No month-end autobuild found.".into()))?
            }
        };
        ctx.phase(Phase::items("read", "Reading the release", 1));
        let rel = github::api_json(&downloader, ctx, &format!("{}/tags/{tag}", api()), token.as_deref())?;
        ctx.add(1);
        build_manifest(&tag, &branch, &rel).map_err(|e| JobError::Failed(e.to_string()))
    })?;
    let verified = if opts.verify {
        let n = checks.len();
        github::verify_all(runner, checks, &opts.work)?;
        n
    } else {
        0
    };
    for out in &opts.out {
        github::write_manifest(out, &manifest)?;
    }
    Ok(Pinned { manifest, notes, verified })
}

/// The default output path under `root`.
pub fn default_out(root: &Path) -> PathBuf {
    root.join(DEFAULT_OUT)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{FakeNet, runner, sha256_hex};
    use super::*;

    fn committed() -> (Value, String) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(DEFAULT_OUT);
        let text = std::fs::read_to_string(path).unwrap();
        (serde_json::from_str(&text).unwrap(), text)
    }

    fn release_of(manifest: &Value) -> Value {
        let assets: Vec<Value> = manifest["platforms"]
            .as_object()
            .unwrap()
            .values()
            .map(|a| json!({"name": a["name"], "browser_download_url": a["url"], "size": a["size"], "digest": format!("sha256:{}", a["sha256"].as_str().unwrap())}))
            .collect();
        json!({"assets": assets})
    }

    #[test]
    fn rebuilding_the_committed_manifest_from_its_own_assets_gives_it_back_byte_for_byte() {
        let (manifest, text) = committed();
        let branch = manifest["platforms"]["windows-x64"]["name"]
            .as_str()
            .unwrap()
            .rsplit('-')
            .next()
            .unwrap()
            .trim_end_matches(".zip")
            .to_string();
        let (built, _, checks) = build_manifest(manifest["tag"].as_str().unwrap(), &branch, &release_of(&manifest)).unwrap();
        assert_eq!(built, manifest);
        assert_eq!(checks.len(), manifest["platforms"].as_object().unwrap().len());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.json");
        github::write_manifest(&path, &built).unwrap();
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            text,
            "the same bytes the Python updater wrote"
        );
    }

    #[test]
    fn month_end_tags_are_the_last_day_of_their_month() {
        for t in [
            "autobuild-2026-09-30-13-08",
            "autobuild-2026-10-31-00-00",
            "autobuild-2026-02-28-01-01",
            "autobuild-2028-02-29-01-01",
            "autobuild-2000-02-29-01-01",
        ] {
            assert!(month_end(t), "{t}");
        }
        for t in [
            "autobuild-2026-09-29-13-08",
            "autobuild-2026-02-29-01-01",
            "autobuild-2100-02-29-01-01",
            "autobuild-2026-13-31-01-01",
            "latest",
            "autobuild-2026-09-30-13",
            "autobuild-2026-00-30-13-08",
        ] {
            assert!(!month_end(t), "{t}");
        }
        let rels = [
            json!({"tag_name": "autobuild-2026-08-31-10-00", "published_at": "2026-08-31T10:30:00Z"}),
            json!({"tag_name": "autobuild-2026-09-15-10-00", "published_at": "2026-09-15T10:30:00Z"}),
            json!({"tag_name": "autobuild-2026-09-30-13-08", "published_at": "2026-09-30T13:30:00Z"}),
            json!({"tag_name": "latest", "published_at": "2026-10-30T13:30:00Z"}),
        ];
        assert_eq!(newest_month_end(&rels).as_deref(), Some("autobuild-2026-09-30-13-08"));
        assert_eq!(newest_month_end(&rels[1..2]), None);
        assert_eq!(newest_month_end(&[]), None);
    }

    fn mini(tag: &str) -> Value {
        let a = |n: &str, body: &[u8]| json!({"name": n, "browser_download_url": format!("https://dl.test/{n}"), "size": body.len(), "digest": format!("sha256:{}", sha256_hex(body))});
        let _ = tag;
        json!({"assets": [
            a("ffmpeg-n9.0.1-3-gabc-win64-lgpl-shared-9.0.zip", b"old"),
            a("ffmpeg-n9.0.2-17-g2a571b6068-win64-lgpl-shared-9.0.zip", b"win"),
            a("ffmpeg-n9.0.2-17-g2a571b6068-linux64-lgpl-shared-9.0.tar.xz", b"lin"),
            a("ffmpeg-n9.0.2-17-g2a571b6068-win64-gpl-9.0.zip", b"gpl")]})
    }

    #[test]
    fn builds_the_manifest_for_the_branch_and_notes_missing_platforms() {
        let (m, notes, checks) = build_manifest("autobuild-2026-09-30-13-08", "9.0", &mini("t")).unwrap();
        assert_eq!(m["release"], "n9.0.2-17-g2a571b6068");
        assert_eq!(
            m["platforms"].as_object().unwrap().keys().collect::<Vec<_>>(),
            ["linux-x64", "windows-x64"]
        );
        assert_eq!(
            notes,
            [
                "linux-arm64 is not published for autobuild-2026-09-30-13-08.",
                "windows-arm64 is not published for autobuild-2026-09-30-13-08."
            ]
        );
        assert_eq!(checks.len(), 2);
        assert!(
            build_manifest("autobuild-2026-09-30-13-08", "8.1", &mini("t"))
                .unwrap_err()
                .to_string()
                .contains("no LGPL shared build of the 8.1 branch")
        );
        assert!(
            build_manifest("latest", "9.0", &mini("t"))
                .unwrap_err()
                .to_string()
                .contains("not an autobuild tag")
        );
        let no_digest = json!({"assets": [{"name": "ffmpeg-n9.0.2-1-gabc-win64-lgpl-shared-9.0.zip"}]});
        assert!(build_manifest("autobuild-2026-09-30-13-08", "9.0", &no_digest).is_err());
    }

    fn served() -> std::sync::Arc<FakeNet> {
        let net = FakeNet::new();
        let page = json!([{"tag_name": "autobuild-2026-09-30-13-08", "published_at": "2026-09-30T13:30:00Z"}]);
        net.serve(&format!("{}?per_page=100&page=1", api()), serde_json::to_vec(&page).unwrap());
        net.serve(
            &format!("{}/tags/autobuild-2026-09-30-13-08", api()),
            serde_json::to_vec(&mini("t")).unwrap(),
        );
        net.serve(
            "https://dl.test/ffmpeg-n9.0.2-17-g2a571b6068-win64-lgpl-shared-9.0.zip",
            b"win".to_vec(),
        );
        net.serve(
            "https://dl.test/ffmpeg-n9.0.2-17-g2a571b6068-linux64-lgpl-shared-9.0.tar.xz",
            b"lin".to_vec(),
        );
        net
    }

    #[test]
    fn finds_the_newest_month_end_pins_it_and_verifies() {
        let net = served();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("f.json");
        let opts = Options {
            tag: None,
            branch: DEFAULT_BRANCH.into(),
            verify: true,
            out: vec![out.clone()],
            token: None,
            work: dir.path().join("w"),
        };
        let pinned = pin(&runner(&net, 2), &opts).unwrap();
        assert_eq!(
            (pinned.verified, pinned.manifest["tag"].clone()),
            (2, "autobuild-2026-09-30-13-08".into())
        );
        assert!(out.is_file());
        assert_eq!(default_out(Path::new("r")), Path::new("r").join(DEFAULT_OUT));
        // A given tag skips the search.
        let given = Options {
            tag: Some("autobuild-2026-09-30-13-08".into()),
            verify: false,
            ..opts.clone()
        };
        let before = net.seen().len();
        assert_eq!(pin(&runner(&net, 1), &given).unwrap().verified, 0);
        assert_eq!(net.seen().len() - before, 1);
    }

    #[test]
    fn no_month_end_in_ten_pages_or_a_bad_digest_is_an_error_and_writes_nothing() {
        let net = FakeNet::new();
        let none = serde_json::to_vec(&json!([{"tag_name": "autobuild-2026-09-15-10-00", "published_at": "x"}])).unwrap();
        for page in 1..=MAX_PAGES {
            net.serve(&format!("{}?per_page=100&page={page}", api()), none.clone());
        }
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("f.json");
        let opts = Options {
            tag: None,
            branch: "9.0".into(),
            verify: false,
            out: vec![out.clone()],
            token: None,
            work: dir.path().join("w"),
        };
        assert!(
            pin(&runner(&net, 1), &opts)
                .unwrap_err()
                .to_string()
                .contains("No month-end autobuild found")
        );
        net.serve(&format!("{}?per_page=100&page=1", api()), b"[]".to_vec());
        assert!(pin(&runner(&net, 1), &opts).is_err(), "an empty page ends the search");
        let net = served();
        net.serve(
            "https://dl.test/ffmpeg-n9.0.2-17-g2a571b6068-win64-lgpl-shared-9.0.zip",
            b"changed".to_vec(),
        );
        let verify = Options { verify: true, ..opts };
        assert!(pin(&runner(&net, 2), &verify).is_err());
        assert!(!out.exists());
        assert!(version_of(&[], "(").is_ok(), "the branch is escaped, not read as a pattern");
    }
}
