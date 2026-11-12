//! The Windows `StartupItems` (ADR 0033, WP-4): which sources are read, how each is timed and logged, and what a failure says. The OS
//! calls sit behind [`RegistryView`] and [`FolderList`] (`windows/startup.rs` is the real pair), so every OS tests this part with
//! recorded answers and `parse::win_startup::assemble`.
//!
//! Each source is read on its own: one that fails becomes one sentence in `unavailable()` and the others still list. When every source
//! fails, `list()` is an error, as the macOS reader does. Nothing is cached between calls: the sources are a few registry keys and two
//! folders (logon tasks, the slow source, are deferred), so `refresh()` has nothing to forget.
//!
//! Logging (ER-27): one `Startup source read` line per source with `Source`, `Count`, `Skipped`, `DurationMs` and `Outcome`, and a
//! summary line. Entry names, commands and paths can hold user paths, so they are never logged; a failure logs the OS error text the
//! reader gave, which names a code and never a path.

use crate::parse::win_startup::{
    assemble, ApprovedKey, ApprovedValues, FolderKind, FolderRead, Hive, Reads, RegSource, RunKeyRead, SourceOutcome, COVERAGE,
};
use dyno_core::ports::{PlatformError, StartupItems};
use dyno_core::startup::StartupEntry;
use std::time::Instant;

/// The registry reads startup needs. An `Err` is the reason the key could not be read (`access denied (error 5)`), without a path and
/// without a final full stop; a key that does not exist is `Ok` and empty.
pub trait RegistryView: Send {
    /// The values of one Run or RunOnce key through one view, at most `MAX_RUN_VALUES` of them, each at most `MAX_VALUE_BYTES`.
    fn run_key(&mut self, source: RegSource) -> Result<RunKeyRead, String>;
    /// The values of one `StartupApproved` key: name (lower case) to bytes, at most `MAX_RUN_VALUES` of them.
    fn approved(&mut self, hive: Hive, key: ApprovedKey) -> Result<ApprovedValues, String>;
}

/// The Startup folder reads startup needs, with the same error contract as [`RegistryView`].
pub trait FolderList: Send {
    /// The files of one Startup folder, at most `MAX_FOLDER_FILES`, with each `.lnk` read up to `MAX_LNK_BYTES`.
    fn folder(&mut self, kind: FolderKind) -> Result<FolderRead, String>;
}

/// `%NAME%` lookups for expandable values.
pub type Env = Box<dyn Fn(&str) -> Option<String> + Send>;

/// The `StartupApproved` keys that say whether an entry is turned off, and the hive each lives in.
const APPROVED: [(Hive, ApprovedKey); 5] = [
    (Hive::User, ApprovedKey::Run),
    (Hive::Machine, ApprovedKey::Run),
    (Hive::Machine, ApprovedKey::Run32),
    (Hive::User, ApprovedKey::StartupFolder),
    (Hive::Machine, ApprovedKey::StartupFolder),
];

/// Windows startup entries read through `R` and `F`.
pub struct WinStartup<R: RegistryView, F: FolderList> {
    registry: R,
    folders: F,
    env: Env,
    unavailable: Vec<String>,
}

impl<R: RegistryView, F: FolderList> WinStartup<R, F> {
    /// A reader over `registry` and `folders`; `env` expands `%NAME%` in expandable values.
    pub fn new(registry: R, folders: F, env: Env) -> Self {
        WinStartup {
            registry,
            folders,
            env,
            unavailable: Vec::new(),
        }
    }
}

fn ms(since: Instant) -> u128 {
    since.elapsed().as_millis()
}

impl<R: RegistryView, F: FolderList> StartupItems for WinStartup<R, F> {
    fn list(&mut self) -> Result<Vec<StartupEntry>, PlatformError> {
        let whole = Instant::now();
        let mut reads = Reads::default();
        let mut durations: Vec<(&'static str, u128)> = Vec::new();
        for src in RegSource::ALL {
            let started = Instant::now();
            reads.registry.insert(src, self.registry.run_key(src));
            durations.push((src.log_name(), ms(started)));
        }
        for kind in FolderKind::ALL {
            let started = Instant::now();
            reads.folders.insert(kind, self.folders.folder(kind));
            durations.push((kind.log_name(), ms(started)));
        }
        let started = Instant::now();
        for (hive, key) in APPROVED {
            let read = self.registry.approved(hive, key);
            if let Err(why) = &read {
                // The entries stay enabled, which is what Windows does; the list is still whole, so this is not an `unavailable` sentence.
                log::warn!(target: "dyno::startup", "Startup approvals unreadable: Key={key:?} Hive={hive:?} Reason={why}");
            }
            reads.approved.insert((hive, key), read);
        }
        log::info!(target: "dyno::startup", "Startup approvals read: DurationMs={}", ms(started));

        let assembled = assemble(&reads, &*self.env);
        for stat in &assembled.sources {
            let took = durations.iter().find(|(s, _)| *s == stat.source).map_or(0, |(_, d)| *d);
            match &stat.outcome {
                SourceOutcome::Ok => log::info!(
                    target: "dyno::startup",
                    "Startup source read: Source={} Count={} Skipped={} DurationMs={took} Outcome=ok",
                    stat.source, stat.count, stat.skipped
                ),
                SourceOutcome::Failed(why) => log::warn!(
                    target: "dyno::startup",
                    "Startup source read: Source={} Count=0 Skipped=0 DurationMs={took} Outcome=error Reason={why}",
                    stat.source
                ),
            }
        }
        log::info!(
            target: "dyno::startup",
            "Startup list read: Total={} Unavailable={} AllFailed={} DurationMs={}",
            assembled.entries.len(), assembled.unavailable.len(), assembled.all_failed, ms(whole)
        );
        self.unavailable = assembled.unavailable.clone();
        if assembled.all_failed {
            let first = assembled.unavailable.first().cloned().unwrap_or_default();
            return Err(PlatformError::Failed(format!("No Windows startup source could be read. {first}")));
        }
        Ok(assembled.entries)
    }

    fn unavailable(&self) -> Vec<String> {
        self.unavailable.clone()
    }

    /// Publishers are not read on Windows in 1.1.1 (ADR 0033 D8), so "Hide signed apps" cannot work.
    fn signer_read(&self) -> bool {
        false
    }

    fn coverage(&self) -> Option<String> {
        Some(COVERAGE.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::win_startup::{RegData, RegValue, RunKind, View};
    use dyno_core::startup::StartupState;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    type Shared<K, V> = Arc<Mutex<HashMap<K, Result<V, String>>>>;

    #[derive(Clone, Default)]
    struct Registry {
        keys: Shared<RegSource, RunKeyRead>,
        approved: Shared<(Hive, ApprovedKey), ApprovedValues>,
    }

    impl RegistryView for Registry {
        fn run_key(&mut self, source: RegSource) -> Result<RunKeyRead, String> {
            self.keys
                .lock()
                .unwrap()
                .get(&source)
                .cloned()
                .unwrap_or_else(|| Ok(RunKeyRead::default()))
        }

        fn approved(&mut self, hive: Hive, key: ApprovedKey) -> Result<ApprovedValues, String> {
            self.approved
                .lock()
                .unwrap()
                .get(&(hive, key))
                .cloned()
                .unwrap_or_else(|| Ok(ApprovedValues::new()))
        }
    }

    #[derive(Clone, Default)]
    struct Folders(Shared<FolderKind, FolderRead>);

    impl FolderList for Folders {
        fn folder(&mut self, kind: FolderKind) -> Result<FolderRead, String> {
            self.0
                .lock()
                .unwrap()
                .get(&kind)
                .cloned()
                .unwrap_or_else(|| Ok(FolderRead::default()))
        }
    }

    fn reader() -> (WinStartup<Registry, Folders>, Registry, Folders) {
        let (r, f) = (Registry::default(), Folders::default());
        (WinStartup::new(r.clone(), f.clone(), Box::new(|_| None)), r, f)
    }

    fn value(name: &str, cmd: &str) -> Result<RunKeyRead, String> {
        Ok(RunKeyRead {
            values: vec![RegValue {
                name: name.into(),
                data: RegData::Str(cmd.into()),
            }],
            past_cap: 0,
        })
    }

    const HKCU_RUN: RegSource = RegSource::new(Hive::User, RunKind::Run, View::Bits64);

    #[test]
    fn it_lists_what_the_sources_give_and_reads_again_on_every_call() {
        let (mut w, reg, _) = reader();
        reg.keys.lock().unwrap().insert(HKCU_RUN, value("A", "C:\\a.exe"));
        assert_eq!(w.list().unwrap().len(), 1);
        reg.keys.lock().unwrap().insert(HKCU_RUN, value("B", "C:\\b.exe"));
        w.refresh();
        let again = w.list().unwrap();
        assert_eq!(
            again.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["B"],
            "a changed key shows at once, with no stale cache"
        );
        assert!(w.unavailable().is_empty());
    }

    #[test]
    fn a_source_that_fails_is_a_sentence_and_the_rest_still_list() {
        let (mut w, reg, folders) = reader();
        reg.keys.lock().unwrap().insert(HKCU_RUN, value("A", "C:\\a.exe"));
        reg.keys.lock().unwrap().insert(
            RegSource::new(Hive::Machine, RunKind::Run, View::Bits32),
            Err("access denied (error 5)".into()),
        );
        folders
            .0
            .lock()
            .unwrap()
            .insert(FolderKind::Common, Err("path not found (error 3)".into()));
        assert_eq!(w.list().unwrap().len(), 1);
        assert_eq!(
            w.unavailable(),
            [
                "The all-users Run key (32-bit) could not be read: access denied (error 5).",
                "The all-users Startup folder could not be read: path not found (error 3)."
            ]
        );
        // The next good read clears the sentences.
        reg.keys.lock().unwrap().clear();
        folders.0.lock().unwrap().clear();
        w.list().unwrap();
        assert!(w.unavailable().is_empty());
    }

    #[test]
    fn every_source_failing_is_an_error_that_says_why() {
        let (mut w, reg, folders) = reader();
        for src in RegSource::ALL {
            reg.keys.lock().unwrap().insert(src, Err("access denied (error 5)".into()));
        }
        for kind in FolderKind::ALL {
            folders.0.lock().unwrap().insert(kind, Err("access denied (error 5)".into()));
        }
        let err = w.list().unwrap_err();
        assert!(err.to_string().contains("access denied (error 5)"), "{err}");
        assert_eq!(w.unavailable().len(), 10);
    }

    #[test]
    fn startup_approved_is_read_for_the_entries_it_governs() {
        let (mut w, reg, _) = reader();
        reg.keys.lock().unwrap().insert(HKCU_RUN, value("Off", "C:\\a.exe"));
        let mut off = ApprovedValues::new();
        off.insert("off".into(), vec![0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        reg.approved.lock().unwrap().insert((Hive::User, ApprovedKey::Run), Ok(off));
        assert_eq!(w.list().unwrap()[0].state, StartupState::Disabled);
        // An approvals key that cannot be read leaves it enabled and does not make the list partial.
        reg.approved
            .lock()
            .unwrap()
            .insert((Hive::User, ApprovedKey::Run), Err("access denied (error 5)".into()));
        assert_eq!(w.list().unwrap()[0].state, StartupState::Enabled);
        assert!(w.unavailable().is_empty());
    }

    #[test]
    fn it_says_it_reads_no_signer_and_what_it_covers() {
        let (w, _, _) = reader();
        assert!(!w.signer_read());
        let coverage = w.coverage().unwrap();
        assert!(coverage.contains("Run and RunOnce") && coverage.contains("Not listed"));
    }
}
