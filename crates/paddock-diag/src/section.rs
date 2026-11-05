//! Sections: what goes into a report. The shell contributes the built-in ones ([`builtin`]: system facts, settings, the log,
//! the crash marker); an app adds its own (Pitcrew its runner logs, Dyno its agent, Pitwall its engine logs) by implementing
//! [`Section`]. A section hands over [`Content`], never bytes: the collector redacts every file, so a section cannot add an
//! unredacted one, and a section that panics or fails costs the report only that section.

use crate::manifest::{AppInfo, FileKind};
use chrono::{DateTime, Utc};
use paddock_env::AppDirs;
use serde_json::Value;

/// What a section's file holds. There is no raw variant on purpose (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub enum Content {
    /// Text.
    Text(String),
    /// Structured data (written pretty-printed).
    Json(Value),
    /// CLEF JSON lines from a log (each line redacted on its own so it stays valid JSON).
    Log(Vec<u8>),
}

/// One file a section contributes.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionFile {
    /// Path inside the zip: ASCII letters, digits and `/ . - _`, at most 100 characters ([`valid_entry_name`](crate::valid_entry_name)).
    pub name: String,
    /// What it is.
    pub kind: FileKind,
    /// One line for the preview, in words the user understands ("The last 2 MB of today's log").
    pub summary: String,
    /// The data.
    pub content: Content,
}

impl SectionFile {
    /// A file of an app section ([`FileKind::App`]).
    pub fn app(name: &str, summary: &str, content: Content) -> Self {
        SectionFile {
            name: name.into(),
            kind: FileKind::App,
            summary: summary.into(),
            content,
        }
    }
}

/// What one section collected.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SectionOutput {
    /// The files.
    pub files: Vec<SectionFile>,
    /// What could not be collected, in words for the user ("the crash marker cannot be read").
    pub notes: Vec<String>,
}

impl SectionOutput {
    /// An output with one file.
    pub fn one(file: SectionFile) -> Self {
        SectionOutput {
            files: vec![file],
            notes: Vec::new(),
        }
    }

    /// An output with only a note.
    pub fn note(text: impl Into<String>) -> Self {
        SectionOutput {
            files: Vec::new(),
            notes: vec![text.into()],
        }
    }
}

/// What a section may look at.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The app and build.
    pub app: &'a AppInfo,
    /// The app's folders.
    pub dirs: &'a AppDirs,
    /// When the report is made.
    pub now: DateTime<Utc>,
}

/// A contribution to the report. Implementations must not panic (a panic is contained and noted, but costs the section) and
/// must read bounded amounts ([`window`](crate::window) has the tail readers).
pub trait Section: Send + Sync {
    /// A short stable id (`logs`, `runner-logs`), used in notes.
    fn id(&self) -> &str;
    /// Collects the section's files. Redaction is applied by the caller.
    fn collect(&self, cx: &Context<'_>) -> SectionOutput;
    /// Called once the user saved a report that holds this section's files (the crash section clears the markers it reported).
    /// The default does nothing.
    fn reported(&self, _cx: &Context<'_>) {}
}

pub mod builtin {
    //! The sections every app has.

    use super::{Content, Context, Section, SectionFile, SectionOutput};
    use crate::manifest::FileKind;
    use crate::window::{LOG_WINDOW, log_window_report};
    use paddock_log::CrashMarker;
    use serde_json::json;
    use std::sync::Arc;

    /// Largest settings file read.
    pub const MAX_SETTINGS_FILE: u64 = 1024 * 1024;

    /// The app, build and machine: `system-info.json`.
    pub struct SystemInfo;

    impl Section for SystemInfo {
        fn id(&self) -> &str {
            "system"
        }

        fn collect(&self, cx: &Context<'_>) -> SectionOutput {
            let info = json!({
                "app": cx.app,
                "os": std::env::consts::OS,
                "family": std::env::consts::FAMILY,
                "arch": std::env::consts::ARCH,
                "cpuCores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
                "debugBuild": cfg!(debug_assertions),
                "portable": cx.dirs.source() == paddock_env::Source::Portable,
            });
            SectionOutput::one(SectionFile {
                name: "system-info.json".into(),
                kind: FileKind::SystemInfo,
                summary: format!(
                    "{} {}, your operating system and processor type, no hardware serial numbers",
                    cx.app.name, cx.app.version
                ),
                content: Content::Json(info),
            })
        }
    }

    /// The app's settings file with secret values masked: `settings.json` by default, or the file the app names
    /// ([`Settings::file`]; the shell's own is `shell-settings.json`, so an app's `settings.json` in another format is
    /// never read as the shell's).
    pub struct Settings {
        file: String,
    }

    impl Settings {
        /// The settings file `name` in the config folder.
        pub fn file(name: impl Into<String>) -> Self {
            Settings { file: name.into() }
        }
    }

    impl Default for Settings {
        fn default() -> Self {
            Settings::file(paddock_settings::FILE_NAME)
        }
    }

    impl Section for Settings {
        fn id(&self) -> &str {
            "settings"
        }

        fn collect(&self, cx: &Context<'_>) -> SectionOutput {
            let path = cx.dirs.config().join(&self.file);
            let meta = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return SectionOutput::note(
                        "There is no settings file yet (the defaults are in use).",
                    );
                }
                Err(e) => {
                    return SectionOutput::note(format!(
                        "The settings file cannot be read ({}).",
                        e.kind()
                    ));
                }
            };
            if meta.len() > MAX_SETTINGS_FILE {
                return SectionOutput::note(format!(
                    "The settings file is larger than {} KB and was left out.",
                    MAX_SETTINGS_FILE / 1024
                ));
            }
            let parsed = std::fs::read(&path)
                .map_err(|e| format!("cannot be read ({})", e.kind()))
                .and_then(|b| {
                    serde_json::from_slice::<serde_json::Value>(&b).map_err(|e| {
                        format!(
                            "is not valid JSON (line {}, column {})",
                            e.line(),
                            e.column()
                        )
                    })
                });
            match parsed {
                Ok(value) => SectionOutput::one(SectionFile {
                    name: self.file.clone(),
                    kind: FileKind::Settings,
                    summary: "Your settings; passwords, tokens and keys are replaced by ***".into(),
                    content: Content::Json(value),
                }),
                Err(why) => SectionOutput::note(format!("The settings file {why}.")),
            }
        }
    }

    /// The newest part of the app's log: `logs/<file>` for each of the newest files within [`LOG_WINDOW`].
    pub struct Logs {
        window: u64,
    }

    impl Logs {
        /// The default window ([`LOG_WINDOW`]).
        pub fn new() -> Self {
            Logs { window: LOG_WINDOW }
        }

        /// A window of `bytes` over all files (tests and callers that want a smaller report).
        pub fn with_window(bytes: u64) -> Self {
            Logs { window: bytes }
        }
    }

    impl Default for Logs {
        fn default() -> Self {
            Logs::new()
        }
    }

    impl Section for Logs {
        fn id(&self) -> &str {
            "logs"
        }

        fn collect(&self, cx: &Context<'_>) -> SectionOutput {
            let found = log_window_report(cx.dirs.logs(), cx.app.id.as_str(), self.window);
            let mut notes = Vec::new();
            if let Some(why) = &found.folder_error {
                notes.push(format!(
                    "The log folder cannot be read ({why}), so no logs are in this report."
                ));
            }
            for (name, why) in &found.skipped {
                notes.push(format!(
                    "The log {name} cannot be read ({why}) and was left out."
                ));
            }
            if found.files.is_empty() && notes.is_empty() {
                return SectionOutput::note("There are no log files yet.");
            }
            SectionOutput {
                notes,
                files: found
                    .files
                    .into_iter()
                    .map(|(name, bytes, cut)| {
                        let lines = bytes.iter().filter(|b| **b == b'\n').count().max(1);
                        let part = if cut { "the recent part of" } else { "all of" };
                        SectionFile {
                            name: format!("logs/{name}"),
                            kind: FileKind::Log,
                            summary: format!(
                                "{lines} log lines, {part} {name}; file contents are never logged"
                            ),
                            content: Content::Log(bytes),
                        }
                    })
                    .collect(),
            }
        }
    }

    /// The markers crashed runs left (`crash-<process>.json`, one per process: the app, a helper, an agent), when there are any.
    /// Each is cleared once a report that holds it was saved (a crash is reported once, not at every later report).
    #[derive(Default)]
    pub struct Crash {
        reported: std::sync::Mutex<Vec<std::path::PathBuf>>,
    }

    impl Section for Crash {
        fn id(&self) -> &str {
            "crash"
        }

        fn collect(&self, cx: &Context<'_>) -> SectionOutput {
            let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(cx.dirs.data())
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| {
                            let n = p
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            n.starts_with("crash-") && n.ends_with(".json")
                        })
                        .collect()
                })
                .unwrap_or_default();
            paths.sort();
            let mut out = SectionOutput::default();
            let mut taken = Vec::new();
            for path in paths {
                let process = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
                    .trim_start_matches("crash-")
                    .trim_end_matches(".json")
                    .to_string();
                if process.is_empty()
                    || !process
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    continue;
                }
                match CrashMarker::read(&path) {
                    Some(marker) => {
                        let when = marker.time.clone();
                        out.files.push(SectionFile {
                            name: if process == "app" { "crash.json".into() } else { format!("crash-{process}.json") },
                            kind: FileKind::Crash,
                            summary: format!("A run of {process} ended in a crash at {when}: which thread and where"),
                            content: Content::Json(serde_json::to_value(marker).unwrap_or_default()),
                        });
                    }
                    None => out.notes.push(format!(
                        "The crash marker{} is damaged; a crash happened but its details are lost.",
                        if process == "app" {
                            String::new()
                        } else {
                            format!(" of {process}")
                        }
                    )),
                }
                taken.push(path);
            }
            *self
                .reported
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = taken;
            out
        }

        fn reported(&self, _cx: &Context<'_>) {
            let taken = std::mem::take(
                &mut *self
                    .reported
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            for path in taken {
                // A marker that cannot be removed is reported again next time; nothing else depends on it.
                let _ = CrashMarker::clear(&path);
            }
        }
    }

    /// The built-in sections, in report order.
    pub fn all() -> Vec<Arc<dyn Section>> {
        vec![
            Arc::new(SystemInfo),
            Arc::new(Settings::default()),
            Arc::new(Crash::default()),
            Arc::new(Logs::new()),
        ]
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_default_log_window_is_the_standard_one() {
            assert_eq!(Logs::default().window, Logs::new().window);
            assert_eq!(Logs::new().window, LOG_WINDOW);
            assert_eq!(Logs::with_window(5).window, 5);
        }
    }
}
