//! The tuning adapter behind [`TuningSystem`]: a closed set of typed changes ([`HelperAction`]) turned into fixed
//! programs with validated arguments (`powercfg`, `reg`, `attrib`, `powerprofilesctl`, `pmset`) or small file
//! operations (`.metadata_never_index` markers, the battery charge-limit file). Nothing here builds a command from
//! free text, and nothing here elevates: a change that needs administrator rights answers
//! [`PlatformError::AccessDenied`] and is left to the privileged helper (ADR 0006).
//!
//! [`FakeTuning`] is the in-memory implementation for tests.

use crate::process::{command, run_with_timeout};
use dyno_core::helper::{HelperAction, SleepSource, TuningHost};
use dyno_core::ports::{PlatformError, TuningSystem};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// Every tool call is bounded.
const TOOL_TIMEOUT: Duration = Duration::from_secs(15);
/// The marker file macOS Spotlight honours: a folder holding it is not indexed.
pub const SPOTLIGHT_MARKER: &str = ".metadata_never_index";

/// What a tool answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    /// Exit code zero.
    pub ok: bool,
    /// Standard output, as text.
    pub stdout: String,
    /// The first line of the error output, for messages.
    pub error: String,
}

/// One program with its arguments. Programs come from a fixed list; arguments are validated values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    /// The program (an absolute path where the OS has a fixed place for it).
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Environment variables for this call (a value that must reach a program unparsed travels here, never in text a
    /// shell or a script would read as code).
    pub env: Vec<(String, String)>,
}

pub(crate) fn tool(program: impl Into<String>, args: &[&str]) -> Tool {
    Tool {
        program: program.into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        env: Vec::new(),
    }
}

/// Runs a [`Tool`]. Implemented for real processes and, in tests, by a script.
pub trait ToolRunner: Send {
    /// Runs the tool to completion.
    fn run(&mut self, tool: &Tool) -> Result<ToolOutput, PlatformError>;
}

/// Runs real programs: no console window, a timeout, output bounded.
pub struct SystemRunner;

impl ToolRunner for SystemRunner {
    fn run(&mut self, t: &Tool) -> Result<ToolOutput, PlatformError> {
        let mut cmd = command(&t.program);
        cmd.args(&t.args);
        for (k, v) in &t.env {
            cmd.env(k, v);
        }
        let out = run_with_timeout(cmd, TOOL_TIMEOUT)?;
        let stderr = String::from_utf8_lossy(&out.stderr);
        Ok(ToolOutput {
            ok: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            error: stderr
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim()
                .chars()
                .take(200)
                .collect(),
        })
    }
}

pub(crate) fn windows_tool(name: &str) -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    format!("{root}\\System32\\{name}")
}

const VISUAL_FX_KEY: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VisualEffects";
const BACKGROUND_KEY: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\BackgroundAccessApplications";
pub(crate) const HIBERNATE_KEY: &str = "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Power";

pub(crate) fn denied(what: &str) -> PlatformError {
    PlatformError::AccessDenied(format!(
        "{what} needs administrator rights, which only Dyno's privileged helper can ask for."
    ))
}

pub(crate) fn unsupported(what: &str, host: TuningHost) -> PlatformError {
    let os = match host {
        TuningHost::Windows => "Windows",
        TuningHost::Macos => "macOS",
        TuningHost::Linux => "Linux",
    };
    PlatformError::Unsupported(format!("{what} cannot be changed by Dyno on {os}."))
}

/// The tool calls that make `action`, or why they cannot be made on `host`. File-based changes return no tools.
pub fn change_tools(host: TuningHost, action: &HelperAction) -> Result<Vec<Tool>, PlatformError> {
    use HelperAction as A;
    match (host, action) {
        (TuningHost::Windows, A::SetPowerScheme(g)) => Ok(vec![tool(windows_tool("powercfg.exe"), &["/setactive", g])]),
        (TuningHost::Windows, A::SetSleepMinutes { source, minutes }) => {
            let which = if *source == SleepSource::Ac {
                "standby-timeout-ac"
            } else {
                "standby-timeout-dc"
            };
            Ok(vec![tool(windows_tool("powercfg.exe"), &["/change", which, &minutes.to_string()])])
        }
        (TuningHost::Windows, A::SetHibernate(_)) => Err(denied("Turning hibernation on or off")),
        (TuningHost::Windows, A::SetVisualEffects(v)) => Ok(vec![tool(
            windows_tool("reg.exe"),
            &[
                "add",
                VISUAL_FX_KEY,
                "/v",
                "VisualFXSetting",
                "/t",
                "REG_DWORD",
                "/d",
                &v.to_string(),
                "/f",
            ],
        )]),
        (TuningHost::Windows, A::SetBackgroundAppsDisabled(b)) => Ok(vec![tool(
            windows_tool("reg.exe"),
            &[
                "add",
                BACKGROUND_KEY,
                "/v",
                "GlobalUserDisabled",
                "/t",
                "REG_DWORD",
                "/d",
                if *b { "1" } else { "0" },
                "/f",
            ],
        )]),
        (TuningHost::Windows, A::SetIndexingExcluded { paths, excluded }) => Ok(paths
            .iter()
            .map(|p| tool(windows_tool("attrib.exe"), &[if *excluded { "+I" } else { "-I" }, p, "/D"]))
            .collect()),
        (TuningHost::Linux, A::SetPowerProfile(p)) => Ok(vec![tool("/usr/bin/powerprofilesctl", &["set", p.tool_name()])]),
        (TuningHost::Macos, A::SetSleepMinutes { .. }) => Err(denied("Changing the sleep timer")),
        (TuningHost::Macos, A::SetLowPowerMode(_)) => Err(denied("Changing Low Power Mode")),
        (_, A::SetDefenderExcluded { .. }) => Err(denied("Changing antivirus exclusions")),
        (_, A::SetMachineStartupEnabled { .. }) => Err(denied("Changing an all-users startup entry")),
        (_, A::SetChargeLimit(_)) | (TuningHost::Macos, A::SetIndexingExcluded { .. }) => Ok(vec![]),
        (h, a) => Err(unsupported(&a.preview().join("; "), h)),
    }
}

/// The tool call that reads the setting `action` changes (a file-based setting needs none).
pub fn read_tools(host: TuningHost, action: &HelperAction) -> Result<Vec<Tool>, PlatformError> {
    use HelperAction as A;
    match (host, action) {
        (TuningHost::Windows, A::SetPowerScheme(_)) => Ok(vec![tool(windows_tool("powercfg.exe"), &["/getactivescheme"])]),
        (TuningHost::Windows, A::SetSleepMinutes { .. }) => Ok(vec![tool(
            windows_tool("powercfg.exe"),
            &["/query", "SCHEME_CURRENT", "SUB_SLEEP", "STANDBYIDLE"],
        )]),
        (TuningHost::Windows, A::SetHibernate(_)) => Ok(vec![tool(
            windows_tool("reg.exe"),
            &["query", HIBERNATE_KEY, "/v", "HibernateEnabled"],
        )]),
        (TuningHost::Windows, A::SetVisualEffects(_)) => Ok(vec![tool(
            windows_tool("reg.exe"),
            &["query", VISUAL_FX_KEY, "/v", "VisualFXSetting"],
        )]),
        (TuningHost::Windows, A::SetBackgroundAppsDisabled(_)) => Ok(vec![tool(
            windows_tool("reg.exe"),
            &["query", BACKGROUND_KEY, "/v", "GlobalUserDisabled"],
        )]),
        (TuningHost::Windows, A::SetIndexingExcluded { paths, .. }) => {
            Ok(paths.iter().map(|p| tool(windows_tool("attrib.exe"), &[p, "/D"])).collect())
        }
        (TuningHost::Linux, A::SetPowerProfile(_)) => Ok(vec![tool("/usr/bin/powerprofilesctl", &["get"])]),
        (TuningHost::Macos, A::SetSleepMinutes { .. }) => Ok(vec![tool("/usr/bin/pmset", &["-g", "custom"])]),
        (TuningHost::Macos, A::SetLowPowerMode(_)) => Ok(vec![tool("/usr/bin/pmset", &["-g"])]),
        (TuningHost::Linux, A::SetChargeLimit(_)) | (TuningHost::Macos, A::SetIndexingExcluded { .. }) => Ok(vec![]),
        (h, a) => Err(unsupported(&a.preview().join("; "), h)),
    }
}

/// The GUID in `powercfg /getactivescheme`'s answer.
pub fn parse_active_scheme(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || c == ':' || c == '(' || c == ')')
        .find(|t| t.len() == 36 && t.matches('-').count() == 4 && t.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))
        .map(str::to_lowercase)
}

/// The hexadecimal DWORD `reg query` shows for a value.
pub fn parse_reg_dword(text: &str) -> Option<u32> {
    text.lines().find(|l| l.contains("REG_DWORD")).and_then(|l| {
        let v = l.split_whitespace().last()?;
        u32::from_str_radix(v.trim_start_matches("0x"), 16).ok()
    })
}

/// Minutes of idling before sleep from `powercfg /query ... STANDBYIDLE` (seconds, 0 = never).
pub fn parse_powercfg_sleep(text: &str, source: SleepSource) -> Option<u32> {
    let label = if source == SleepSource::Ac {
        "Current AC Power Setting Index"
    } else {
        "Current DC Power Setting Index"
    };
    let line = text.lines().find(|l| l.contains(label))?;
    let hex = line.rsplit(':').next()?.trim().trim_start_matches("0x");
    u32::from_str_radix(hex, 16).ok().map(|secs| secs.div_ceil(60))
}

/// Minutes before the computer sleeps from `pmset -g custom` (0 = never).
pub fn parse_pmset_sleep(text: &str, source: SleepSource) -> Option<u32> {
    let section = if source == SleepSource::Ac { "AC Power:" } else { "Battery Power:" };
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim();
        if t.ends_with(':') && !t.contains(' ') || t.ends_with("Power:") {
            inside = t == section;
        } else if inside {
            let mut parts = t.split_whitespace();
            if parts.next() == Some("sleep") {
                return parts.next()?.parse().ok();
            }
        }
    }
    None
}

/// Whether `pmset -g` reports Low Power Mode on.
pub fn parse_pmset_low_power(text: &str) -> Option<bool> {
    let line = text.lines().find(|l| l.trim_start().starts_with("lowpowermode"))?;
    Some(line.split_whitespace().nth(1)? == "1")
}

/// Whether `attrib` lists the "not content indexed" flag for the folder it was asked about.
pub fn parse_attrib_not_indexed(text: &str, path: &str) -> bool {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let flags = match line.to_lowercase().find(&path.to_lowercase()) {
        Some(at) => &line[..at],
        None => line.get(..line.len().min(12)).unwrap_or(""),
    };
    flags.contains('I')
}

/// The real adapter for `host`.
pub struct OsTuner<R: ToolRunner> {
    host: TuningHost,
    runner: R,
    /// `/sys/class/power_supply` on Linux; a temp folder in tests.
    power_supply: PathBuf,
}

/// The tuner for the OS Dyno runs on.
pub fn os_tuner() -> OsTuner<SystemRunner> {
    let host = if cfg!(windows) {
        TuningHost::Windows
    } else if cfg!(target_os = "macos") {
        TuningHost::Macos
    } else {
        TuningHost::Linux
    };
    OsTuner::new(host, SystemRunner, PathBuf::from("/sys/class/power_supply"))
}

impl<R: ToolRunner> OsTuner<R> {
    /// A tuner for `host` running tools with `runner` and reading battery files below `power_supply`.
    pub fn new(host: TuningHost, runner: R, power_supply: PathBuf) -> Self {
        OsTuner {
            host,
            runner,
            power_supply,
        }
    }

    /// The system this tuner changes.
    pub fn host_kind(&self) -> TuningHost {
        self.host
    }

    /// Runs one tool and returns what it answered, whether or not it succeeded.
    pub(crate) fn run_tool(&mut self, t: &Tool) -> Result<ToolOutput, PlatformError> {
        self.runner.run(t)
    }

    fn charge_file(&self) -> Option<PathBuf> {
        let dir = std::fs::read_dir(&self.power_supply).ok()?;
        let mut names: Vec<PathBuf> = dir.filter_map(Result::ok).map(|e| e.path()).collect();
        names.sort();
        names
            .into_iter()
            .map(|p| p.join("charge_control_end_threshold"))
            .find(|f| f.is_file())
    }

    pub(crate) fn run_ok(&mut self, t: &Tool) -> Result<String, PlatformError> {
        let out = self.runner.run(t)?;
        if out.ok {
            Ok(out.stdout)
        } else {
            let name = Path::new(&t.program)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            Err(PlatformError::Failed(format!(
                "{name} failed: {}",
                if out.error.is_empty() { "no message" } else { &out.error }
            )))
        }
    }

    fn marker_paths(paths: &[String]) -> Result<Vec<PathBuf>, PlatformError> {
        paths
            .iter()
            .map(|p| {
                std::fs::canonicalize(p)
                    .ok()
                    .filter(|c| c.is_dir())
                    .ok_or_else(|| PlatformError::NotFound(format!("{p} is not a folder on this computer.")))
            })
            .collect()
    }

    fn set_markers(dirs: &[PathBuf], excluded: bool) -> Result<(), PlatformError> {
        let mut done: Vec<&PathBuf> = Vec::new();
        for d in dirs {
            let marker = d.join(SPOTLIGHT_MARKER);
            let r = if excluded {
                std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(&marker)
                    .map(|_| ())
            } else {
                match std::fs::remove_file(&marker) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    other => other,
                }
            };
            match r {
                Ok(()) => done.push(d),
                Err(e) => {
                    // Leave nothing half done: put back what this call changed.
                    for d in done {
                        let m = d.join(SPOTLIGHT_MARKER);
                        if excluded {
                            let _ = std::fs::remove_file(m);
                        } else {
                            let _ = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(m);
                        }
                    }
                    return Err(if e.kind() == std::io::ErrorKind::PermissionDenied {
                        PlatformError::AccessDenied(format!("Dyno is not allowed to change {}.", d.display()))
                    } else {
                        PlatformError::Failed(format!("Could not change {}: {e}", d.display()))
                    });
                }
            }
        }
        Ok(())
    }
}

impl<R: ToolRunner> TuningSystem for OsTuner<R> {
    fn host(&self) -> TuningHost {
        self.host
    }

    fn read(&mut self, action: &HelperAction) -> Result<String, PlatformError> {
        use HelperAction as A;
        action.validate().map_err(PlatformError::Failed)?;
        let tools = read_tools(self.host, action)?;
        let missing = |what: &str| PlatformError::Failed(format!("Could not read {what}."));
        match action {
            A::SetPowerScheme(_) => {
                let text = self.run_ok(&tools[0])?;
                parse_active_scheme(&text).ok_or_else(|| missing("the active power plan"))
            }
            A::SetSleepMinutes { source, .. } => {
                let text = self.run_ok(&tools[0])?;
                let m = if self.host == TuningHost::Windows {
                    parse_powercfg_sleep(&text, *source)
                } else {
                    parse_pmset_sleep(&text, *source)
                };
                m.map(|m| m.to_string()).ok_or_else(|| missing("the sleep timer"))
            }
            A::SetHibernate(_) => {
                let out = self.runner.run(&tools[0])?;
                // Windows has no value until hibernation was ever configured; that is off.
                let on = out.ok && parse_reg_dword(&out.stdout).is_some_and(|v| v != 0);
                Ok(if on { "on" } else { "off" }.into())
            }
            A::SetVisualEffects(_) => {
                let out = self.runner.run(&tools[0])?;
                // Windows shows "let Windows choose" (0) until the setting is written.
                Ok(parse_reg_dword(&out.stdout).filter(|_| out.ok).unwrap_or(0).to_string())
            }
            A::SetBackgroundAppsDisabled(_) => {
                let out = self.runner.run(&tools[0])?;
                Ok(parse_reg_dword(&out.stdout).filter(|_| out.ok).unwrap_or(0).min(1).to_string())
            }
            A::SetIndexingExcluded { paths, .. } if self.host == TuningHost::Windows => {
                let mut states = Vec::new();
                for (p, t) in paths.iter().zip(&tools) {
                    let text = self.run_ok(t)?;
                    states.push(if parse_attrib_not_indexed(&text, p) {
                        "excluded"
                    } else {
                        "included"
                    });
                }
                Ok(states.join(","))
            }
            A::SetIndexingExcluded { paths, .. } => {
                let dirs = Self::marker_paths(paths)?;
                Ok(dirs
                    .iter()
                    .map(|d| {
                        if d.join(SPOTLIGHT_MARKER).exists() {
                            "excluded"
                        } else {
                            "included"
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(","))
            }
            A::SetPowerProfile(_) => {
                let text = self.run_ok(&tools[0])?;
                dyno_core::helper::PowerProfile::from_tool_name(&text)
                    .map(|p| p.tool_name().to_string())
                    .ok_or_else(|| missing("the power profile"))
            }
            A::SetLowPowerMode(_) => {
                let text = self.run_ok(&tools[0])?;
                parse_pmset_low_power(&text)
                    .map(|b| if b { "on" } else { "off" }.to_string())
                    .ok_or_else(|| missing("Low Power Mode"))
            }
            A::SetChargeLimit(_) => {
                let file = self
                    .charge_file()
                    .ok_or_else(|| PlatformError::Unsupported("This battery has no charge limit setting.".into()))?;
                let text =
                    std::fs::read_to_string(&file).map_err(|e| PlatformError::Failed(format!("Could not read the charge limit: {e}")))?;
                text.trim()
                    .parse::<u8>()
                    .map(|v| v.to_string())
                    .map_err(|_| missing("the charge limit"))
            }
            // change_tools/read_tools already refused the rest.
            _ => Err(unsupported(&action.preview().join("; "), self.host)),
        }
    }

    fn apply(&mut self, action: &HelperAction) -> Result<(), PlatformError> {
        use HelperAction as A;
        action.validate().map_err(PlatformError::Failed)?;
        let tools = change_tools(self.host, action)?;
        match action {
            A::SetIndexingExcluded { paths, excluded } if self.host == TuningHost::Macos => {
                let dirs = Self::marker_paths(paths)?;
                Self::set_markers(&dirs, *excluded)
            }
            A::SetChargeLimit(p) => {
                let file = self
                    .charge_file()
                    .ok_or_else(|| PlatformError::Unsupported("This battery has no charge limit setting.".into()))?;
                std::fs::write(&file, p.to_string()).map_err(|e| match e.kind() {
                    std::io::ErrorKind::PermissionDenied => denied("Changing the charge limit"),
                    _ => PlatformError::Failed(format!("Could not set the charge limit: {e}")),
                })
            }
            _ => {
                for (i, t) in tools.iter().enumerate() {
                    if let Err(e) = self.run_ok(t) {
                        // A multi-folder change must not stop half way: undo the folders already done.
                        if let A::SetIndexingExcluded { paths, excluded } = action {
                            for (p, _) in paths.iter().zip(&tools).take(i) {
                                let back = A::SetIndexingExcluded {
                                    paths: vec![p.clone()],
                                    excluded: !excluded,
                                };
                                if let Ok(undo) = change_tools(self.host, &back) {
                                    for u in undo {
                                        let _ = self.run_ok(&u);
                                    }
                                }
                            }
                        }
                        return Err(e);
                    }
                }
                Ok(())
            }
        }
    }
}

/// A scripted system for tests: it holds the value of each setting, applies changes to it, and can be told to fail.
#[derive(Clone)]
pub struct FakeTuning {
    inner: Arc<Mutex<FakeState>>,
    host: TuningHost,
}

#[derive(Default)]
struct FakeState {
    values: HashMap<String, String>,
    applied: Vec<HelperAction>,
    fail: Option<PlatformError>,
}

impl FakeTuning {
    /// A fake for `host` with no settings readable yet.
    pub fn new(host: TuningHost) -> Self {
        FakeTuning {
            inner: Arc::default(),
            host,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Makes the setting `action` changes read as `action`'s own value (the setting as that action would leave it).
    pub fn set(&self, action: &HelperAction) -> &Self {
        let (k, v) = action.setting();
        self.state().values.insert(k, v);
        self
    }

    /// Makes the setting named like `action` read as `value` (for example the sleep timer is 45).
    pub fn set_value(&self, action: &HelperAction, value: &str) -> &Self {
        self.state().values.insert(action.setting().0, value.into());
        self
    }

    /// Every change applied so far, oldest first.
    pub fn applied(&self) -> Vec<HelperAction> {
        self.state().applied.clone()
    }

    /// The next `apply` fails with this error.
    pub fn fail_next(&self, e: PlatformError) {
        self.state().fail = Some(e);
    }
}

impl TuningSystem for FakeTuning {
    fn host(&self) -> TuningHost {
        self.host
    }

    fn read(&mut self, action: &HelperAction) -> Result<String, PlatformError> {
        self.state()
            .values
            .get(&action.setting().0)
            .cloned()
            .ok_or_else(|| PlatformError::Unsupported("That setting cannot be read here.".into()))
    }

    fn apply(&mut self, action: &HelperAction) -> Result<(), PlatformError> {
        let mut s = self.state();
        if let Some(e) = s.fail.take() {
            return Err(e);
        }
        action.validate().map_err(PlatformError::Failed)?;
        let (k, v) = action.setting();
        s.values.insert(k, v);
        s.applied.push(action.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::helper::{PowerProfile, BALANCED_SCHEME};
    use std::collections::VecDeque;

    /// Answers tools from a queue and records what ran.
    #[derive(Default)]
    struct Script {
        answers: VecDeque<Result<ToolOutput, PlatformError>>,
        ran: Arc<Mutex<Vec<Tool>>>,
    }

    impl Script {
        fn say(mut self, stdout: &str) -> Self {
            self.answers.push_back(Ok(ToolOutput {
                ok: true,
                stdout: stdout.into(),
                error: String::new(),
            }));
            self
        }
        fn fail(mut self, error: &str) -> Self {
            self.answers.push_back(Ok(ToolOutput {
                ok: false,
                stdout: String::new(),
                error: error.into(),
            }));
            self
        }
    }

    impl ToolRunner for Script {
        fn run(&mut self, t: &Tool) -> Result<ToolOutput, PlatformError> {
            self.ran.lock().unwrap().push(t.clone());
            self.answers.pop_front().unwrap_or_else(|| {
                Ok(ToolOutput {
                    ok: true,
                    stdout: String::new(),
                    error: String::new(),
                })
            })
        }
    }

    fn tuner(host: TuningHost, script: Script) -> OsTuner<Script> {
        OsTuner::new(host, script, PathBuf::from("/nonexistent-dyno-test"))
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dyno-it-tuning-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn parsers_read_tool_output() {
        let scheme = "Power Scheme GUID: 381B4222-F694-41F0-9685-FF5BB260DF2E  (Balanced)";
        assert_eq!(parse_active_scheme(scheme).as_deref(), Some(BALANCED_SCHEME));
        assert_eq!(parse_active_scheme("nothing"), None);
        assert_eq!(parse_reg_dword("    HibernateEnabled    REG_DWORD    0x1\n"), Some(1));
        assert_eq!(parse_reg_dword("    X    REG_DWORD    0x00000002"), Some(2));
        assert_eq!(parse_reg_dword("ERROR"), None);
        let q = "  Current AC Power Setting Index: 0x00000000\n  Current DC Power Setting Index: 0x00000384\n";
        assert_eq!(parse_powercfg_sleep(q, SleepSource::Ac), Some(0));
        assert_eq!(parse_powercfg_sleep(q, SleepSource::Battery), Some(15));
        assert_eq!(parse_powercfg_sleep("", SleepSource::Ac), None);
        let pm =
            "Battery Power:\n sleep                10\n lowpowermode         1\nAC Power:\n sleep                0\n displaysleep 10\n";
        assert_eq!(parse_pmset_sleep(pm, SleepSource::Battery), Some(10));
        assert_eq!(parse_pmset_sleep(pm, SleepSource::Ac), Some(0));
        assert_eq!(parse_pmset_sleep("AC Power:\n x 1\n", SleepSource::Ac), None);
        assert_eq!(parse_pmset_low_power(" lowpowermode         1\n"), Some(true));
        assert_eq!(parse_pmset_low_power(" lowpowermode 0"), Some(false));
        assert_eq!(parse_pmset_low_power("nothing"), None);
        assert!(parse_attrib_not_indexed("A    I      C:\\src\\app\n", "C:\\src\\app"));
        assert!(!parse_attrib_not_indexed("A           C:\\src\\app\n", "C:\\src\\app"));
        assert!(!parse_attrib_not_indexed("", "C:\\x"));
    }

    #[test]
    fn windows_changes_are_fixed_programs_with_validated_arguments() {
        let ran = Arc::default();
        let mut t = tuner(
            TuningHost::Windows,
            Script {
                ran: Arc::clone(&ran),
                ..Script::default()
            },
        );
        t.apply(&HelperAction::SetPowerScheme(BALANCED_SCHEME.into())).unwrap();
        t.apply(&HelperAction::SetSleepMinutes {
            source: SleepSource::Battery,
            minutes: 15,
        })
        .unwrap();
        t.apply(&HelperAction::SetVisualEffects(2)).unwrap();
        t.apply(&HelperAction::SetBackgroundAppsDisabled(true)).unwrap();
        t.apply(&HelperAction::SetIndexingExcluded {
            paths: vec!["C:\\src\\app".into()],
            excluded: true,
        })
        .unwrap();
        let ran = ran.lock().unwrap();
        assert!(ran[0].program.ends_with("powercfg.exe") && ran[0].args == ["/setactive", BALANCED_SCHEME]);
        assert_eq!(ran[1].args, ["/change", "standby-timeout-dc", "15"]);
        assert!(ran[2].args.contains(&"VisualFXSetting".to_string()) && ran[2].args.contains(&"2".to_string()));
        assert!(ran[3].args.contains(&"GlobalUserDisabled".to_string()));
        assert_eq!(ran[4].args, ["+I", "C:\\src\\app", "/D"]);
    }

    #[test]
    fn invalid_arguments_never_reach_a_tool() {
        let ran = Arc::default();
        let mut t = tuner(
            TuningHost::Windows,
            Script {
                ran: Arc::clone(&ran),
                ..Script::default()
            },
        );
        assert!(t.apply(&HelperAction::SetPowerScheme("x; calc".into())).is_err());
        assert!(t
            .apply(&HelperAction::SetIndexingExcluded {
                paths: vec!["C:\\".into()],
                excluded: true
            })
            .is_err());
        assert!(t.read(&HelperAction::SetChargeLimit(10)).is_err());
        assert!(ran.lock().unwrap().is_empty());
    }

    #[test]
    fn administrator_changes_are_refused_not_attempted() {
        let ran = Arc::default();
        let mut win = tuner(
            TuningHost::Windows,
            Script {
                ran: Arc::clone(&ran),
                ..Script::default()
            },
        );
        for a in [
            HelperAction::SetHibernate(false),
            HelperAction::SetDefenderExcluded {
                paths: vec!["C:\\src\\app".into()],
                excluded: true,
            },
        ] {
            assert!(matches!(win.apply(&a), Err(PlatformError::AccessDenied(m)) if m.contains("administrator rights")));
        }
        let mut mac = tuner(
            TuningHost::Macos,
            Script {
                ran: Arc::clone(&ran),
                ..Script::default()
            },
        );
        for a in [
            HelperAction::SetLowPowerMode(false),
            HelperAction::SetSleepMinutes {
                source: SleepSource::Ac,
                minutes: 5,
            },
        ] {
            assert!(matches!(mac.apply(&a), Err(PlatformError::AccessDenied(_))));
        }
        let mut lin = tuner(TuningHost::Linux, Script::default());
        assert!(matches!(
            lin.apply(&HelperAction::SetPowerScheme(BALANCED_SCHEME.into())),
            Err(PlatformError::Unsupported(_))
        ));
        assert!(matches!(
            lin.read(&HelperAction::SetLowPowerMode(true)),
            Err(PlatformError::Unsupported(_))
        ));
        assert!(ran.lock().unwrap().is_empty(), "nothing was run");
    }

    #[test]
    fn reads_follow_the_tool_answers() {
        let mut win = tuner(
            TuningHost::Windows,
            Script::default()
                .say("Power Scheme GUID: a1841308-3541-4fab-bc81-f71556f20b4a  (Power saver)")
                .say("  Current DC Power Setting Index: 0x00000000\n")
                .say("    HibernateEnabled    REG_DWORD    0x1")
                .fail("ERROR: The system was unable to find the specified registry key or value.")
                .fail("missing")
                .say("A    I      C:\\a\n")
                .say("A           C:\\b\n"),
        );
        assert_eq!(
            win.read(&HelperAction::SetPowerScheme(BALANCED_SCHEME.into())).unwrap(),
            "a1841308-3541-4fab-bc81-f71556f20b4a"
        );
        assert_eq!(
            win.read(&HelperAction::SetSleepMinutes {
                source: SleepSource::Battery,
                minutes: 1
            })
            .unwrap(),
            "0"
        );
        assert_eq!(win.read(&HelperAction::SetHibernate(false)).unwrap(), "on");
        assert_eq!(
            win.read(&HelperAction::SetVisualEffects(2)).unwrap(),
            "0",
            "unset means let Windows choose"
        );
        assert_eq!(win.read(&HelperAction::SetBackgroundAppsDisabled(true)).unwrap(), "0");
        let idx = HelperAction::SetIndexingExcluded {
            paths: vec!["C:\\a".into(), "C:\\b".into()],
            excluded: true,
        };
        assert_eq!(win.read(&idx).unwrap(), "excluded,included");
        let mut broken = tuner(TuningHost::Windows, Script::default().fail("boom"));
        let err = broken.read(&HelperAction::SetPowerScheme(BALANCED_SCHEME.into())).unwrap_err();
        assert!(err.to_string().contains("boom"), "{err}");
        let mut junk = tuner(TuningHost::Windows, Script::default().say("junk"));
        assert!(junk.read(&HelperAction::SetPowerScheme(BALANCED_SCHEME.into())).is_err());
    }

    #[test]
    fn linux_profile_and_mac_power_settings() {
        let mut lin = tuner(TuningHost::Linux, Script::default().say("balanced\n").say("what"));
        assert_eq!(
            lin.read(&HelperAction::SetPowerProfile(PowerProfile::PowerSaver)).unwrap(),
            "balanced"
        );
        assert!(lin.read(&HelperAction::SetPowerProfile(PowerProfile::PowerSaver)).is_err());
        let ran = Arc::default();
        let mut lin = tuner(
            TuningHost::Linux,
            Script {
                ran: Arc::clone(&ran),
                ..Script::default()
            },
        );
        lin.apply(&HelperAction::SetPowerProfile(PowerProfile::Performance)).unwrap();
        assert_eq!(ran.lock().unwrap()[0].args, ["set", "performance"]);
        let mut mac = tuner(
            TuningHost::Macos,
            Script::default()
                .say("Battery Power:\n sleep 5\nAC Power:\n sleep 0\n")
                .say(" lowpowermode 1\n"),
        );
        assert_eq!(
            mac.read(&HelperAction::SetSleepMinutes {
                source: SleepSource::Battery,
                minutes: 0
            })
            .unwrap(),
            "5"
        );
        assert_eq!(mac.read(&HelperAction::SetLowPowerMode(false)).unwrap(), "on");
    }

    #[test]
    fn spotlight_markers_are_made_and_removed_and_a_failure_leaves_nothing_half_done() {
        let root = temp("markers");
        let (a, b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let paths = vec![a.to_string_lossy().into_owned(), b.to_string_lossy().into_owned()];
        let mut mac = tuner(TuningHost::Macos, Script::default());
        let excl = HelperAction::SetIndexingExcluded {
            paths: paths.clone(),
            excluded: true,
        };
        assert_eq!(mac.read(&excl).unwrap(), "included,included");
        mac.apply(&excl).unwrap();
        assert!(a.join(SPOTLIGHT_MARKER).exists() && b.join(SPOTLIGHT_MARKER).exists());
        assert_eq!(mac.read(&excl).unwrap(), "excluded,excluded");
        mac.apply(&HelperAction::SetIndexingExcluded {
            paths: paths.clone(),
            excluded: false,
        })
        .unwrap();
        assert!(!a.join(SPOTLIGHT_MARKER).exists());
        // A path that is not a folder stops the change before anything is touched.
        let bad = HelperAction::SetIndexingExcluded {
            paths: vec![paths[0].clone(), root.join("gone").to_string_lossy().into_owned()],
            excluded: true,
        };
        assert!(matches!(mac.apply(&bad), Err(PlatformError::NotFound(_))));
        assert!(!a.join(SPOTLIGHT_MARKER).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn a_marker_that_cannot_be_written_is_reported_and_rolled_back() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp("rollback");
        let (a, b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::set_permissions(&b, std::fs::Permissions::from_mode(0o555)).unwrap();
        let denied_here = std::fs::File::create(b.join("probe")).is_err();
        let paths = vec![a.to_string_lossy().into_owned(), b.to_string_lossy().into_owned()];
        let mut mac = tuner(TuningHost::Macos, Script::default());
        let r = mac.apply(&HelperAction::SetIndexingExcluded { paths, excluded: true });
        std::fs::set_permissions(&b, std::fs::Permissions::from_mode(0o755)).unwrap();
        if denied_here {
            assert!(matches!(r, Err(PlatformError::AccessDenied(_))));
            assert!(!a.join(SPOTLIGHT_MARKER).exists(), "the folder done first was put back");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_charge_limit_file_is_read_and_a_refusal_means_administrator_rights() {
        let root = temp("charge");
        let bat = root.join("BAT0");
        std::fs::create_dir_all(&bat).unwrap();
        let mut none = OsTuner::new(TuningHost::Linux, Script::default(), root.join("missing"));
        assert!(matches!(
            none.read(&HelperAction::SetChargeLimit(80)),
            Err(PlatformError::Unsupported(_))
        ));
        assert!(matches!(
            none.apply(&HelperAction::SetChargeLimit(80)),
            Err(PlatformError::Unsupported(_))
        ));
        std::fs::write(bat.join("charge_control_end_threshold"), "100\n").unwrap();
        let mut t = OsTuner::new(TuningHost::Linux, Script::default(), root.clone());
        assert_eq!(t.read(&HelperAction::SetChargeLimit(80)).unwrap(), "100");
        t.apply(&HelperAction::SetChargeLimit(80)).unwrap();
        assert_eq!(std::fs::read_to_string(bat.join("charge_control_end_threshold")).unwrap(), "80");
        std::fs::write(bat.join("charge_control_end_threshold"), "junk").unwrap();
        assert!(t.read(&HelperAction::SetChargeLimit(80)).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failing_tool_is_an_error_with_its_message_and_rolls_back_earlier_folders() {
        let ran = Arc::default();
        let script = Script {
            ran: Arc::clone(&ran),
            ..Script::default()
        }
        .say("")
        .fail("Access is denied.");
        let mut win = tuner(TuningHost::Windows, script);
        let a = HelperAction::SetIndexingExcluded {
            paths: vec!["C:\\src\\one".into(), "C:\\src\\two".into()],
            excluded: true,
        };
        let err = win.apply(&a).unwrap_err();
        assert!(err.to_string().contains("Access is denied."), "{err}");
        let ran = ran.lock().unwrap();
        assert_eq!(ran.len(), 3, "two attempts and one rollback");
        assert_eq!(ran[2].args, ["-I", "C:\\src\\one", "/D"]);
    }

    #[test]
    fn the_real_runner_runs_a_program_and_reports_a_missing_one() {
        let mut r = SystemRunner;
        let (prog, args): (&str, &[&str]) = if cfg!(windows) {
            ("cmd.exe", &["/d", "/c", "echo hi"])
        } else {
            ("/bin/echo", &["hi"])
        };
        let out = r.run(&tool(prog, args)).unwrap();
        assert!(out.ok && out.stdout.contains("hi"));
        assert!(matches!(
            r.run(&tool("/nonexistent/dyno-test-tool", &[])),
            Err(PlatformError::Unsupported(_))
        ));
        let t = os_tuner();
        assert!(matches!(t.host(), TuningHost::Windows | TuningHost::Macos | TuningHost::Linux));
    }

    #[test]
    fn the_fake_applies_to_its_own_state() {
        let fake = FakeTuning::new(TuningHost::Windows);
        let sleep = |m| HelperAction::SetSleepMinutes {
            source: SleepSource::Ac,
            minutes: m,
        };
        let mut sys = fake.clone();
        assert!(sys.read(&sleep(0)).is_err());
        fake.set_value(&sleep(0), "45");
        assert_eq!(sys.read(&sleep(0)).unwrap(), "45");
        sys.apply(&sleep(10)).unwrap();
        assert_eq!(sys.read(&sleep(0)).unwrap(), "10");
        fake.fail_next(PlatformError::Failed("no".into()));
        assert!(sys.apply(&sleep(5)).is_err());
        assert!(sys.apply(&sleep(5000)).is_err());
        assert_eq!(fake.applied(), [sleep(10)]);
        fake.set(&HelperAction::SetHibernate(true));
        assert_eq!(sys.read(&HelperAction::SetHibernate(false)).unwrap(), "on");
        assert_eq!(sys.host(), TuningHost::Windows);
    }
}
