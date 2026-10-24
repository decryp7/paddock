//! The Tauri build step of the Paddock apps (plan X34), the three jobs every app's `build.rs` did on its own:
//!
//! 1. **The Windows application manifest in every binary.** Tauri embeds its manifest into the app binary only; test
//!    binaries then fail to load on Windows (`STATUS_ENTRYPOINT_NOT_FOUND`, Common Controls v6 missing). [`MANIFEST`] is the
//!    one manifest (it used to be a copy in every app); [`Build`] writes it to `OUT_DIR`, links it into every binary of the
//!    crate (`rustc-link-arg` covers tests) and tells tauri-build not to add its own.
//! 2. **Empty placeholders for the sidecars.** tauri-build requires every `bundle.externalBin` file to exist, even for
//!    `cargo check` and tests. Bundles build the real ones first (`paddock-sidecar`, in `@decryp7/paddock-conformance`);
//!    elsewhere an empty file stands in, which the app reports as missing when asked to start it. A real file is never
//!    overwritten.
//! 3. **Rerun triggers** (`TAURI_UPDATER_PUBKEY` by default: the updater key is compiled in).
//!
//! ```text
//! // build.rs
//! fn main() {
//!     paddock_build::Build::new().sidecars(["pitcrew-service"]).run();
//! }
//! ```
//!
//! Everything but [`Build::run`] is a pure function of an injected environment and is tested on every OS; `run` (feature
//! `tauri-build`) is the thin call into tauri-build.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(feature = "tauri-build")]
mod native;

/// The Windows application manifest: Common Controls v6.
pub const MANIFEST: &str = include_str!("windows-app-manifest.xml");

/// The file name [`MANIFEST`] is written under in `OUT_DIR`.
pub const MANIFEST_FILE: &str = "paddock-app.manifest";

/// The longest sidecar name accepted (a name is a file name: letters, digits, `-`, `_`).
pub const MAX_NAME: usize = 64;

/// Why a [`Build`] cannot be planned or applied.
#[derive(Debug)]
pub enum BuildError {
    /// A sidecar name that is not a plain file name.
    BadSidecarName(String),
    /// A folder or file could not be written.
    Io {
        /// What was being written.
        path: PathBuf,
        /// The failure.
        source: io::Error,
    },
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::BadSidecarName(n) => write!(
                f,
                "{n:?} is not a sidecar name (letters, digits, `-` and `_`, at most {MAX_NAME} characters)"
            ),
            BuildError::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for BuildError {}

/// The environment a build script runs in (Cargo's variables), injected so every case is testable.
#[derive(Debug, Clone, Default)]
pub struct Env(BTreeMap<String, String>);

impl Env {
    /// The process environment.
    pub fn process() -> Env {
        Env(std::env::vars().collect())
    }

    /// An environment of the given variables.
    pub fn of<'a>(vars: impl IntoIterator<Item = (&'a str, &'a str)>) -> Env {
        Env(vars
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect())
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }
}

/// Whether a name is usable as a sidecar's file name stem.
pub fn valid_sidecar_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The file Tauri expects for a sidecar: `binaries/<name>-<target triple>[.exe]`.
pub fn sidecar_file(name: &str, target: &str) -> String {
    let ext = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    format!("{name}-{target}{ext}")
}

/// What a build script is to do, as data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// `cargo:` lines to print.
    pub directives: Vec<String>,
    /// Whether tauri-build must not add its own Windows manifest (ours is linked instead).
    pub own_manifest: bool,
    /// The placeholder files to create when missing.
    pub placeholders: Vec<PathBuf>,
}

/// The build step's configuration.
#[derive(Debug, Clone)]
pub struct Build {
    sidecars: Vec<String>,
    manifest: bool,
    rerun_env: Vec<String>,
    binaries_dir: PathBuf,
}

impl Default for Build {
    fn default() -> Self {
        Build::new()
    }
}

impl Build {
    /// The defaults: the manifest on, `TAURI_UPDATER_PUBKEY` as a rerun trigger, sidecars in `binaries/`.
    pub fn new() -> Build {
        Build {
            sidecars: Vec::new(),
            manifest: true,
            rerun_env: vec!["TAURI_UPDATER_PUBKEY".to_string()],
            binaries_dir: PathBuf::from("binaries"),
        }
    }

    /// The sidecars of `bundle.externalBin` (their file name stems, without the target triple).
    #[must_use]
    pub fn sidecars<I, S>(mut self, names: I) -> Build
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.sidecars.extend(names.into_iter().map(Into::into));
        self
    }

    /// Leaves the Windows manifest to tauri-build (for an app that does not run tests on Windows).
    #[must_use]
    pub fn without_windows_manifest(mut self) -> Build {
        self.manifest = false;
        self
    }

    /// Reruns the build script when this variable changes.
    #[must_use]
    pub fn rerun_if_env_changed(mut self, var: impl Into<String>) -> Build {
        self.rerun_env.push(var.into());
        self
    }

    /// The plan for an environment: the directives to print and the placeholders to create. `out_dir` is where the manifest
    /// file lives (Cargo's `OUT_DIR`).
    pub fn plan(&self, env: &Env) -> Result<Plan, BuildError> {
        if let Some(bad) = self.sidecars.iter().find(|n| !valid_sidecar_name(n)) {
            return Err(BuildError::BadSidecarName(bad.clone()));
        }
        let mut directives = Vec::new();
        let msvc = env.get("CARGO_CFG_TARGET_OS") == Some("windows")
            && env.get("CARGO_CFG_TARGET_ENV") == Some("msvc");
        let own_manifest = self.manifest && msvc;
        if own_manifest {
            let out = Path::new(env.get("OUT_DIR").unwrap_or_default()).join(MANIFEST_FILE);
            directives.push("cargo:rustc-link-arg=/MANIFEST:EMBED".to_string());
            directives.push(format!(
                "cargo:rustc-link-arg=/MANIFESTINPUT:{}",
                out.display()
            ));
        }
        for var in &self.rerun_env {
            directives.push(format!("cargo:rerun-if-env-changed={var}"));
        }
        let manifest_dir = Path::new(env.get("CARGO_MANIFEST_DIR").unwrap_or_default());
        let placeholders = match env.get("TARGET") {
            Some(target) => self
                .sidecars
                .iter()
                .map(|n| {
                    manifest_dir
                        .join(&self.binaries_dir)
                        .join(sidecar_file(n, target))
                })
                .collect(),
            None => Vec::new(),
        };
        Ok(Plan {
            directives,
            own_manifest,
            placeholders,
        })
    }

    /// Does what [`plan`](Build::plan) says: writes the manifest (when it is linked) and the missing placeholders, and prints
    /// the directives through `print`.
    pub fn apply(&self, env: &Env, print: &mut dyn FnMut(&str)) -> Result<Plan, BuildError> {
        let plan = self.plan(env)?;
        if plan.own_manifest {
            write_manifest(Path::new(env.get("OUT_DIR").unwrap_or_default()))?;
        }
        create_placeholders(&plan.placeholders)?;
        for line in &plan.directives {
            print(line);
        }
        Ok(plan)
    }
}

/// Writes [`MANIFEST`] to `dir/`[`MANIFEST_FILE`] and returns the path.
pub fn write_manifest(dir: &Path) -> Result<PathBuf, BuildError> {
    let path = dir.join(MANIFEST_FILE);
    let io = |source| BuildError::Io {
        path: path.clone(),
        source,
    };
    fs::create_dir_all(dir).map_err(io)?;
    fs::write(&path, MANIFEST).map_err(io)?;
    Ok(path)
}

/// Creates each missing file as an empty one, with its folder; a file that exists (the real sidecar) is never touched.
/// Returns the files created.
pub fn create_placeholders(files: &[PathBuf]) -> Result<Vec<PathBuf>, BuildError> {
    let mut created = Vec::new();
    for file in files {
        if file.exists() {
            continue;
        }
        let io = |source| BuildError::Io {
            path: file.clone(),
            source,
        };
        fs::create_dir_all(file.parent().unwrap_or(Path::new(""))).map_err(io)?;
        fs::write(file, b"").map_err(io)?;
        created.push(file.clone());
    }
    Ok(created)
}
