//! The real Windows readers behind [`crate::win_startup_source::{RegistryView, FolderList}`] (ADR 0033, WP-4): the Run and RunOnce keys
//! through both registry views, the `StartupApproved` keys and the two Startup folders, all with the caller's own rights and read-only
//! (Dyno does not enable or disable an entry, ADR 0021). Which source maps to which entry, the bounds' use and every state live in
//! `parse::win_startup`; this file only calls the OS, bounds what it reads and says what failed in a sentence with an error code.
//!
//! Bounds (ER-20): at most `MAX_RUN_VALUES` values per key, no value over `MAX_VALUE_BYTES` read, at most `MAX_FOLDER_FILES` files per
//! folder and `MAX_LNK_BYTES` per shortcut. A key or folder that is not there is empty, not an error.

use crate::parse::lnk::MAX_LNK_BYTES;
use crate::parse::win_startup::{
    ApprovedKey, ApprovedValues, FolderFile, FolderKind, FolderRead, Hive, RegData, RegSource, RegValue, RunKeyRead, RunKind, View,
    MAX_FOLDER_FILES, MAX_RUN_VALUES, MAX_VALUE_BYTES,
};
use crate::win_startup_source::{Env, FolderList, RegistryView, WinStartup};
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

    /// Reads from a temporary tree under `HKCU\<subkey>` instead (the Windows-runner test, ADR 0033 section 5).
    #[cfg(test)]
    fn under_hkcu(subkey: &str) -> Self {
        OsRegistry {
            base: Base::Temp(subkey.to_string()),
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

/// The values of an open key, at most `MAX_RUN_VALUES` of them, with how many more the key holds.
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
    for index in 0..count.min(MAX_RUN_VALUES as u32) {
        let (mut kind, mut size) = (0u32, 0u32);
        let mut name_len = MAX_NAME_CHARS as u32;
        // SAFETY: `name` is valid for `name_len` characters; with no data pointer the call reports the data size in `size`.
        let rc = unsafe {
            RegEnumValueW(
                key,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                Some(&mut kind),
                None,
                Some(&mut size),
            )
        };
        if rc == ERROR_NO_MORE_ITEMS {
            break;
        }
        if rc != ERROR_SUCCESS && rc != ERROR_MORE_DATA {
            return Err(why(rc));
        }
        let value_name = String::from_utf16_lossy(&name[..name_len as usize]);
        let kind = REG_VALUE_TYPE(kind);
        let data = if size as usize > max_data {
            None
        } else {
            let mut buf = vec![0u8; size as usize];
            let mut got = size;
            let mut name_len = MAX_NAME_CHARS as u32;
            // SAFETY: `name` is valid for `name_len` characters and `buf` for `got` bytes, as announced.
            let rc = unsafe {
                RegEnumValueW(
                    key,
                    index,
                    Some(PWSTR(name.as_mut_ptr())),
                    &mut name_len,
                    None,
                    None,
                    Some(buf.as_mut_ptr()),
                    Some(&mut got),
                )
            };
            if rc != ERROR_SUCCESS {
                return Err(why(rc));
            }
            buf.truncate(got as usize);
            Some(buf)
        };
        out.push(RawValue {
            name: value_name,
            kind,
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
        let _closed = unsafe { RegCloseKey(self.0) };
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
        let Some(key) = open(root, &path, view)? else {
            return Ok(ApprovedValues::new());
        };
        let key = Open(key);
        // A StartupApproved value is 12 bytes; anything past a few hundred is not one and is left out.
        let (raw, _past_cap) = raw_values(key.0, 256)?;
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
            let Ok(entry) = entry else { continue };
            // A subfolder of the Startup folder is not started by Windows.
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            if read.files.len() >= MAX_FOLDER_FILES {
                read.past_cap += 1;
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let lnk = if name.to_ascii_lowercase().ends_with(".lnk") && entry.metadata().is_ok_and(|m| m.len() as usize <= MAX_LNK_BYTES) {
                std::fs::read(&path).ok()
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
    use windows::Win32::System::Registry::{
        RegCreateKeyExW, RegDeleteTreeW, RegEnumKeyExW, RegSetValueExW, KEY_ALL_ACCESS, KEY_ENUMERATE_SUB_KEYS, REG_BINARY, REG_DWORD,
        REG_OPTION_NON_VOLATILE,
    };

    const PREFIX: &str = "DynoTest-";

    /// Deletes `HKCU\Software\<name>` and everything under it.
    #[allow(unsafe_code)]
    fn delete_tree(name: &str) {
        let wide = HSTRING::from(format!(r"Software\{name}"));
        // SAFETY: a NUL-terminated key name; the call only deletes a tree the tests created under the prefix.
        let _gone = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(wide.as_ptr())) };
        // SAFETY: as above, for the key itself.
        let _gone = unsafe { windows::Win32::System::Registry::RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(wide.as_ptr())) };
    }

    /// Removes leftovers of an earlier run that was killed (every key under HKCU\Software that starts with the test prefix).
    #[allow(unsafe_code)]
    fn sweep() {
        let Ok(Some(software)) = open(HKEY_CURRENT_USER, "Software", REG_SAM_FLAGS(KEY_ENUMERATE_SUB_KEYS.0)) else {
            return;
        };
        let software = Open(software);
        let mut leftovers = Vec::new();
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
                leftovers.push(found);
            }
        }
        for found in leftovers {
            delete_tree(&found);
        }
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
        let id = format!("{PREFIX}{}", std::process::id());
        let _tree = Tree(id.clone());
        let dir = tempfile::tempdir().unwrap();

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
        let mut missing = OsRegistry::under_hkcu(&format!("{PREFIX}missing-{}", std::process::id()));
        let src = RegSource::new(Hive::User, RunKind::Run, View::Bits64);
        assert_eq!(missing.run_key(src), Ok(RunKeyRead::default()));
        // Read-only on the real key: only that it answers, never what it holds.
        assert!(OsRegistry::new().run_key(src).is_ok());
    }
}
