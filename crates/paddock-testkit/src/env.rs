//! [`EnvLock`]: tests that touch environment variables take turns and put everything back.

use crate::scratch::Scratch;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::sync::{Mutex, MutexGuard};

static LOCK: Mutex<()> = Mutex::new(());

/// Takes the process-wide environment lock (waits for the holder; a lock poisoned by a failed test is recovered).
/// Every test that reads or writes the environment of the process holds it for the whole test.
pub fn env_lock() -> EnvLock {
    EnvLock {
        _guard: LOCK.lock().unwrap_or_else(|e| e.into_inner()),
        saved: Vec::new(),
    }
}

/// Holds the environment lock; every variable changed through it is restored, in reverse order, when it drops.
///
/// Only tests that also hold the lock are protected; code running on other threads that reads the environment without it
/// can still observe the change, which is why production code takes its environment as a parameter (`paddock-env`'s `Vars`).
#[derive(Debug)]
pub struct EnvLock {
    _guard: MutexGuard<'static, ()>,
    saved: Vec<(OsString, Option<OsString>)>,
}

impl EnvLock {
    /// Sets a variable for the rest of the test.
    pub fn set(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) {
        self.remember(key.as_ref());
        // SAFETY: the environment lock is held, so no other test changes the environment at the same time.
        unsafe { std::env::set_var(key.as_ref(), value.as_ref()) };
    }

    /// Removes a variable for the rest of the test.
    pub fn remove(&mut self, key: impl AsRef<OsStr>) {
        self.remember(key.as_ref());
        // SAFETY: as in `set`.
        unsafe { std::env::remove_var(key.as_ref()) };
    }

    /// The current value of a variable.
    pub fn get(&self, key: impl AsRef<OsStr>) -> Option<OsString> {
        std::env::var_os(key)
    }

    /// An in-process hermetic environment: `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, the `XDG_*` folders and
    /// `TMPDIR`/`TEMP`/`TMP` point into `scratch` (the home folder and the `tmp` folder). Code that asks the OS where the
    /// user's folders are lands in the scratch folder, and what it leaves in `tmp` fails the test. Variables an app reads
    /// itself (`<APP>_HOME`) are removed with [`remove`](EnvLock::remove) when the test needs that.
    pub fn hermetic(&mut self, scratch: &Scratch) {
        let home = scratch.home();
        let tmp = scratch.tmp();
        let vars: [(&str, std::path::PathBuf); 12] = [
            ("HOME", home.clone()),
            ("USERPROFILE", home.clone()),
            ("APPDATA", home.join("AppData/Roaming")),
            ("LOCALAPPDATA", home.join("AppData/Local")),
            ("XDG_CONFIG_HOME", home.join(".config")),
            ("XDG_DATA_HOME", home.join(".local/share")),
            ("XDG_CACHE_HOME", home.join(".cache")),
            ("XDG_STATE_HOME", home.join(".local/state")),
            ("XDG_RUNTIME_DIR", home.join(".run")),
            ("TMPDIR", tmp.clone()),
            ("TEMP", tmp.clone()),
            ("TMP", tmp),
        ];
        for (k, v) in vars {
            self.set(k, v);
        }
    }

    fn remember(&mut self, key: &OsStr) {
        // Only the first change of a variable records the original; later ones keep it.
        if !self.saved.iter().any(|(k, _)| k == key) {
            self.saved.push((key.to_os_string(), std::env::var_os(key)));
        }
    }

    /// The variables this lock changed and their original values (for assertions about the restore).
    pub fn changed(&self) -> HashMap<OsString, Option<OsString>> {
        self.saved.iter().cloned().collect()
    }
}

impl Drop for EnvLock {
    fn drop(&mut self) {
        for (key, old) in self.saved.drain(..).rev() {
            // SAFETY: the lock guard is still held (fields drop after this body).
            unsafe {
                match old {
                    Some(v) => std::env::set_var(&key, v),
                    None => std::env::remove_var(&key),
                }
            }
        }
    }
}
