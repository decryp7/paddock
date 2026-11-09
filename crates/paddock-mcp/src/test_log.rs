//! The one logger this crate's lib tests install (tests only). A process allows one global logger and the lib tests share a
//! process under `cargo test` (nextest runs each in its own), so two modules that each installed "their" logger made the capture
//! of whichever lost the race see nothing: three loopback log tests failed on Linux, 3 of 3 full runs.

use std::sync::Mutex;

/// Every log line of the process since [`install`], for a test that checks what a connection's lines say (ER-27). Each test looks
/// for the lines of its own connection (its port or id), so tests running side by side do not disturb each other.
pub(crate) static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        LOGGED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(record.args().to_string());
    }
    fn flush(&self) {}
}

/// Installs the capturing logger once (every level, so the lines' arguments are built) and checks that it is the one listening.
pub(crate) fn install() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = log::set_logger(&Capture);
        log::set_max_level(log::LevelFilter::Trace);
    });
    log::info!("paddock_mcp test_log: installed check");
    assert!(
        LOGGED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .any(|l| l == "paddock_mcp test_log: installed check"),
        "another logger is installed: only crate::test_log may install one in this crate's tests"
    );
}

#[cfg(test)]
mod tests {
    /// Failing first (ER-11): `loopback` and `client::connection` each called `log::set_logger` in their tests.
    #[test]
    fn only_this_module_installs_a_logger_in_the_crates_sources() {
        fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs")
                    && p.file_name().is_some_and(|n| n != "test_log.rs")
                {
                    let text = std::fs::read_to_string(&p).unwrap();
                    if text.contains("set_logger(") || text.contains("set_boxed_logger(") {
                        out.push(p.display().to_string());
                    }
                }
            }
        }
        let mut found = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut found,
        );
        assert!(
            found.is_empty(),
            "these files install a logger of their own: {found:?}"
        );
    }
}
