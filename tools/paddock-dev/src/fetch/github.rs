//! What the release updaters (`fetch llama`, `fetch ffmpeg`) share: reading a GitHub release as JSON through the downloader
//! (same transport, proxy and host caps as every other download), the SHA-256 GitHub computed for each asset, and the
//! optional `--verify` that downloads every chosen asset and checks that digest. Write a manifest only after all of it held.

use super::error::FetchError;
use super::runner::Runner;
use paddock_download::{Downloader, Fetch, Integrity, Request, Sha256Digest};
use paddock_jobs::{JobContext, JobError};
use regex::Regex;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The largest API answer read (a page of 100 releases with their assets is a few MB).
const MAX_API_BYTES: u64 = 32 << 20;
/// The largest asset `--verify` downloads.
const MAX_ASSET_BYTES: u64 = 4 << 30;

/// A GET of a GitHub API URL as JSON; `token` (a `GITHUB_TOKEN`) lifts the anonymous rate limit.
pub fn api_json(d: &Downloader, ctx: &JobContext, url: &str, token: Option<&str>) -> Result<Value, JobError> {
    let mut fetch = Fetch::new()
        .cancel(ctx.cancel_token())
        .max_bytes(MAX_API_BYTES)
        .header("Accept", "application/vnd.github+json")?;
    if let Some(t) = token.filter(|t| !t.is_empty()) {
        fetch = fetch.header("Authorization", &format!("Bearer {t}"))?;
    }
    Ok(d.get_json(url, &fetch)?)
}

/// A GET of a small text file (the llama.cpp release's `nightly-tag.txt`).
pub fn text(d: &Downloader, ctx: &JobContext, url: &str) -> Result<String, JobError> {
    let fetch = Fetch::new().cancel(ctx.cancel_token()).max_bytes(64 * 1024);
    Ok(d.get_text(url, &fetch)?)
}

/// The assets of a release object, in the order GitHub lists them.
pub fn assets(release: &Value) -> Vec<&Value> {
    release
        .get("assets")
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default()
}

/// The text of an asset field, or "".
pub fn field<'a>(asset: &'a Value, name: &str) -> &'a str {
    asset.get(name).and_then(Value::as_str).unwrap_or("")
}

/// The asset whose name matches `pattern` (a regular expression, anchored), the last in name order when several do.
pub fn pick<'a>(assets: &[&'a Value], pattern: &str) -> Result<Option<&'a Value>, FetchError> {
    let rx =
        Regex::new(&format!("^{pattern}$")).map_err(|e| FetchError::failed(format!("The asset pattern {pattern} is not valid: {e}.")))?;
    let mut hits: Vec<&'a Value> = assets.iter().copied().filter(|a| rx.is_match(field(a, "name"))).collect();
    hits.sort_by(|a, b| field(a, "name").cmp(field(b, "name")));
    Ok(hits.pop())
}

/// The SHA-256 GitHub computed for `asset` when it was uploaded (`digest: "sha256:<64 hex>"`).
pub fn digest(asset: &Value) -> Result<String, FetchError> {
    let d = field(asset, "digest").strip_prefix("sha256:").unwrap_or("");
    if d.len() == 64 && Sha256Digest::from_hex(d).is_ok() {
        Ok(d.to_ascii_lowercase())
    } else {
        Err(FetchError::failed(format!(
            "{}: the release API gave no SHA-256.",
            field(asset, "name")
        )))
    }
}

/// One asset to check: name, URL and the digest it must have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// The asset name.
    pub name: String,
    /// Where to download it.
    pub url: String,
    /// The SHA-256 it must have.
    pub sha256: String,
}

/// Downloads every asset (up to `runner.jobs()` at a time; each file is removed once checked) and fails on the first, in the
/// order given, whose bytes do not have the digest the release API gave.
pub fn verify_all(runner: &Runner, checks: Vec<Check>, work: &Path) -> Result<(), FetchError> {
    std::fs::create_dir_all(work).map_err(|e| FetchError::io(work.display(), &e))?;
    let mut pending = Vec::new();
    for (i, c) in checks.into_iter().enumerate() {
        let digest = Sha256Digest::from_hex(&c.sha256).map_err(|e| FetchError::failed(format!("{}: {e}.", c.name)))?;
        let file: PathBuf = work.join(format!("{i}.download"));
        let downloader = runner.downloader().clone();
        let url = c.url.clone();
        let job = runner.spawn("verify", "Verifying the asset", Some(&c.name), move |ctx| {
            let request = Request::new(&url, &file, Integrity::Sha256(digest))?.max_bytes(MAX_ASSET_BYTES);
            let got = downloader.download(ctx, &request);
            let _ = std::fs::remove_file(&file);
            got.map(|_| ()).map_err(JobError::from)
        })?;
        pending.push((c.name, job));
    }
    let mut first: Option<FetchError> = None;
    for (name, job) in pending {
        if let Err(e) = job.wait() {
            first.get_or_insert_with(|| match e {
                FetchError::Failed { message, hint } => FetchError::Failed {
                    message: format!("{name}: {message}"),
                    hint,
                },
                other => other,
            });
        }
    }
    first.map_or(Ok(()), Err)
}

/// Writes the manifest as pretty JSON with a final newline, over `path` by renaming a sibling file.
pub fn write_manifest(path: &Path, manifest: &Value) -> Result<(), FetchError> {
    let mut text =
        serde_json::to_string_pretty(manifest).map_err(|e| FetchError::failed(format!("The manifest cannot be written: {e}.")))?;
    text.push('\n');
    let tmp = path.with_extension("json.new");
    std::fs::write(&tmp, text).map_err(|e| FetchError::io(tmp.display(), &e))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        FetchError::io(path.display(), &e)
    })
}

#[cfg(test)]
mod tests {
    use super::super::testing::{FakeNet, runner, sha256_hex};
    use super::*;
    use serde_json::json;

    fn release() -> Value {
        json!({"assets": [
            {"name": "b-1.zip", "digest": format!("sha256:{}", "a".repeat(64))},
            {"name": "b-2.zip", "digest": "sha256:short"},
            {"name": "c.tar.gz"}]})
    }

    #[test]
    fn picks_the_last_name_that_matches_the_anchored_pattern() {
        let rel = release();
        let all = assets(&rel);
        assert_eq!(all.len(), 3);
        assert_eq!(field(pick(&all, r"b-[0-9]\.zip").unwrap().unwrap(), "name"), "b-2.zip");
        assert!(pick(&all, r"b-").unwrap().is_none(), "anchored at both ends");
        assert!(pick(&all, "(").is_err());
        assert!(assets(&json!({})).is_empty());
        assert_eq!(field(&json!({}), "name"), "");
    }

    #[test]
    fn the_digest_must_be_a_sha256() {
        let rel = release();
        let all = assets(&rel);
        assert_eq!(digest(all[0]).unwrap(), "a".repeat(64));
        assert!(
            digest(all[1])
                .unwrap_err()
                .to_string()
                .contains("b-2.zip: the release API gave no SHA-256.")
        );
        assert!(digest(all[2]).is_err());
        let upper = json!({"name": "x", "digest": format!("sha256:{}", "A".repeat(64))});
        assert_eq!(digest(&upper).unwrap(), "a".repeat(64));
        let not_hex = json!({"name": "x", "digest": format!("sha256:{}", "g".repeat(64))});
        assert!(digest(&not_hex).is_err());
    }

    #[test]
    fn verification_checks_every_asset_and_names_the_one_that_differs() {
        let net = FakeNet::new();
        net.serve("https://dl.test/a", b"alpha".to_vec());
        net.serve("https://dl.test/b", b"beta".to_vec());
        let work = tempfile::tempdir().unwrap();
        let r = runner(&net, 2);
        let check = |name: &str, body: &[u8]| Check {
            name: name.into(),
            url: format!("https://dl.test/{}", &name[..1]),
            sha256: sha256_hex(body),
        };
        verify_all(&r, vec![check("a.zip", b"alpha"), check("b.zip", b"beta")], work.path()).unwrap();
        assert!(std::fs::read_dir(work.path()).unwrap().next().is_none());
        let err = verify_all(&r, vec![check("a.zip", b"alpha"), check("b.zip", b"other")], work.path()).unwrap_err();
        assert!(err.to_string().starts_with("b.zip: "), "{err}");
        let bad = Check {
            name: "z.zip".into(),
            url: "https://dl.test/z".into(),
            sha256: "nothex".into(),
        };
        assert!(verify_all(&r, vec![bad], work.path()).is_err());
        let file = work.path().join("f");
        std::fs::write(&file, "x").unwrap();
        assert!(verify_all(&r, vec![], &file).is_err(), "a work folder that is a file");
    }

    #[test]
    fn the_api_reads_json_with_an_optional_token() {
        let net = FakeNet::new();
        net.serve("https://api.test/r", br#"{"tag_name": "b1"}"#.to_vec());
        let r = runner(&net, 1);
        let d = r.downloader().clone();
        let out = r
            .run("t", "t", None, move |ctx| {
                let a = api_json(&d, ctx, "https://api.test/r", None)?;
                let b = api_json(&d, ctx, "https://api.test/r", Some("secret"))?;
                let c = api_json(&d, ctx, "https://api.test/r", Some(""))?;
                Ok((a, b, c))
            })
            .unwrap();
        assert_eq!(out.0["tag_name"], "b1");
        assert_eq!(out.1, out.0);
        assert_eq!(out.2, out.0);
        net.serve("https://api.test/t.txt", b"b99\n".to_vec());
        let d = r.downloader().clone();
        assert_eq!(
            r.run("t", "t", None, move |ctx| text(&d, ctx, "https://api.test/t.txt")).unwrap(),
            "b99\n"
        );
    }

    #[test]
    fn manifests_are_written_pretty_with_a_newline_or_not_at_all() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.json");
        write_manifest(&path, &json!({"b": 1, "a": [1, 2], "c": []})).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    2\n  ],\n  \"c\": []\n}\n"
        );
        assert!(write_manifest(&dir.path().join("none").join("m.json"), &json!({})).is_err());
        let folder = dir.path().join("f.json");
        std::fs::create_dir(&folder).unwrap();
        assert!(write_manifest(&folder, &json!({})).is_err());
        assert!(!dir.path().join("f.json.new").exists());
    }
}
