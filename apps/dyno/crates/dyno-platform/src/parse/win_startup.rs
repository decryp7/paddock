//! Windows startup entries from recorded reads (ADR 0033, WP-4): [`assemble`] turns what the registry views and the Startup folders
//! gave, source by source, into `StartupEntry` rows, partial-failure sentences and per-source counts. It is pure: the OS calls sit
//! behind `win_startup_source::{RegistryView, FolderList}`, so every OS tests this part with recorded inputs.
//!
//! What it reads, where each state comes from and the id formats are the table of ADR 0033 section 4.1. Logon tasks, packaged apps and
//! signers are not read in 1.1.1 (the 2026-11-11 trim), which [`COVERAGE`] says.

use crate::parse::lnk::parse_lnk;
use crate::parse::startup_approved::approved_state;
use dyno_core::startup::{exe_of_command, ImpactRating, StartupEntry, StartupImpact, StartupLocation, StartupState};
use std::collections::HashMap;

/// At most this many values are read from one Run key; the rest are counted as skipped.
pub const MAX_RUN_VALUES: usize = 1_000;
/// A value larger than this (in bytes, as stored) is not read; it is counted as skipped.
pub const MAX_VALUE_BYTES: usize = 32 * 1024;
/// At most this many files are read from one Startup folder; the rest are counted as skipped.
pub const MAX_FOLDER_FILES: usize = 1_000;
/// A command whose environment-variable expansion is longer than this (in characters) is skipped.
pub const MAX_EXPANDED_CHARS: usize = 32 * 1024;

/// What the Windows startup card covers, said once under the list (coverage is not a failure, so it is not in `unavailable`).
pub const COVERAGE: &str =
    "Lists registry Run and RunOnce entries and Startup folders. Not listed: sign-in tasks, Store apps, services, tasks that \
start with Windows, policy Run keys and Winlogon entries.";

/// Whose registry hive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hive {
    /// `HKEY_CURRENT_USER`.
    User,
    /// `HKEY_LOCAL_MACHINE`.
    Machine,
}

/// Which key under `CurrentVersion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunKind {
    /// `Run`: every sign-in.
    Run,
    /// `RunOnce`: the next sign-in only.
    RunOnce,
}

/// Which registry view (WOW64) the key is read through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum View {
    /// The 64-bit view (`KEY_WOW64_64KEY`).
    Bits64,
    /// The 32-bit view (`KEY_WOW64_32KEY`, `WOW6432Node` under HKLM).
    Bits32,
}

/// One Run or RunOnce key as read through one view: the unit of a registry source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegSource {
    /// Whose hive.
    pub hive: Hive,
    /// `Run` or `RunOnce`.
    pub kind: RunKind,
    /// Which view.
    pub view: View,
}

/// The `StartupApproved` subkeys Task Manager writes (measured as ADR 0033, WP-0 asks, on the Windows 10 and 11 layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApprovedKey {
    /// `StartupApproved\Run`: the 64-bit Run key.
    Run,
    /// `StartupApproved\Run32`: HKLM's 32-bit Run key.
    Run32,
    /// `StartupApproved\StartupFolder`: the files of a Startup folder.
    StartupFolder,
}

/// Which Startup folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FolderKind {
    /// `FOLDERID_Startup`.
    User,
    /// `FOLDERID_CommonStartup`.
    Common,
}

impl RegSource {
    /// Every registry source, in the order the list shows them. HKCU's 32-bit view is the same key as its 64-bit one
    /// (`HKCU\Software` is not redirected), so [`assemble`] merges their duplicates.
    pub const ALL: [RegSource; 8] = [
        RegSource::new(Hive::User, RunKind::Run, View::Bits64),
        RegSource::new(Hive::User, RunKind::Run, View::Bits32),
        RegSource::new(Hive::User, RunKind::RunOnce, View::Bits64),
        RegSource::new(Hive::User, RunKind::RunOnce, View::Bits32),
        RegSource::new(Hive::Machine, RunKind::Run, View::Bits64),
        RegSource::new(Hive::Machine, RunKind::Run, View::Bits32),
        RegSource::new(Hive::Machine, RunKind::RunOnce, View::Bits64),
        RegSource::new(Hive::Machine, RunKind::RunOnce, View::Bits32),
    ];

    /// A source of `hive`, `kind` and `view`.
    pub const fn new(hive: Hive, kind: RunKind, view: View) -> Self {
        RegSource { hive, kind, view }
    }

    /// The name in log lines (no user content).
    pub fn log_name(&self) -> &'static str {
        match (self.hive, self.kind, self.view) {
            (Hive::User, RunKind::Run, View::Bits64) => "hkcu-run-64",
            (Hive::User, RunKind::Run, View::Bits32) => "hkcu-run-32",
            (Hive::User, RunKind::RunOnce, View::Bits64) => "hkcu-runonce-64",
            (Hive::User, RunKind::RunOnce, View::Bits32) => "hkcu-runonce-32",
            (Hive::Machine, RunKind::Run, View::Bits64) => "hklm-run-64",
            (Hive::Machine, RunKind::Run, View::Bits32) => "hklm-run-32",
            (Hive::Machine, RunKind::RunOnce, View::Bits64) => "hklm-runonce-64",
            (Hive::Machine, RunKind::RunOnce, View::Bits32) => "hklm-runonce-32",
        }
    }

    /// The name in the sentence the page shows when this source could not be read.
    pub fn sentence_name(&self) -> &'static str {
        match (self.hive, self.kind, self.view) {
            (Hive::User, RunKind::Run, View::Bits64) => "Your Run key",
            (Hive::User, RunKind::Run, View::Bits32) => "Your Run key (32-bit)",
            (Hive::User, RunKind::RunOnce, View::Bits64) => "Your RunOnce key",
            (Hive::User, RunKind::RunOnce, View::Bits32) => "Your RunOnce key (32-bit)",
            (Hive::Machine, RunKind::Run, View::Bits64) => "The all-users Run key",
            (Hive::Machine, RunKind::Run, View::Bits32) => "The all-users Run key (32-bit)",
            (Hive::Machine, RunKind::RunOnce, View::Bits64) => "The all-users RunOnce key",
            (Hive::Machine, RunKind::RunOnce, View::Bits32) => "The all-users RunOnce key (32-bit)",
        }
    }

    /// The `StartupApproved` key that governs this source, `None` for RunOnce (always enabled: it runs once, then Windows removes it).
    /// HKCU's two views share `Run`; HKLM's 32-bit view has its own `Run32`.
    pub fn approved_key(&self) -> Option<ApprovedKey> {
        match (self.kind, self.hive, self.view) {
            (RunKind::RunOnce, _, _) => None,
            (RunKind::Run, Hive::Machine, View::Bits32) => Some(ApprovedKey::Run32),
            (RunKind::Run, _, _) => Some(ApprovedKey::Run),
        }
    }

    fn location(&self) -> StartupLocation {
        match (self.hive, self.kind) {
            (Hive::User, RunKind::Run) => StartupLocation::UserRunKey,
            (Hive::Machine, RunKind::Run) => StartupLocation::MachineRunKey,
            (Hive::User, RunKind::RunOnce) => StartupLocation::UserRunOnceKey,
            (Hive::Machine, RunKind::RunOnce) => StartupLocation::MachineRunOnceKey,
        }
    }

    fn id_prefix(&self) -> String {
        let hive = if self.hive == Hive::User { "HKCU" } else { "HKLM" };
        let kind = if self.kind == RunKind::Run { "run" } else { "runonce" };
        let bits = if self.view == View::Bits64 { "64" } else { "32" };
        format!("reg:{hive}:{kind}:{bits}:")
    }
}

impl FolderKind {
    /// Both folders, in list order.
    pub const ALL: [FolderKind; 2] = [FolderKind::User, FolderKind::Common];

    /// The name in log lines.
    pub fn log_name(&self) -> &'static str {
        match self {
            FolderKind::User => "startup-folder-user",
            FolderKind::Common => "startup-folder-common",
        }
    }

    /// The name in the sentence the page shows when this folder could not be read.
    pub fn sentence_name(&self) -> &'static str {
        match self {
            FolderKind::User => "Your Startup folder",
            FolderKind::Common => "The all-users Startup folder",
        }
    }

    /// The hive of the `StartupApproved\StartupFolder` key that governs it.
    pub fn approved_hive(&self) -> Hive {
        match self {
            FolderKind::User => Hive::User,
            FolderKind::Common => Hive::Machine,
        }
    }

    fn location(&self) -> StartupLocation {
        match self {
            FolderKind::User => StartupLocation::UserStartupFolder,
            FolderKind::Common => StartupLocation::CommonStartupFolder,
        }
    }

    fn id_prefix(&self) -> &'static str {
        match self {
            FolderKind::User => "folder:user:",
            FolderKind::Common => "folder:common:",
        }
    }
}

/// A registry value's data, as far as the reader looked at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegData {
    /// `REG_SZ`.
    Str(String),
    /// `REG_EXPAND_SZ`, not yet expanded.
    ExpandStr(String),
    /// Any other type (a number, binary data, a list): not a command.
    Other,
    /// A string longer than [`MAX_VALUE_BYTES`], which the reader did not read.
    TooLong,
}

/// One value of a Run key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegValue {
    /// The value's name (the entry's name).
    pub name: String,
    /// Its data.
    pub data: RegData,
}

/// One Run key as read: its values, and how many the reader left unread past [`MAX_RUN_VALUES`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunKeyRead {
    /// The values that were read, in enumeration order.
    pub values: Vec<RegValue>,
    /// Values past the cap that were not read.
    pub past_cap: u32,
}

/// One file of a Startup folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderFile {
    /// The file name with its extension.
    pub name: String,
    /// The full path.
    pub path: String,
    /// A `.lnk`'s bytes, when the reader could read them within [`crate::parse::lnk::MAX_LNK_BYTES`].
    pub lnk: Option<Vec<u8>>,
}

/// One Startup folder as read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FolderRead {
    /// The files that were read.
    pub files: Vec<FolderFile>,
    /// Files past [`MAX_FOLDER_FILES`] that were not read.
    pub past_cap: u32,
}

/// What each `StartupApproved` key held: value name (lower case) to its bytes.
pub type ApprovedValues = HashMap<String, Vec<u8>>;

/// Everything the readers answered, source by source. An `Err` is the reason a source could not be read, without a final full stop
/// (for example `access denied (error 5)`); a key or folder that does not exist is an `Ok` with nothing in it.
#[derive(Debug, Default)]
pub struct Reads {
    /// The Run and RunOnce keys.
    pub registry: HashMap<RegSource, Result<RunKeyRead, String>>,
    /// The `StartupApproved` keys; one that cannot be read leaves its entries enabled, which is what Windows does.
    pub approved: HashMap<(Hive, ApprovedKey), Result<ApprovedValues, String>>,
    /// The Startup folders.
    pub folders: HashMap<FolderKind, Result<FolderRead, String>>,
}

/// How one source ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceOutcome {
    /// Read; a key or folder that is not there reads as empty.
    Ok,
    /// Could not be read; the reason, without a final full stop.
    Failed(String),
}

/// What one source gave, for the log line (ER-27): no names, commands or paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceStat {
    /// The source in log form (`hklm-run-32`).
    pub source: &'static str,
    /// Entries it listed (after merging duplicates).
    pub count: u32,
    /// Values or files it skipped: non-text, too long, past a cap, empty, or not a startup file.
    pub skipped: u32,
    /// How it ended.
    pub outcome: SourceOutcome,
}

/// The assembled list.
#[derive(Debug, Default, PartialEq)]
pub struct Assembled {
    /// The entries, source by source.
    pub entries: Vec<StartupEntry>,
    /// One sentence for each source that could not be read.
    pub unavailable: Vec<String>,
    /// One record for each source, in list order.
    pub sources: Vec<SourceStat>,
    /// Whether every source failed (the caller then reports an error instead of an empty list).
    pub all_failed: bool,
}

/// Whether `name` can be an environment variable's name: not empty, not longer than Windows allows (255) and without `=` or NUL, which
/// the OS calls refuse (`std::env::var` panics on them). A registry value is user data, so `%a=b%` must not reach the lookup.
fn is_variable_name(name: &str) -> bool {
    (1..=255).contains(&name.chars().count()) && !name.contains(['=', '\0'])
}

/// Expands `%NAME%` references through `env`. A name `env` does not know is left as written, as Windows does. `None` when the result
/// would pass [`MAX_EXPANDED_CHARS`].
pub fn expand_env(text: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if is_variable_name(&after[..end]) => match env(&after[..end]) {
                Some(value) => {
                    out.push_str(&value);
                    rest = &after[end + 1..];
                }
                None => {
                    out.push('%');
                    rest = after;
                }
            },
            _ => {
                out.push('%');
                rest = after;
            }
        }
        if out.chars().count() > MAX_EXPANDED_CHARS {
            return None;
        }
    }
    out.push_str(rest);
    (out.chars().count() <= MAX_EXPANDED_CHARS).then_some(out)
}

fn not_measured() -> StartupImpact {
    StartupImpact {
        cpu_secs: 0.0,
        disk_bytes: 0,
        rating: ImpactRating::NotMeasured,
    }
}

fn approved_state_of(reads: &Reads, hive: Hive, key: Option<ApprovedKey>, name: &str) -> StartupState {
    let Some(key) = key else { return StartupState::Enabled };
    let Some(Ok(values)) = reads.approved.get(&(hive, key)) else {
        return StartupState::Enabled;
    };
    values
        .get(&name.to_lowercase())
        .and_then(|bytes| approved_state(bytes))
        .unwrap_or(StartupState::Enabled)
}

fn registry_entries(
    reads: &Reads,
    src: RegSource,
    read: &RunKeyRead,
    env: &dyn Fn(&str) -> Option<String>,
    seen: &mut Vec<(Hive, RunKind, String, String)>,
) -> (Vec<StartupEntry>, u32) {
    let mut skipped = read
        .past_cap
        .saturating_add(read.values.len().saturating_sub(MAX_RUN_VALUES) as u32);
    let mut entries = Vec::new();
    for value in read.values.iter().take(MAX_RUN_VALUES) {
        let command = match &value.data {
            RegData::Str(text) => Some(text.clone()),
            RegData::ExpandStr(text) => expand_env(text, env),
            RegData::Other | RegData::TooLong => None,
        };
        let Some(command) = command.map(|c| c.trim().to_string()).filter(|c| !c.is_empty()) else {
            skipped += 1;
            continue;
        };
        // HKCU is one key seen through two views; HKLM's views are two keys, so their entries stay apart.
        if src.hive == Hive::User {
            let id = (src.hive, src.kind, value.name.to_lowercase(), command.clone());
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
        }
        entries.push(StartupEntry {
            id: format!("{}{}", src.id_prefix(), value.name),
            name: value.name.clone(),
            exe: exe_of_command(&command),
            command,
            location: src.location(),
            state: approved_state_of(reads, src.hive, src.approved_key(), &value.name),
            publisher: None,
            impact: not_measured(),
        });
    }
    entries.sort_by_cached_key(|e| e.name.to_lowercase());
    (entries, skipped)
}

fn folder_entries(reads: &Reads, kind: FolderKind, read: &FolderRead) -> (Vec<StartupEntry>, u32) {
    let mut skipped = read
        .past_cap
        .saturating_add(read.files.len().saturating_sub(MAX_FOLDER_FILES) as u32);
    let mut entries = Vec::new();
    for file in read.files.iter().take(MAX_FOLDER_FILES) {
        if file.name.eq_ignore_ascii_case("desktop.ini") {
            skipped += 1;
            continue;
        }
        let is_lnk = file.name.len() > 4 && file.name.to_ascii_lowercase().ends_with(".lnk");
        // A shortcut with no local path (a Store app's) or one that was too large to read shows as the file itself.
        let target = if is_lnk { file.lnk.as_deref().and_then(parse_lnk) } else { None };
        let (name, command, exe) = match (&target, is_lnk) {
            (Some(t), _) => (file.name[..file.name.len() - 4].to_string(), t.command(), Some(t.path.clone())),
            (None, true) => (
                file.name[..file.name.len() - 4].to_string(),
                file.path.clone(),
                Some(file.path.clone()),
            ),
            (None, false) => (file.name.clone(), file.path.clone(), Some(file.path.clone())),
        };
        entries.push(StartupEntry {
            id: format!("{}{}", kind.id_prefix(), file.name),
            name,
            command,
            exe,
            location: kind.location(),
            state: approved_state_of(reads, kind.approved_hive(), Some(ApprovedKey::StartupFolder), &file.name),
            publisher: None,
            impact: not_measured(),
        });
    }
    entries.sort_by_cached_key(|e| e.name.to_lowercase());
    (entries, skipped)
}

/// Builds the list from the recorded reads. `env` answers `%NAME%` references.
pub fn assemble(reads: &Reads, env: &dyn Fn(&str) -> Option<String>) -> Assembled {
    let mut out = Assembled::default();
    let mut seen = Vec::new();
    let mut failed = 0usize;
    let mut total = 0usize;
    let mut finish =
        |out: &mut Assembled, source: &'static str, sentence: &'static str, got: Option<&Result<(Vec<StartupEntry>, u32), String>>| {
            total += 1;
            match got {
                Some(Ok((entries, skipped))) => {
                    out.sources.push(SourceStat {
                        source,
                        count: entries.len() as u32,
                        skipped: *skipped,
                        outcome: SourceOutcome::Ok,
                    });
                    out.entries.extend(entries.iter().cloned());
                }
                Some(Err(why)) => {
                    failed += 1;
                    out.unavailable.push(format!("{sentence} could not be read: {why}."));
                    out.sources.push(SourceStat {
                        source,
                        count: 0,
                        skipped: 0,
                        outcome: SourceOutcome::Failed(why.clone()),
                    });
                }
                // A source nobody answered for is a read that was not made: it counts as failed, never as empty.
                None => {
                    failed += 1;
                    let why = "it was not read".to_string();
                    out.unavailable.push(format!("{sentence} could not be read: {why}."));
                    out.sources.push(SourceStat {
                        source,
                        count: 0,
                        skipped: 0,
                        outcome: SourceOutcome::Failed(why),
                    });
                }
            }
        };
    for src in RegSource::ALL {
        let got = reads.registry.get(&src).map(|r| {
            r.as_ref()
                .map(|k| registry_entries(reads, src, k, env, &mut seen))
                .map_err(Clone::clone)
        });
        finish(&mut out, src.log_name(), src.sentence_name(), got.as_ref());
    }
    for kind in FolderKind::ALL {
        let got = reads
            .folders
            .get(&kind)
            .map(|r| r.as_ref().map(|f| folder_entries(reads, kind, f)).map_err(Clone::clone));
        finish(&mut out, kind.log_name(), kind.sentence_name(), got.as_ref());
    }
    out.all_failed = total > 0 && failed == total;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::lnk::tests::build as lnk;
    use crate::parse::startup_approved::approved_value;

    const HKCU_RUN: RegSource = RegSource::new(Hive::User, RunKind::Run, View::Bits64);
    const HKCU_RUN32: RegSource = RegSource::new(Hive::User, RunKind::Run, View::Bits32);
    const HKCU_ONCE: RegSource = RegSource::new(Hive::User, RunKind::RunOnce, View::Bits64);
    const HKLM_RUN: RegSource = RegSource::new(Hive::Machine, RunKind::Run, View::Bits64);
    const HKLM_RUN32: RegSource = RegSource::new(Hive::Machine, RunKind::Run, View::Bits32);
    const HKLM_ONCE: RegSource = RegSource::new(Hive::Machine, RunKind::RunOnce, View::Bits64);

    fn env(name: &str) -> Option<String> {
        match name.to_ascii_lowercase().as_str() {
            "programfiles" => Some("C:\\Program Files".into()),
            "appdata" => Some("C:\\Users\\Pat\\AppData\\Roaming".into()),
            _ => None,
        }
    }

    fn s(name: &str, data: &str) -> RegValue {
        RegValue {
            name: name.into(),
            data: RegData::Str(data.into()),
        }
    }

    fn key(values: Vec<RegValue>) -> Result<RunKeyRead, String> {
        Ok(RunKeyRead { values, past_cap: 0 })
    }

    /// Every source readable and empty, so a test names only what it sets.
    fn quiet() -> Reads {
        let mut r = Reads::default();
        for src in RegSource::ALL {
            r.registry.insert(src, key(vec![]));
        }
        for f in FolderKind::ALL {
            r.folders.insert(f, Ok(FolderRead::default()));
        }
        r
    }

    fn approved(pairs: &[(&str, bool)]) -> Result<ApprovedValues, String> {
        Ok(pairs
            .iter()
            .map(|(n, on)| (n.to_ascii_lowercase(), approved_value(*on, 0x01d9_0000_0000_0000).to_vec()))
            .collect())
    }

    fn find<'a>(a: &'a Assembled, id: &str) -> &'a StartupEntry {
        a.entries
            .iter()
            .find(|e| e.id == id)
            .unwrap_or_else(|| panic!("no {id} in {:?}", a.entries.iter().map(|e| &e.id).collect::<Vec<_>>()))
    }

    fn stat<'a>(a: &'a Assembled, source: &str) -> &'a SourceStat {
        a.sources.iter().find(|s| s.source == source).unwrap()
    }

    #[test]
    fn an_enabled_user_run_value_is_listed_with_its_command_and_executable() {
        let mut r = quiet();
        r.registry
            .insert(HKCU_RUN, key(vec![s("Spotify", "\"C:\\Apps\\Spotify.exe\" --minimized")]));
        let a = assemble(&r, &env);
        let e = find(&a, "reg:HKCU:run:64:Spotify");
        assert_eq!(e.name, "Spotify");
        assert_eq!(e.command, "\"C:\\Apps\\Spotify.exe\" --minimized");
        assert_eq!(e.exe.as_deref(), Some("C:\\Apps\\Spotify.exe"));
        assert_eq!(e.location, StartupLocation::UserRunKey);
        assert_eq!(e.state, StartupState::Enabled);
        assert_eq!(e.publisher, None, "signers are not read in 1.1.1");
        assert_eq!(e.impact.rating, ImpactRating::NotMeasured);
        assert!(a.unavailable.is_empty() && !a.all_failed);
        assert_eq!(a.sources.len(), RegSource::ALL.len() + FolderKind::ALL.len());
    }

    #[test]
    fn a_value_turned_off_in_startup_approved_is_disabled() {
        let mut r = quiet();
        r.registry.insert(
            HKCU_RUN,
            key(vec![s("Off", "C:\\a.exe"), s("On", "C:\\b.exe"), s("Unlisted", "C:\\c.exe")]),
        );
        // Value names compare without case; a name with no value means Enabled, which is what Windows does.
        r.approved
            .insert((Hive::User, ApprovedKey::Run), approved(&[("off", false), ("ON", true)]));
        let a = assemble(&r, &env);
        assert_eq!(find(&a, "reg:HKCU:run:64:Off").state, StartupState::Disabled);
        assert_eq!(find(&a, "reg:HKCU:run:64:On").state, StartupState::Enabled);
        assert_eq!(find(&a, "reg:HKCU:run:64:Unlisted").state, StartupState::Enabled);
    }

    #[test]
    fn an_unreadable_approved_key_leaves_entries_enabled_and_is_not_a_missing_source() {
        let mut r = quiet();
        r.registry.insert(HKCU_RUN, key(vec![s("A", "C:\\a.exe")]));
        r.approved
            .insert((Hive::User, ApprovedKey::Run), Err("access denied (error 5)".into()));
        let a = assemble(&r, &env);
        assert_eq!(find(&a, "reg:HKCU:run:64:A").state, StartupState::Enabled);
        assert!(a.unavailable.is_empty());
    }

    #[test]
    fn the_32_bit_hklm_run_key_is_governed_by_run32_not_run() {
        let mut r = quiet();
        r.registry
            .insert(HKLM_RUN32, key(vec![s("Legacy", "C:\\Program Files (x86)\\L\\l.exe")]));
        // Turned off in `Run` only: the 32-bit entry is unaffected.
        r.approved.insert((Hive::Machine, ApprovedKey::Run), approved(&[("Legacy", false)]));
        assert_eq!(find(&assemble(&r, &env), "reg:HKLM:run:32:Legacy").state, StartupState::Enabled);
        // Turned off in `Run32`: disabled.
        r.approved
            .insert((Hive::Machine, ApprovedKey::Run32), approved(&[("Legacy", false)]));
        let a = assemble(&r, &env);
        let e = find(&a, "reg:HKLM:run:32:Legacy");
        assert_eq!(
            (e.state, e.location.clone()),
            (StartupState::Disabled, StartupLocation::MachineRunKey)
        );
    }

    #[test]
    fn the_same_name_in_the_two_hklm_views_is_two_entries_with_their_own_state() {
        let mut r = quiet();
        r.registry.insert(HKLM_RUN, key(vec![s("Helper", "C:\\Program Files\\H\\h64.exe")]));
        r.registry
            .insert(HKLM_RUN32, key(vec![s("Helper", "C:\\Program Files (x86)\\H\\h32.exe")]));
        r.approved.insert((Hive::Machine, ApprovedKey::Run), approved(&[("Helper", false)]));
        let a = assemble(&r, &env);
        assert_eq!(find(&a, "reg:HKLM:run:64:Helper").state, StartupState::Disabled);
        assert_eq!(find(&a, "reg:HKLM:run:32:Helper").state, StartupState::Enabled);
    }

    #[test]
    fn hkcu_values_seen_through_both_views_are_one_entry() {
        let mut r = quiet();
        let same = vec![s("Dyno", "\"C:\\Dyno\\dyno-agent.exe\"")];
        r.registry.insert(HKCU_RUN, key(same.clone()));
        r.registry.insert(HKCU_RUN32, key(same));
        let a = assemble(&r, &env);
        assert_eq!(a.entries.len(), 1, "{:?}", a.entries);
        assert_eq!(a.entries[0].id, "reg:HKCU:run:64:Dyno");
        assert_eq!((stat(&a, "hkcu-run-64").count, stat(&a, "hkcu-run-32").count), (1, 0));
    }

    #[test]
    fn hkcu_values_with_the_same_name_but_another_command_are_not_merged() {
        let mut r = quiet();
        r.registry.insert(HKCU_RUN, key(vec![s("X", "C:\\one.exe")]));
        r.registry.insert(HKCU_RUN32, key(vec![s("x", "C:\\two.exe")]));
        let a = assemble(&r, &env);
        assert_eq!(a.entries.len(), 2);
    }

    #[test]
    fn run_once_values_are_always_enabled_and_have_their_own_locations() {
        let mut r = quiet();
        r.registry.insert(HKCU_ONCE, key(vec![s("Setup", "C:\\setup.exe /finish")]));
        r.registry.insert(HKLM_ONCE, key(vec![s("Admin", "C:\\admin.exe")]));
        // StartupApproved does not govern RunOnce, whatever it holds.
        r.approved.insert((Hive::User, ApprovedKey::Run), approved(&[("Setup", false)]));
        let a = assemble(&r, &env);
        let u = find(&a, "reg:HKCU:runonce:64:Setup");
        assert_eq!(
            (u.state, u.location.clone()),
            (StartupState::Enabled, StartupLocation::UserRunOnceKey)
        );
        let m = find(&a, "reg:HKLM:runonce:64:Admin");
        assert_eq!(
            (m.state, m.location.clone()),
            (StartupState::Enabled, StartupLocation::MachineRunOnceKey)
        );
    }

    #[test]
    fn expandable_strings_are_expanded_through_the_environment_and_unknown_names_stay() {
        let mut r = quiet();
        r.registry.insert(
            HKLM_RUN,
            key(vec![
                RegValue {
                    name: "Tool".into(),
                    data: RegData::ExpandStr("\"%ProgramFiles%\\Tool\\tool.exe\" --dir %UNKNOWN% 100%".into()),
                },
                // A plain string is never expanded.
                s("Literal", "%ProgramFiles%\\x.exe"),
            ]),
        );
        let a = assemble(&r, &env);
        assert_eq!(
            find(&a, "reg:HKLM:run:64:Tool").command,
            "\"C:\\Program Files\\Tool\\tool.exe\" --dir %UNKNOWN% 100%"
        );
        assert_eq!(
            find(&a, "reg:HKLM:run:64:Tool").exe.as_deref(),
            Some("C:\\Program Files\\Tool\\tool.exe")
        );
        assert_eq!(find(&a, "reg:HKLM:run:64:Literal").command, "%ProgramFiles%\\x.exe");
    }

    #[test]
    fn a_percent_pair_that_is_not_a_variable_name_never_reaches_the_lookup() {
        // `std::env::var` panics on a name with `=` or NUL; the lookup here panics the same way.
        let strict = |n: &str| {
            assert!(
                !n.is_empty() && !n.contains(['=', '\0']) && n.chars().count() <= 255,
                "looked up {n:?}"
            );
            None
        };
        let long = format!("%{}%", "n".repeat(256));
        for text in ["%a=b%", "%a\0b%", "%%", long.as_str(), "100%=%x"] {
            assert_eq!(expand_env(text, &strict).as_deref(), Some(text), "{text:?}");
        }
    }

    #[test]
    fn expansion_is_bounded() {
        let big = "x".repeat(MAX_EXPANDED_CHARS);
        let env_big = |n: &str| (n == "BIG").then(|| big.clone());
        assert_eq!(expand_env("%BIG%", &env_big).map(|s| s.len()), Some(MAX_EXPANDED_CHARS));
        assert_eq!(expand_env("%BIG%y", &env_big), None);
        assert_eq!(expand_env("%BIG%%BIG%", &env_big), None);
        assert_eq!(expand_env("a%%b", &env).as_deref(), Some("a%%b"));
        assert_eq!(expand_env("50%", &env).as_deref(), Some("50%"));
        // A value whose expansion is too long is skipped and counted, and the others still list.
        let mut r = quiet();
        r.registry.insert(
            HKCU_RUN,
            key(vec![
                RegValue {
                    name: "Huge".into(),
                    data: RegData::ExpandStr("%BIG%%BIG%".into()),
                },
                s("Fine", "C:\\f.exe"),
            ]),
        );
        let a = assemble(&r, &|n| env_big(n));
        assert_eq!(a.entries.len(), 1);
        assert_eq!(stat(&a, "hkcu-run-64").skipped, 1);
    }

    #[test]
    fn non_text_over_long_empty_and_past_cap_values_are_skipped_and_counted() {
        let mut r = quiet();
        r.registry.insert(
            HKCU_RUN,
            Ok(RunKeyRead {
                values: vec![
                    s("Good", "C:\\good.exe"),
                    RegValue {
                        name: "Number".into(),
                        data: RegData::Other,
                    },
                    RegValue {
                        name: "Huge".into(),
                        data: RegData::TooLong,
                    },
                    s("Blank", "   "),
                ],
                past_cap: 7,
            }),
        );
        let a = assemble(&r, &env);
        assert_eq!(a.entries.len(), 1);
        let st = stat(&a, "hkcu-run-64");
        assert_eq!((st.count, st.skipped, &st.outcome), (1, 3 + 7, &SourceOutcome::Ok));
    }

    #[test]
    fn a_reader_that_ignores_the_cap_is_still_held_to_it() {
        let mut r = quiet();
        let values: Vec<RegValue> = (0..MAX_RUN_VALUES + 5).map(|i| s(&format!("v{i}"), "C:\\a.exe")).collect();
        r.registry.insert(HKLM_RUN, key(values));
        let files: Vec<FolderFile> = (0..MAX_FOLDER_FILES + 3)
            .map(|i| FolderFile {
                name: format!("f{i}.txt"),
                path: format!("C:\\S\\f{i}.txt"),
                lnk: None,
            })
            .collect();
        r.folders.insert(FolderKind::Common, Ok(FolderRead { files, past_cap: 0 }));
        let a = assemble(&r, &env);
        assert_eq!(
            (stat(&a, "hklm-run-64").count, stat(&a, "hklm-run-64").skipped),
            (MAX_RUN_VALUES as u32, 5)
        );
        assert_eq!(
            (stat(&a, "startup-folder-common").count, stat(&a, "startup-folder-common").skipped),
            (MAX_FOLDER_FILES as u32, 3)
        );
    }

    fn file(name: &str, lnk_bytes: Option<Vec<u8>>) -> FolderFile {
        FolderFile {
            name: name.into(),
            path: format!("C:\\Users\\Pat\\Startup\\{name}"),
            lnk: lnk_bytes,
        }
    }

    #[test]
    fn a_shortcut_in_the_user_startup_folder_is_resolved_and_turned_off_by_startupfolder() {
        let mut r = quiet();
        r.folders.insert(
            FolderKind::User,
            Ok(FolderRead {
                files: vec![
                    file("Chat.lnk", Some(lnk("C:\\Program Files\\Chat\\chat.exe", "--tray", true))),
                    file("Notes.lnk", Some(lnk("C:\\Notes\\notes.exe", "", false))),
                    file("desktop.ini", None),
                    file("DESKTOP.INI", None),
                ],
                past_cap: 0,
            }),
        );
        r.approved
            .insert((Hive::User, ApprovedKey::StartupFolder), approved(&[("Chat.lnk", false)]));
        let a = assemble(&r, &env);
        assert_eq!(a.entries.len(), 2);
        let chat = find(&a, "folder:user:Chat.lnk");
        assert_eq!(chat.name, "Chat");
        assert_eq!(chat.command, "\"C:\\Program Files\\Chat\\chat.exe\" --tray");
        assert_eq!(chat.exe.as_deref(), Some("C:\\Program Files\\Chat\\chat.exe"));
        assert_eq!(
            (chat.state, chat.location.clone()),
            (StartupState::Disabled, StartupLocation::UserStartupFolder)
        );
        assert_eq!(find(&a, "folder:user:Notes.lnk").state, StartupState::Enabled);
        assert_eq!(stat(&a, "startup-folder-user").skipped, 2, "desktop.ini twice");
    }

    #[test]
    fn the_common_folder_is_governed_by_the_machine_startupfolder_key() {
        let mut r = quiet();
        r.folders.insert(
            FolderKind::Common,
            Ok(FolderRead {
                files: vec![file("Vpn.lnk", Some(lnk("C:\\Vpn\\v.exe", "", true)))],
                past_cap: 0,
            }),
        );
        // The user's key has the name, the machine's does not: not governed by the user's.
        r.approved
            .insert((Hive::User, ApprovedKey::StartupFolder), approved(&[("Vpn.lnk", false)]));
        assert_eq!(find(&assemble(&r, &env), "folder:common:Vpn.lnk").state, StartupState::Enabled);
        r.approved
            .insert((Hive::Machine, ApprovedKey::StartupFolder), approved(&[("Vpn.lnk", false)]));
        let a = assemble(&r, &env);
        let e = find(&a, "folder:common:Vpn.lnk");
        assert_eq!(
            (e.state, e.location.clone()),
            (StartupState::Disabled, StartupLocation::CommonStartupFolder)
        );
    }

    #[test]
    fn a_shortcut_with_no_path_and_other_files_are_listed_as_their_own_path() {
        let mut r = quiet();
        r.folders.insert(
            FolderKind::User,
            Ok(FolderRead {
                files: vec![
                    file("Store app.lnk", Some(b"not a shortcut".to_vec())),
                    file("Unreadable.lnk", None),
                    file("run me.bat", None),
                ],
                past_cap: 0,
            }),
        );
        let a = assemble(&r, &env);
        for (id, name) in [
            ("Store app.lnk", "Store app"),
            ("Unreadable.lnk", "Unreadable"),
            ("run me.bat", "run me.bat"),
        ] {
            let e = find(&a, &format!("folder:user:{id}"));
            let path = format!("C:\\Users\\Pat\\Startup\\{id}");
            assert_eq!(
                (e.name.as_str(), e.command.as_str(), e.exe.as_deref()),
                (name, path.as_str(), Some(path.as_str())),
                "{id}"
            );
        }
    }

    #[test]
    fn one_failing_source_gives_one_sentence_and_the_others_still_list() {
        let mut r = quiet();
        r.registry.insert(HKLM_RUN32, Err("access denied (error 5)".into()));
        r.registry.insert(HKCU_RUN, key(vec![s("A", "C:\\a.exe")]));
        let a = assemble(&r, &env);
        assert_eq!(
            a.unavailable,
            ["The all-users Run key (32-bit) could not be read: access denied (error 5)."]
        );
        assert_eq!(a.entries.len(), 1);
        assert!(!a.all_failed);
        assert_eq!(
            stat(&a, "hklm-run-32").outcome,
            SourceOutcome::Failed("access denied (error 5)".into())
        );
        // A folder failing reads the same way.
        r.folders.insert(FolderKind::Common, Err("path not found (error 3)".into()));
        let a = assemble(&r, &env);
        assert_eq!(a.unavailable.len(), 2);
        assert_eq!(
            a.unavailable[1],
            "The all-users Startup folder could not be read: path not found (error 3)."
        );
    }

    #[test]
    fn every_source_failing_is_an_error_not_an_empty_list() {
        let mut r = Reads::default();
        for src in RegSource::ALL {
            r.registry.insert(src, Err("access denied (error 5)".into()));
        }
        for f in FolderKind::ALL {
            r.folders.insert(f, Err("access denied (error 5)".into()));
        }
        let a = assemble(&r, &env);
        assert!(a.all_failed);
        assert_eq!(a.unavailable.len(), 10);
        assert!(a.entries.is_empty());
        // Nothing read at all (a reader that answered for no source) is not "everything failed" but also not a success.
        assert!(!assemble(&quiet(), &env).all_failed);
    }

    #[test]
    fn entries_come_source_by_source_in_list_order_and_by_name_within_one() {
        let mut r = quiet();
        r.registry.insert(HKLM_RUN, key(vec![s("m", "C:\\m.exe")]));
        r.registry.insert(HKCU_RUN, key(vec![s("b", "C:\\b.exe"), s("A", "C:\\a.exe")]));
        r.folders.insert(
            FolderKind::User,
            Ok(FolderRead {
                files: vec![file("z.lnk", None)],
                past_cap: 0,
            }),
        );
        let ids: Vec<_> = assemble(&r, &env).entries.into_iter().map(|e| e.id).collect();
        assert_eq!(
            ids,
            ["reg:HKCU:run:64:A", "reg:HKCU:run:64:b", "reg:HKLM:run:64:m", "folder:user:z.lnk"]
        );
    }

    #[test]
    fn the_coverage_sentence_names_what_is_not_read() {
        for gap in ["sign-in tasks", "Store apps", "services", "policy Run keys", "Winlogon"] {
            assert!(COVERAGE.contains(gap), "{gap}");
        }
        assert!(exe_of_command("\"C:\\a b\\c.exe\" -x").is_some());
    }
}
