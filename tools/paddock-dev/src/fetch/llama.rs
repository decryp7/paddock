//! `dev fetch llama`: pins the llama.cpp release the apps download their inference runtimes from (Debrief ADR 0009).
//! Writes `llama-runtimes.json`: the release tag and, per platform and build, the official prebuilt assets with their URL,
//! size and SHA-256. The app embeds the file and downloads nothing that is not listed in it. The checksums are the digests
//! GitHub computed at upload; `--verify` downloads every asset and recomputes them. Review the diff: a new tag is a
//! supply-chain change. Replaces `apps/debrief/scripts/update-llama-runtimes.py`.

use super::error::FetchError;
use super::github::{self, Check};
use super::runner::Runner;
use paddock_jobs::{JobError, Phase};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// The repository the releases come from.
pub const REPO: &str = "ggml-org/llama.cpp";
/// The default output, relative to the repository root.
pub const DEFAULT_OUT: &str = "apps/debrief/crates/debrief-ai/src/runtimes/llama-runtimes.json";

/// platform, then build id, then the asset name patterns (`{tag}` is the build tag). The first pattern is the llama-server
/// archive; the others are runtimes unpacked into the same folder (CUDA's cudart).
type Builds = &'static [(&'static str, &'static [&'static str])];
const BUILDS: &[(&str, Builds)] = &[
    (
        "windows-x64",
        &[
            ("cpu", &[r"llama-{tag}-bin-win-cpu-x64\.zip"]),
            ("vulkan", &[r"llama-{tag}-bin-win-vulkan-x64\.zip"]),
            (
                "cuda-12",
                &[
                    r"llama-{tag}-bin-win-cuda-12\.[0-9]+-x64\.zip",
                    r"cudart-llama-bin-win-cuda-12\.[0-9]+-x64\.zip",
                ],
            ),
            (
                "cuda-13",
                &[
                    r"llama-{tag}-bin-win-cuda-13\.[0-9]+-x64\.zip",
                    r"cudart-llama-bin-win-cuda-13\.[0-9]+-x64\.zip",
                ],
            ),
            ("rocm", &[r"llama-{tag}-bin-win-rocm-[0-9.]+-x64\.zip"]),
            ("sycl", &[r"llama-{tag}-bin-win-sycl-x64\.zip"]),
            ("openvino", &[r"llama-{tag}-bin-win-openvino-[0-9.]+-x64\.zip"]),
        ],
    ),
    (
        "windows-arm64",
        &[
            ("cpu", &[r"llama-{tag}-bin-win-cpu-arm64\.zip"]),
            ("vulkan", &[r"llama-{tag}-bin-win-vulkan-arm64\.zip"]),
            (
                "cuda-13",
                &[
                    r"llama-{tag}-bin-win-cuda-13\.[0-9]+-arm64\.zip",
                    r"cudart-llama-bin-win-cuda-13\.[0-9]+-arm64\.zip",
                ],
            ),
        ],
    ),
    (
        "linux-x64",
        &[
            ("cpu", &[r"llama-{tag}-bin-ubuntu-x64\.tar\.gz"]),
            ("vulkan", &[r"llama-{tag}-bin-ubuntu-vulkan-x64\.tar\.gz"]),
            (
                "cuda-12",
                &[
                    r"llama-{tag}-bin-ubuntu-cuda-12\.[0-9]+-x64\.tar\.gz",
                    r"cudart-llama-{tag}-bin-ubuntu-cuda-12\.[0-9]+-x64\.tar\.gz",
                ],
            ),
            (
                "cuda-13",
                &[
                    r"llama-{tag}-bin-ubuntu-cuda-13\.[0-9]+-x64\.tar\.gz",
                    r"cudart-llama-{tag}-bin-ubuntu-cuda-13\.[0-9]+-x64\.tar\.gz",
                ],
            ),
            ("rocm", &[r"llama-{tag}-bin-ubuntu-rocm-[0-9.]+-x64\.tar\.gz"]),
            ("sycl", &[r"llama-{tag}-bin-ubuntu-sycl-fp16-x64\.tar\.gz"]),
            ("sycl-fp32", &[r"llama-{tag}-bin-ubuntu-sycl-fp32-x64\.tar\.gz"]),
            ("openvino", &[r"llama-{tag}-bin-ubuntu-openvino-[0-9.]+-x64\.tar\.gz"]),
        ],
    ),
    (
        "linux-arm64",
        &[
            ("cpu", &[r"llama-{tag}-bin-ubuntu-arm64\.tar\.gz"]),
            ("vulkan", &[r"llama-{tag}-bin-ubuntu-vulkan-arm64\.tar\.gz"]),
            (
                "cuda-13",
                &[
                    r"llama-{tag}-bin-ubuntu-cuda-13\.[0-9]+-arm64\.tar\.gz",
                    r"cudart-llama-{tag}-bin-ubuntu-cuda-13\.[0-9]+-arm64\.tar\.gz",
                ],
            ),
        ],
    ),
    ("macos-arm64", &[("metal", &[r"llama-{tag}-bin-macos-arm64\.tar\.gz"])]),
    ("macos-x64", &[("cpu", &[r"llama-{tag}-bin-macos-x64\.tar\.gz"])]),
];

/// What to pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// A build tag such as `b11669`; `None` is the release GitHub marks as latest.
    pub tag: Option<String>,
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
    /// Builds a platform has none of in this release (not an error: the release may not publish them).
    pub notes: Vec<String>,
    /// Assets checked by `--verify`.
    pub verified: usize,
}

/// The manifest for build `tag` from its release object `rel`; `release_name` is the release's own tag when the build tag came
/// from a stable release's `nightly-tag.txt`.
pub fn build_manifest(tag: &str, release_name: Option<&str>, rel: &Value) -> Result<(Value, Vec<String>, Vec<Check>), FetchError> {
    let assets = github::assets(rel);
    let mut platforms = Map::new();
    let (mut notes, mut checks) = (Vec::new(), Vec::new());
    for (platform, builds) in BUILDS {
        let mut out = Vec::new();
        'builds: for (build, patterns) in *builds {
            let (mut files, mut found) = (Vec::new(), Vec::new());
            for pattern in *patterns {
                let pattern = pattern.replace("{tag}", &regex::escape(tag));
                let Some(a) = github::pick(&assets, &pattern)? else {
                    notes.push(format!("{platform}/{build} is not published for {tag}."));
                    continue 'builds;
                };
                let sha = github::digest(a)?;
                found.push(Check {
                    name: github::field(a, "name").to_string(),
                    url: github::field(a, "browser_download_url").to_string(),
                    sha256: sha.clone(),
                });
                files.push(json!({"name": a.get("name"), "url": a.get("browser_download_url"), "size": a.get("size"), "sha256": sha}));
            }
            checks.extend(found);
            out.push(json!({"id": build, "assets": files}));
        }
        platforms.insert((*platform).to_string(), Value::Array(out));
    }
    let published: String = rel
        .get("published_at")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(10)
        .collect();
    let manifest = json!({
        "version": 1,
        "release": release_name.unwrap_or(tag),
        "tag": tag,
        "commit": rel.get("target_commitish").cloned().unwrap_or_else(|| json!("")),
        "published": published,
        "source": rel.get("html_url").cloned().unwrap_or_else(|| json!(format!("https://github.com/{REPO}/releases/tag/{tag}"))),
        "platforms": Value::Object(platforms),
    });
    Ok((manifest, notes, checks))
}

fn api() -> String {
    format!("https://api.github.com/repos/{REPO}/releases")
}

/// Pins the release, checks it when asked and writes the manifests.
pub fn pin(runner: &Runner, opts: &Options) -> Result<Pinned, FetchError> {
    let (downloader, tag, token) = (runner.downloader().clone(), opts.tag.clone(), opts.token.clone());
    let (manifest, notes, checks) = runner.run("pin", "Pinning llama.cpp", Some(REPO), move |ctx| {
        ctx.phase(Phase::items("read", "Reading the release", 2));
        let (tag, release_name) = match tag {
            Some(t) => (t, None),
            None => {
                let latest = github::api_json(&downloader, ctx, &format!("{}/latest", api()), token.as_deref())?;
                ctx.add(1);
                let name = latest.get("tag_name").and_then(Value::as_str).unwrap_or_default().to_string();
                // Stable releases (v0.x.y) point at a build tag in nightly-tag.txt; their assets live there.
                let pointer = github::assets(&latest)
                    .into_iter()
                    .find(|a| github::field(a, "name") == "nightly-tag.txt");
                let tag = match pointer {
                    Some(a) => github::text(&downloader, ctx, github::field(a, "browser_download_url"))?
                        .trim()
                        .to_string(),
                    None => name.clone(),
                };
                (tag, Some(name))
            }
        };
        if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
            return Err(JobError::Failed(format!("'{tag}' is not a build tag.")));
        }
        let rel = github::api_json(&downloader, ctx, &format!("{}/tags/{tag}", api()), token.as_deref())?;
        ctx.add(1);
        ctx.phase(Phase::items("build", "Choosing the assets", 1));
        let built = build_manifest(&tag, release_name.as_deref(), &rel).map_err(|e| JobError::Failed(e.to_string()))?;
        ctx.add(1);
        Ok(built)
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

    /// A release object that offers exactly the assets of `manifest`, so building from it must give `manifest` back.
    pub(super) fn release_of(manifest: &Value) -> Value {
        let mut assets = Vec::new();
        for builds in manifest["platforms"].as_object().unwrap().values() {
            for build in builds.as_array().unwrap() {
                for a in build["assets"].as_array().unwrap() {
                    assets.push(json!({"name": a["name"], "browser_download_url": a["url"], "size": a["size"], "digest": format!("sha256:{}", a["sha256"].as_str().unwrap())}));
                }
            }
        }
        json!({"assets": assets, "target_commitish": manifest["commit"], "published_at": format!("{}T10:00:00Z", manifest["published"].as_str().unwrap()), "html_url": manifest["source"]})
    }

    fn committed() -> (Value, String) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(DEFAULT_OUT);
        let text = std::fs::read_to_string(path).unwrap();
        (serde_json::from_str(&text).unwrap(), text)
    }

    #[test]
    fn rebuilding_the_committed_manifest_from_its_own_assets_gives_it_back_byte_for_byte() {
        let (manifest, text) = committed();
        let tag = manifest["tag"].as_str().unwrap();
        let (built, notes, checks) =
            build_manifest(tag, manifest["release"].as_str().filter(|r| *r != tag), &release_of(&manifest)).unwrap();
        assert_eq!(built, manifest);
        assert!(notes.is_empty() || notes.iter().all(|n| n.ends_with(&format!("is not published for {tag}."))));
        assert!(!checks.is_empty());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.json");
        github::write_manifest(&path, &built).unwrap();
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            text,
            "the same bytes the Python updater wrote"
        );
    }

    fn mini_release(tag: &str, with_cpu: bool) -> Value {
        let mut assets = vec![];
        if with_cpu {
            assets.push(json!({"name": format!("llama-{tag}-bin-macos-x64.tar.gz"), "browser_download_url": "https://dl.test/mac.tgz", "size": 5, "digest": format!("sha256:{}", sha256_hex(b"mac!!"))}));
        }
        assets.push(json!({"name": format!("llama-{tag}-bin-win-cuda-12.4-x64.zip"), "browser_download_url": "https://dl.test/cuda.zip", "size": 4, "digest": format!("sha256:{}", sha256_hex(b"cuda"))}));
        assets.push(json!({"name": "cudart-llama-bin-win-cuda-12.4-x64.zip", "browser_download_url": "https://dl.test/cudart.zip", "size": 6, "digest": format!("sha256:{}", sha256_hex(b"cudart"))}));
        json!({"assets": assets, "published_at": "2026-10-01T00:00:00Z"})
    }

    #[test]
    fn a_build_missing_any_of_its_assets_is_left_out_with_a_note() {
        let (m, notes, checks) = build_manifest("b1", None, &mini_release("b1", true)).unwrap();
        assert_eq!(m["platforms"]["macos-x64"][0]["id"], "cpu");
        assert_eq!(m["platforms"]["windows-x64"][0]["assets"].as_array().unwrap().len(), 2);
        assert_eq!(m["platforms"]["linux-x64"], json!([]));
        assert_eq!(
            (m["release"].clone(), m["tag"].clone(), m["published"].clone(), m["commit"].clone()),
            ("b1".into(), "b1".into(), "2026-10-01".into(), "".into())
        );
        assert_eq!(m["source"], "https://github.com/ggml-org/llama.cpp/releases/tag/b1");
        assert!(notes.contains(&"windows-x64/cpu is not published for b1.".to_string()));
        assert_eq!(checks.len(), 3);
        let (_, _, none) = build_manifest("b1", Some("v0.1"), &mini_release("b1", false)).unwrap();
        assert_eq!(none.len(), 2);
        let bad = json!({"assets": [{"name": "llama-b1-bin-macos-x64.tar.gz"}]});
        assert!(build_manifest("b1", None, &bad).is_err(), "an asset without a digest stops the pin");
    }

    fn served(tag: &str) -> std::sync::Arc<FakeNet> {
        let net = FakeNet::new();
        net.serve(
            &format!("{}/tags/{tag}", api()),
            serde_json::to_vec(&mini_release(tag, true)).unwrap(),
        );
        net.serve("https://dl.test/mac.tgz", b"mac!!".to_vec());
        net.serve("https://dl.test/cuda.zip", b"cuda".to_vec());
        net.serve("https://dl.test/cudart.zip", b"cudart".to_vec());
        net
    }

    #[test]
    fn pins_a_given_tag_verifies_and_writes_every_output() {
        let net = served("b7");
        let dir = tempfile::tempdir().unwrap();
        let opts = Options {
            tag: Some("b7".into()),
            verify: true,
            out: vec![dir.path().join("a.json"), dir.path().join("b.json")],
            token: Some("t".into()),
            work: dir.path().join("w"),
        };
        let pinned = pin(&runner(&net, 2), &opts).unwrap();
        assert_eq!(pinned.verified, 3);
        assert_eq!(
            std::fs::read(dir.path().join("a.json")).unwrap(),
            std::fs::read(dir.path().join("b.json")).unwrap()
        );
        assert_eq!(pinned.manifest["tag"], "b7");
        assert!(!pinned.notes.is_empty());
        assert_eq!(default_out(Path::new("r")), Path::new("r").join(DEFAULT_OUT));
    }

    #[test]
    fn the_latest_release_may_point_at_a_build_tag_and_a_wrong_digest_writes_nothing() {
        let net = served("b9");
        net.serve(
            &format!("{}/latest", api()),
            br#"{"tag_name": "v0.5.0", "assets": [{"name": "nightly-tag.txt", "browser_download_url": "https://dl.test/n.txt"}]}"#.to_vec(),
        );
        net.serve("https://dl.test/n.txt", b"b9\n".to_vec());
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("m.json");
        let opts = Options {
            tag: None,
            verify: false,
            out: vec![out.clone()],
            token: None,
            work: dir.path().join("w"),
        };
        let pinned = pin(&runner(&net, 1), &opts).unwrap();
        assert_eq!(
            (pinned.manifest["release"].clone(), pinned.manifest["tag"].clone(), pinned.verified),
            ("v0.5.0".into(), "b9".into(), 0)
        );
        std::fs::remove_file(&out).unwrap();
        net.serve("https://dl.test/cuda.zip", b"tampered".to_vec());
        let opts = Options { verify: true, ..opts };
        assert!(pin(&runner(&net, 1), &opts).is_err());
        assert!(!out.exists(), "nothing is written when the check failed");
        // Without a pointer file the latest release's own tag is the build tag.
        net.serve(&format!("{}/latest", api()), br#"{"tag_name": "b9", "assets": []}"#.to_vec());
        let ok = Options { verify: false, ..opts };
        assert_eq!(pin(&runner(&net, 1), &ok).unwrap().manifest["release"], "b9");
        let weird = Options {
            tag: Some("b9/../x".into()),
            ..ok.clone()
        };
        assert!(pin(&runner(&net, 1), &weird).unwrap_err().to_string().contains("not a build tag"));
        let unknown = Options {
            tag: Some("b0".into()),
            ..ok
        };
        assert!(pin(&runner(&net, 1), &unknown).is_err());
    }
}
