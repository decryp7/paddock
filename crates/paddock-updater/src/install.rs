//! Where and how an update is installed (ADR 0053 section 4): the [`InstallScope`] an app declares, and for a per-machine
//! Windows app the [`ElevatedHandoff`] installer, which runs `$INSTDIR\paddock-setup.exe apply-update` elevated through the
//! UAC prompt and maps what that program answers to a sentence and a next step.
//!
//! The core is pure: the elevated start, the check of the downloaded file's path, the sentences of `paddock-setup`'s exit codes
//! and the installer lock are ports ([`Elevator`], [`SetupContract`], [`InstallerLock`]). The adapter's Windows implementation
//! of the first is `tauri::native::ShellElevator`; the app supplies the other two from `paddock-setup`, which depends on this
//! crate (for the signature check) and so cannot be used here.

use crate::error::UpdateError;
use crate::updater::{Installer, Package};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Where the app is installed, declared by the app in its composition (ADR 0037).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum InstallScope {
    /// For the account that installed it (every other platform, and Windows apps not yet per machine).
    #[default]
    PerUser,
    /// Once per PC, for every user, in a protected folder: updates need an administrator and never install on quit.
    PerMachine,
}

/// What the Updates UI needs to know about installing, with the texts to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct InstallInfo {
    pub scope: InstallScope,
    /// Whether "Install when the app quits" exists (never for [`InstallScope::PerMachine`]).
    pub install_on_quit_available: bool,
    /// Shown beside Install for an account that is not an administrator; the button stays (ADR 0053 decision O3).
    pub administrator_notice: Option<String>,
}

/// The text for an account that is not an administrator.
pub const ADMINISTRATOR_NOTICE: &str = "An administrator must install this update. Windows will ask for an administrator's name and password.";
/// Why "Install when the app quits" is not offered for a per-machine copy.
pub const PER_MACHINE_NO_QUIT_INSTALL: &str =
    "This copy is installed for all users, so updates are installed when you click Install.";
/// The sentence for a declined UAC prompt (error 1223, `ERROR_CANCELLED`).
pub const DECLINED: &str = "Windows permission was declined";
/// The sentence when the installer ended before it finished.
pub fn installer_stopped(install_dir: Option<&Path>) -> String {
    match install_dir {
        Some(dir) => format!(
            "The installer stopped before it finished. The reason is in {} (setup.log).",
            dir.join("updates").display()
        ),
        None => "The installer stopped before it finished. The reason is in setup.log in the install folder's updates folder.".into(),
    }
}

impl InstallInfo {
    /// The info for `scope`; `administrator` is whether the account may approve the prompt itself (`None`: not known).
    pub fn of(scope: InstallScope, administrator: Option<bool>) -> InstallInfo {
        let per_machine = scope == InstallScope::PerMachine;
        InstallInfo {
            scope,
            install_on_quit_available: !per_machine,
            administrator_notice: (per_machine && administrator == Some(false))
                .then(|| ADMINISTRATOR_NOTICE.to_string()),
        }
    }
}

/// What the elevated start came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElevatedOutcome {
    /// The prompt was declined (`ShellExecuteExW` failed with error 1223).
    Declined,
    /// The program ran and ended with this exit code.
    Exited(i32),
    /// The program was still running when the patience ran out.
    StillRunning,
    /// The program could not be started (the system's message).
    Failed(String),
}

/// Starts a program elevated (the UAC prompt) and waits for it, at most `patience`. The adapter's is `ShellExecuteExW`
/// with the verb `runas`.
pub trait Elevator: Send + Sync {
    fn run(&self, program: &Path, args: &[String], patience: Duration) -> ElevatedOutcome;
}

/// What the app knows from `paddock-setup` and the updater does not (rule 55: one implementation, in `paddock-setup`).
pub trait SetupContract: Send + Sync {
    /// Whether `apply-update` accepts the downloaded file at `path` (a plain local drive-letter path); the reason if not.
    fn check_download(&self, path: &str) -> Result<(), String>;
    /// The sentence for an exit code of `apply-update` (`None`: a code the program never returns).
    fn sentence(&self, code: i32) -> Option<String>;
}

/// The installer lock from the app's side (`paddock_setup::installer_probe` and `wait_for_installer`).
pub trait InstallerLock: Send + Sync {
    /// Whether an administrator's installer of this app is running now.
    fn installing(&self) -> bool;
    /// Waits for it to end, at most `bound`; `true`: it is gone, `false`: still running (unknown).
    fn wait(&self, bound: Duration) -> bool;
}

/// How long the app waits for `paddock-setup apply-update` to finish (it only copies, checks and starts the installer).
pub const DEFAULT_PATIENCE: Duration = Duration::from_secs(120);

/// Quotes one argument for a Windows command line (`CommandLineToArgvW` rules).
pub fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            c => {
                out.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                out.push(c);
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// The arguments of `paddock-setup apply-update` for `package` (the signature goes as base64 text, never as a path).
pub fn apply_update_args(package: &Package<'_>) -> Vec<String> {
    let mut args = vec![
        "apply-update".to_string(),
        "--file".into(),
        package.file.display().to_string(),
        "--signature".into(),
        package.signature.to_string(),
        "--version".into(),
        package.version.to_string(),
    ];
    if let Some(size) = package.size {
        args.push("--size".into());
        args.push(size.to_string());
    }
    args.push("--restart".into());
    args
}

/// The installer of a per-machine Windows app: the download stays where the user's account put it, and
/// `$INSTDIR\paddock-setup.exe apply-update` copies it to the protected folder, checks it again and runs it, elevated
/// through one UAC prompt (ADR 0053 section 4.1 step 2).
pub struct ElevatedHandoff {
    install_dir: PathBuf,
    elevator: Arc<dyn Elevator>,
    contract: Arc<dyn SetupContract>,
    patience: Duration,
}

impl ElevatedHandoff {
    /// `install_dir`: the folder of the running executable (a Program Files folder).
    pub fn new(
        install_dir: impl Into<PathBuf>,
        elevator: Arc<dyn Elevator>,
        contract: Arc<dyn SetupContract>,
    ) -> ElevatedHandoff {
        ElevatedHandoff {
            install_dir: install_dir.into(),
            elevator,
            contract,
            patience: DEFAULT_PATIENCE,
        }
    }

    /// Another patience (tests).
    pub fn with_patience(mut self, patience: Duration) -> ElevatedHandoff {
        self.patience = patience;
        self
    }

    /// The program that is run elevated.
    pub fn program(&self) -> PathBuf {
        self.install_dir.join("paddock-setup.exe")
    }
}

impl Installer for ElevatedHandoff {
    fn install(&self, package: &Package<'_>, _relaunch: bool) -> Result<(), UpdateError> {
        let began = std::time::Instant::now();
        if let Err(reason) = self
            .contract
            .check_download(&package.file.display().to_string())
        {
            log_line(format!("hand-off refused before the prompt: {reason}"));
            return Err(UpdateError::NotInstalled {
                reason: "The update must be on this PC's own disk.".into(),
            });
        }
        let args = apply_update_args(package);
        log_line(format!(
            "hand-off to paddock-setup apply-update, version {}, patience {} s",
            package.version,
            self.patience.as_secs()
        ));
        let outcome = self.elevator.run(&self.program(), &args, self.patience);
        let ms = began.elapsed().as_millis();
        match outcome {
            ElevatedOutcome::Declined => {
                log_line(format!(
                    "hand-off: the prompt was declined (error 1223) after {ms} ms"
                ));
                Err(UpdateError::Declined)
            }
            ElevatedOutcome::Exited(0) => {
                log_line(format!(
                    "hand-off: paddock-setup handed over (exit 0) after {ms} ms"
                ));
                Ok(())
            }
            ElevatedOutcome::Exited(code) => {
                let reason = self.contract.sentence(code).unwrap_or_else(|| {
                    format!("The setup program ended with code {code}. The reason is in setup.log in the install folder's updates folder.")
                });
                log_line(format!(
                    "hand-off: paddock-setup exit {code} after {ms} ms: {reason}"
                ));
                Err(UpdateError::NotInstalled { reason })
            }
            ElevatedOutcome::StillRunning => {
                log_line(format!(
                    "hand-off still running after {ms} ms; the app exits and the next start settles it"
                ));
                Ok(())
            }
            ElevatedOutcome::Failed(detail) => {
                log_line(format!(
                    "hand-off: paddock-setup could not be started after {ms} ms: {detail}"
                ));
                Err(UpdateError::NotInstalled {
                    reason: format!("Windows could not start the installer ({detail})."),
                })
            }
        }
    }
}

fn log_line(line: String) {
    // The core has no logger; the adapter's `log` feature is optional, so the line goes to stderr only in debug builds.
    #[cfg(feature = "tauri")]
    log::info!("{line}");
    #[cfg(not(feature = "tauri"))]
    let _ = line;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeElevator {
        outcome: ElevatedOutcome,
        seen: Mutex<Vec<(PathBuf, Vec<String>, Duration)>>,
    }

    impl FakeElevator {
        fn new(outcome: ElevatedOutcome) -> Arc<FakeElevator> {
            Arc::new(FakeElevator {
                outcome,
                seen: Mutex::new(Vec::new()),
            })
        }
    }

    impl Elevator for FakeElevator {
        fn run(&self, program: &Path, args: &[String], patience: Duration) -> ElevatedOutcome {
            self.seen
                .lock()
                .unwrap()
                .push((program.to_path_buf(), args.to_vec(), patience));
            self.outcome.clone()
        }
    }

    struct Contract;
    impl SetupContract for Contract {
        fn check_download(&self, path: &str) -> Result<(), String> {
            if path.starts_with("C:\\") {
                Ok(())
            } else {
                Err("not local".into())
            }
        }
        fn sentence(&self, code: i32) -> Option<String> {
            (code == 26).then(|| {
                "The downloaded update did not pass the signature check. Download it again."
                    .to_string()
            })
        }
    }

    fn package<'a>(file: &'a Path, manifest: &'a serde_json::Value) -> Package<'a> {
        Package {
            file,
            manifest,
            version: "1.2.0",
            signature: "c2lnbmF0dXJl",
            size: Some(1234),
        }
    }

    fn handoff(outcome: ElevatedOutcome) -> (ElevatedHandoff, Arc<FakeElevator>) {
        let e = FakeElevator::new(outcome);
        (
            ElevatedHandoff::new(r"C:\Program Files\Dyno", e.clone(), Arc::new(Contract)),
            e,
        )
    }

    fn run(outcome: ElevatedOutcome) -> (Result<(), UpdateError>, Arc<FakeElevator>) {
        let (h, e) = handoff(outcome);
        let m = serde_json::json!({});
        let file =
            PathBuf::from(r"C:\Users\Ann\AppData\Local\net.test.dyno\update-1.2.0-x-setup.exe");
        let r = h.install(&package(&file, &m), true);
        (r, e)
    }

    #[test]
    fn runs_paddock_setup_from_the_install_folder_with_the_signature_as_text() {
        let (r, e) = run(ElevatedOutcome::Exited(0));
        assert_eq!(r, Ok(()));
        let seen = e.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(
            seen[0].0,
            PathBuf::from(r"C:\Program Files\Dyno").join("paddock-setup.exe")
        );
        assert_eq!(
            seen[0].1,
            [
                "apply-update",
                "--file",
                r"C:\Users\Ann\AppData\Local\net.test.dyno\update-1.2.0-x-setup.exe",
                "--signature",
                "c2lnbmF0dXJl",
                "--version",
                "1.2.0",
                "--size",
                "1234",
                "--restart"
            ]
        );
        assert_eq!(seen[0].2, DEFAULT_PATIENCE);
    }

    #[test]
    fn a_declined_prompt_is_the_windows_permission_sentence() {
        let (r, _) = run(ElevatedOutcome::Declined);
        let e = r.unwrap_err();
        assert_eq!(e, UpdateError::Declined);
        assert_eq!(
            e.message(),
            "Not installed: Windows permission was declined."
        );
        assert!(e.next_step().contains("Install"));
    }

    #[test]
    fn every_exit_code_has_a_sentence_and_a_next_step() {
        let (r, _) = run(ElevatedOutcome::Exited(26));
        let e = r.unwrap_err();
        assert_eq!(
            e.message(),
            "Not installed: The downloaded update did not pass the signature check. Download it again."
        );
        assert!(!e.next_step().is_empty());
        // A code the program never returns still says where to look.
        let (r, _) = run(ElevatedOutcome::Exited(77));
        let m = r.unwrap_err().message();
        assert!(m.contains("code 77") && m.contains("setup.log"), "{m}");
    }

    #[test]
    fn a_wait_that_runs_out_is_a_hand_off_not_a_failure() {
        let (r, _) = run(ElevatedOutcome::StillRunning);
        assert_eq!(r, Ok(()));
    }

    #[test]
    fn a_program_that_cannot_start_is_not_installed() {
        let (r, _) = run(ElevatedOutcome::Failed("error 2".into()));
        assert!(r.unwrap_err().message().contains("error 2"));
    }

    #[test]
    fn a_download_that_apply_update_would_refuse_never_reaches_the_prompt() {
        let (h, e) = handoff(ElevatedOutcome::Exited(0));
        let m = serde_json::json!({});
        let file = PathBuf::from(r"\\server\share\update.exe");
        let err = h.install(&package(&file, &m), true).unwrap_err();
        assert_eq!(
            err.message(),
            "Not installed: The update must be on this PC's own disk."
        );
        assert!(
            e.seen.lock().unwrap().is_empty(),
            "no prompt for a file that will be refused"
        );
    }

    #[test]
    fn the_size_is_left_out_when_the_release_does_not_say() {
        let m = serde_json::json!({});
        let file = PathBuf::from(r"C:\a\b.exe");
        let mut p = package(&file, &m);
        p.size = None;
        let args = apply_update_args(&p);
        assert!(!args.contains(&"--size".to_string()));
        assert_eq!(args.last().unwrap(), "--restart");
    }

    #[test]
    fn arguments_are_quoted_for_the_windows_command_line() {
        assert_eq!(quote_arg("plain"), "plain");
        assert_eq!(
            quote_arg(r"C:\Program Files\Dyno\x.exe"),
            r#""C:\Program Files\Dyno\x.exe""#
        );
        assert_eq!(quote_arg(""), r#""""#);
        assert_eq!(quote_arg(r#"a"b"#), r#""a\"b""#);
        assert_eq!(quote_arg(r"C:\dir with space\"), r#""C:\dir with space\\""#);
    }

    #[test]
    fn install_info_hides_install_on_quit_and_names_the_administrator_only_for_standard_users() {
        let per_user = InstallInfo::of(InstallScope::PerUser, Some(false));
        assert!(per_user.install_on_quit_available && per_user.administrator_notice.is_none());
        let admin = InstallInfo::of(InstallScope::PerMachine, Some(true));
        assert!(!admin.install_on_quit_available && admin.administrator_notice.is_none());
        let standard = InstallInfo::of(InstallScope::PerMachine, Some(false));
        assert_eq!(
            standard.administrator_notice.as_deref(),
            Some(ADMINISTRATOR_NOTICE)
        );
        // Unknown is not a reason to scare anyone.
        assert!(
            InstallInfo::of(InstallScope::PerMachine, None)
                .administrator_notice
                .is_none()
        );
    }

    #[test]
    fn the_stopped_installer_sentence_names_the_log_folder() {
        let s = installer_stopped(Some(Path::new(r"C:\Program Files\Dyno")));
        assert!(s.contains("updates") && s.contains("setup.log"), "{s}");
        assert!(installer_stopped(None).contains("setup.log"));
    }
}
