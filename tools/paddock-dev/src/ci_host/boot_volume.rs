//! `dev ci boot-volume`: the one rule for "is this folder on an external volume", and the boot volume's temp folder for programs that
//! macOS treats as their own responsible process (the port of `conformance/ci/boot-volume.sh`; ER-21, ER-22; docs/plans/ci-in-rust.md,
//! WP-5). macOS TCC (Removable Volumes) asks the user, or denies silently, when such a program (a child started with the responsibility
//! disclaim, a `launchctl submit` job) touches a folder on an external disk; Full Disk Access of the parent does not help
//! (docs/lessons.md, 2026-11-04 and 2026-11-11).
//!
//! ```text
//! dev ci boot-volume external <folder>   exit 0 when the folder is on an external volume (`diskutil info -plist` of its mount says
//!                                        Internal=false), 1 otherwise or when that cannot be told
//! dev ci boot-volume temp [<folder>]     prints <folder> (default $TMPDIR, else /tmp) when it is not external, else the per-user temp
//!                                        folder of the boot volume (`getconf DARWIN_USER_TEMP_DIR`), with one line on standard error
//!                                        saying why; when getconf gives none it prints <folder> and says that macOS may prompt (ER-27)
//! ```
//!
//! The rule is `paddock_env::boot_volume` (one implementation for this command and `paddock_testkit::boot_volume`, ER-22);
//! `job-tmpdir` ([`crate::ci_isolation`]) uses it in process. The real machine runs `df`, `diskutil` and `getconf` through
//! [`ToolCall`](crate::ci_tool::ToolCall) with a limit (ER-32).

use crate::ci_tool::ToolCall;
use clap::{Args as ClapArgs, Subcommand};
use paddock_env::boot_volume::{Probe, boot_volume_temp, mount_from_df, volume_is_external};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

/// How long `df`, `diskutil` or `getconf` may run (about 0.2 s on the CI Mac) before it is ended and the folder is judged internal.
const LIMIT: Duration = Duration::from_secs(20);

/// `dev ci boot-volume`.
#[derive(ClapArgs, Debug)]
pub struct BootVolumeArgs {
    /// What to ask.
    #[command(subcommand)]
    pub command: BootVolumeCommand,
}

/// The short name the CLI uses.
pub type Args = BootVolumeArgs;

/// What to ask.
#[derive(Subcommand, Debug)]
pub enum BootVolumeCommand {
    /// Exit 0 when the folder is on an external volume, 1 otherwise or when that cannot be told.
    External {
        /// The folder (it may not exist yet: its nearest existing parent decides).
        folder: String,
    },
    /// Prints the folder when it is not external, else the boot volume's per-user temp folder.
    Temp {
        /// The folder (default `$TMPDIR`, else `/tmp`).
        folder: Option<String>,
    },
}

/// The machine this process runs on.
pub struct RealProbe<'a> {
    /// The job's environment, for the programs' `PATH`.
    pub env: &'a HashMap<String, String>,
}

impl RealProbe<'_> {
    fn run(&self, program: &str, args: &[&str]) -> Option<String> {
        let ran = ToolCall::new(program)
            .args(args.iter().map(|a| a.to_string()))
            .timeout(LIMIT)
            .run(self.env, &mut std::io::sink())
            .ok()?;
        ran.success.then_some(ran.stdout)
    }
}

impl Probe for RealProbe<'_> {
    fn is_dir(&self, p: &str) -> bool {
        Path::new(p).is_dir()
    }

    fn physical(&self, p: &str) -> Option<String> {
        paddock_env::winpath::real(Path::new(p)).ok().map(|c| c.display().to_string())
    }

    fn mount_of(&self, dir: &str) -> Option<String> {
        mount_from_df(&self.run("df", &["-P", dir])?)
    }

    fn plist_of(&self, mount: &str) -> Option<String> {
        self.run("diskutil", &["info", "-plist", mount])
    }

    fn user_temp(&self) -> Option<String> {
        self.run("getconf", &["DARWIN_USER_TEMP_DIR"])
            .map(|t| t.trim_end_matches('\n').to_string())
    }
}

/// Whether the volume of `folder` is external, asked of this machine (no notes; `job-tmpdir` says its own).
pub fn external_on_this_machine(env: &HashMap<String, String>, folder: &str) -> bool {
    volume_is_external(&RealProbe { env }, folder, &mut Vec::new())
}

/// The command over the real machine.
pub fn run(env: &HashMap<String, String>, args: &Args, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    run_with(&RealProbe { env }, env.get("TMPDIR").map(String::as_str), &args.command, out, err)
}

/// The command over a probe (tests).
pub fn run_with(probe: &dyn Probe, tmpdir: Option<&str>, command: &BootVolumeCommand, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match command {
        BootVolumeCommand::External { folder } => {
            let mut notes = Vec::new();
            let external = volume_is_external(probe, folder, &mut notes);
            for n in notes {
                let _ = writeln!(err, "{n}");
            }
            i32::from(!external)
        }
        BootVolumeCommand::Temp { folder } => {
            let (dir, notes) = boot_volume_temp(probe, folder.as_deref(), tmpdir);
            for n in notes {
                let _ = writeln!(err, "{n}");
            }
            let _ = writeln!(out, "{dir}");
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A scripted machine: the folders that exist (each on its own mount) and which mounts are external.
    #[derive(Default)]
    struct Fake {
        dirs: HashSet<String>,
        external: HashSet<String>,
    }

    impl Fake {
        fn folder(mut self, dir: &str, _mount: &str) -> Fake {
            self.dirs.insert(dir.to_string());
            self
        }
    }

    impl Probe for Fake {
        fn is_dir(&self, p: &str) -> bool {
            self.dirs.contains(p)
        }
        fn physical(&self, p: &str) -> Option<String> {
            self.dirs.contains(p).then(|| p.to_string())
        }
        fn mount_of(&self, dir: &str) -> Option<String> {
            Some(dir.to_string())
        }
        fn plist_of(&self, mount: &str) -> Option<String> {
            let v = if self.external.contains(mount) { "false" } else { "true" };
            Some(format!("<plist><dict><key>Internal</key>\n<{v}/>\n</dict></plist>"))
        }
        fn user_temp(&self) -> Option<String> {
            None
        }
    }

    #[test]
    fn the_command_prints_the_folder_and_exits_by_the_answer() {
        let mut f = Fake::default().folder("/Volumes/ci", "/Volumes/ci").folder("/a", "/");
        f.external.insert("/Volumes/ci".into());
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let ext = |folder: &str| BootVolumeCommand::External { folder: folder.into() };
        assert_eq!(run_with(&f, None, &ext("/Volumes/ci"), &mut out, &mut err), 0);
        assert_eq!(run_with(&f, None, &ext("/a"), &mut out, &mut err), 1);
        let temp = BootVolumeCommand::Temp { folder: Some("/a".into()) };
        assert_eq!(run_with(&f, None, &temp, &mut out, &mut err), 0);
        assert_eq!(String::from_utf8(out).unwrap(), "/a\n");
        assert!(err.is_empty());
    }
}
