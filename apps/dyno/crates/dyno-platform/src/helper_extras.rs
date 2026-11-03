//! The helper's Windows extras reader (ADR 0027 WP-10): `powercfg /requests` and `wbadmin get versions` with fixed arguments
//! and a 15 second limit, parsed by `dyno_core::helper_extras`. A tool that fails says so with its own message.

use crate::runner::ToolRunner;
use dyno_core::helper_extras::{parse_requests, parse_versions, BackupVersion, ExtrasReader, WakeRequest};
use dyno_core::ports::PlatformError;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);
const POWERCFG: &str = r"C:\Windows\System32\powercfg.exe";
const WBADMIN: &str = r"C:\Windows\System32\wbadmin.exe";

/// Reads the extras through `runner`.
pub struct WindowsExtras(pub Box<dyn ToolRunner>);

impl ExtrasReader for WindowsExtras {
    fn wake_requests(&self) -> Result<Vec<WakeRequest>, PlatformError> {
        Ok(parse_requests(&self.0.run(POWERCFG, &["/requests".to_string()], TIMEOUT)?))
    }

    fn backup_versions(&self) -> Result<Vec<BackupVersion>, PlatformError> {
        let out = self.0.run_tool(&crate::runner::tool(WBADMIN, &["get", "versions"]), TIMEOUT)?;
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

    #[test]
    fn requests_and_versions_are_parsed_and_a_missing_backup_is_an_empty_answer() {
        let ok = WindowsExtras(Box::new(Fake(true, "SYSTEM:\n[PROCESS] a.exe\n", "")));
        assert_eq!(ok.wake_requests().unwrap().len(), 1);
        let none = WindowsExtras(Box::new(Fake(false, "ERROR - No backup was found.\n", "")));
        assert!(none.backup_versions().unwrap().is_empty());
        let denied = WindowsExtras(Box::new(Fake(false, "", "Access denied.")));
        assert!(denied.backup_versions().unwrap_err().to_string().contains("Access denied"));
        assert!(WindowsExtras(Box::new(Fake(false, "", "You do not have the permissions required")))
            .wake_requests()
            .is_err());
    }
}
