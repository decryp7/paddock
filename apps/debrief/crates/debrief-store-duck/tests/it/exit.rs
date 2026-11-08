//! A process that exits while one of its threads is inside a DuckDB statement must exit cleanly. DuckDB 2.0 keeps
//! process-wide state that C++ destroys during `exit()`; a statement still running then crashed with SIGSEGV
//! (CI run 6734: the `debrief-app` tests that leave a live-tail flush running). The store's exit handler stops new
//! statements and waits for the running ones (`shutdown.rs`).

use debrief_core::ports::Query;
use debrief_core::{PluginRegistry, Profile};
use debrief_store_duck::DuckStore;

const CHILD: &str = "DEBRIEF_EXIT_CHILD";

/// Runs in the child process only: a store, a thread that queries it for ever, and an immediate return.
#[test]
fn a_background_query_at_exit_child() {
    let Some(dir) = std::env::var_os(CHILD) else { return };
    let store = DuckStore::create(std::path::Path::new(&dir), Profile::default_profile(), PluginRegistry::new()).unwrap();
    let background = store.clone();
    std::thread::spawn(move || loop {
        let _ = background.summary();
    });
    // Let the thread get into its first statements before the harness returns and the process exits.
    while store.summary().is_err() {}
    std::mem::forget(store);
}

#[test]
fn a_process_exits_cleanly_with_a_statement_still_running() {
    let dir = crate::common::TempDir::new("exit-child");
    for round in 0..20 {
        let status = debrief_core::process::quiet_command(std::env::current_exe().unwrap())
            .args(["--exact", "exit::a_background_query_at_exit_child", "--test-threads=1"])
            .env(CHILD, dir.0.join(format!("s{round}")))
            .status()
            .unwrap();
        assert!(status.success(), "round {round}: {status:?}");
    }
}
