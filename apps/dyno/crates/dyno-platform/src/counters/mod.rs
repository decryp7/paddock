//! The OS's performance counters behind [`dyno_core::ports::CounterSource`] (WP-12). Windows enumerates and reads PDH
//! (`PdhEnumObjectsW`, `PdhEnumObjectItemsW`); macOS reads the host's own tools (`vm_stat`, `netstat -inb`, `iostat`,
//! `sysctl`); Linux reads `/proc`. Everything that turns text into counters is pure and compiled on every OS, so it
//! is tested everywhere; only the code that calls the OS is per-OS. Observe-only.

mod entry;
pub mod mac;
pub mod proc;

pub use entry::{catalog_of, read_paths, Entry};

use dyno_core::counters::{CounterCatalog, CounterPath, RawCounter};
use dyno_core::ports::{CounterSource, PlatformError};

/// The counters of this machine.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsCounters;

impl CounterSource for OsCounters {
    #[cfg(target_os = "macos")]
    fn catalog(&self) -> Result<CounterCatalog, PlatformError> {
        mac::MacCounters::new(mac::run_tool).catalog()
    }

    #[cfg(target_os = "macos")]
    fn read(&self, paths: &[CounterPath]) -> Vec<RawCounter> {
        mac::MacCounters::new(mac::run_tool).read(paths)
    }

    #[cfg(target_os = "linux")]
    fn catalog(&self) -> Result<CounterCatalog, PlatformError> {
        proc::ProcCounters::host().catalog()
    }

    #[cfg(target_os = "linux")]
    fn read(&self, paths: &[CounterPath]) -> Vec<RawCounter> {
        proc::ProcCounters::host().read(paths)
    }

    #[cfg(windows)]
    fn catalog(&self) -> Result<CounterCatalog, PlatformError> {
        crate::windows::counters::catalog()
    }

    #[cfg(windows)]
    fn read(&self, paths: &[CounterPath]) -> Vec<RawCounter> {
        crate::windows::counters::read(paths)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    fn catalog(&self) -> Result<CounterCatalog, PlatformError> {
        Err(PlatformError::Unsupported(
            "Performance counters are not available on this system.".into(),
        ))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    fn read(&self, paths: &[CounterPath]) -> Vec<RawCounter> {
        read_paths(&[], paths)
    }
}

/// A [`CounterSource`] over fixed entries, for the agent's and the CLI's tests: `fail` makes the next reads fail with
/// the given reason, as a vanished counter or a refusing OS would.
pub struct FakeCounters {
    entries: std::sync::Mutex<Vec<Entry>>,
    catalog_error: std::sync::Mutex<Option<PlatformError>>,
}

impl FakeCounters {
    /// A source offering `entries`.
    pub fn new(entries: Vec<Entry>) -> FakeCounters {
        FakeCounters {
            entries: std::sync::Mutex::new(entries),
            catalog_error: std::sync::Mutex::new(None),
        }
    }

    /// Replaces what is on offer (a counter that disappears is simply no longer there).
    pub fn set(&self, entries: Vec<Entry>) {
        *self.entries.lock().unwrap_or_else(|e| e.into_inner()) = entries;
    }

    /// Makes `catalog` fail with `error` (or work again with `None`).
    pub fn fail_catalog(&self, error: Option<PlatformError>) {
        *self.catalog_error.lock().unwrap_or_else(|e| e.into_inner()) = error;
    }
}

impl CounterSource for FakeCounters {
    fn catalog(&self) -> Result<CounterCatalog, PlatformError> {
        if let Some(e) = self.catalog_error.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Err(e);
        }
        Ok(catalog_of(&self.entries.lock().unwrap_or_else(|e| e.into_inner())))
    }

    fn read(&self, paths: &[CounterPath]) -> Vec<RawCounter> {
        read_paths(&self.entries.lock().unwrap_or_else(|e| e.into_inner()), paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::counters::{CounterKind, CounterUnit};

    fn entries() -> Vec<Entry> {
        vec![Entry::new("Memory", None, "Free", CounterUnit::Bytes, CounterKind::Gauge, 5.0)]
    }

    #[test]
    fn the_fake_offers_its_entries_and_forgets_them_when_replaced() {
        let f = FakeCounters::new(entries());
        assert_eq!(f.catalog().unwrap().objects.len(), 1);
        let p: CounterPath = "\\Memory\\Free".parse().unwrap();
        assert!(f.read(std::slice::from_ref(&p))[0].value.is_ok());
        f.set(vec![]);
        assert!(f.read(&[p])[0].value.is_err());
        f.fail_catalog(Some(PlatformError::Timeout("slow".into())));
        assert_eq!(f.catalog(), Err(PlatformError::Timeout("slow".into())));
        f.fail_catalog(None);
        assert!(f.catalog().is_ok());
    }

    /// Runs against this machine's real counters (read-only): whatever OS this is, the catalog is not empty, and a
    /// counter listed by it can be read back by its own path.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_real_counters_list_and_read_back() {
        let catalog = OsCounters.catalog().unwrap();
        assert!(!catalog.objects.is_empty());
        let o = catalog.objects.iter().find(|o| o.name == "Memory").expect("a Memory object");
        let c = &o.counters[0];
        let path = CounterPath::new(&o.name, None, &c.name).unwrap();
        let read = OsCounters.read(std::slice::from_ref(&path));
        assert_eq!(read[0].path, path.to_string());
        assert!(read[0].value.is_ok(), "{read:?}");
    }
}
