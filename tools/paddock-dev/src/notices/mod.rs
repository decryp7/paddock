//! `dev notices`: regenerates (or with `--check` verifies) an app's `THIRD_PARTY_NOTICES.md` (ADR 0022 R6). It ports the notices
//! generator `conformance/src/notices` (the `paddock-notices` bin) and `scripts/notices.sh`, which stay CI's tools until the call
//! sites switch (ADR 0022 §3.7); `docs/known-copies.toml` registers the pair.
//!
//! Rust licences come from `cargo about generate` (one run per manifest, bounded) unless `--rust-json` files are given; the npm side
//! reads `package-lock.json` and `node_modules`. The app states its facts once in the `notices` block of `apps/<app>/paddock.ci.json`
//! (`name`, `manifests`, `npm`, `out`, `about`, `rustJson`; paths relative to that file's folder).
//!
//! - [`licences`]: the allowed licences and the SPDX expression check;
//! - [`sources`]: the packages, from cargo-about JSON and the npm lock (pure parsing, bounded file reads);
//! - [`render`]: the document, a pure function (golden-tested against the generator's output);
//! - [`exec`]: the programs (cargo about, cargo install, npm ci) behind a port;
//! - this file: the arguments, the plan that resolves them, and the command (gather, render, check or write).

mod error;
pub mod exec;
pub mod licences;
pub mod render;
pub mod sources;
pub mod text;

pub use error::NoticesError;

use clap::Args as ClapArgs;
use exec::Tools;
use paddock_cli::{CliError, Exit, Report};
use serde_json::{Value, json};
use sources::{MAX_LOCK_BYTES, read_text};
use std::fs;
use std::path::{Component, Path, PathBuf};
use text::lf;

/// The file name written into every output folder.
pub const FILE_NAME: &str = "THIRD_PARTY_NOTICES.md";
/// The default output folder (relative to the app's folder), as the generator had it.
const DEFAULT_OUT: &str = "app/src-tauri/resources";
/// The default Rust manifest.
const DEFAULT_MANIFEST: &str = "app/src-tauri/Cargo.toml";
/// Largest `cargo about` JSON read from a `--rust-json` file or existing notices file.
const MAX_DOC_BYTES: u64 = 256 << 20;

/// Arguments of `dev notices`.
#[derive(ClapArgs, Debug, Default, Clone)]
pub struct Args {
    /// The app's folder name under apps/ (reads apps/<app>/paddock.ci.json); instead of --config.
    #[arg(conflicts_with = "config")]
    pub app: Option<String>,
    /// The app's paddock.ci.json (relative to --root), instead of naming the app.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// The product name in the intro (default: notices.name of the config).
    #[arg(long)]
    pub name: Option<String>,
    /// The repository root (default: the folder containing .git, found from the current folder).
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// A Rust manifest for cargo about, relative to the app's folder (repeatable; default: the config's, or app/src-tauri/Cargo.toml).
    #[arg(long = "manifest")]
    pub manifests: Vec<String>,
    /// The app's npm folder, relative to the app's folder (default: the config's, or app).
    #[arg(long)]
    pub npm: Option<String>,
    /// An output folder, relative to the app's folder (repeatable; default: the config's, or app/src-tauri/resources).
    #[arg(long = "out")]
    pub out: Vec<String>,
    /// A cargo about JSON file to use instead of running cargo about (repeatable).
    #[arg(long = "rust-json")]
    pub rust_json: Vec<String>,
    /// The cargo-about configuration (default: the app's about.toml if it has one, else templates/repo/about.toml).
    #[arg(long)]
    pub about: Option<String>,
    /// Verify instead of write: exit 5 when a file is missing or stale or a licence is refused.
    #[arg(long)]
    pub check: bool,
    /// Do not install the pinned cargo-about or run `npm ci` when they are missing (CI images that have them).
    #[arg(long)]
    pub no_install: bool,
}

/// What the arguments and the app's config resolve to: every path absolute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The product name.
    pub name: String,
    /// The folder the app's paths are relative to (the config file's folder, or the root).
    pub base: PathBuf,
    /// The app's npm folder.
    pub npm_dir: PathBuf,
    /// The folders that get `THIRD_PARTY_NOTICES.md`.
    pub outs: Vec<PathBuf>,
    /// The Rust manifests for cargo about.
    pub manifests: Vec<PathBuf>,
    /// The cargo about JSON files to read instead.
    pub rust_json: Vec<PathBuf>,
    /// The cargo-about configuration.
    pub about_toml: PathBuf,
}

/// `base` joined with `rel` (an absolute `rel` wins) with `.` and `..` resolved lexically, as Node's `path.resolve` does.
pub fn resolve_path(base: &Path, rel: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in base.join(rel).components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

fn config_strings(notices: &Value, key: &str, file: &str) -> Result<Option<Vec<String>>, NoticesError> {
    let bad = |why: &str| NoticesError::Config {
        file: file.to_string(),
        why: format!("notices.{key} {why}"),
    };
    match notices.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| v.as_str().map(String::from).ok_or_else(|| bad("must be a list of strings")))
            .collect::<Result<_, _>>()
            .map(Some),
        Some(_) => Err(bad("must be a list of strings")),
    }
}

fn config_string(notices: &Value, key: &str, file: &str) -> Result<Option<String>, NoticesError> {
    match notices.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(NoticesError::Config {
            file: file.to_string(),
            why: format!("notices.{key} must be a string"),
        }),
    }
}

/// An app folder name: lower-case letters, digits and dashes only (it becomes a path).
fn valid_app(app: &str) -> bool {
    !app.is_empty() && app.len() <= 64 && app.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Which config file the arguments name, relative to the root.
pub fn config_path(args: &Args) -> Result<Option<PathBuf>, NoticesError> {
    match (&args.app, &args.config) {
        (Some(app), _) if !valid_app(app) => Err(NoticesError::Usage(format!("`{app}` is not an app folder name under apps/"))),
        (Some(app), _) => Ok(Some(PathBuf::from(format!("apps/{app}/paddock.ci.json")))),
        (None, config) => Ok(config.clone()),
    }
}

/// Resolves the arguments and the config's `notices` block (None without a config) to a [`Plan`]. `exists` answers whether a file
/// exists (the defaults depend on it). Flags win over the config; the config wins over the defaults.
pub fn resolve(args: &Args, root: &Path, config: Option<(&Path, &Value)>, exists: &dyn Fn(&Path) -> bool) -> Result<Plan, NoticesError> {
    let (base, notices, file) = match config {
        Some((file, doc)) => {
            let shown = file.display().to_string();
            let notices = doc.get("notices").filter(|n| n.is_object()).ok_or_else(|| NoticesError::Config {
                file: shown.clone(),
                why: r#"has no "notices" block ({ "name": "<product>", "manifests": [...], "npm": "app", "out": [...] })"#.into(),
            })?;
            (
                file.parent().map_or_else(|| root.to_path_buf(), Path::to_path_buf),
                notices.clone(),
                shown,
            )
        }
        None => (root.to_path_buf(), Value::Null, String::new()),
    };
    let name = match (args.name.clone(), config_string(&notices, "name", &file)?) {
        (Some(n), _) | (None, Some(n)) => n,
        (None, None) if config.is_some() => {
            return Err(NoticesError::Config {
                file,
                why: "notices.name (the product name) is needed".into(),
            });
        }
        (None, None) => {
            return Err(NoticesError::Usage(
                "an app, --config <paddock.ci.json> or --name <product name> is needed".into(),
            ));
        }
    };
    let strings = |flag: &[String], key: &str, default: Option<Vec<String>>| -> Result<Vec<String>, NoticesError> {
        if !flag.is_empty() {
            return Ok(flag.to_vec());
        }
        Ok(config_strings(&notices, key, &file)?.or(default).unwrap_or_default())
    };
    let npm = args
        .npm
        .clone()
        .or(config_string(&notices, "npm", &file)?)
        .unwrap_or_else(|| "app".into());
    let outs = strings(&args.out, "out", None)?;
    let outs = if outs.is_empty() { vec![DEFAULT_OUT.to_string()] } else { outs };
    let default_manifests = match config {
        Some(_) => Some(vec![DEFAULT_MANIFEST.to_string()]),
        None => exists(&base.join(DEFAULT_MANIFEST)).then(|| vec![DEFAULT_MANIFEST.to_string()]),
    };
    let manifests = strings(&args.manifests, "manifests", default_manifests)?;
    let rust_json = strings(&args.rust_json, "rustJson", None)?;
    let about_rel = args.about.clone().or(config_string(&notices, "about", &file)?);
    let about_toml = match about_rel {
        Some(a) => resolve_path(&base, Path::new(&a)),
        None if exists(&base.join("about.toml")) => base.join("about.toml"),
        None => root.join("templates/repo/about.toml"),
    };
    let abs = |list: Vec<String>| list.iter().map(|p| resolve_path(&base, Path::new(p))).collect::<Vec<_>>();
    Ok(Plan {
        name,
        npm_dir: resolve_path(&base, Path::new(&npm)),
        outs: abs(outs),
        manifests: abs(manifests),
        rust_json: abs(rust_json),
        about_toml,
        base,
    })
}

/// What happened to one output file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    /// Written (write mode).
    Written,
    /// Check mode: identical to what would be written.
    UpToDate,
    /// Check mode: different.
    Stale,
    /// Check mode: not there.
    Missing,
}

impl FileState {
    fn name(self) -> &'static str {
        match self {
            FileState::Written => "written",
            FileState::UpToDate => "up-to-date",
            FileState::Stale => "stale",
            FileState::Missing => "missing",
        }
    }
}

/// One line for the person, and whether it is a complaint (the generator printed those on standard error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// A warning, a refused licence or a stale file.
    pub complaint: bool,
    /// The text.
    pub text: String,
}

/// The result of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Check mode.
    pub check: bool,
    /// The counts of what was rendered.
    pub counts: render::Counts,
    /// Packages without licence text and the like.
    pub warnings: Vec<String>,
    /// Packages whose licence is refused.
    pub non_permissive: Vec<String>,
    /// Each output file and what happened to it.
    pub files: Vec<(PathBuf, FileState)>,
    /// The document.
    pub markdown: String,
}

impl Outcome {
    /// A refused licence, or in check mode a stale or missing file.
    pub fn has_findings(&self) -> bool {
        !self.non_permissive.is_empty() || self.files.iter().any(|(_, s)| matches!(s, FileState::Stale | FileState::Missing))
    }

    /// The lines the generator logged, in its order: warnings, refused licences, the summary, then a line per file.
    pub fn messages(&self) -> Vec<Message> {
        let c = &self.counts;
        let mut out: Vec<Message> = self
            .warnings
            .iter()
            .map(|w| Message {
                complaint: true,
                text: format!("warning: {w}"),
            })
            .collect();
        out.extend(self.non_permissive.iter().map(|b| Message {
            complaint: true,
            text: format!("NON-PERMISSIVE: {b}"),
        }));
        out.push(Message {
            complaint: false,
            text: format!(
                "notices: {} Rust crates, {} npm packages, {} distinct licence texts, {} licence expressions",
                c.rust, c.npm, c.texts, c.expressions
            ),
        });
        for (path, state) in &self.files {
            let shown = path.display();
            match state {
                FileState::Written => out.push(Message {
                    complaint: false,
                    text: format!("wrote {shown}"),
                }),
                FileState::UpToDate => {}
                FileState::Stale | FileState::Missing => out.push(Message {
                    complaint: true,
                    text: format!(
                        "{shown} is {}: run `./dev notices` (without --check) and commit the result",
                        if *state == FileState::Missing { "missing" } else { "out of date" }
                    ),
                }),
            }
        }
        out
    }
}

fn write_atomic(path: &Path, content: &str) -> Result<(), NoticesError> {
    let fail = |why: String| NoticesError::Write {
        path: path.display().to_string(),
        why,
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| fail(e.to_string()))?;
    }
    // Build the new file beside the old one, then swap, so a stop in between leaves the committed file whole.
    let tmp = path.with_extension(format!("md.tmp{}", std::process::id()));
    fs::write(&tmp, content).map_err(|e| fail(e.to_string()))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        fail(e.to_string())
    })
}

/// Gathers the packages, renders, and in check mode compares each output with the document, otherwise writes it.
pub fn generate(plan: &Plan, root: &Path, check: bool, install: bool, tools: &dyn Tools) -> Result<Outcome, NoticesError> {
    let docs = if !plan.rust_json.is_empty() {
        plan.rust_json
            .iter()
            .map(|f| {
                serde_json::from_str::<Value>(&read_text(f, MAX_DOC_BYTES)?).map_err(|e| NoticesError::Read {
                    path: f.display().to_string(),
                    why: format!("it is not valid JSON: {e}"),
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else if !plan.manifests.is_empty() {
        if install {
            exec::ensure_cargo_about(tools, root)?;
            exec::ensure_node_modules(tools, root)?;
        }
        let manifests: Vec<&Path> = plan.manifests.iter().map(PathBuf::as_path).collect();
        exec::cargo_about(tools, &manifests, &plan.about_toml, &plan.base)?
    } else {
        return Err(NoticesError::NoRustSource);
    };
    let (rust, rust_warnings) = sources::rust_crates(&docs);
    let (npm, npm_warnings) = sources::npm_packages(&plan.npm_dir, root)?;
    let rendered = render::render(&rust, &npm, &plan.name);
    let non_permissive = licences::non_permissive(&[rust, npm].concat());
    let mut files = Vec::new();
    for dir in &plan.outs {
        let file = dir.join(FILE_NAME);
        let state = if check {
            if !file.is_file() {
                FileState::Missing
            } else if lf(&read_text(&file, MAX_DOC_BYTES)?) == rendered.markdown {
                FileState::UpToDate
            } else {
                FileState::Stale
            }
        } else {
            write_atomic(&file, &rendered.markdown)?;
            FileState::Written
        };
        files.push((file, state));
    }
    Ok(Outcome {
        check,
        counts: rendered.counts,
        warnings: rust_warnings.into_iter().chain(npm_warnings).collect(),
        non_permissive,
        files,
        markdown: rendered.markdown,
    })
}

fn find_root(explicit: &Option<PathBuf>) -> Result<PathBuf, CliError> {
    let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("The current folder cannot be read: {e}.")))?;
    match explicit {
        Some(r) => Ok(resolve_path(&cwd, r)),
        None => {
            let out = crate::input_hash::Exec::run(&crate::input_hash::Real, "git", &["rev-parse", "--show-toplevel"], &cwd, None)?;
            Ok(PathBuf::from(out.trim()))
        }
    }
}

/// `dev notices`.
pub fn command(args: &Args) -> Result<Report, CliError> {
    let root = find_root(&args.root)?;
    let config = match config_path(args)? {
        Some(rel) => {
            let file = resolve_path(&root, &rel);
            let doc: Value = serde_json::from_str(&read_text(&file, MAX_LOCK_BYTES)?).map_err(|e| NoticesError::Config {
                file: rel.display().to_string(),
                why: format!("it is not valid JSON: {e}"),
            })?;
            Some((file, doc))
        }
        None => None,
    };
    let plan = resolve(args, &root, config.as_ref().map(|(f, d)| (f.as_path(), d)), &|p| p.is_file())?;
    let outcome = generate(&plan, &root, args.check, !args.no_install, &exec::Real)?;
    let text = outcome.messages().into_iter().map(|m| m.text).collect::<Vec<_>>().join("\n");
    let data = json!({
        "app": plan.name,
        "check": outcome.check,
        "rust": outcome.counts.rust,
        "npm": outcome.counts.npm,
        "texts": outcome.counts.texts,
        "expressions": outcome.counts.expressions,
        "warnings": outcome.warnings,
        "nonPermissive": outcome.non_permissive,
        "files": outcome.files.iter().map(|(p, s)| json!({"path": p.display().to_string(), "state": s.name()})).collect::<Vec<_>>(),
    });
    let report = Report::new(data, text);
    Ok(if outcome.has_findings() {
        report.with_exit(Exit::Findings)
    } else {
        report
    })
}

#[cfg(test)]
mod tests;
