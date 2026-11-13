//! Where an app's version is written, and how to read it from each place. The port of `util/version.mjs`.

use crate::check::Ctx;
use crate::check::jsre::{WS, rx};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

static SEMVER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$").unwrap_or_else(|e| panic!("{e}")));
static TOML_VERSION: LazyLock<Regex> = LazyLock::new(|| rx(r#"(?m)^version\s*=\s*"([^"]+)""#));
static TABLE_START: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)^\["));
static WORKSPACE_VERSION: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)^version\.workspace\s*=\s*true"));
static CHANGELOG_HEAD: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"(?m)^##\s+\[?v?(\d+\.\d+\.\d+[^\]{WS}]*)\]?")));

/// Whether `v` is a semantic version.
pub fn is_semver(v: &str) -> bool {
    SEMVER.is_match(v)
}

fn json_version(text: &str) -> Option<String> {
    let v = serde_json::from_str::<Value>(text).ok()?;
    let v = v.get("version")?.as_str()?;
    is_semver(v).then(|| v.to_string())
}

/// The first `version = "x"` of the TOML table `[header]`.
fn toml_version(text: &str, header: &str) -> Option<String> {
    let tag = format!("[{header}]");
    let at = text.find(&tag)?;
    let rest = &text[at + tag.len()..];
    let rest = TABLE_START.find(rest).map_or(rest, |m| &rest[..m.start()]);
    let v = TOML_VERSION.captures(rest)?[1].to_string();
    is_semver(&v).then_some(v)
}

/// One place the version is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// The file.
    pub file: String,
    /// The version.
    pub version: String,
}

/// Every place the version is written. A placeholder `0.0.0` in `package.json` is not a version. A `tauri.conf.json` `version` that
/// is a path, or Cargo's `version.workspace = true`, defer to their source.
pub fn version_sources(ctx: &Ctx) -> Vec<Source> {
    let (tauri_dir, app_dir) = (ctx.config().str("tauriDir"), ctx.config().str("appDir"));
    let read = |p: &str| ctx.read(p).map(|t| t.to_string()).unwrap_or_default();
    let mut out = Vec::new();
    let mut add = |file: String, v: Option<String>| {
        if let Some(version) = v {
            out.push(Source { file, version });
        }
    };
    add(
        format!("{tauri_dir}/tauri.conf.json"),
        json_version(&read(&format!("{tauri_dir}/tauri.conf.json"))),
    );
    let pkg = json_version(&read(&format!("{app_dir}/package.json")));
    if let Some(p) = pkg.filter(|p| p != "0.0.0") {
        add(format!("{app_dir}/package.json"), Some(p));
    }
    let cargo = read(&format!("{tauri_dir}/Cargo.toml"));
    add(format!("{tauri_dir}/Cargo.toml"), toml_version(&cargo, "package"));
    if WORKSPACE_VERSION.is_match(&cargo) {
        add("Cargo.toml".into(), toml_version(&read("Cargo.toml"), "workspace.package"));
    }
    if let Some(c) = CHANGELOG_HEAD.captures(&read("CHANGELOG.md")).filter(|c| is_semver(&c[1])) {
        add("CHANGELOG.md".into(), Some(c[1].to_string()));
    }
    out
}

/// The app version: `tauri.conf.json` first, then the other sources. `None` when none is readable.
pub fn app_version(ctx: &Ctx) -> Option<String> {
    version_sources(ctx).into_iter().next().map(|s| s.version)
}
