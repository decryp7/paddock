//! The honest adapter for a port an OS has no reader for yet (ADR 0033). It names its gap, so a page shows "not read here"
//! with the reason instead of an empty list that looks like a real answer, and it is not a test double, so it may ship.

use dyno_core::facts::SystemFacts;
use dyno_core::metrics::Reading;
use dyno_core::ports::{BatteryCapacitySource, FactsSource, FileLocks, PlatformError, StartupItems, UserActivity};
use dyno_core::process::{Locker, Pid};
use dyno_core::startup::StartupEntry;
use std::path::Path;

/// A port Dyno does not read on this OS: `what` is the thing in the user's words ("Startup items"), `why` the reason that is
/// shown, with the next step where there is one.
pub struct NotReadHere {
    what: &'static str,
    why: String,
}

impl NotReadHere {
    /// A gap named `what`, explained by `why`.
    pub fn new(what: &'static str, why: impl Into<String>) -> NotReadHere {
        NotReadHere { what, why: why.into() }
    }

    /// The reason shown to the user.
    pub fn why(&self) -> &str {
        &self.why
    }

    fn unsupported(&self) -> PlatformError {
        log::debug!(target: "dyno::platform", "{} asked, not read on this OS: {}", self.what, self.why);
        PlatformError::Unsupported(self.why.clone())
    }
}

impl FileLocks for NotReadHere {
    fn lockers(&mut self, _path: &Path) -> Result<Vec<Locker>, PlatformError> {
        Err(self.unsupported())
    }
}

impl StartupItems for NotReadHere {
    fn list(&mut self) -> Result<Vec<StartupEntry>, PlatformError> {
        Err(self.unsupported())
    }
}

impl FactsSource for NotReadHere {
    fn facts(&mut self) -> SystemFacts {
        facts_not_read(sysinfo::System::long_os_version().unwrap_or_else(|| "Unknown OS".into()), &self.why)
    }
}

impl BatteryCapacitySource for NotReadHere {
    fn read(&mut self) -> Reading<dyno_core::hardware::BatteryCapacity> {
        Reading::unavailable(self.why.clone())
    }
}

impl UserActivity for NotReadHere {
    fn idle_secs(&mut self) -> Option<u64> {
        None
    }
    fn foreground_pid(&mut self) -> Option<Pid> {
        None
    }
}

/// The real OS name and, for every other fact, the reason it is not read (never "Not checked yet": that says it will be).
pub fn facts_not_read(os: String, why: &str) -> SystemFacts {
    SystemFacts {
        os,
        disks: Reading::unavailable(why),
        restart_pending: Reading::unavailable(why),
        pending_updates: Reading::unavailable(why),
        power_plan: Reading::unavailable(why),
        visual_effects_full: Reading::unavailable(why),
        background_apps: Reading::unavailable(why),
        dev_folders: Vec::new(),
        last_boot_secs: Reading::unavailable(why),
        not_read_here: dyno_core::facts::FactKind::ALL.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_and_startup_answer_unsupported_with_the_reason() {
        let mut gap = NotReadHere::new("Startup items", "Windows startup items are not read yet.");
        assert_eq!(
            StartupItems::list(&mut gap).unwrap_err(),
            PlatformError::Unsupported("Windows startup items are not read yet.".into())
        );
        assert!(matches!(gap.lockers(Path::new("x")), Err(PlatformError::Unsupported(m)) if m.contains("not read yet")));
    }

    #[test]
    fn facts_keep_the_os_name_and_give_the_reason_for_the_rest() {
        let f = facts_not_read("Some OS 1".into(), "Not read here.");
        assert_eq!(f.os, "Some OS 1");
        assert_eq!(f.disks, Reading::Unavailable("Not read here.".into()));
        assert_eq!(f.last_boot_secs, Reading::Unavailable("Not read here.".into()));
        assert!(f.dev_folders.is_empty());
    }

    #[test]
    fn the_battery_says_why_it_has_no_reading() {
        let mut gap = NotReadHere::new("Battery", "Battery capacity is not read on this OS yet.");
        assert_eq!(
            gap.read(),
            Reading::Unavailable("Battery capacity is not read on this OS yet.".into())
        );
    }

    #[test]
    fn the_adapter_is_named_by_its_type() {
        assert!(FileLocks::adapter(&NotReadHere::new("Locks", "x")).ends_with("unsupported::NotReadHere"));
    }
}
