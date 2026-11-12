//! The integration tests of this crate, one binary (linking one test binary per file cost more than running them).
//! Each module is a former `tests/<name>.rs`; run one with `cargo test --test it <name>::`.

mod exit_measure;
mod parity;
mod rpath;
mod worker;

/// One line per phase on stderr, with the time since the test began (ER-27): a test that nextest stops at its limit then
/// shows where it was. CI 7967 stopped five of these tests at 90 s with no output at all, on a loaded Mac runner.
pub(crate) struct Phases {
    test: &'static str,
    began: std::time::Instant,
}

impl Phases {
    pub(crate) fn new(test: &'static str) -> Phases {
        let p = Phases {
            test,
            began: std::time::Instant::now(),
        };
        p.at("begins");
        p
    }

    pub(crate) fn at(&self, what: &str) {
        eprintln!("[{} +{:.1} s] {what}", self.test, self.began.elapsed().as_secs_f64());
    }
}

/// The fresh worker and plugins binaries' first runs, paid once per test process before any start with a bound of its own
/// (the plugins' manifest read, their connect window in the worker): the OS holds the first run of a freshly linked binary
/// on macOS (ADR 0071). The two are checked side by side. Elsewhere nothing is held, and nothing is run.
pub(crate) fn warm_binaries() {
    #[cfg(target_os = "macos")]
    {
        let worker = std::path::PathBuf::from(env!("CARGO_BIN_EXE_debrief-store-worker"));
        let plugins = debrief_plugin_host::builtins::default_binary();
        let mut programs: Vec<(&std::path::Path, &[&str])> = vec![(worker.as_path(), &["--self-test"])];
        if plugins.is_file() {
            programs.push((plugins.as_path(), &[debrief_plugin_host::builtins::MANIFESTS_FLAG]));
        }
        paddock_testkit::start::warm_first_starts(&programs);
    }
}

/// The built-in plugins as the worker loads them, each waited for until it connects or exits (ADR 0071 row 8), not for the
/// product's connect window.
pub(crate) fn builtins() -> debrief_plugin_host::Builtins {
    debrief_plugin_host::Builtins::load_with(&debrief_plugin_host::builtins::default_binary(), |spec| {
        spec.connect_window = debrief_ipc::process::UNTIL_CONNECTED;
    })
}
