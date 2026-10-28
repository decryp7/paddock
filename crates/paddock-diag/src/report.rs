//! Collecting, previewing and writing a report.
//!
//! 1. [`Collected::collect`] runs the sections (each contained), redacts every file, bounds it and keeps the notes.
//! 2. [`Collected::assemble`] adds the user's description and the manifest: a [`Report`], the exact list of files.
//! 3. [`Report::preview`] lists those files with their sizes; [`Report::save`] writes the zip of the **same** list, so the
//!    preview equals the zip (names, kinds, sizes). The zip is built in memory (bounded) and replaces the destination
//!    atomically at the end: a cancel or a failure leaves no partial file.

use crate::error::{DiagError, Result, io};
use crate::manifest::{
    AppInfo, FileKind, MANIFEST_KIND, MANIFEST_NAME, Manifest, ManifestFile, SCHEMA_VERSION,
};
use crate::personal::RedactionCounts;
use crate::redact::Redactor;
use crate::section::{Content, Context, Section};
use chrono::{DateTime, Datelike, SecondsFormat, Timelike, Utc};
use paddock_env::AppDirs;
use paddock_jobs::{CancelToken, Counter};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Most files in a report (the manifest and description included).
pub const MAX_FILES: usize = 64;
/// Largest single file, bytes; a longer one is cut and says so.
pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
/// Largest report in total, bytes (uncompressed); a file that would pass it is left out and noted.
pub const MAX_TOTAL_BYTES: usize = 16_000_000;
/// Longest path inside the zip, characters.
pub const MAX_NAME_CHARS: usize = 100;
/// Longest description kept, bytes.
pub const MAX_DESCRIPTION: usize = 10 * 1024;
/// Longest note kept, characters.
pub const MAX_NOTE: usize = 300;
/// The marker appended to a cut file.
pub const TRUNCATION_NOTE: &str = "\n[cut: the file was longer than a report allows]\n";
/// Bytes written between two cancel checks and progress steps.
const CHUNK: usize = 256 * 1024;

/// Whether `name` is a safe name inside the zip: relative, `/` separated, ASCII letters, digits and `. - _`, no `..`.
pub fn valid_entry_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && !name.starts_with('/')
        && name
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '_'))
}

/// The bounds of a report. The defaults are [`MAX_FILES`], [`MAX_FILE_BYTES`] and [`MAX_TOTAL_BYTES`]; tests use small ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Most files, the manifest and description included.
    pub max_files: usize,
    /// Largest single file, bytes.
    pub max_file_bytes: usize,
    /// Largest report in total, bytes.
    pub max_total_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_files: MAX_FILES,
            max_file_bytes: MAX_FILE_BYTES,
            max_total_bytes: MAX_TOTAL_BYTES,
        }
    }
}

/// One file of a report as it will be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportFile {
    /// Path inside the zip.
    pub name: String,
    /// What it is.
    pub kind: FileKind,
    /// One line for the preview.
    pub summary: String,
    /// The redacted, bounded bytes.
    pub data: Vec<u8>,
}

/// The sections' output, redacted and bounded, before the description and manifest are added.
#[derive(Debug, Clone)]
pub struct Collected {
    app: AppInfo,
    created_at: DateTime<Utc>,
    files: Vec<ReportFile>,
    notes: Vec<String>,
    redactions: RedactionCounts,
}

fn cut_to_char(bytes: &mut Vec<u8>, max: usize) -> bool {
    if bytes.len() <= max {
        return false;
    }
    let mut end = max;
    // Back up to a UTF-8 boundary (continuation bytes are 0b10xxxxxx).
    while end > 0 && (bytes[end] & 0xC0) == 0x80 {
        end -= 1;
    }
    bytes.truncate(end);
    true
}

fn note(text: String) -> String {
    text.chars().take(MAX_NOTE).collect()
}

impl Collected {
    /// Runs `sections`, redacts and bounds their files. A section that panics gives a note and no files; a file with an
    /// invalid or repeated name, or one that would pass the limits, is left out with a note. Never fails.
    pub fn collect(
        app: &AppInfo,
        dirs: &AppDirs,
        sections: &[Arc<dyn Section>],
        redactor: &Redactor,
        now: DateTime<Utc>,
    ) -> Collected {
        Collected::collect_with(app, dirs, sections, redactor, now, Limits::default())
    }

    /// As [`Collected::collect`] with other bounds.
    pub fn collect_with(
        app: &AppInfo,
        dirs: &AppDirs,
        sections: &[Arc<dyn Section>],
        redactor: &Redactor,
        now: DateTime<Utc>,
        limits: Limits,
    ) -> Collected {
        let cx = Context { app, dirs, now };
        let mut out = Collected {
            app: app.clone(),
            created_at: now,
            files: Vec::new(),
            notes: Vec::new(),
            redactions: RedactionCounts::default(),
        };
        let mut total = 0usize;
        for section in sections {
            let id = section.id().to_string();
            let Ok(output) = catch_unwind(AssertUnwindSafe(|| section.collect(&cx))) else {
                out.notes
                    .push(note(format!("The {id} section failed and was left out.")));
                continue;
            };
            out.notes.extend(output.notes.into_iter().map(note));
            for file in output.files {
                // The manifest and description are the report's own; 2 slots are kept for them.
                if out.files.len() + 2 >= limits.max_files {
                    out.notes.push(note(format!(
                        "{} was left out: a report holds at most {} files.",
                        file.name, limits.max_files
                    )));
                    continue;
                }
                if !valid_entry_name(&file.name)
                    || file.name == MANIFEST_NAME
                    || file.name == DESCRIPTION_NAME
                {
                    out.notes.push(note(format!(
                        "A file of the {id} section was left out: \"{}\" is not an allowed name.",
                        file.name.chars().take(60).collect::<String>()
                    )));
                    continue;
                }
                if out.files.iter().any(|f| f.name == file.name) {
                    out.notes.push(note(format!(
                        "{} was left out: the name is used twice.",
                        file.name
                    )));
                    continue;
                }
                // The input is bounded before redaction: a section that hands over 100 MB costs a 4 MB pass, not a 100 MB one.
                let (content, input_cut) = bound_content(file.content, limits.max_file_bytes);
                let (mut data, counts) = redact_content(redactor, content);
                let cut = cut_to_char(&mut data, limits.max_file_bytes) || input_cut;
                if cut {
                    data.extend_from_slice(TRUNCATION_NOTE.as_bytes());
                }
                if total + data.len() > limits.max_total_bytes {
                    out.notes.push(note(format!(
                        "{} was left out: a report holds at most {}.",
                        file.name,
                        paddock_units::format_bytes(
                            limits.max_total_bytes as u64,
                            paddock_units::Kind::Storage
                        )
                    )));
                    continue;
                }
                total += data.len();
                out.redactions.add(&counts);
                out.files.push(ReportFile {
                    name: file.name,
                    kind: file.kind,
                    summary: file.summary,
                    data,
                });
            }
        }
        out
    }

    /// The files collected so far (without description and manifest).
    pub fn files(&self) -> &[ReportFile] {
        &self.files
    }

    /// What could not be collected.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Adds the user's `description` (cut to [`MAX_DESCRIPTION`] bytes; it is written as typed, not redacted: the user chose
    /// it and sees it in the preview) and the manifest. Cheap, so the preview can follow every keystroke.
    pub fn assemble(&self, description: &str) -> Report {
        let description = cap_description(description.trim());
        let mut files = vec![ReportFile {
            name: DESCRIPTION_NAME.into(),
            kind: FileKind::Description,
            summary: if description.is_empty() {
                "No description".into()
            } else {
                format!(
                    "Your description ({} characters)",
                    description.chars().count()
                )
            },
            data: description.into_bytes(),
        }];
        files.extend(self.files.iter().cloned());
        let manifest = Manifest {
            kind: MANIFEST_KIND.into(),
            schema_version: SCHEMA_VERSION,
            app: self.app.clone(),
            created_at: self.created_at.to_rfc3339_opts(SecondsFormat::Millis, true),
            redactions: self.redactions,
            notes: self.notes.clone(),
            files: files
                .iter()
                .map(|f| ManifestFile {
                    name: f.name.clone(),
                    kind: f.kind,
                    bytes: f.data.len() as u64,
                })
                .collect(),
        };
        let mut all = Vec::with_capacity(files.len() + 1);
        all.push(ReportFile {
            name: MANIFEST_NAME.into(),
            kind: FileKind::Manifest,
            summary: format!(
                "The index of the other {} files and what was removed from them",
                files.len()
            ),
            data: serde_json::to_vec_pretty(&manifest).unwrap_or_default(),
        });
        all.extend(files);
        Report {
            created_at: self.created_at,
            files: all,
            notes: self.notes.clone(),
            redactions: self.redactions,
        }
    }
}

/// The name of the description file.
pub const DESCRIPTION_NAME: &str = "description.txt";

/// The description, cut to [`MAX_DESCRIPTION`] bytes at a character boundary.
pub fn cap_description(text: &str) -> String {
    if text.len() <= MAX_DESCRIPTION {
        return text.to_string();
    }
    let mut end = MAX_DESCRIPTION;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// The content cut to `max` bytes (at a character boundary), and whether it was cut.
fn bound_content(content: Content, max: usize) -> (Content, bool) {
    match content {
        Content::Text(t) if t.len() > max => {
            let mut bytes = t.into_bytes();
            cut_to_char(&mut bytes, max);
            (
                Content::Text(String::from_utf8_lossy(&bytes).into_owned()),
                true,
            )
        }
        Content::Log(mut raw) if raw.len() > max => {
            raw.truncate(max);
            (Content::Log(raw), true)
        }
        other => (other, false),
    }
}

fn redact_content(redactor: &Redactor, content: Content) -> (Vec<u8>, RedactionCounts) {
    match content {
        Content::Text(t) => {
            let (s, c) = redactor.text(&t);
            (s.into_bytes(), c)
        }
        Content::Json(v) => {
            let (v, c) = redactor.json(&v);
            let mut bytes = serde_json::to_vec_pretty(&v).unwrap_or_default();
            bytes.push(b'\n');
            (bytes, c)
        }
        Content::Log(raw) => {
            let mut counts = RedactionCounts::default();
            let mut out = Vec::with_capacity(raw.len());
            for line in String::from_utf8_lossy(&raw).lines() {
                let (l, c) = redactor.log_line(line);
                counts.add(&c);
                out.extend_from_slice(l.as_bytes());
                out.push(b'\n');
            }
            (out, counts)
        }
    }
}

/// One file as the preview lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PreviewFile {
    /// Path inside the zip.
    pub name: String,
    /// What it is.
    pub kind: FileKind,
    /// Size in bytes (uncompressed).
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    /// What it holds, in words.
    pub summary: String,
}

/// Exactly what a report would hold: every file with its size, what was removed and what could not be collected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// The files, in zip order (the manifest first).
    pub files: Vec<PreviewFile>,
    /// Total bytes before compression.
    #[cfg_attr(test, ts(type = "number"))]
    pub total_bytes: u64,
    /// What redaction removed, by kind.
    pub redactions: RedactionCounts,
    /// What could not be collected.
    pub notes: Vec<String>,
}

/// What a saved report is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    /// Where the zip was written.
    pub path: String,
    /// Its size on disk, bytes.
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    /// How many files it holds (the manifest included).
    pub files: u32,
}

/// The files of a report, ready to preview and to write.
#[derive(Debug, Clone)]
pub struct Report {
    created_at: DateTime<Utc>,
    files: Vec<ReportFile>,
    notes: Vec<String>,
    redactions: RedactionCounts,
}

impl Report {
    /// The files in zip order: the manifest, the description, then the sections' files.
    pub fn files(&self) -> &[ReportFile] {
        &self.files
    }

    /// Total bytes before compression.
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.data.len() as u64).sum()
    }

    /// The preview: the same list [`Report::save`] writes.
    pub fn preview(&self) -> Preview {
        Preview {
            files: self
                .files
                .iter()
                .map(|f| PreviewFile {
                    name: f.name.clone(),
                    kind: f.kind,
                    bytes: f.data.len() as u64,
                    summary: f.summary.clone(),
                })
                .collect(),
            total_bytes: self.total_bytes(),
            redactions: self.redactions,
            notes: self.notes.clone(),
        }
    }

    /// The zip, in memory. `cancel` is checked between entries and every 256 KiB; `progress` counts the bytes of content
    /// packed so far. The same inputs give the same bytes (entry times are the report's, the platform is fixed).
    pub fn zip_bytes(&self, cancel: &CancelToken, progress: &dyn Counter) -> Result<Vec<u8>> {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip_time(&self.created_at))
            .system(zip::System::Unix)
            .unix_permissions(0o644);
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let zip_err = |e: zip::result::ZipError| DiagError::Zip(e.to_string());
        for file in &self.files {
            cancel.check().map_err(|_| DiagError::Cancelled)?;
            zip.start_file(file.name.as_str(), options)
                .map_err(zip_err)?;
            for chunk in file.data.chunks(CHUNK) {
                cancel.check().map_err(|_| DiagError::Cancelled)?;
                zip.write_all(chunk)
                    .map_err(|e| DiagError::Zip(e.to_string()))?;
                progress.add(chunk.len() as u64);
            }
        }
        Ok(zip.finish().map_err(zip_err)?.into_inner())
    }

    /// Writes the zip to `dest` (see [`validate_destination`]) and returns what was written. Nothing is written before the
    /// zip is complete, then the file replaces `dest` atomically: a cancel or an error leaves no partial file.
    pub fn save(&self, dest: &Path, cancel: &CancelToken, progress: &dyn Counter) -> Result<Saved> {
        let dest = validate_destination(dest)?;
        let bytes = self.zip_bytes(cancel, progress)?;
        cancel.check().map_err(|_| DiagError::Cancelled)?;
        paddock_env::write_atomic(&dest, &bytes, false).map_err(|e| DiagError::Io {
            action: "write",
            path: dest.clone(),
            source: std::io::Error::other(e.to_string()),
        })?;
        Ok(Saved {
            path: dest.display().to_string(),
            bytes: bytes.len() as u64,
            files: self.files.len() as u32,
        })
    }
}

/// A zip entry time (zip times are local wall clock without a zone; reports use UTC), clamped to the format's range.
fn zip_time(t: &DateTime<Utc>) -> zip::DateTime {
    let year = t.year().clamp(1980, 2107) as u16;
    zip::DateTime::from_date_and_time(
        year,
        t.month() as u8,
        t.day() as u8,
        t.hour() as u8,
        t.minute() as u8,
        t.second() as u8,
    )
    .unwrap_or_default()
}

/// Checks the destination the user chose: absolute, no `..`, a name ending in `.zip`, an existing folder, not a folder
/// itself. Returns the path with the folder canonicalised.
pub fn validate_destination(path: &Path) -> Result<PathBuf> {
    let bad = |m: &str| DiagError::BadDestination(m.into());
    let text = path.to_string_lossy();
    if text.trim().is_empty() || text.len() > 1024 || text.contains('\0') {
        return Err(bad("Choose where to save the report."));
    }
    if !path.is_absolute() {
        return Err(bad("Choose a file with its full path."));
    }
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(bad("The save path may not contain \"..\"."));
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !name.to_ascii_lowercase().ends_with(".zip") || name.len() <= 4 {
        return Err(bad("Save the report as a file ending in .zip."));
    }
    let parent = path
        .parent()
        .ok_or_else(|| bad("Choose a folder to save in."))?;
    let parent = std::fs::canonicalize(parent)
        .map_err(io("find the folder", parent))
        .map_err(|_| bad("The folder does not exist. Choose another one."))?;
    if !parent.is_dir() {
        return Err(bad("The folder does not exist. Choose another one."));
    }
    let dest = parent.join(name);
    if dest.is_dir() {
        return Err(bad(
            "That name is a folder. Choose a file name ending in .zip.",
        ));
    }
    Ok(dest)
}

/// The file name suggested in the save dialog: `<app id>-diagnostics-<UTC time>.zip`.
pub fn suggested_name(app: &AppInfo, now: DateTime<Utc>) -> String {
    format!("{}-diagnostics-{}.zip", app.id, now.format("%Y%m%d-%H%M%S"))
}
