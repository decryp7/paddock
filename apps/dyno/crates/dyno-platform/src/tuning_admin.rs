//! The changes that need administrator rights, as the privileged helper makes them (ADR 0006, 0016): hibernation,
//! the macOS sleep timer and Low Power Mode, Microsoft Defender exclusions, all-users startup entries and the battery
//! charge limit. [`AdminTuner`] is a [`TuningSystem`] that only the helper's process builds; the app and the agent
//! never run it.
//!
//! Like [`crate::tuning::OsTuner`] it turns a typed [`HelperAction`] into fixed programs with validated arguments or a
//! small file operation, never into text a shell would read. A value that must reach a program unparsed (a folder
//! path for Defender) travels in an environment variable.

use crate::tuning::{denied, parse_reg_dword, tool, unsupported, windows_tool, OsTuner, SystemRunner, Tool, ToolRunner, HIBERNATE_KEY};
use dyno_core::helper::{HelperAction, MachineStartup, SleepSource, TuningHost};
use dyno_core::ports::{PlatformError, TuningSystem};
use std::path::{Path, PathBuf};

/// Where the all-users launch agents live on macOS.
pub const SYSTEM_LAUNCH_AGENTS: &str = "/Library/LaunchAgents";
/// The suffix a disabled all-users launch agent gets: launchd only loads `*.plist`, and the name stays recognisable so
/// Dyno lists it as disabled and can put it back.
pub const DISABLED_SUFFIX: &str = ".dyno-disabled";
/// The environment variable that carries a folder path to PowerShell.
pub const PATH_VARIABLE: &str = "DYNO_PATH";

const APPROVED_RUN: &str = "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
const APPROVED_FOLDER: &str = "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\StartupFolder";
/// The value Task Manager writes for "enabled" and "disabled" (12 bytes; the first says which, the rest is a time).
const APPROVED_ENABLED: &str = "020000000000000000000000";
const APPROVED_DISABLED: &str = "030000000000000000000000";

fn powershell() -> String {
    windows_tool("WindowsPowerShell\\v1.0\\powershell.exe")
}

fn ps(script: &str, path: &str) -> Tool {
    let mut t = tool(powershell(), &["-NoProfile", "-NonInteractive", "-Command", script]);
    t.env.push((PATH_VARIABLE.into(), path.into()));
    t
}

fn approved_key(location: MachineStartup) -> Option<&'static str> {
    match location {
        MachineStartup::RunKey => Some(APPROVED_RUN),
        MachineStartup::StartupFolder => Some(APPROVED_FOLDER),
        MachineStartup::ScheduledTask | MachineStartup::LaunchAgent => None,
    }
}

/// The tool calls that make `action` when it is an administrator change done with programs; `None` for the ones done
/// with files (the launch agents, the charge limit) or that are not administrator changes at all.
pub fn admin_change_tools(host: TuningHost, action: &HelperAction) -> Option<Result<Vec<Tool>, PlatformError>> {
    use HelperAction as A;
    Some(match (host, action) {
        (TuningHost::Windows, A::SetHibernate(on)) => Ok(vec![tool(
            windows_tool("powercfg.exe"),
            &["/hibernate", if *on { "on" } else { "off" }],
        )]),
        (TuningHost::Windows, A::SetDefenderExcluded { paths, excluded }) => {
            let script = if *excluded {
                "Add-MpPreference -ExclusionPath $env:DYNO_PATH"
            } else {
                "Remove-MpPreference -ExclusionPath $env:DYNO_PATH"
            };
            Ok(paths.iter().map(|p| ps(script, p)).collect())
        }
        (TuningHost::Windows, A::SetMachineStartupEnabled { location, name, enabled }) => match approved_key(*location) {
            Some(key) => Ok(vec![tool(
                windows_tool("reg.exe"),
                &[
                    "add",
                    key,
                    "/v",
                    name,
                    "/t",
                    "REG_BINARY",
                    "/d",
                    if *enabled { APPROVED_ENABLED } else { APPROVED_DISABLED },
                    "/f",
                ],
            )]),
            None if *location == MachineStartup::ScheduledTask => Ok(vec![tool(
                windows_tool("schtasks.exe"),
                &["/Change", "/TN", name, if *enabled { "/ENABLE" } else { "/DISABLE" }],
            )]),
            None => Err(unsupported("An all-users launch agent", host)),
        },
        (TuningHost::Macos, A::SetLowPowerMode(on)) => {
            Ok(vec![tool("/usr/bin/pmset", &["-a", "lowpowermode", if *on { "1" } else { "0" }])])
        }
        (TuningHost::Macos, A::SetSleepMinutes { source, minutes }) => Ok(vec![tool(
            "/usr/bin/pmset",
            &[if *source == SleepSource::Ac { "-c" } else { "-b" }, "sleep", &minutes.to_string()],
        )]),
        _ => return None,
    })
}

/// The tool calls that read the setting of an administrator change only an administrator can read (or that the helper
/// reads for simplicity); `None` for the rest.
pub fn admin_read_tools(host: TuningHost, action: &HelperAction) -> Option<Result<Vec<Tool>, PlatformError>> {
    use HelperAction as A;
    Some(match (host, action) {
        (TuningHost::Windows, A::SetHibernate(_)) => Ok(vec![tool(
            windows_tool("reg.exe"),
            &["query", HIBERNATE_KEY, "/v", "HibernateEnabled"],
        )]),
        (TuningHost::Windows, A::SetDefenderExcluded { paths, .. }) => Ok(paths
            .iter()
            .map(|p| {
                ps(
                    "if ((Get-MpPreference).ExclusionPath -contains $env:DYNO_PATH) { 'excluded' } else { 'included' }",
                    p,
                )
            })
            .collect()),
        (TuningHost::Windows, A::SetMachineStartupEnabled { location, name, .. }) => match approved_key(*location) {
            Some(key) => Ok(vec![tool(windows_tool("reg.exe"), &["query", key, "/v", name])]),
            None if *location == MachineStartup::ScheduledTask => Ok(vec![tool(
                windows_tool("schtasks.exe"),
                &["/Query", "/TN", name, "/FO", "CSV", "/NH"],
            )]),
            None => Err(unsupported("An all-users launch agent", host)),
        },
        _ => return None,
    })
}

/// Whether the `reg query` answer for a `StartupApproved` value says the entry runs. A missing value means enabled
/// (Windows writes it only when the user disables or re-enables the entry).
pub fn parse_startup_approved(text: &str, query_succeeded: bool) -> Option<bool> {
    if !query_succeeded {
        return Some(true);
    }
    let line = text.lines().find(|l| l.contains("REG_BINARY"))?;
    let hex = line.split_whitespace().last()?;
    let first = u8::from_str_radix(hex.get(..2)?, 16).ok()?;
    // Task Manager: 02 and 06 enabled, 03 disabled (odd means disabled).
    Some(first & 1 == 0)
}

/// Whether a `schtasks /Query /FO CSV /NH` row says the task is enabled (`"\Name","next run","Ready"`).
pub fn parse_task_enabled(text: &str) -> Option<bool> {
    let row = text.lines().find(|l| !l.trim().is_empty())?;
    let status = row.trim().trim_end_matches('"').rsplit("\",\"").next()?;
    Some(!status.eq_ignore_ascii_case("disabled"))
}

/// The only strip of the verbatim prefix `canonicalize` adds on Windows (`\\?\C:\…`), which Defender does not take.
fn plain(p: &Path) -> String {
    let s = p.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

/// A folder as the helper will name it to a tool: it exists, is a directory (not a link), and is spelled canonically.
fn canonical_dir(path: &str) -> Result<String, PlatformError> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| PlatformError::NotFound(format!("{path} is not a folder on this computer.")))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(PlatformError::Failed(format!(
            "{path} is not a plain folder (links are not followed)."
        )));
    }
    std::fs::canonicalize(path)
        .map(|c| plain(&c))
        .map_err(|e| PlatformError::Failed(format!("Could not resolve {path}: {e}")))
}

/// The administrator changes of the helper, over a tool runner and the all-users launch-agent folder.
pub struct AdminTuner<R: ToolRunner> {
    inner: OsTuner<R>,
    agents_dir: PathBuf,
}

/// The tuner the helper uses on this system.
pub fn system_admin_tuner() -> AdminTuner<SystemRunner> {
    AdminTuner::new(crate::tuning::os_tuner(), PathBuf::from(SYSTEM_LAUNCH_AGENTS))
}

impl<R: ToolRunner> AdminTuner<R> {
    /// An admin tuner over `inner` (which also carries the host and the unprivileged operations) and `agents_dir`.
    pub fn new(inner: OsTuner<R>, agents_dir: PathBuf) -> Self {
        AdminTuner { inner, agents_dir }
    }

    fn agent_files(&self, label: &str) -> (PathBuf, PathBuf) {
        let on = self.agents_dir.join(format!("{label}.plist"));
        let off = self.agents_dir.join(format!("{label}.plist{DISABLED_SUFFIX}"));
        (on, off)
    }

    fn launch_agent_state(&self, label: &str) -> Result<bool, PlatformError> {
        let (on, off) = self.agent_files(label);
        let regular = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_file());
        match (regular(&on), regular(&off)) {
            (true, false) => Ok(true),
            (false, true) => Ok(false),
            (true, true) => Err(PlatformError::Failed(format!(
                "{label} exists both enabled and disabled; Dyno leaves it alone."
            ))),
            (false, false) => Err(PlatformError::NotFound(format!("There is no all-users launch agent {label}."))),
        }
    }

    fn set_launch_agent(&self, label: &str, enabled: bool) -> Result<(), PlatformError> {
        if self.launch_agent_state(label)? == enabled {
            return Ok(());
        }
        let (on, off) = self.agent_files(label);
        let (from, to) = if enabled { (off, on) } else { (on, off) };
        std::fs::rename(&from, &to).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => denied("Changing an all-users launch agent"),
            _ => PlatformError::Failed(format!("Could not change the launch agent {label}: {e}")),
        })
    }

    fn defender_paths(action: &HelperAction, must_exist: bool) -> Result<HelperAction, PlatformError> {
        match action {
            HelperAction::SetDefenderExcluded { paths, excluded } => {
                let mut out = Vec::new();
                for p in paths {
                    // Removing an exclusion for a folder that is gone must still work.
                    out.push(match canonical_dir(p) {
                        Ok(c) => c,
                        Err(PlatformError::NotFound(_)) if !must_exist => p.clone(),
                        Err(e) => return Err(e),
                    });
                }
                Ok(HelperAction::SetDefenderExcluded {
                    paths: out,
                    excluded: *excluded,
                })
            }
            other => Ok(other.clone()),
        }
    }
}

impl<R: ToolRunner> TuningSystem for AdminTuner<R> {
    fn host(&self) -> TuningHost {
        self.inner.host_kind()
    }

    fn read(&mut self, action: &HelperAction) -> Result<String, PlatformError> {
        use HelperAction as A;
        action.validate().map_err(PlatformError::Failed)?;
        let host = self.host();
        match action {
            A::SetMachineStartupEnabled {
                location: MachineStartup::LaunchAgent,
                name,
                ..
            } if host == TuningHost::Macos => return self.launch_agent_state(name).map(|on| if on { "on" } else { "off" }.into()),
            _ => {}
        }
        let Some(tools) = admin_read_tools(host, action) else {
            return self.inner.read(action);
        };
        let tools = tools?;
        match action {
            A::SetHibernate(_) => {
                let out = self.inner.run_tool(&tools[0])?;
                Ok(if out.ok && parse_reg_dword(&out.stdout).is_some_and(|v| v != 0) {
                    "on"
                } else {
                    "off"
                }
                .into())
            }
            A::SetDefenderExcluded { .. } => {
                let mut states = Vec::new();
                for t in &tools {
                    let text = self.inner.run_ok(t)?;
                    states.push(match text.trim() {
                        "excluded" => "excluded",
                        "included" => "included",
                        _ => return Err(PlatformError::Failed("Could not read the Microsoft Defender exclusions.".into())),
                    });
                }
                Ok(states.join(","))
            }
            A::SetMachineStartupEnabled { location, .. } => {
                let out = self.inner.run_tool(&tools[0])?;
                let on = if *location == MachineStartup::ScheduledTask {
                    if !out.ok {
                        return Err(PlatformError::NotFound("Windows has no such scheduled task.".into()));
                    }
                    parse_task_enabled(&out.stdout)
                } else {
                    parse_startup_approved(&out.stdout, out.ok)
                };
                on.map(|b| if b { "on" } else { "off" }.to_string())
                    .ok_or_else(|| PlatformError::Failed("Could not read that startup entry's state.".into()))
            }
            _ => Err(unsupported(&action.preview().join("; "), host)),
        }
    }

    fn apply(&mut self, action: &HelperAction) -> Result<(), PlatformError> {
        use HelperAction as A;
        action.validate().map_err(PlatformError::Failed)?;
        let host = self.host();
        if !action.needs_admin_on(host) {
            return Err(PlatformError::Unsupported("That is not a change the helper makes.".into()));
        }
        if let A::SetMachineStartupEnabled {
            location: MachineStartup::LaunchAgent,
            name,
            enabled,
        } = action
        {
            return if host == TuningHost::Macos {
                self.set_launch_agent(name, *enabled)
            } else {
                Err(unsupported("An all-users launch agent", host))
            };
        }
        if matches!(action, A::SetDefenderExcluded { .. }) && host != TuningHost::Windows {
            return Err(unsupported("Microsoft Defender exclusions", host));
        }
        if admin_change_tools(host, action).is_none() {
            // The charge limit is a file write the unprivileged tuner already knows; run as the helper it is allowed.
            return self.inner.apply(action);
        }
        let resolved = Self::defender_paths(action, matches!(action, A::SetDefenderExcluded { excluded: true, .. }))?;
        let tools = admin_change_tools(host, &resolved).unwrap_or_else(|| Ok(vec![]))?;
        let done = tools
            .iter()
            .enumerate()
            .try_for_each(|(i, t)| self.inner.run_ok(t).map(|_| ()).map_err(|e| (i, e)));
        match done {
            Ok(()) => Ok(()),
            Err((i, e)) => {
                // A multi-folder change never stops half way: put the folders already done back.
                if let A::SetDefenderExcluded { paths, excluded } = &resolved {
                    for p in paths.iter().take(i) {
                        let back = A::SetDefenderExcluded {
                            paths: vec![p.clone()],
                            excluded: !excluded,
                        };
                        for t in admin_change_tools(host, &back).and_then(Result::ok).unwrap_or_default() {
                            let _ = self.inner.run_ok(&t);
                        }
                    }
                }
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::ToolOutput;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Script {
        answers: VecDeque<ToolOutput>,
        ran: Arc<Mutex<Vec<Tool>>>,
    }

    fn out(ok: bool, stdout: &str) -> ToolOutput {
        ToolOutput {
            ok,
            stdout: stdout.into(),
            error: if ok { String::new() } else { "access is denied".into() },
        }
    }

    impl ToolRunner for Script {
        fn run(&mut self, t: &Tool) -> Result<ToolOutput, PlatformError> {
            self.ran.lock().unwrap().push(t.clone());
            Ok(self.answers.pop_front().unwrap_or_else(|| out(true, "")))
        }
    }

    fn admin(host: TuningHost, answers: Vec<ToolOutput>, dir: &Path) -> (AdminTuner<Script>, Arc<Mutex<Vec<Tool>>>) {
        let ran = Arc::new(Mutex::new(Vec::new()));
        let script = Script {
            answers: answers.into(),
            ran: Arc::clone(&ran),
        };
        (
            AdminTuner::new(OsTuner::new(host, script, dir.join("power_supply")), dir.join("LaunchAgents")),
            ran,
        )
    }

    fn temp(name: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("dyno-it-admin-{name}-"))
            .tempdir()
            .unwrap()
    }

    fn startup(location: MachineStartup, name: &str, enabled: bool) -> HelperAction {
        HelperAction::SetMachineStartupEnabled {
            location,
            name: name.into(),
            enabled,
        }
    }

    #[test]
    fn parsers_read_what_windows_prints() {
        let on = "    Updater    REG_BINARY    020000000000000000000000
";
        let off = "    Updater    REG_BINARY    03000000A1B2C3D4E5F60708
";
        assert_eq!(parse_startup_approved(on, true), Some(true));
        assert_eq!(parse_startup_approved(off, true), Some(false));
        assert_eq!(parse_startup_approved("06000000", true), None);
        assert_eq!(
            parse_startup_approved("ERROR: unable to find the specified registry key", false),
            Some(true)
        );
        assert_eq!(parse_startup_approved("junk", true), None);
        assert_eq!(parse_task_enabled("\"\\Updater\",\"N/A\",\"Disabled\"\r\n"), Some(false));
        assert_eq!(parse_task_enabled("\"Updater\",\"10/24/2026 9:00:00 AM\",\"Ready\"\n"), Some(true));
        assert_eq!(parse_task_enabled(""), None);
    }

    #[test]
    fn windows_changes_are_fixed_programs_and_a_folder_path_travels_in_the_environment() {
        let dir = temp("win");
        let folder = dir.path().join("src");
        std::fs::create_dir_all(&folder).unwrap();
        let (mut t, ran) = admin(TuningHost::Windows, vec![], dir.path());
        t.apply(&HelperAction::SetHibernate(false)).unwrap();
        t.apply(&startup(MachineStartup::RunKey, "Updater", false)).unwrap();
        t.apply(&startup(MachineStartup::StartupFolder, "App.lnk", true)).unwrap();
        t.apply(&startup(MachineStartup::ScheduledTask, "Updater", false)).unwrap();
        t.apply(&HelperAction::SetDefenderExcluded {
            paths: vec![folder.to_string_lossy().into_owned()],
            excluded: true,
        })
        .unwrap();
        let ran = ran.lock().unwrap();
        assert!(ran[0].program.ends_with("powercfg.exe") && ran[0].args == ["/hibernate", "off"]);
        assert!(ran[1].program.ends_with("reg.exe") && ran[1].args.contains(&APPROVED_RUN.to_string()));
        assert!(ran[1].args.contains(&APPROVED_DISABLED.to_string()) && ran[1].args.contains(&"Updater".to_string()));
        assert!(ran[2].args.contains(&APPROVED_FOLDER.to_string()) && ran[2].args.contains(&APPROVED_ENABLED.to_string()));
        assert!(ran[3].program.ends_with("schtasks.exe") && ran[3].args == ["/Change", "/TN", "Updater", "/DISABLE"]);
        // The folder never appears in the script or the arguments, only in the environment.
        assert!(ran[4].program.ends_with("powershell.exe"));
        assert_eq!(ran[4].args.last().unwrap(), "Add-MpPreference -ExclusionPath $env:DYNO_PATH");
        assert!(ran[4].args.iter().all(|a| !a.contains("src")));
        assert_eq!(ran[4].env.len(), 1);
        assert_eq!(ran[4].env[0].0, PATH_VARIABLE);
        assert!(ran[4].env[0].1.ends_with("src") && !ran[4].env[0].1.starts_with(r"\\?\"));
    }

    #[test]
    fn a_defender_change_refuses_a_missing_folder_a_file_and_a_link_but_may_remove_a_gone_folder() {
        let dir = temp("def");
        let file = dir.path().join("f");
        std::fs::write(&file, b"x").unwrap();
        let gone = dir.path().join("gone").to_string_lossy().into_owned();
        let (mut t, ran) = admin(TuningHost::Windows, vec![], dir.path());
        let add = |p: &str| HelperAction::SetDefenderExcluded {
            paths: vec![p.to_string()],
            excluded: true,
        };
        assert!(matches!(t.apply(&add(&gone)), Err(PlatformError::NotFound(_))));
        assert!(matches!(t.apply(&add(&file.to_string_lossy())), Err(PlatformError::Failed(_))));
        #[cfg(unix)]
        {
            let real = dir.path().join("real");
            std::fs::create_dir(&real).unwrap();
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            assert!(matches!(t.apply(&add(&link.to_string_lossy())), Err(PlatformError::Failed(_))));
        }
        assert!(ran.lock().unwrap().is_empty(), "nothing ran for a refused folder");
        t.apply(&HelperAction::SetDefenderExcluded {
            paths: vec![gone.clone()],
            excluded: false,
        })
        .unwrap();
        assert_eq!(ran.lock().unwrap()[0].env[0].1, gone);
    }

    #[test]
    fn a_failing_folder_puts_the_earlier_folders_back() {
        let dir = temp("rollback");
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let (mut t, ran) = admin(TuningHost::Windows, vec![out(true, ""), out(false, "")], dir.path());
        let err = t
            .apply(&HelperAction::SetDefenderExcluded {
                paths: vec![a.to_string_lossy().into_owned(), b.to_string_lossy().into_owned()],
                excluded: true,
            })
            .unwrap_err();
        assert!(matches!(err, PlatformError::Failed(_)));
        let ran = ran.lock().unwrap();
        assert_eq!(ran.len(), 3);
        assert!(ran[2].args.last().unwrap().starts_with("Remove-MpPreference"));
        assert!(ran[2].env[0].1.ends_with('a'));
    }

    #[test]
    fn windows_reads_say_what_is_set() {
        let dir = temp("winread");
        let folder = dir.path().to_string_lossy().into_owned();
        let (mut t, _) = admin(
            TuningHost::Windows,
            vec![
                out(true, "    HibernateEnabled    REG_DWORD    0x1\n"),
                out(true, "excluded\r\n"),
                out(true, "    Updater    REG_BINARY    030000000000000000000000\n"),
                out(false, ""),
                out(true, "\"Updater\",\"N/A\",\"Disabled\""),
                out(false, ""),
                out(true, "maybe"),
            ],
            dir.path(),
        );
        assert_eq!(t.read(&HelperAction::SetHibernate(false)).unwrap(), "on");
        let d = HelperAction::SetDefenderExcluded {
            paths: vec![folder.clone()],
            excluded: true,
        };
        assert_eq!(t.read(&d).unwrap(), "excluded");
        assert_eq!(t.read(&startup(MachineStartup::RunKey, "Updater", true)).unwrap(), "off");
        // No value yet: enabled.
        assert_eq!(t.read(&startup(MachineStartup::StartupFolder, "x.lnk", true)).unwrap(), "on");
        assert_eq!(t.read(&startup(MachineStartup::ScheduledTask, "Updater", true)).unwrap(), "off");
        assert!(matches!(
            t.read(&startup(MachineStartup::ScheduledTask, "Nope", true)),
            Err(PlatformError::NotFound(_))
        ));
        assert!(matches!(t.read(&d), Err(PlatformError::Failed(_))));
    }

    #[test]
    fn mac_changes_are_pmset_calls() {
        let dir = temp("mac");
        let (mut t, ran) = admin(TuningHost::Macos, vec![], dir.path());
        t.apply(&HelperAction::SetLowPowerMode(true)).unwrap();
        t.apply(&HelperAction::SetSleepMinutes {
            source: SleepSource::Battery,
            minutes: 10,
        })
        .unwrap();
        t.apply(&HelperAction::SetSleepMinutes {
            source: SleepSource::Ac,
            minutes: 0,
        })
        .unwrap();
        let ran = ran.lock().unwrap();
        assert_eq!(ran[0].program, "/usr/bin/pmset");
        assert_eq!(ran[0].args, ["-a", "lowpowermode", "1"]);
        assert_eq!(ran[1].args, ["-b", "sleep", "10"]);
        assert_eq!(ran[2].args, ["-c", "sleep", "0"]);
    }

    #[test]
    fn all_users_launch_agents_are_renamed_inside_their_folder_and_back() {
        let dir = temp("agents");
        let agents = dir.path().join("LaunchAgents");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::write(agents.join("com.example.up.plist"), b"<plist/>").unwrap();
        let (mut t, _) = admin(TuningHost::Macos, vec![], dir.path());
        let off = startup(MachineStartup::LaunchAgent, "com.example.up", false);
        let on = startup(MachineStartup::LaunchAgent, "com.example.up", true);
        assert_eq!(t.read(&off).unwrap(), "on");
        t.apply(&off).unwrap();
        assert!(!agents.join("com.example.up.plist").exists());
        assert!(agents.join("com.example.up.plist.dyno-disabled").exists());
        assert_eq!(t.read(&on).unwrap(), "off");
        t.apply(&off).unwrap();
        t.apply(&on).unwrap();
        assert_eq!(std::fs::read(agents.join("com.example.up.plist")).unwrap(), b"<plist/>");
        assert!(matches!(
            t.apply(&startup(MachineStartup::LaunchAgent, "com.example.missing", false)),
            Err(PlatformError::NotFound(_))
        ));
        // Both files present: leave it alone.
        std::fs::write(agents.join("com.example.up.plist.dyno-disabled"), b"x").unwrap();
        assert!(matches!(t.apply(&off), Err(PlatformError::Failed(_))));
        // A label that would leave the folder never gets that far.
        assert!(t.apply(&startup(MachineStartup::LaunchAgent, "../etc/passwd", false)).is_err());
        assert!(t.apply(&startup(MachineStartup::LaunchAgent, "a/b", false)).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_launch_agent_that_is_a_link_is_not_followed() {
        let dir = temp("agentlink");
        let agents = dir.path().join("LaunchAgents");
        std::fs::create_dir_all(&agents).unwrap();
        let target = dir.path().join("elsewhere.plist");
        std::fs::write(&target, b"x").unwrap();
        std::os::unix::fs::symlink(&target, agents.join("com.example.link.plist")).unwrap();
        let (mut t, _) = admin(TuningHost::Macos, vec![], dir.path());
        let r = t.apply(&startup(MachineStartup::LaunchAgent, "com.example.link", false));
        assert!(matches!(r, Err(PlatformError::NotFound(_))));
        assert!(target.exists());
    }

    #[test]
    fn only_administrator_changes_are_made_and_the_charge_limit_is_a_file_write() {
        let dir = temp("linux");
        let (mut t, ran) = admin(TuningHost::Linux, vec![], dir.path());
        // Not an administrator change on Linux: refused, nothing ran.
        assert!(matches!(
            t.apply(&HelperAction::SetPowerProfile(dyno_core::helper::PowerProfile::Balanced)),
            Err(PlatformError::Unsupported(_))
        ));
        assert!(matches!(
            t.apply(&HelperAction::SetDefenderExcluded {
                paths: vec!["/home/me/src".into()],
                excluded: true
            }),
            Err(PlatformError::Unsupported(_))
        ));
        assert!(matches!(
            t.apply(&startup(MachineStartup::LaunchAgent, "com.example.up", false)),
            Err(PlatformError::Unsupported(_))
        ));
        // Invalid arguments are refused before anything happens.
        assert!(matches!(t.apply(&HelperAction::SetChargeLimit(10)), Err(PlatformError::Failed(_))));
        let battery = dir.path().join("power_supply").join("BAT0");
        std::fs::create_dir_all(&battery).unwrap();
        std::fs::write(battery.join("charge_control_end_threshold"), "100").unwrap();
        t.apply(&HelperAction::SetChargeLimit(80)).unwrap();
        assert_eq!(std::fs::read_to_string(battery.join("charge_control_end_threshold")).unwrap(), "80");
        assert_eq!(t.read(&HelperAction::SetChargeLimit(80)).unwrap(), "80");
        assert!(ran.lock().unwrap().is_empty());
    }

    #[test]
    fn tools_exist_only_for_the_hosts_that_can_run_them() {
        let h = HelperAction::SetHibernate(true);
        assert!(admin_change_tools(TuningHost::Windows, &h).is_some());
        assert!(admin_change_tools(TuningHost::Linux, &h).is_none());
        assert!(admin_read_tools(TuningHost::Macos, &h).is_none());
        let agent = startup(MachineStartup::LaunchAgent, "a.b", true);
        assert!(matches!(
            admin_change_tools(TuningHost::Windows, &agent),
            Some(Err(PlatformError::Unsupported(_)))
        ));
        assert!(matches!(
            admin_read_tools(TuningHost::Windows, &agent),
            Some(Err(PlatformError::Unsupported(_)))
        ));
    }
}
