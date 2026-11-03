//! Installing and removing the helper as a typed plan (ADR 0016). A plan is a list of [`Step`]s built by pure
//! functions from the system's paths and the account; a [`Machine`] carries them out. Tests assert the exact plan and
//! the generated launchd, systemd and Windows service configuration; nothing here is run against the real system
//! except by `dyno-helper install` on the user's click.

use crate::paths::{join_windows, HelperPaths, WindowsRoots, LINUX_UNIT, MAC_LABEL, WINDOWS_SERVICE};
use dyno_core::helper::HelperHost;
use dyno_core::helper_wire::HelperUser;
use dyno_platform::runner::{SystemRunner, Tool, ToolRunner};
use paddock_ipc::Token;
use std::path::{Path, PathBuf};

/// One thing the installer does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Creates a folder (and its parents) with this mode (ignored on Windows).
    CreateDir {
        /// The folder.
        path: PathBuf,
        /// Unix mode.
        mode: u32,
    },
    /// Copies the running program to `to` (via a temporary name, so a running copy is replaced atomically).
    CopyProgram {
        /// The destination.
        to: PathBuf,
        /// Unix mode.
        mode: u32,
    },
    /// Writes a file.
    WriteFile {
        /// The file.
        path: PathBuf,
        /// Its text.
        contents: String,
        /// Unix mode.
        mode: u32,
    },
    /// Writes a fresh random token that only `owner` (and root) can read.
    NewToken {
        /// The file.
        path: PathBuf,
        /// The authorised user.
        owner: HelperUser,
    },
    /// Runs a program.
    Run {
        /// What to run.
        tool: Tool,
        /// Whether a failure is fine (stopping a service that is not there).
        tolerate_failure: bool,
    },
    /// Starts a program and does not wait for it (it outlives the helper: deletes the helper's own file on Windows).
    Deferred(Tool),
    /// Removes a file (a missing file is fine).
    RemoveFile(PathBuf),
    /// Removes a folder and what is in it (a missing folder is fine).
    RemoveTree(PathBuf),
}

impl Step {
    /// What this step does, for the message when it fails.
    pub fn describe(&self) -> String {
        match self {
            Step::CreateDir { path, .. } => format!("creating {}", path.display()),
            Step::CopyProgram { to, .. } => format!("copying the helper to {}", to.display()),
            Step::WriteFile { path, .. } => format!("writing {}", path.display()),
            Step::NewToken { path, .. } => format!("writing {}", path.display()),
            Step::Run { tool, .. } | Step::Deferred(tool) => format!("running {} {}", tool.program, tool.args.join(" ")),
            Step::RemoveFile(p) | Step::RemoveTree(p) => format!("removing {}", p.display()),
        }
    }
}

/// Carries out steps.
pub trait Machine {
    /// Does one step.
    fn perform(&mut self, step: &Step) -> Result<(), String>;
}

/// Runs `steps` in order and stops at the first that fails, saying which.
pub fn execute(machine: &mut dyn Machine, steps: &[Step]) -> Result<(), String> {
    for step in steps {
        machine
            .perform(step)
            .map_err(|e| format!("The helper installer failed while {}: {e}", step.describe()))?;
    }
    Ok(())
}

/// What `install` was told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallParams {
    /// The one user to serve.
    pub user: HelperUser,
    /// The agent program that may call.
    pub agent_exe: PathBuf,
    /// The Dyno version.
    pub version: String,
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The LaunchDaemon property list.
pub fn launchd_plist(paths: &HelperPaths) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>serve</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>ThrottleInterval</key>
    <integer>10</integer>
    <key>ProcessType</key>
    <string>Background</string>
    <key>StandardErrorPath</key>
    <string>/var/log/{label}.log</string>
</dict>
</plist>
"#,
        label = MAC_LABEL,
        binary = xml(&paths.binary.to_string_lossy()),
    )
}

/// The systemd unit.
pub fn systemd_unit(paths: &HelperPaths) -> String {
    format!(
        "[Unit]\n\
         Description=Dyno privileged helper\n\
         Documentation=https://dev.decryptology.net/decryp7/dyno\n\
         After=local-fs.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={binary} serve\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         RuntimeDirectory=dyno-helper\n\
         RuntimeDirectoryMode=0755\n\
         NoNewPrivileges=yes\n\
         PrivateTmp=yes\n\
         ProtectHome=yes\n\
         LockPersonality=yes\n\
         RestrictAddressFamilies=AF_UNIX\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        binary = paths.binary.display()
    )
}

fn tool(program: impl AsRef<Path>, args: &[&str]) -> Tool {
    Tool {
        program: program.as_ref().to_string_lossy().into_owned(),
        args: args.iter().map(|a| a.to_string()).collect(),
        env: Vec::new(),
    }
}

fn run(program: impl AsRef<Path>, args: &[&str]) -> Step {
    Step::Run {
        tool: tool(program, args),
        tolerate_failure: false,
    }
}

fn run_if_present(program: impl AsRef<Path>, args: &[&str]) -> Step {
    Step::Run {
        tool: tool(program, args),
        tolerate_failure: true,
    }
}

/// The folder of a Windows path, by Windows rules (the plan may be built on another system).
fn windows_parent(p: &Path) -> PathBuf {
    PathBuf::from(
        p.to_string_lossy()
            .rsplit_once('\\')
            .map(|(d, _)| d.to_string())
            .unwrap_or_default(),
    )
}

fn windows_grant(sid: &str, rights: &str) -> String {
    format!("*{sid}:(OI)(CI){rights}")
}

/// The plan that installs the helper for `params.user` on `paths.host`.
pub fn install_plan(paths: &HelperPaths, windows: &WindowsRoots, params: &InstallParams) -> Result<Vec<Step>, String> {
    if !crate::paths::is_absolute_on(paths.host, &params.agent_exe) {
        return Err("The agent program must be given as a full path.".into());
    }
    match (&params.user, paths.host) {
        (HelperUser::Uid(_), HelperHost::Windows) | (HelperUser::Sid(_), HelperHost::Macos | HelperHost::Linux) => {
            return Err("The account does not belong to this system.".into())
        }
        _ => {}
    }
    let config = crate::config::HelperConfig {
        schema: crate::config::SCHEMA,
        user: params.user.clone(),
        agent_exe: params.agent_exe.clone(),
        version: params.version.clone(),
    };
    let parent = |p: &Path| p.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut steps = Vec::new();
    match paths.host {
        HelperHost::Macos => {
            let registration = paths.registration.clone().unwrap_or_default();
            let target = format!("system/{MAC_LABEL}");
            steps.push(run_if_present("/bin/launchctl", &["bootout", &target]));
            steps.push(Step::CreateDir {
                path: parent(&paths.binary),
                mode: 0o755,
            });
            steps.push(Step::CreateDir {
                path: paths.data_dir.clone(),
                mode: 0o755,
            });
            steps.push(Step::CopyProgram {
                to: paths.binary.clone(),
                mode: 0o755,
            });
            steps.push(Step::WriteFile {
                path: paths.config.clone(),
                contents: config.to_text(),
                mode: 0o644,
            });
            steps.push(Step::NewToken {
                path: paths.token.clone(),
                owner: params.user.clone(),
            });
            steps.push(Step::WriteFile {
                path: registration.clone(),
                contents: launchd_plist(paths),
                mode: 0o644,
            });
            steps.push(run("/bin/launchctl", &["bootstrap", "system", &registration.to_string_lossy()]));
            steps.push(run_if_present("/bin/launchctl", &["kickstart", "-k", &target]));
        }
        HelperHost::Linux => {
            let registration = paths.registration.clone().unwrap_or_default();
            steps.push(run_if_present("/usr/bin/systemctl", &["stop", LINUX_UNIT]));
            steps.push(Step::CreateDir {
                path: parent(&paths.binary),
                mode: 0o755,
            });
            steps.push(Step::CreateDir {
                path: paths.data_dir.clone(),
                mode: 0o755,
            });
            steps.push(Step::CopyProgram {
                to: paths.binary.clone(),
                mode: 0o755,
            });
            steps.push(Step::WriteFile {
                path: paths.config.clone(),
                contents: config.to_text(),
                mode: 0o644,
            });
            steps.push(Step::NewToken {
                path: paths.token.clone(),
                owner: params.user.clone(),
            });
            steps.push(Step::WriteFile {
                path: registration,
                contents: systemd_unit(paths),
                mode: 0o644,
            });
            steps.push(run("/usr/bin/systemctl", &["daemon-reload"]));
            steps.push(run("/usr/bin/systemctl", &["enable", LINUX_UNIT]));
            steps.push(run("/usr/bin/systemctl", &["restart", LINUX_UNIT]));
        }
        HelperHost::Windows => {
            let HelperUser::Sid(sid) = &params.user else {
                return Err("The account does not belong to this system.".into());
            };
            let system32 = join_windows(&windows.system_root, "System32");
            let sc = join_windows(&system32, "sc.exe");
            let icacls = join_windows(&system32, "icacls.exe");
            let binpath = format!("\"{}\" service", paths.binary.display());
            steps.push(run_if_present(&sc, &["stop", WINDOWS_SERVICE]));
            steps.push(Step::CreateDir {
                path: windows_parent(&paths.binary),
                mode: 0,
            });
            steps.push(Step::CreateDir {
                path: paths.data_dir.clone(),
                mode: 0,
            });
            // Only SYSTEM and administrators write the data folder; the authorised account reads it (its token lives there).
            steps.push(run(
                &icacls,
                &[
                    &paths.data_dir.to_string_lossy(),
                    "/inheritance:r",
                    "/grant:r",
                    &windows_grant("S-1-5-18", "F"),
                    "/grant:r",
                    &windows_grant("S-1-5-32-544", "F"),
                    "/grant:r",
                    &windows_grant(sid, "RX"),
                ],
            ));
            steps.push(Step::CopyProgram {
                to: paths.binary.clone(),
                mode: 0,
            });
            steps.push(Step::WriteFile {
                path: paths.config.clone(),
                contents: config.to_text(),
                mode: 0,
            });
            steps.push(Step::NewToken {
                path: paths.token.clone(),
                owner: params.user.clone(),
            });
            steps.push(run_if_present(&sc, &["delete", WINDOWS_SERVICE]));
            steps.push(run(
                &sc,
                &[
                    "create",
                    WINDOWS_SERVICE,
                    "binPath=",
                    &binpath,
                    "start=",
                    "auto",
                    "obj=",
                    "LocalSystem",
                    "DisplayName=",
                    "Dyno privileged helper",
                ],
            ));
            steps.push(run(
                &sc,
                &[
                    "description",
                    WINDOWS_SERVICE,
                    "Answers the few read-only questions Dyno asks that need administrator rights, for one approved user.",
                ],
            ));
            steps.push(run(
                &sc,
                &[
                    "failure",
                    WINDOWS_SERVICE,
                    "reset=",
                    "86400",
                    "actions=",
                    "restart/5000/restart/30000//0",
                ],
            ));
            steps.push(run(&sc, &["start", WINDOWS_SERVICE]));
        }
    }
    Ok(steps)
}

/// The plan that removes the helper. Its last step may end the running helper, so everything else comes first.
pub fn uninstall_plan(paths: &HelperPaths, windows: &WindowsRoots) -> Vec<Step> {
    let mut steps = Vec::new();
    match paths.host {
        HelperHost::Macos => {
            steps.push(Step::RemoveTree(paths.data_dir.clone()));
            steps.push(Step::RemoveFile(paths.registration.clone().unwrap_or_default()));
            steps.push(Step::RemoveFile(paths.binary.clone()));
            steps.push(run_if_present("/bin/launchctl", &["bootout", &format!("system/{MAC_LABEL}")]));
        }
        HelperHost::Linux => {
            steps.push(run_if_present("/usr/bin/systemctl", &["disable", LINUX_UNIT]));
            steps.push(Step::RemoveFile(paths.registration.clone().unwrap_or_default()));
            steps.push(Step::RemoveTree(paths.data_dir.clone()));
            steps.push(Step::RemoveFile(paths.binary.clone()));
            steps.push(run_if_present("/usr/bin/systemctl", &["daemon-reload"]));
            steps.push(run_if_present("/usr/bin/systemctl", &["stop", LINUX_UNIT]));
        }
        HelperHost::Windows => {
            let sys32 = join_windows(&windows.system_root, "System32");
            let dir = windows_parent(&paths.binary);
            steps.push(Step::RemoveTree(paths.data_dir.clone()));
            steps.push(run_if_present(join_windows(&sys32, "sc.exe"), &["delete", WINDOWS_SERVICE]));
            // The running program cannot delete itself; a short-lived shell does it after the service has stopped.
            steps.push(Step::Deferred(tool(
                join_windows(&sys32, "cmd.exe"),
                &[
                    "/d",
                    "/c",
                    &format!(
                        "ping -n 6 127.0.0.1 >nul & del /f /q \"{}\" & rmdir \"{}\"",
                        paths.binary.display(),
                        dir.display()
                    ),
                ],
            )));
            steps.push(run_if_present(join_windows(&sys32, "sc.exe"), &["stop", WINDOWS_SERVICE]));
        }
    }
    steps
}

/// Carries steps out on this computer.
pub struct SystemMachine {
    runner: Box<dyn ToolRunner>,
    program: PathBuf,
}

impl SystemMachine {
    /// A machine that copies the running program and runs real tools.
    pub fn new() -> Result<SystemMachine, String> {
        let program = std::env::current_exe().map_err(|e| format!("The helper cannot find its own program: {e}"))?;
        Ok(SystemMachine {
            runner: Box::new(SystemRunner),
            program,
        })
    }

    /// A machine with another tool runner and program (tests).
    pub fn with(runner: Box<dyn ToolRunner>, program: PathBuf) -> SystemMachine {
        SystemMachine { runner, program }
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: u32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn give_to(path: &Path, owner: &HelperUser) -> std::io::Result<()> {
    match owner {
        HelperUser::Uid(uid) => std::os::unix::fs::chown(path, Some(*uid), None),
        HelperUser::Sid(_) => Err(std::io::Error::other("a Windows account on a Unix system")),
    }
}

#[cfg(not(unix))]
fn give_to(_: &Path, _: &HelperUser) -> std::io::Result<()> {
    // The data folder's access list (icacls) already gives the account read access to what is created inside it.
    Ok(())
}

/// Writes `bytes` to `path` through a temporary file in the same folder and a rename, with `mode` set before the
/// file becomes visible (the token is never readable by others, even for a moment).
fn write_atomic(path: &Path, bytes: &[u8], mode: u32, owner: Option<&HelperUser>) -> std::io::Result<()> {
    let tmp = path.with_extension("new");
    let _ = std::fs::remove_file(&tmp);
    std::fs::write(&tmp, bytes)?;
    set_mode(&tmp, mode)?;
    if let Some(o) = owner {
        give_to(&tmp, o)?;
    }
    std::fs::rename(&tmp, path)
}

impl Machine for SystemMachine {
    fn perform(&mut self, step: &Step) -> Result<(), String> {
        let io = |e: std::io::Error| e.to_string();
        match step {
            Step::CreateDir { path, mode } => {
                std::fs::create_dir_all(path).map_err(io)?;
                set_mode(path, *mode).map_err(io)
            }
            Step::CopyProgram { to, mode } => {
                let tmp = to.with_extension("new");
                let _ = std::fs::remove_file(&tmp);
                std::fs::copy(&self.program, &tmp).map_err(io)?;
                set_mode(&tmp, *mode).map_err(io)?;
                // A stopping service still holds its file for a moment on Windows.
                let mut last = None;
                for _ in 0..10 {
                    match std::fs::rename(&tmp, to) {
                        Ok(()) => return Ok(()),
                        Err(e) => {
                            last = Some(e);
                            std::thread::sleep(std::time::Duration::from_millis(500));
                        }
                    }
                }
                let _ = std::fs::remove_file(&tmp);
                Err(last.map(|e| e.to_string()).unwrap_or_default())
            }
            Step::WriteFile { path, contents, mode } => write_atomic(path, contents.as_bytes(), *mode, None).map_err(io),
            Step::NewToken { path, owner } => {
                let token = Token::random().map_err(io)?;
                write_atomic(path, format!("{}\n", token.to_hex()).as_bytes(), 0o600, Some(owner)).map_err(io)
            }
            Step::Run { tool, tolerate_failure } => match self.runner.run(tool) {
                Ok(out) if out.ok || *tolerate_failure => Ok(()),
                Ok(out) => Err(if out.error.is_empty() {
                    "the program reported a failure".into()
                } else {
                    out.error
                }),
                Err(_) if *tolerate_failure => Ok(()),
                Err(e) => Err(e.to_string()),
            },
            Step::Deferred(tool) => {
                let mut cmd = dyno_platform::process::command(&tool.program);
                cmd.args(&tool.args);
                cmd.spawn().map(|_| ()).map_err(io)
            }
            Step::RemoveFile(path) => match std::fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            },
            Step::RemoveTree(path) => match std::fs::remove_dir_all(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_platform::runner::ToolOutput;
    use std::sync::{Arc, Mutex};

    fn roots() -> WindowsRoots {
        WindowsRoots {
            program_files: PathBuf::from(r"C:\Program Files"),
            program_data: PathBuf::from(r"C:\ProgramData"),
            system_root: PathBuf::from(r"C:\Windows"),
        }
    }

    fn params(user: HelperUser) -> InstallParams {
        let agent_exe = if matches!(user, HelperUser::Sid(_)) {
            PathBuf::from(r"C:\Program Files\Dyno\dyno-agent.exe")
        } else {
            // Plans for Unix systems are checked by Unix rules (a leading slash) on every build host.
            PathBuf::from("/Applications/Dyno.app/Contents/MacOS/dyno-agent")
        };
        InstallParams {
            user,
            agent_exe,
            version: "1.0.0".into(),
        }
    }

    fn programs(steps: &[Step]) -> Vec<String> {
        steps
            .iter()
            .filter_map(|s| match s {
                Step::Run { tool, .. } | Step::Deferred(tool) => {
                    let name = tool.program.rsplit(['\\', '/']).next().unwrap_or_default();
                    Some(format!("{name} {}", tool.args.join(" ")))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_launchd_plist_runs_the_installed_copy_and_restarts_it_after_a_crash() {
        let p = HelperPaths::for_host(HelperHost::Macos, &roots());
        let plist = launchd_plist(&p);
        assert!(plist.contains("<string>net.decryptology.dyno.helper</string>"));
        assert!(
            plist.contains("<string>/Library/PrivilegedHelperTools/net.decryptology.dyno.helper</string>\n        <string>serve</string>")
        );
        assert!(plist.contains("<key>RunAtLoad</key>\n    <true/>"));
        assert!(plist.contains("<key>SuccessfulExit</key>\n        <false/>"));
        assert!(plist.contains("/var/log/net.decryptology.dyno.helper.log"));
        // The program path is escaped, not trusted.
        let mut odd = p.clone();
        odd.binary = PathBuf::from("/x/<a&b>");
        assert!(launchd_plist(&odd).contains("/x/&lt;a&amp;b&gt;"));
    }

    #[test]
    fn the_systemd_unit_is_a_restarting_hardened_service_with_its_runtime_folder() {
        let unit = systemd_unit(&HelperPaths::for_host(HelperHost::Linux, &roots()));
        for line in [
            "ExecStart=/usr/local/libexec/dyno-helper serve",
            "Restart=on-failure",
            "RuntimeDirectory=dyno-helper",
            "RuntimeDirectoryMode=0755",
            "NoNewPrivileges=yes",
            "ProtectHome=yes",
            "RestrictAddressFamilies=AF_UNIX",
            "WantedBy=multi-user.target",
        ] {
            assert!(unit.lines().any(|l| l == line), "{line}");
        }
    }

    #[test]
    fn the_mac_plan_copies_first_registers_last_and_gives_the_token_to_the_user() {
        let p = HelperPaths::for_host(HelperHost::Macos, &roots());
        let plan = install_plan(&p, &roots(), &params(HelperUser::Uid(501))).unwrap();
        assert!(
            matches!(
                &plan[0],
                Step::Run {
                    tolerate_failure: true,
                    ..
                }
            ),
            "an older copy is stopped first"
        );
        let copy = plan.iter().position(|s| matches!(s, Step::CopyProgram { .. })).unwrap();
        let bootstrap = plan
            .iter()
            .position(|s| matches!(s, Step::Run { tool, .. } if tool.args.first().map(String::as_str) == Some("bootstrap")))
            .unwrap();
        assert!(copy < bootstrap);
        assert!(plan.contains(&Step::NewToken {
            path: p.token.clone(),
            owner: HelperUser::Uid(501)
        }));
        let config = plan
            .iter()
            .find_map(|s| match s {
                Step::WriteFile { path, contents, .. } if *path == p.config => Some(contents.clone()),
                _ => None,
            })
            .unwrap();
        // Deserialised, not `HelperConfig::parse`: that also requires the path to be absolute on the build host, and a
        // macOS path has no drive letter on Windows. `parse` has its own tests.
        let parsed: crate::config::HelperConfig = serde_json::from_str(&config).unwrap();
        assert_eq!((parsed.user, parsed.version.as_str()), (HelperUser::Uid(501), "1.0.0"));
        assert_eq!(
            programs(&plan).last().unwrap(),
            "launchctl kickstart -k system/net.decryptology.dyno.helper"
        );
        assert!(
            programs(&plan).contains(&"launchctl bootstrap system /Library/LaunchDaemons/net.decryptology.dyno.helper.plist".to_string())
        );
        // Nothing runs a shell.
        assert!(plan
            .iter()
            .all(|s| !matches!(s, Step::Run { tool, .. } if tool.program.ends_with("sh"))));
    }

    #[test]
    fn the_linux_plan_installs_a_unit_enables_and_restarts_it() {
        let p = HelperPaths::for_host(HelperHost::Linux, &roots());
        let plan = install_plan(&p, &roots(), &params(HelperUser::Uid(1000))).unwrap();
        assert_eq!(
            programs(&plan),
            [
                "systemctl stop dyno-helper.service",
                "systemctl daemon-reload",
                "systemctl enable dyno-helper.service",
                "systemctl restart dyno-helper.service"
            ]
        );
        assert!(plan.iter().any(|s| matches!(s, Step::WriteFile { path, contents, .. }
            if path.ends_with("dyno-helper.service") && contents.contains("ExecStart=/usr/local/libexec/dyno-helper serve"))));
    }

    #[test]
    fn the_windows_plan_locks_the_data_folder_and_creates_a_localsystem_service() {
        let p = HelperPaths::for_host(HelperHost::Windows, &roots());
        let sid = "S-1-5-21-1-2-3-1001";
        let plan = install_plan(&p, &roots(), &params(HelperUser::Sid(sid.into()))).unwrap();
        let progs = programs(&plan);
        assert!(progs[1].starts_with("icacls.exe "));
        assert!(
            progs[1].contains("/inheritance:r") && progs[1].contains("*S-1-5-18:(OI)(CI)F") && progs[1].contains("*S-1-5-32-544:(OI)(CI)F")
        );
        assert!(progs[1].contains(&format!("*{sid}:(OI)(CI)RX")));
        let create = plan
            .iter()
            .find_map(|s| match s {
                Step::Run { tool, .. } if tool.args.first().map(String::as_str) == Some("create") => Some(tool.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            create.args,
            [
                "create",
                "DynoHelper",
                "binPath=",
                "\"C:\\Program Files\\Dyno\\Helper\\dyno-helper.exe\" service",
                "start=",
                "auto",
                "obj=",
                "LocalSystem",
                "DisplayName=",
                "Dyno privileged helper"
            ]
        );
        assert!(create.program.ends_with("sc.exe"));
        assert!(progs
            .iter()
            .any(|p| p.starts_with("sc.exe failure DynoHelper reset= 86400 actions= restart/5000/restart/30000//0")));
        assert_eq!(progs.last().unwrap(), "sc.exe start DynoHelper");
    }

    #[test]
    fn a_plan_for_the_wrong_kind_of_account_or_a_relative_agent_is_refused() {
        let mac = HelperPaths::for_host(HelperHost::Macos, &roots());
        let win = HelperPaths::for_host(HelperHost::Windows, &roots());
        assert!(install_plan(&mac, &roots(), &params(HelperUser::Sid("S-1-5-21-1".into()))).is_err());
        assert!(install_plan(&win, &roots(), &params(HelperUser::Uid(5))).is_err());
        let relative = InstallParams {
            agent_exe: PathBuf::from("dyno-agent"),
            ..params(HelperUser::Uid(5))
        };
        assert!(install_plan(&mac, &roots(), &relative).unwrap_err().contains("full path"));
    }

    #[test]
    fn removal_takes_the_data_and_the_registration_and_stops_the_helper_last() {
        for host in [HelperHost::Macos, HelperHost::Linux, HelperHost::Windows] {
            let p = HelperPaths::for_host(host, &roots());
            let plan = uninstall_plan(&p, &roots());
            assert!(plan.contains(&Step::RemoveTree(p.data_dir.clone())), "{host:?}");
            let last = programs(&plan).pop().unwrap();
            assert!(
                last.contains("bootout") || last.contains("systemctl stop") || last.contains("sc.exe stop"),
                "{host:?}: {last}"
            );
            assert!(
                plan.iter().all(|s| !matches!(
                    s,
                    Step::Run {
                        tolerate_failure: false,
                        ..
                    }
                )),
                "removal never fails on a missing piece"
            );
        }
        let win = uninstall_plan(&HelperPaths::for_host(HelperHost::Windows, &roots()), &roots());
        assert!(
            matches!(&win[2], Step::Deferred(t) if t.args[2].contains("del /f /q \"C:\\Program Files\\Dyno\\Helper\\dyno-helper.exe\""))
        );
    }

    struct Recorder(Vec<Step>, Option<usize>);
    impl Machine for Recorder {
        fn perform(&mut self, step: &Step) -> Result<(), String> {
            self.0.push(step.clone());
            if self.1 == Some(self.0.len()) {
                Err("access denied".into())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn a_failing_step_stops_the_plan_and_says_which() {
        let steps = vec![
            Step::RemoveFile("/a".into()),
            Step::RemoveFile("/b".into()),
            Step::RemoveFile("/c".into()),
        ];
        let mut r = Recorder(vec![], Some(2));
        let e = execute(&mut r, &steps).unwrap_err();
        assert_eq!(r.0.len(), 2);
        assert!(e.contains("removing /b") && e.contains("access denied"));
        assert!(execute(&mut Recorder(vec![], None), &steps).is_ok());
    }

    #[derive(Clone, Default)]
    struct Tools(Arc<Mutex<Vec<Tool>>>, bool);
    impl ToolRunner for Tools {
        fn run(&mut self, t: &Tool) -> Result<ToolOutput, dyno_core::ports::PlatformError> {
            self.0.lock().unwrap().push(t.clone());
            Ok(ToolOutput {
                ok: !self.1,
                stdout: String::new(),
                error: if self.1 { "boom".into() } else { String::new() },
            })
        }
    }

    #[test]
    fn the_real_machine_writes_files_atomically_with_modes_and_removes_them_again() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("running-helper");
        std::fs::write(&program, b"binary").unwrap();
        let tools = Tools::default();
        let mut m = SystemMachine::with(Box::new(tools.clone()), program);
        let data = dir.path().join("data");
        let bin = dir.path().join("lib").join("dyno-helper");
        let token = data.join("token");
        #[cfg(unix)]
        let owner = {
            use std::os::unix::fs::MetadataExt;
            HelperUser::Uid(std::fs::metadata(dir.path()).unwrap().uid())
        };
        #[cfg(not(unix))]
        let owner = HelperUser::Sid("S-1-5-21-1".into());
        let steps = vec![
            Step::CreateDir {
                path: data.clone(),
                mode: 0o755,
            },
            Step::CreateDir {
                path: dir.path().join("lib"),
                mode: 0o755,
            },
            Step::CopyProgram {
                to: bin.clone(),
                mode: 0o755,
            },
            Step::WriteFile {
                path: data.join("config.json"),
                contents: "{}".into(),
                mode: 0o644,
            },
            Step::NewToken {
                path: token.clone(),
                owner,
            },
            Step::Run {
                tool: tool("/bin/true", &[]),
                tolerate_failure: false,
            },
        ];
        execute(&mut m, &steps).unwrap();
        assert_eq!(std::fs::read(&bin).unwrap(), b"binary");
        assert_eq!(std::fs::read_to_string(data.join("config.json")).unwrap(), "{}");
        let t = crate::config::load_token(&token).unwrap();
        assert_ne!(t.to_hex(), "0".repeat(64));
        assert_eq!(tools.0.lock().unwrap().len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&token).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(&bin).unwrap().permissions().mode() & 0o777, 0o755);
        }
        // A second install replaces the token and the program, leaving no temporary files behind.
        execute(&mut m, &steps).unwrap();
        assert!(!bin.with_extension("new").exists() && !token.with_extension("new").exists());
        let removal = vec![
            Step::RemoveTree(data.clone()),
            Step::RemoveFile(bin.clone()),
            Step::RemoveFile(bin.clone()),
            Step::RemoveTree(data.clone()),
        ];
        execute(&mut m, &removal).unwrap();
        assert!(!data.exists() && !bin.exists());
    }

    #[test]
    fn tools_that_fail_stop_the_plan_unless_the_step_tolerates_it() {
        let mut m = SystemMachine::with(Box::new(Tools(Arc::default(), true)), PathBuf::from("/x"));
        let fatal = Step::Run {
            tool: tool("/bin/launchctl", &["bootstrap"]),
            tolerate_failure: false,
        };
        let fine = Step::Run {
            tool: tool("/bin/launchctl", &["bootout"]),
            tolerate_failure: true,
        };
        assert!(execute(&mut m, std::slice::from_ref(&fine)).is_ok());
        assert!(execute(&mut m, &[fatal]).unwrap_err().contains("boom"));
        let missing_program = PathBuf::from("/nonexistent/dyno-helper-test");
        let mut m = SystemMachine::with(Box::new(Tools::default()), missing_program);
        let e = execute(
            &mut m,
            &[Step::CopyProgram {
                to: std::env::temp_dir().join("dyno-never-written"),
                mode: 0o755,
            }],
        )
        .unwrap_err();
        assert!(e.contains("copying the helper"));
    }
}
