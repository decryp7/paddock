//! The helper's Windows extras reader (ADR 0027 WP-10): `powercfg /requests` and `wbadmin get versions` with fixed arguments
//! and a 15 second limit, parsed by `dyno_core::helper_extras`. A tool that fails says so with its own message.

use crate::runner::ToolRunner;
use dyno_core::helper_extras::{parse_requests, parse_versions, BackupVersion, ExtrasReader, WakeRequest};
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
    /// An extras reader running `powercfg` and `wbadmin` from `system_dir`.
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
        Ok(parse_requests(&self.runner.run(&self.tool("powercfg"), &["/requests".to_string()], TIMEOUT)?))
    }

    fn backup_versions(&self) -> Result<Vec<BackupVersion>, PlatformError> {
        let out = self.runner.run_tool(&crate::runner::tool(self.tool("wbadmin"), &["get", "versions"]), TIMEOUT)?;
        // "No backup was found" exits non-zero but is an answer: there are no versions.
        if !out.ok && parse_versions(&out.stdout).is_empty() && !out.stdout.to_lowercase().contains("no backup") {
            return Err(PlatformError::Failed(format!(
                "wbadmin failed: {}",
                if out.error.is_empty() {
                    "no message (it needs an administrator or Backup Operator)"
                } else {
                    out.error.as_str()
                }
            )));
        }
        Ok(parse_versions(&out.stdout))
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
        let _ = reader.backup_versions();
        assert_eq!(
            *seen.lock().unwrap(),
            ["D:\\RealWin\\System32\\powercfg.exe", "D:\\RealWin\\System32\\wbadmin.exe"]
        );
    }

    #[test]
    fn requests_and_versions_are_parsed_and_a_missing_backup_is_an_empty_answer() {
        let ok = ext(Box::new(Fake(true, "SYSTEM:\n[PROCESS] a.exe\n", "")));
        assert_eq!(ok.wake_requests().unwrap().len(), 1);
        let none = ext(Box::new(Fake(false, "ERROR - No backup was found.\n", "")));
        assert!(none.backup_versions().unwrap().is_empty());
        let denied = ext(Box::new(Fake(false, "", "Access denied.")));
        assert!(denied.backup_versions().unwrap_err().to_string().contains("Access denied"));
        assert!(ext(Box::new(Fake(false, "", "You do not have the permissions required")))
            .wake_requests()
            .is_err());
    }
}
