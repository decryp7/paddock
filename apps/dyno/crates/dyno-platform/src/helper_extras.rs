//! The helper's Windows extras reader (ADR 0027 WP-10): `powercfg /requests` with fixed arguments
//! and a 15 second limit, parsed by `dyno_core::helper_extras`. A tool that fails says so with its own message.

use crate::runner::ToolRunner;
use dyno_core::helper_extras::{parse_requests, ExtrasReader, WakeRequest};
use dyno_core::ports::PlatformError;
use std::path::PathBuf;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);

/// Reads the extras through `runner`; the tools are run from `system_dir`, the system folder the OS names
/// (`GetSystemDirectoryW`, never `%SystemRoot%` and never a literal `C:\Windows`).
pub struct WindowsExtras {
    runner: Box<dyn ToolRunner>,
    system_dir: PathBuf,
}

impl WindowsExtras {
    /// An extras reader running `powercfg` from `system_dir`.
    pub fn new(runner: Box<dyn ToolRunner>, system_dir: PathBuf) -> Self {
        Self { runner, system_dir }
    }

    /// `<system folder>\<name>.exe`, joined as text (a Windows path on any host).
    fn tool(&self, name: &str) -> String {
        format!("{}\\{name}.exe", self.system_dir.to_string_lossy().trim_end_matches(['\\', '/']))
    }
}

impl ExtrasReader for WindowsExtras {
    fn wake_requests(&self) -> Result<Vec<WakeRequest>, PlatformError> {
        Ok(parse_requests(&self.runner.run(
            &self.tool("powercfg"),
            &["/requests".to_string()],
            TIMEOUT,
        )?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{Tool, ToolOutput};

    struct Fake(bool, &'static str, &'static str);
    impl ToolRunner for Fake {
        fn run_tool(&self, _: &Tool, _: Duration) -> Result<ToolOutput, PlatformError> {
            Ok(ToolOutput {
                ok: self.0,
                stdout: self.1.into(),
                error: self.2.into(),
            })
        }
    }

    fn ext(runner: Box<dyn ToolRunner>) -> WindowsExtras {
        WindowsExtras::new(runner, PathBuf::from("D:\\RealWin\\System32"))
    }

    /// Remembers the programs it was asked to run.
    struct Recorder(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
    impl ToolRunner for Recorder {
        fn run_tool(&self, t: &Tool, _: Duration) -> Result<ToolOutput, PlatformError> {
            self.0.lock().unwrap().push(t.program.clone());
            Ok(ToolOutput {
                ok: true,
                stdout: String::new(),
                error: String::new(),
            })
        }
    }

    #[test]
    fn the_tools_run_from_the_system_folder_the_os_named_not_a_literal_c_windows() {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let reader = ext(Box::new(Recorder(seen.clone())));
        let _ = reader.wake_requests();
        assert_eq!(*seen.lock().unwrap(), ["D:\\RealWin\\System32\\powercfg.exe"]);
    }

    #[test]
    fn requests_are_parsed() {
        let ok = ext(Box::new(Fake(true, "SYSTEM:\n[PROCESS] a.exe\n", "")));
        assert_eq!(ok.wake_requests().unwrap().len(), 1);
        assert!(ext(Box::new(Fake(false, "", "You do not have the permissions required")))
            .wake_requests()
            .is_err());
    }
}
