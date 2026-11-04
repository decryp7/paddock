//! The per-machine installer's generated files (family ADR 0053 section 2.1). `paddock-per-machine.generated.nsh` carries
//! `PADDOCK_STOP_IMAGES`, the file names of every program the installer, updater and uninstaller must stop in every session:
//! `mainBinaryName` plus the file name of every `bundle.externalBin` entry (Tauri installs them with the target triple
//! removed), minus the app's declared exclusions, each with a reason. A new sidecar is in the list without anyone remembering
//! it, and [`check_app`] (run by a test over every app that includes the shared installer include) fails when the generated
//! file, the include copy or the exclusions are stale.

use std::path::{Path, PathBuf};

/// A program that is in the bundle but must not be stopped (Dyno: `dyno-helper.exe`, which runs from its own protected folder).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct StopExclusion {
    /// The installed file name.
    pub image: String,
    /// Why it is not stopped.
    pub reason: String,
}

/// The app's exclusions file (`windows/per-machine.json`).
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PerMachineConfig {
    /// A note for the reader of the file.
    #[serde(rename = "$comment", default)]
    pub comment: Option<String>,
    /// The exclusions.
    #[serde(default)]
    pub stop_exclusions: Vec<StopExclusion>,
    /// The adoption's statement, per program of the stop list, that a forced end loses no more than its last unfinished write.
    #[serde(default)]
    pub survives_forced_end: Vec<Survival>,
}

/// One statement that a program survives a forced end, with what was checked.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct Survival {
    /// The installed file name.
    pub image: String,
    /// What was checked.
    pub why: String,
}

/// Why the list cannot be made.
#[derive(Debug, Clone, thiserror::Error)]
pub enum PerMachineError {
    /// A configuration file is not valid JSON.
    #[error("{file} is not valid JSON: {source}")]
    Json {
        /// The file.
        file: String,
        /// The parser's error.
        #[source]
        source: std::sync::Arc<serde_json::Error>,
    },
    /// `mainBinaryName` is missing or empty.
    #[error("tauri.conf.json has no mainBinaryName")]
    NoMainBinary,
    /// A name cannot be put in the installer's command line safely.
    #[error("`{0}` cannot be a program file name in the stop list")]
    BadName(String),
    /// An exclusion has no reason.
    #[error("the exclusion of `{0}` has no reason")]
    NoReason(String),
    /// An exclusion names a program that is not in the bundle (a stale exclusion).
    #[error("the exclusion `{0}` is not in the bundle (stale)")]
    UnknownExclusion(String),
    /// A program in the stop list has no statement that it survives a forced end.
    #[error("`{0}` is in the stop list but has no entry in survivesForcedEnd (ADR 0053 section 3)")]
    NoSurvivalStatement(String),
}

impl PartialEq for PerMachineError {
    fn eq(&self, other: &Self) -> bool {
        self.to_string() == other.to_string()
    }
}

impl Eq for PerMachineError {}

fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 120
        && !n.starts_with(['-', ' ', '.'])
        && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ' '))
}

fn exe_name(stem: &str) -> String {
    let base = stem.rsplit(['/', '\\']).next().unwrap_or(stem);
    if base.to_ascii_lowercase().ends_with(".exe") { base.to_owned() } else { format!("{base}.exe") }
}

/// The programs to stop: the main binary, then every external binary, without duplicates (case-insensitive), minus the
/// exclusions. Every name is checked, every exclusion needs a reason and must name a program of the bundle.
pub fn stop_images(
    main_binary_name: &str,
    external_bin: &[String],
    exclusions: &[StopExclusion],
) -> Result<Vec<String>, PerMachineError> {
    if main_binary_name.trim().is_empty() {
        return Err(PerMachineError::NoMainBinary);
    }
    let mut all: Vec<String> = Vec::new();
    for stem in std::iter::once(main_binary_name).chain(external_bin.iter().map(String::as_str)) {
        let name = exe_name(stem);
        if !valid_name(&name) {
            return Err(PerMachineError::BadName(name));
        }
        if !all.iter().any(|a| a.eq_ignore_ascii_case(&name)) {
            all.push(name);
        }
    }
    for e in exclusions {
        if e.reason.trim().is_empty() {
            return Err(PerMachineError::NoReason(e.image.clone()));
        }
        if !all.iter().any(|a| a.eq_ignore_ascii_case(&e.image)) {
            return Err(PerMachineError::UnknownExclusion(e.image.clone()));
        }
    }
    all.retain(|a| !exclusions.iter().any(|e| e.image.eq_ignore_ascii_case(a)));
    Ok(all)
}

/// The text of `paddock-per-machine.generated.nsh`.
pub fn generated_nsh(images: &[String]) -> String {
    format!(
        "; Generated by paddock-service from tauri.conf.json, tauri.windows.conf.json and windows/per-machine.json; do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-service --lib per_machine\n; The programs the installer, updater and uninstaller stop in every session (family ADR 0053 section 2.1).\n!define PADDOCK_STOP_IMAGES \"{}\"\n",
        images.join(",")
    )
}

fn json_error(file: &str, e: serde_json::Error) -> PerMachineError {
    PerMachineError::Json { file: file.to_owned(), source: std::sync::Arc::new(e) }
}

fn parse(text: &str, file: &str) -> Result<serde_json::Value, PerMachineError> {
    serde_json::from_str(text).map_err(|e| json_error(file, e))
}

fn config_of(per_machine_json: Option<&str>) -> Result<PerMachineConfig, PerMachineError> {
    match per_machine_json {
        Some(t) => serde_json::from_str(t).map_err(|e| json_error("per-machine.json", e)),
        None => Ok(PerMachineConfig::default()),
    }
}

/// Every program of the stop list has a statement that it survives a forced end (ADR 0053 section 3: each adopting app says so).
pub fn check_survival(images: &[String], per_machine_json: Option<&str>) -> Result<(), PerMachineError> {
    let config = config_of(per_machine_json)?;
    for image in images {
        let said = config.survives_forced_end.iter().any(|s| s.image.eq_ignore_ascii_case(image) && !s.why.trim().is_empty());
        if !said {
            return Err(PerMachineError::NoSurvivalStatement(image.clone()));
        }
    }
    Ok(())
}

/// The generated file for an app from its Tauri configuration (`tauri.conf.json`, then the Windows overlay, whose
/// `bundle.externalBin` replaces the base list as Tauri's merge does) and its exclusions file.
pub fn generated_from_config(
    tauri_conf: &str,
    windows_conf: Option<&str>,
    per_machine_json: Option<&str>,
) -> Result<String, PerMachineError> {
    let base = parse(tauri_conf, "tauri.conf.json")?;
    let overlay = windows_conf.map(|t| parse(t, "tauri.windows.conf.json")).transpose()?;
    let main = base.get("mainBinaryName").and_then(|v| v.as_str()).unwrap_or_default();
    let ext = |v: &serde_json::Value| -> Option<Vec<String>> {
        v.pointer("/bundle/externalBin")
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect())
    };
    let external = overlay.as_ref().and_then(ext).or_else(|| ext(&base)).unwrap_or_default();
    let config = config_of(per_machine_json)?;
    Ok(generated_nsh(&stop_images(main, &external, &config.stop_exclusions)?))
}

/// The files of one adopting app that must equal what is generated: the generated list and the copy of the include.
const GENERATED_FILE: &str = "paddock-per-machine.generated.nsh";
const INCLUDE_FILE: &str = "paddock-per-machine.nsh";

fn read(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|t| t.replace("\r\n", "\n"))
}

/// Checks one app's `src-tauri` folder against the canonical include: the hooks file includes the shared include, the
/// generated list equals the configuration's, the include copy equals the canonical file. Returns what is stale.
pub fn check_app(src_tauri: &Path, canonical_include: &Path) -> Vec<String> {
    let windows = src_tauri.join("windows");
    let mut stale = Vec::new();
    let generated = generated_from_config(
        &read(&src_tauri.join("tauri.conf.json")).unwrap_or_default(),
        read(&src_tauri.join("tauri.windows.conf.json")).as_deref(),
        read(&windows.join("per-machine.json")).as_deref(),
    );
    match generated {
        Err(e) => stale.push(format!("{}: {e}", src_tauri.display())),
        Ok(text) => {
            let images: Vec<String> = text
                .lines()
                .find_map(|l| l.strip_prefix("!define PADDOCK_STOP_IMAGES \""))
                .map(|l| l.trim_end_matches('"').split(',').map(str::to_owned).collect())
                .unwrap_or_default();
            if let Err(e) = check_survival(&images, read(&windows.join("per-machine.json")).as_deref()) {
                stale.push(format!("{}: {e}", windows.join("per-machine.json").display()));
            }
            if read(&windows.join(GENERATED_FILE)).as_deref() != Some(text.as_str()) {
                stale.push(format!("{} is stale (PADDOCK_WRITE_TS=1 cargo test -p paddock-service --lib per_machine)", windows.join(GENERATED_FILE).display()));
            }
        }
    }
    match (read(canonical_include), read(&windows.join(INCLUDE_FILE))) {
        (Some(a), Some(b)) if a == b => {}
        (Some(_), Some(_)) => stale.push(format!("{} differs from {}", windows.join(INCLUDE_FILE).display(), canonical_include.display())),
        _ => stale.push(format!(
            "{} or {} is missing or unreadable",
            windows.join(INCLUDE_FILE).display(),
            canonical_include.display()
        )),
    }
    stale
}

/// Writes the two files for an app (what the regenerate mode of the test does).
pub fn write_app(src_tauri: &Path, canonical_include: &Path) -> Result<(), String> {
    let windows = src_tauri.join("windows");
    let text = generated_from_config(
        &read(&src_tauri.join("tauri.conf.json")).unwrap_or_default(),
        read(&src_tauri.join("tauri.windows.conf.json")).as_deref(),
        read(&windows.join("per-machine.json")).as_deref(),
    )
    .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&windows).map_err(|e| e.to_string())?;
    std::fs::write(windows.join(GENERATED_FILE), text).map_err(|e| e.to_string())?;
    let include = std::fs::read(canonical_include).map_err(|e| e.to_string())?;
    std::fs::write(windows.join(INCLUDE_FILE), include).map_err(|e| e.to_string())
}

/// Every app folder (`apps/<app>/app/src-tauri`) whose Windows hooks file includes the shared include.
pub fn adopting_apps(repo_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(apps) = std::fs::read_dir(repo_root.join("apps")) else { return out };
    for app in apps.flatten() {
        let src = app.path().join("app").join("src-tauri");
        let hooks = src.join("windows").join("installer-hooks.nsh");
        if read(&hooks).is_some_and(|t| t.contains(INCLUDE_FILE)) {
            out.push(src);
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests;
