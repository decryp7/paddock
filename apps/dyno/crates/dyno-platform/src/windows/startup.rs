//! The real Windows readers behind [`crate::win_startup_source::{RegistryView, FolderList}`] (ADR 0033, WP-4): the Run and RunOnce keys
//! through both registry views, the `StartupApproved` keys and the two Startup folders, all with the caller's own rights and read-only
//! (Dyno does not enable or disable an entry, ADR 0021). Which source maps to which entry, the bounds' use and every state live in
//! `parse::win_startup`; this file only calls the OS, bounds what it reads and says what failed in a sentence with an error code.
//!
//! Bounds (ER-20): at most `MAX_RUN_VALUES` values per key, no value over `MAX_VALUE_BYTES` read, at most `MAX_FOLDER_FILES` files per
//! folder and `MAX_LNK_BYTES` per shortcut. A key or folder that is not there is empty, not an error.

#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]

use crate::parse::lnk::MAX_LNK_BYTES;
use crate::parse::win_startup::{
    ApprovedKey, ApprovedValues, FolderFile, FolderKind, FolderRead, Hive, RegData, RegSource, RegValue, RunKeyRead, RunKind, View,
    MAX_FOLDER_FILES, MAX_RUN_VALUES, MAX_VALUE_BYTES,
};
use crate::win_startup_source::{Env, FolderList, RegistryView, WinStartup};
use std::io::Read;
use std::path::PathBuf;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE,
    KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_EXPAND_SZ, REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::UI::Shell::{FOLDERID_CommonStartup, FOLDERID_Startup, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

/// More files than the cap that are still counted (so "skipped" says how many), never read.
const MAX_FOLDER_COUNTED: usize = 10_000;
/// The longest value name Windows allows, in characters, plus the terminator.
const MAX_NAME_CHARS: usize = 16_384;

/// Windows startup entries as the user sees them in Task Manager's sources of Run keys and Startup folders.
pub fn os_startup() -> WinStartup<OsRegistry, OsFolders> {
    let env: Env = Box::new(|name| std::env::var(name).ok());
    WinStartup::new(OsRegistry::new(), OsFolders::new(), env)
}

/// Where the registry reads go: the real keys, or a temporary tree a test owns.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code, reason = "only the Windows-runner tests build a temporary tree"))]
enum Base {
    Real,
    /// Every hive, kind and view maps to `HKCU\<prefix>\<User|Machine>\<Run|RunOnce>` and `…\Approved\<key>` (no WOW64 flags): the
    /// real `Run` keys are never written, and a standard user cannot write HKLM.
    Temp(String),
}

/// The registry reads. It holds no handle between calls.
#[derive(Debug, Clone)]
pub struct OsRegistry {
    base: Base,
}

impl OsRegistry {
    /// The real Run, RunOnce and StartupApproved keys.
    pub fn new() -> Self {
        OsRegistry { base: Base::Real }
    }

    /// Reads from a temporary tree `HKCU\Software\<name>` instead (the Windows-runner test, ADR 0033 section 5): the tree the
    /// test's `own_tree` makes and the sweep removes.
    #[cfg(test)]
    fn under_hkcu(name: &str) -> Self {
        OsRegistry {
            base: Base::Temp(format!(r"Software\{name}")),
        }
    }

    fn run_path(&self, source: RegSource) -> (HKEY, String, REG_SAM_FLAGS) {
        let kind = if source.kind == RunKind::Run { "Run" } else { "RunOnce" };
        match &self.base {
            Base::Real => {
                let hive = if source.hive == Hive::User {
                    HKEY_CURRENT_USER
                } else {
                    HKEY_LOCAL_MACHINE
                };
                let view = if source.view == View::Bits64 {
                    KEY_WOW64_64KEY
                } else {
                    KEY_WOW64_32KEY
                };
                (hive, format!(r"Software\Microsoft\Windows\CurrentVersion\{kind}"), view)
            }
            Base::Temp(root) => (
                HKEY_CURRENT_USER,
                format!(r"{root}\{}\{kind}", hive_dir(source.hive)),
                REG_SAM_FLAGS(0),
            ),
        }
    }

    fn approved_path(&self, hive: Hive, key: ApprovedKey) -> (HKEY, String, REG_SAM_FLAGS) {
        let name = match key {
            ApprovedKey::Run => "Run",
            ApprovedKey::Run32 => "Run32",
            ApprovedKey::StartupFolder => "StartupFolder",
        };
        match &self.base {
            // Explorer writes these from a 64-bit process, so the 64-bit view is the one that holds them.
            Base::Real => (
                if hive == Hive::User {
                    HKEY_CURRENT_USER
                } else {
                    HKEY_LOCAL_MACHINE
                },
                format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\{name}"),
                KEY_WOW64_64KEY,
            ),
            Base::Temp(root) => (
                HKEY_CURRENT_USER,
                format!(r"{root}\{}\Approved\{name}", hive_dir(hive)),
                REG_SAM_FLAGS(0),
            ),
        }
    }
}

impl Default for OsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn hive_dir(hive: Hive) -> &'static str {
    if hive == Hive::User {
        "User"
    } else {
        "Machine"
    }
}

/// The sentence-ready reason for a Win32 error: a name for the common ones and always the code (never a path).
fn why(rc: WIN32_ERROR) -> String {
    match rc {
        ERROR_ACCESS_DENIED => format!("access denied (error {})", rc.0),
        other => format!("error {}", other.0),
    }
}

/// One value as the registry gave it.
struct RawValue {
    name: String,
    kind: REG_VALUE_TYPE,
    /// The data, or `None` when it is longer than the bound and was not read.
    data: Option<Vec<u8>>,
}

/// The values of an open key, at most `MAX_RUN_VALUES` of them, with how many more the key holds. One `RegEnumValueW` call per value,
/// into a data buffer of `max_data` bytes: a value that does not fit comes back as `ERROR_MORE_DATA` and is not read (`data: None`); its name is then read again without a data buffer.
#[allow(unsafe_code)]
fn raw_values(key: HKEY, max_data: usize) -> Result<(Vec<RawValue>, u32), String> {
    let mut count = 0u32;
    // SAFETY: `key` is open and `count` is a valid out pointer; every other out pointer is absent.
    let rc = unsafe { RegQueryInfoKeyW(key, None, None, None, None, None, None, Some(&mut count), None, None, None, None) };
    if rc != ERROR_SUCCESS {
        return Err(why(rc));
    }
    let mut out = Vec::new();
    let mut name = vec![0u16; MAX_NAME_CHARS];
    let mut buf = vec![0u8; max_data];
    for index in 0..count.min(MAX_RUN_VALUES as u32) {
        let mut kind = 0u32;
        let mut name_len = MAX_NAME_CHARS as u32;
        let mut got = max_data as u32;
        // SAFETY: `name` is valid for `name_len` characters and `buf` for `got` bytes, as announced.
        let rc = unsafe {
            RegEnumValueW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                Some(&mut kind),
                Some(buf.as_mut_ptr()),
                Some(&mut got),
            )
        };
        if rc == ERROR_NO_MORE_ITEMS {
            break;
        }
        let data = match rc {
            ERROR_SUCCESS => Some(buf[..(got as usize).min(max_data)].to_vec()),
            ERROR_MORE_DATA => {
                // Microsoft documents that on ERROR_MORE_DATA the outputs are not guaranteed to be filled, so the name and type of this
                // value are asked again without a data buffer: with `lpData` NULL the call succeeds and reports name, type and size.
                name_len = MAX_NAME_CHARS as u32;
                kind = 0;
                let mut needed = 0u32;
                // SAFETY: `name` is valid for `name_len` characters; the data pointer is absent, so only the size is written.
                let again = unsafe {
                    RegEnumValueW(
                        key,
                        index,
                        Some(PWSTR(name.as_mut_ptr())),
                        &mut name_len,
                        None,
                        Some(&mut kind),
                        None,
                        Some(&mut needed),
                    )
                };
                if again != ERROR_SUCCESS {
                    log::warn!(target: "dyno::startup", "Registry value name re-read failed: Index={index} Reason={}", why(again));
                    return Err(why(again));
                }
                log::debug!(target: "dyno::startup", "Registry value past the bound: Index={index} Bytes={needed} Bound={max_data} Path=more-data-reread");
                None
            }
            other => return Err(why(other)),
        };
        out.push(RawValue {
            name: String::from_utf16_lossy(&name[..(name_len as usize).min(MAX_NAME_CHARS)]),
            kind: REG_VALUE_TYPE(kind),
            data,
        });
    }
    Ok((out, count.saturating_sub(MAX_RUN_VALUES as u32)))
}

/// Opens `path` for reading; `Ok(None)` when it does not exist.
#[allow(unsafe_code)]
fn open(hive: HKEY, path: &str, view: REG_SAM_FLAGS) -> Result<Option<HKEY>, String> {
    let mut key = HKEY::default();
    let wide = HSTRING::from(path);
    // SAFETY: `wide` is NUL-terminated and outlives the call; `key` is a valid out pointer, closed by the caller.
    let rc = unsafe { RegOpenKeyExW(hive, PCWSTR(wide.as_ptr()), None, KEY_QUERY_VALUE | view, &mut key) };
    match rc {
        ERROR_SUCCESS => Ok(Some(key)),
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => Ok(None),
        other => Err(why(other)),
    }
}

/// An open key that is closed when it goes out of scope, on every path.
struct Open(HKEY);

impl Drop for Open {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: the key was opened by `open` and is closed once.
        let closed = unsafe { RegCloseKey(self.0) };
        if closed != ERROR_SUCCESS {
            log::warn!(target: "dyno::startup", "Registry key close failed: Reason={}", why(closed));
        }
    }
}

/// A string value's text up to its first NUL.
fn text_of(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

impl RegistryView for OsRegistry {
    fn run_key(&mut self, source: RegSource) -> Result<RunKeyRead, String> {
        let (hive, path, view) = self.run_path(source);
        let Some(key) = open(hive, &path, view)? else {
            return Ok(RunKeyRead::default());
        };
        let key = Open(key);
        let (raw, past_cap) = raw_values(key.0, MAX_VALUE_BYTES)?;
        let values = raw
            .into_iter()
            .map(|v| {
                let data = match (v.kind, v.data) {
                    (REG_SZ, Some(b)) => RegData::Str(text_of(&b)),
                    (REG_EXPAND_SZ, Some(b)) => RegData::ExpandStr(text_of(&b)),
                    (REG_SZ | REG_EXPAND_SZ, None) => RegData::TooLong,
                    _ => RegData::Other,
                };
                RegValue { name: v.name, data }
            })
            .collect();
        Ok(RunKeyRead { values, past_cap })
    }

    fn approved(&mut self, hive: Hive, key: ApprovedKey) -> Result<ApprovedValues, String> {
        let (root, path, view) = self.approved_path(hive, key);
        let Some(handle) = open(root, &path, view)? else {
            return Ok(ApprovedValues::new());
        };
        let handle = Open(handle);
        // A StartupApproved value is 12 bytes; anything past a few hundred is not one and is left out.
        let (raw, past_cap) = raw_values(handle.0, 256)?;
        if past_cap > 0 {
            log::info!(target: "dyno::startup", "StartupApproved values past the cap, not read: Key={key:?} Hive={hive:?} Count={past_cap}");
        }
        Ok(raw.into_iter().filter_map(|v| v.data.map(|d| (v.name.to_lowercase(), d))).collect())
    }
}

/// The two Startup folders.
#[derive(Debug, Clone, Default)]
pub struct OsFolders {
    /// A folder a test owns, in place of the known folder.
    #[cfg(test)]
    override_dir: Option<PathBuf>,
}

impl OsFolders {
    /// The user's and the all-users Startup folders.
    pub fn new() -> Self {
        OsFolders::default()
    }

    /// Reads `dir` in place of both known folders (the Windows-runner test).
    #[cfg(test)]
    fn at(dir: PathBuf) -> Self {
        OsFolders { override_dir: Some(dir) }
    }

    #[allow(unsafe_code)]
    fn dir(&self, kind: FolderKind) -> Result<PathBuf, String> {
        #[cfg(test)]
        if let Some(dir) = &self.override_dir {
            return Ok(dir.join(kind.log_name()));
        }
        let id = if kind == FolderKind::User {
            &FOLDERID_Startup
        } else {
            &FOLDERID_CommonStartup
        };
        // SAFETY: `id` is a known-folder id; the call allocates a NUL-terminated string that is freed below.
        let path = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None) }.map_err(|e| format!("error {:#x}", e.code().0))?;
        // SAFETY: `path` came from the call above and is read once, before it is freed.
        let text = unsafe { path.to_string() };
        // SAFETY: allocated by `SHGetKnownFolderPath`, which documents CoTaskMemFree.
        unsafe { CoTaskMemFree(Some(path.0.cast())) };
        text.map(PathBuf::from).map_err(|_| "the folder path is not valid text".to_string())
    }
}

fn io_why(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(code) if code == ERROR_ACCESS_DENIED.0 as i32 => format!("access denied (error {code})"),
        Some(code) => format!("error {code}"),
        None => "could not be read".to_string(),
    }
}

/// A shortcut's bytes when it is at most `MAX_LNK_BYTES` long. It reads at most one byte more than that, so a file that grows after a
/// size check (or a device name) cannot make it read without bound.
fn read_lnk(path: &std::path::Path) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    // silent: an unreadable shortcut shows as the file itself (the caller's documented fallback), so the error has no other use.
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_LNK_BYTES as u64 + 1).read_to_end(&mut bytes))
        .ok()?;
    (bytes.len() <= MAX_LNK_BYTES).then_some(bytes)
}

impl FolderList for OsFolders {
    fn folder(&mut self, kind: FolderKind) -> Result<FolderRead, String> {
        let dir = self.dir(kind)?;
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(FolderRead::default()),
            Err(e) => return Err(io_why(&e)),
        };
        let mut read = FolderRead::default();
        for entry in entries.take(MAX_FOLDER_FILES + MAX_FOLDER_COUNTED) {
            let Ok(entry) = entry else {
                // An entry the directory iterator could not read is counted with the files left unread.
                read.past_cap += 1;
                continue;
            };
            // A subfolder of the Startup folder is not started by Windows; it is counted with the files left unread.
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                read.past_cap += 1;
                continue;
            }
            if read.files.len() >= MAX_FOLDER_FILES {
                read.past_cap += 1;
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let lnk = if name.to_ascii_lowercase().ends_with(".lnk") {
                read_lnk(&path)
            } else {
                None
            };
            read.files.push(FolderFile {
                name,
                path: path.to_string_lossy().into_owned(),
                lnk,
            });
        }
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::lnk::tests::build as lnk;
    use crate::parse::startup_approved::approved_value;
    use crate::parse::win_startup::MAX_VALUE_BYTES;
    use dyno_core::ports::StartupItems;
    use dyno_core::startup::{StartupLocation, StartupState};
    use paddock_proc::{ProcessId, StartTime};
    use std::time::Duration;
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Registry::{
        RegCreateKeyExW, RegDeleteTreeW, RegEnumKeyExW, RegSetValueExW, KEY_ALL_ACCESS, KEY_ENUMERATE_SUB_KEYS, REG_BINARY, REG_DWORD,
        REG_OPTION_NON_VOLATILE, REG_QWORD,
    };

    const PREFIX: &str = "DynoTest-";
    /// A tree with no owner record (written by a build before the record, or killed between creating it and writing it) is swept
    /// only when nothing wrote to it for this long.
    const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(60 * 60);
    const OWNER_PID: &str = "OwnerPid";
    const OWNER_START: &str = "OwnerStartMicros";

    /// Deletes `HKCU\Software\<name>` and everything under it.
    #[allow(unsafe_code)]
    fn delete_tree(name: &str) {
        let wide = HSTRING::from(format!(r"Software\{name}"));
        // SAFETY: a NUL-terminated key name; the call only deletes a tree this test file created under the prefix.
        let _gone = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(wide.as_ptr())) };
        // SAFETY: as above, for the key itself.
        let _gone = unsafe { windows::Win32::System::Registry::RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(wide.as_ptr())) };
    }

    /// Whether a leftover tree may be deleted: its owner process is gone (an unreadable process table keeps it, rule 53), or, with no
    /// owner record, nothing wrote to it for [`STALE_AFTER`].
    fn sweepable(owner: Option<ProcessId>, since_last_write: std::time::Duration) -> bool {
        match owner {
            Some(id) => id.presence() == paddock_proc::Presence::Gone,
            None => since_last_write > STALE_AFTER,
        }
    }

    /// The owner a tree's root key records, if it records one.
    #[allow(unsafe_code)]
    fn owner_and_age(name: &str) -> Option<(Option<ProcessId>, std::time::Duration)> {
        let root = open(HKEY_CURRENT_USER, &format!(r"Software\{name}"), REG_SAM_FLAGS(0)).ok()??;
        let root = Open(root);
        let mut written = FILETIME::default();
        // SAFETY: `root` is open and `written` is a valid out pointer; every other out pointer is absent.
        let rc = unsafe {
            RegQueryInfoKeyW(
                root.0,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(&mut written),
            )
        };
        if rc != ERROR_SUCCESS {
            return None;
        }
        // 100 ns ticks since 1601 to the Unix epoch.
        let ticks = (u64::from(written.dwHighDateTime) << 32) | u64::from(written.dwLowDateTime);
        let written_secs = (ticks / 10_000_000).saturating_sub(11_644_473_600);
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let age = std::time::Duration::from_secs(now_secs.saturating_sub(written_secs));
        let (values, _) = raw_values(root.0, 16).ok()?;
        let get = |n: &str| values.iter().find(|v| v.name == n).and_then(|v| v.data.clone());
        let pid = get(OWNER_PID).and_then(|b| Some(u32::from_le_bytes(b.get(..4)?.try_into().ok()?)));
        let start = get(OWNER_START).and_then(|b| Some(u64::from_le_bytes(b.get(..8)?.try_into().ok()?)));
        let owner = pid.zip(start).map(|(p, s)| ProcessId::new(p, StartTime::from_micros(s)));
        Some((owner, age))
    }

    /// Removes the leftovers of an earlier run that was killed: the `HKCU\Software\DynoTest-*` trees whose owner process is gone, or
    /// that are stale with no owner record. A tree of a test that runs right now in another process is never touched.
    #[allow(unsafe_code)]
    fn sweep() {
        let Ok(Some(software)) = open(HKEY_CURRENT_USER, "Software", REG_SAM_FLAGS(KEY_ENUMERATE_SUB_KEYS.0)) else {
            return;
        };
        let software = Open(software);
        let mut candidates = Vec::new();
        for index in 0.. {
            let mut name = vec![0u16; 256];
            let mut len = name.len() as u32;
            // SAFETY: `name` is valid for `len` characters.
            let rc = unsafe { RegEnumKeyExW(software.0, index, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None) };
            if rc != ERROR_SUCCESS {
                break;
            }
            let found = String::from_utf16_lossy(&name[..len as usize]);
            if found.starts_with(PREFIX) {
                candidates.push(found);
            }
        }
        for found in candidates {
            if let Some((owner, age)) = owner_and_age(&found) {
                if sweepable(owner, age) {
                    delete_tree(&found);
                }
            }
        }
    }

    /// A tree name no other test, process or run shares: this process's pid and start time, and a counter.
    fn unique_name(label: &str) -> String {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let me = ProcessId::of(std::process::id()).expect("this process has a start time");
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("{PREFIX}{}-{}-{n}-{label}", me.pid(), me.started().as_micros())
    }

    /// Creates the tree's root with the owner's pid and start time as values, so a later sweep knows whose it is.
    fn own_tree(name: &str) -> Tree {
        let tree = Tree(name.to_string());
        let root = create(&format!(r"Software\{name}"));
        let me = ProcessId::of(std::process::id()).expect("this process has a start time");
        set(&root, OWNER_PID, REG_DWORD, &me.pid().to_le_bytes());
        set(&root, OWNER_START, REG_QWORD, &me.started().as_micros().to_le_bytes());
        tree
    }

    /// The temporary tree, deleted when the guard goes (also on a panic).
    struct Tree(String);

    impl Drop for Tree {
        fn drop(&mut self) {
            delete_tree(&self.0);
        }
    }

    #[allow(unsafe_code)]
    fn create(path: &str) -> Open {
        let wide = HSTRING::from(path);
        let mut key = HKEY::default();
        // SAFETY: a NUL-terminated name under the test's own prefix; `key` is a valid out pointer, closed by `Open`.
        let rc = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(wide.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS,
                None,
                &mut key,
                None,
            )
        };
        assert_eq!(rc, ERROR_SUCCESS, "could not create the temporary key {path}");
        Open(key)
    }

    #[allow(unsafe_code)]
    fn set(key: &Open, name: &str, kind: REG_VALUE_TYPE, data: &[u8]) {
        let wide = HSTRING::from(name);
        // SAFETY: a NUL-terminated name and a byte slice of the length given.
        let rc = unsafe { RegSetValueExW(key.0, PCWSTR(wide.as_ptr()), None, kind, Some(data)) };
        assert_eq!(rc, ERROR_SUCCESS);
    }

    fn utf16z(s: &str) -> Vec<u8> {
        s.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()
    }

    /// Owned by the Windows runner (ADR 0033 section 5): the reader over a temporary key `HKCU\Software\DynoTest-<id>` and a temporary
    /// folder, through the same traits the real wiring uses. The real Run key is never written. It lists REG_SZ and REG_EXPAND_SZ values,
    /// skips a number and an over-long value, honours StartupApproved, resolves a shortcut and skips desktop.ini.
    #[test]
    fn the_readers_list_a_temporary_key_and_folder() {
        sweep();
        let id = unique_name("readers");
        let _tree = own_tree(&id);
        let dir = paddock_testkit::private_tempdir().unwrap();

        let run = create(&format!(r"Software\{id}\User\Run"));
        set(&run, "Plain", REG_SZ, &utf16z("\"C:\\Tools\\plain.exe\" --x"));
        set(&run, "Expanding", REG_EXPAND_SZ, &utf16z("%SystemRoot%\\system32\\expand.exe"));
        set(&run, "Off", REG_SZ, &utf16z("C:\\Tools\\off.exe"));
        set(&run, "Number", REG_DWORD, &7u32.to_le_bytes());
        set(&run, "Huge", REG_SZ, &utf16z(&"x".repeat(MAX_VALUE_BYTES)));
        let approved = create(&format!(r"Software\{id}\User\Approved\Run"));
        set(&approved, "Off", REG_BINARY, &approved_value(false, 0x01d9_0000_0000_0000));
        let once = create(&format!(r"Software\{id}\Machine\RunOnce"));
        set(&once, "Finish", REG_SZ, &utf16z("C:\\Tools\\finish.exe"));

        let user_folder = dir.path().join(FolderKind::User.log_name());
        std::fs::create_dir_all(&user_folder).unwrap();
        std::fs::write(
            user_folder.join("Chat.lnk"),
            lnk("C:\\Program Files\\Chat\\chat.exe", "--tray", true),
        )
        .unwrap();
        std::fs::write(user_folder.join("desktop.ini"), "[.ShellClassInfo]").unwrap();

        let env: Env = Box::new(|name| std::env::var(name).ok());
        let mut startup = WinStartup::new(OsRegistry::under_hkcu(&id), OsFolders::at(dir.path().to_path_buf()), env);
        let list = startup.list().expect("the temporary sources are readable");
        let by_id = |id: &str| {
            list.iter()
                .find(|e| e.id == id)
                .unwrap_or_else(|| panic!("no {id}: {:?}", list.iter().map(|e| &e.id).collect::<Vec<_>>()))
        };

        assert_eq!(by_id("reg:HKCU:run:64:Plain").command, "\"C:\\Tools\\plain.exe\" --x");
        let expanded = &by_id("reg:HKCU:run:64:Expanding").command;
        assert!(!expanded.contains('%') && expanded.ends_with("expand.exe"), "{expanded}");
        assert_eq!(by_id("reg:HKCU:run:64:Off").state, StartupState::Disabled);
        assert_eq!(by_id("reg:HKLM:runonce:64:Finish").location, StartupLocation::MachineRunOnceKey);
        assert_eq!(
            by_id("folder:user:Chat.lnk").command,
            "\"C:\\Program Files\\Chat\\chat.exe\" --tray"
        );
        assert!(
            list.iter()
                .all(|e| !e.id.contains("Number") && !e.id.contains("Huge") && !e.id.contains("desktop")),
            "{list:?}"
        );
        assert!(startup.unavailable().is_empty(), "{:?}", startup.unavailable());
    }

    /// Owned by the Windows runner: a key that does not exist is empty, not an error, and the real HKCU Run key opens for reading.
    #[test]
    fn a_missing_key_is_empty_and_the_real_user_run_key_is_readable() {
        sweep();
        let mut missing = OsRegistry::under_hkcu(&unique_name("missing"));
        let src = RegSource::new(Hive::User, RunKind::Run, View::Bits64);
        assert_eq!(missing.run_key(src), Ok(RunKeyRead::default()));
        // Read-only on the real key: only that it answers, never what it holds.
        assert!(OsRegistry::new().run_key(src).is_ok());
    }

    /// Owned by the Windows runner: the sweep keeps a live owner's tree, deletes a dead owner's, and judges a tree with no owner record
    /// by the age of its last write.
    #[test]
    fn the_sweep_keeps_a_live_owners_tree_and_deletes_a_dead_ones() {
        let me = ProcessId::of(std::process::id()).unwrap();
        let dead = ProcessId::new(me.pid(), StartTime::from_micros(me.started().as_micros() + 10_000_000_000));
        assert!(
            !sweepable(Some(me), Duration::from_secs(10 * 3600)),
            "a running owner's tree is never swept, however old"
        );
        assert!(
            sweepable(Some(dead), Duration::ZERO),
            "a pid now held by another process is a dead owner"
        );
        assert!(
            !sweepable(None, Duration::from_secs(60)),
            "a fresh tree with no owner record may be mid-creation"
        );
        assert!(sweepable(None, STALE_AFTER + Duration::from_secs(1)));

        let mine = unique_name("sweep-live");
        let _mine = own_tree(&mine);
        let theirs = unique_name("sweep-dead");
        let _theirs = Tree(theirs.clone());
        let root = create(&format!(r"Software\{theirs}"));
        set(&root, OWNER_PID, REG_DWORD, &dead.pid().to_le_bytes());
        set(&root, OWNER_START, REG_QWORD, &dead.started().as_micros().to_le_bytes());
        sweep();
        assert!(owner_and_age(&mine).is_some(), "the live test's tree survived the sweep");
        assert!(owner_and_age(&theirs).is_none(), "the dead owner's tree was swept");
    }

    /// Owned by the Windows runner: two trees made by one process get different names, so tests that run in parallel never share one.
    #[test]
    fn every_tree_name_is_its_own() {
        assert_ne!(unique_name("a"), unique_name("a"));
    }

    /// Owned by the Windows runner: a value past the data bound is `TooLong` (ERROR_MORE_DATA), not an error and not read; the next
    /// value still lists.
    #[test]
    fn a_value_past_the_bound_is_too_long_and_the_next_value_is_still_read() {
        sweep();
        let id = unique_name("toolong");
        let _tree = own_tree(&id);
        let run = create(&format!(r"Software\{id}\User\Run"));
        set(&run, "Huge", REG_SZ, &utf16z(&"x".repeat(MAX_VALUE_BYTES)));
        set(&run, "After", REG_SZ, &utf16z("C:\\a.exe"));
        let read = OsRegistry::under_hkcu(&id)
            .run_key(RegSource::new(Hive::User, RunKind::Run, View::Bits64))
            .expect("readable");
        let data_of = |n: &str| read.values.iter().find(|v| v.name == n).map(|v| v.data.clone());
        assert_eq!(data_of("Huge"), Some(RegData::TooLong));
        assert_eq!(data_of("After"), Some(RegData::Str("C:\\a.exe".into())));
    }
}
