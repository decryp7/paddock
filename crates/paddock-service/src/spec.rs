//! What an app's background process is, declared once: [`ServiceSpec`]. Every OS file (the launchd plist, the systemd
//! unit, the Windows `Run` value and service, the NSIS hooks) is rendered from it by [`crate::render`], so the files an
//! app ships cannot drift from what the app registers at run time. Built through [`ServiceSpec::builder`], which refuses
//! names and arguments that would break a file or a command line (parse, don't validate).

use crate::error::SpecError;
use crate::hardening::Hardening;

/// Who the process runs as (decision D4: a per-user agent unless the work must run without a logged-on user).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The user's own background process: a launch agent (macOS), a systemd user unit (Linux), the per-user `Run` entry
    /// (Windows). No administrator rights anywhere.
    UserAgent,
    /// A machine-wide service: a launch daemon, a systemd system unit, a Windows SCM service (LocalSystem). Registering
    /// it needs the OS's own elevation (the installer or Windows' prompt); Paddock never handles a password.
    SystemService,
}

/// How eagerly the OS schedules the process (launchd `ProcessType`; systemd and Windows follow [`ServiceSpec::low_priority`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessType {
    /// Throttled when the user is busy: measuring, indexing, housekeeping.
    Background,
    /// The default.
    Standard,
    /// Answers the user directly (a local server the apps wait for).
    Interactive,
}

impl ProcessType {
    /// The launchd value.
    pub fn launchd(self) -> &'static str {
        match self {
            ProcessType::Background => "Background",
            ProcessType::Standard => "Standard",
            ProcessType::Interactive => "Interactive",
        }
    }
}

/// When the OS starts the process again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restart {
    /// Whenever it exits, which is also how it picks up a new version (it exits itself after an update, see
    /// [`crate::watch`]).
    Always,
    /// Only after a failure; a clean exit stays stopped.
    OnFailure,
}

/// The longest label, bundle id or name part.
pub const MAX_NAME: usize = 128;
/// The longest display name or description.
pub const MAX_TEXT: usize = 256;
/// The longest argument.
pub const MAX_ARG: usize = 1024;
/// The most arguments.
pub const MAX_ARGS: usize = 32;

/// An app's background process, validated. See the crate documentation for an example.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceSpec {
    label: String,
    name: String,
    unit: String,
    display_name: String,
    description: String,
    bundle_id: String,
    program: String,
    args: Vec<String>,
    kind: Kind,
    process_type: ProcessType,
    restart: Restart,
    throttle_secs: u32,
    supervised_env: Option<String>,
    low_priority: bool,
    memory_high_mb: Option<u32>,
    docs_url: Option<String>,
    hardening: Option<Hardening>,
    windows_start: WindowsStart,
}

impl ServiceSpec {
    /// Starts a spec: `label` is the launchd label (`net.decryptology.pitwall.service`), `name` the app's name as users
    /// see it in Services, Task Manager and messages (`Pitwall`), `program` the executable's file name without `.exe`
    /// (`pitwall-server`).
    pub fn builder(label: &str, name: &str, program: &str) -> SpecBuilder {
        SpecBuilder {
            spec: ServiceSpec {
                label: label.to_string(),
                name: name.to_string(),
                unit: name.to_ascii_lowercase(),
                display_name: name.to_string(),
                description: String::new(),
                bundle_id: String::new(),
                program: program.to_string(),
                args: Vec::new(),
                kind: Kind::UserAgent,
                process_type: ProcessType::Standard,
                restart: Restart::Always,
                throttle_secs: 10,
                supervised_env: None,
                low_priority: false,
                memory_high_mb: None,
                docs_url: None,
                hardening: None,
                windows_start: WindowsStart::Delayed,
            },
        }
    }

    /// The launchd label.
    pub fn label(&self) -> &str {
        &self.label
    }
    /// The app's name (Windows service name and `Run` value name).
    pub fn name(&self) -> &str {
        &self.name
    }
    /// The systemd unit's file name (`pitwall.service`).
    pub fn unit_file_name(&self) -> String {
        format!("{}.service", self.unit)
    }
    /// The longer name the OS shows (Windows Services, systemd `Description=`).
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
    /// One sentence about what it does.
    pub fn description(&self) -> &str {
        &self.description
    }
    /// The app bundle's identifier (macOS `AssociatedBundleIdentifiers`); empty when the process ships without an app.
    pub fn bundle_id(&self) -> &str {
        &self.bundle_id
    }
    /// The executable's file name without an extension.
    pub fn program(&self) -> &str {
        &self.program
    }
    /// The executable's file name on `windows` or elsewhere.
    pub fn program_file(&self, windows: bool) -> String {
        if windows {
            format!("{}.exe", self.program)
        } else {
            self.program.clone()
        }
    }
    /// The arguments the OS starts it with (a Windows SCM service gets `--service` instead, see [`crate::verbs`]).
    pub fn args(&self) -> &[String] {
        &self.args
    }
    /// Who it runs as.
    pub fn kind(&self) -> Kind {
        self.kind
    }
    /// The launchd process type.
    pub fn process_type(&self) -> ProcessType {
        self.process_type
    }
    /// When it is started again.
    pub fn restart(&self) -> Restart {
        self.restart
    }
    /// The shortest time between two starts, in seconds (a crash loop cannot spin).
    pub fn throttle_secs(&self) -> u32 {
        self.throttle_secs
    }
    /// The variable set to `1` in the process's environment so it knows it is supervised (it then merges the login
    /// `PATH` and watches its own executable).
    pub fn supervised_env(&self) -> Option<&str> {
        self.supervised_env.as_deref()
    }
    /// Lower CPU and I/O priority.
    pub fn low_priority(&self) -> bool {
        self.low_priority
    }
    /// The soft memory limit (systemd `MemoryHigh=`), in MiB.
    pub fn memory_high_mb(&self) -> Option<u32> {
        self.memory_high_mb
    }
    /// The documentation link (systemd `Documentation=`).
    pub fn docs_url(&self) -> Option<&str> {
        self.docs_url.as_deref()
    }
    /// What a privileged service gives up (Windows token privileges, who may start it, the systemd sandbox).
    pub fn hardening(&self) -> Option<&Hardening> {
        self.hardening.as_ref()
    }
    /// When the Service Control Manager starts a Windows service after boot.
    pub fn windows_start(&self) -> WindowsStart {
        self.windows_start
    }
}

/// When Windows starts an automatic service after boot (`sc create start=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowsStart {
    /// `delayed-auto`: a minute or two after the boot, when the load of the start-up has eased (the default).
    #[default]
    Delayed,
    /// `auto`: with the boot itself, for a service people wait for at log-in (a helper the app expects).
    Auto,
}

impl WindowsStart {
    /// The word `sc.exe` takes.
    pub fn sc_word(self) -> &'static str {
        match self {
            WindowsStart::Delayed => "delayed-auto",
            WindowsStart::Auto => "auto",
        }
    }
}

/// Builds a [`ServiceSpec`]; [`SpecBuilder::build`] checks every part.
#[derive(Debug, Clone)]
#[must_use]
pub struct SpecBuilder {
    spec: ServiceSpec,
}

impl SpecBuilder {
    /// The systemd unit's name without `.service` (default: the name in lower case).
    pub fn unit(mut self, unit: &str) -> Self {
        self.spec.unit = unit.to_string();
        self
    }
    /// The longer name the OS shows (default: the name).
    pub fn display_name(mut self, text: &str) -> Self {
        self.spec.display_name = text.to_string();
        self
    }
    /// One sentence about what it does.
    pub fn description(mut self, text: &str) -> Self {
        self.spec.description = text.to_string();
        self
    }
    /// The app bundle's identifier (`net.decryptology.pitwall`).
    pub fn bundle_id(mut self, id: &str) -> Self {
        self.spec.bundle_id = id.to_string();
        self
    }
    /// The arguments the OS starts it with (`run`).
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.spec.args = args.into_iter().map(Into::into).collect();
        self
    }
    /// Who it runs as (default: [`Kind::UserAgent`]).
    pub fn kind(mut self, kind: Kind) -> Self {
        self.spec.kind = kind;
        self
    }
    /// The launchd process type (default: [`ProcessType::Standard`]).
    pub fn process_type(mut self, t: ProcessType) -> Self {
        self.spec.process_type = t;
        self
    }
    /// When it is started again (default: [`Restart::Always`]).
    pub fn restart(mut self, r: Restart) -> Self {
        self.spec.restart = r;
        self
    }
    /// The shortest time between two starts (default 10 s; 1 to 3600).
    pub fn throttle_secs(mut self, secs: u32) -> Self {
        self.spec.throttle_secs = secs;
        self
    }
    /// The variable set to `1` when supervised (`PITWALL_SERVICE`).
    pub fn supervised_env(mut self, var: &str) -> Self {
        self.spec.supervised_env = Some(var.to_string());
        self
    }
    /// Lower CPU and I/O priority (systemd `Nice=10`, `CPUWeight=20`, idle I/O; launchd low-priority I/O).
    pub fn low_priority(mut self) -> Self {
        self.spec.low_priority = true;
        self
    }
    /// A soft memory limit in MiB (systemd `MemoryHigh=`; 16 to 65536).
    pub fn memory_high_mb(mut self, mb: u32) -> Self {
        self.spec.memory_high_mb = Some(mb);
        self
    }
    /// The documentation link (`https://` only).
    pub fn docs_url(mut self, url: &str) -> Self {
        self.spec.docs_url = Some(url.to_string());
        self
    }

    /// What a privileged service gives up ([`Hardening`]); only for [`Kind::SystemService`].
    pub fn hardening(mut self, hardening: Hardening) -> Self {
        self.spec.hardening = Some(hardening);
        self
    }

    /// When Windows starts the service after boot ([`WindowsStart::Delayed`] unless this is called).
    pub fn windows_start(mut self, start: WindowsStart) -> Self {
        self.spec.windows_start = start;
        self
    }

    /// Checks every part and returns the spec.
    pub fn build(self) -> Result<ServiceSpec, SpecError> {
        let s = self.spec;
        reverse_dns("label", &s.label)?;
        if !s.bundle_id.is_empty() {
            reverse_dns("bundle id", &s.bundle_id)?;
        }
        simple_name("name", &s.name)?;
        simple_name("unit", &s.unit)?;
        simple_name("program", &s.program)?;
        if s.program.to_ascii_lowercase().ends_with(".exe") {
            return Err(SpecError::new(
                "program",
                "is written without .exe (it is added on Windows)",
            ));
        }
        text("display name", &s.display_name, false)?;
        text("description", &s.description, true)?;
        if s.args.len() > MAX_ARGS {
            return Err(SpecError::new(
                "args",
                format!("has more than {MAX_ARGS} entries"),
            ));
        }
        for a in &s.args {
            if a.is_empty() || a.len() > MAX_ARG || a.chars().any(char::is_control) {
                return Err(SpecError::new(
                    "args",
                    format!(
                        "an argument is empty, longer than {MAX_ARG} bytes or has a control character"
                    ),
                ));
            }
        }
        if !(1..=3600).contains(&s.throttle_secs) {
            return Err(SpecError::new("throttle", "must be 1 to 3600 seconds"));
        }
        if let Some(v) = &s.supervised_env {
            let ok = !v.is_empty()
                && v.len() <= 64
                && v.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                && !v.starts_with(|c: char| c.is_ascii_digit());
            if !ok {
                return Err(SpecError::new(
                    "supervised env",
                    "must be an upper-case variable name (A-Z, 0-9, _)",
                ));
            }
        }
        if let Some(mb) = s.memory_high_mb
            && !(16..=65536).contains(&mb)
        {
            return Err(SpecError::new("memory high", "must be 16 to 65536 MiB"));
        }
        if let Some(h) = &s.hardening {
            if s.kind != Kind::SystemService {
                return Err(SpecError::new("hardening", "is only for a system service"));
            }
            h.check()?;
        }
        if let Some(u) = &s.docs_url {
            let ok = u.starts_with("https://")
                && u.len() <= MAX_TEXT
                && !u.chars().any(|c| {
                    c.is_control() || c.is_whitespace() || c == '"' || c == '<' || c == '>'
                });
            if !ok {
                return Err(SpecError::new(
                    "docs url",
                    "must be an https:// link without spaces or quotes",
                ));
            }
        }
        Ok(s)
    }
}

fn reverse_dns(what: &'static str, v: &str) -> Result<(), SpecError> {
    let parts: Vec<&str> = v.split('.').collect();
    let ok = v.len() <= MAX_NAME
        && parts.len() >= 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && !p.starts_with('-')
                && !p.ends_with('-')
                && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    if ok {
        Ok(())
    } else {
        Err(SpecError::new(
            what,
            "must be a reverse-DNS name like net.example.app (letters, digits, '-' and dots)",
        ))
    }
}

fn simple_name(what: &'static str, v: &str) -> Result<(), SpecError> {
    let ok = !v.is_empty()
        && v.len() <= 64
        && !v.starts_with(['-', '.'])
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && v != "."
        && v != ".."
        && !v.contains("..");
    if ok {
        Ok(())
    } else {
        Err(SpecError::new(
            what,
            "must be 1 to 64 letters, digits, '-', '_' or '.' (not first)",
        ))
    }
}

fn text(what: &'static str, v: &str, empty_ok: bool) -> Result<(), SpecError> {
    if (!empty_ok && v.trim().is_empty()) || v.len() > MAX_TEXT || v.chars().any(char::is_control) {
        return Err(SpecError::new(
            what,
            format!(
                "must be one line of at most {MAX_TEXT} bytes{}",
                if empty_ok { "" } else { ", not empty" }
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> SpecBuilder {
        ServiceSpec::builder(
            "net.decryptology.pitwall.service",
            "Pitwall",
            "pitwall-server",
        )
    }

    #[test]
    fn defaults_and_accessors() {
        let s = base()
            .bundle_id("net.decryptology.pitwall")
            .display_name("Pitwall local LLM server")
            .description("Serves local models.")
            .args(["run"])
            .supervised_env("PITWALL_SERVICE")
            .low_priority()
            .memory_high_mb(256)
            .docs_url("https://dev.decryptology.net/decryp7/pitwall")
            .process_type(ProcessType::Interactive)
            .restart(Restart::OnFailure)
            .throttle_secs(5)
            .kind(Kind::SystemService)
            .build()
            .unwrap();
        assert_eq!(s.label(), "net.decryptology.pitwall.service");
        assert_eq!(s.name(), "Pitwall");
        assert_eq!(s.unit_file_name(), "pitwall.service");
        assert_eq!(s.program_file(true), "pitwall-server.exe");
        assert_eq!(s.program_file(false), "pitwall-server");
        assert_eq!(s.args(), ["run"]);
        assert_eq!(s.kind(), Kind::SystemService);
        assert_eq!(s.process_type().launchd(), "Interactive");
        assert_eq!(s.restart(), Restart::OnFailure);
        assert_eq!(s.throttle_secs(), 5);
        assert_eq!(s.supervised_env(), Some("PITWALL_SERVICE"));
        assert!(s.low_priority());
        assert_eq!(s.memory_high_mb(), Some(256));
        assert_eq!(
            s.docs_url(),
            Some("https://dev.decryptology.net/decryp7/pitwall")
        );
        assert_eq!(s.bundle_id(), "net.decryptology.pitwall");
        assert_eq!(s.display_name(), "Pitwall local LLM server");
        assert_eq!(s.description(), "Serves local models.");
        assert_eq!(ProcessType::Background.launchd(), "Background");
        assert_eq!(ProcessType::Standard.launchd(), "Standard");
        let d = base().unit("pitwall-x").build().unwrap();
        assert_eq!(d.unit_file_name(), "pitwall-x.service");
        assert_eq!(
            (d.kind(), d.restart(), d.throttle_secs()),
            (Kind::UserAgent, Restart::Always, 10)
        );
        assert_eq!(d.display_name(), "Pitwall");
    }

    #[test]
    fn names_that_would_break_a_file_or_command_line_are_refused() {
        let bad = |b: SpecBuilder| b.build().unwrap_err().to_string();
        assert!(bad(ServiceSpec::builder("pitwall", "Pitwall", "p")).contains("label"));
        assert!(bad(ServiceSpec::builder("net..x", "Pitwall", "p")).contains("label"));
        assert!(bad(ServiceSpec::builder("net.x-.y", "Pitwall", "p")).contains("label"));
        assert!(bad(ServiceSpec::builder("net.x y.z", "Pitwall", "p")).contains("label"));
        assert!(
            bad(ServiceSpec::builder(
                &format!("a.{}", "b".repeat(200)),
                "P",
                "p"
            ))
            .contains("label")
        );
        assert!(bad(base().bundle_id("nodots")).contains("bundle id"));
        assert!(bad(ServiceSpec::builder("net.a.b", "Pit wall", "p")).contains("name"));
        assert!(bad(ServiceSpec::builder("net.a.b", "", "p")).contains("name"));
        assert!(bad(ServiceSpec::builder("net.a.b", "P", "../evil")).contains("program"));
        assert!(bad(ServiceSpec::builder("net.a.b", "P", "..")).contains("program"));
        assert!(bad(ServiceSpec::builder("net.a.b", "P", "-flag")).contains("program"));
        assert!(bad(ServiceSpec::builder("net.a.b", "P", "x.EXE")).contains(".exe"));
        assert!(bad(base().unit("a/b")).contains("unit"));
        assert!(bad(base().display_name("  ")).contains("display name"));
        assert!(bad(base().description("a\nb")).contains("description"));
        assert!(bad(base().args(["a\nExecStart=/bin/evil"])).contains("args"));
        assert!(bad(base().args([""])).contains("args"));
        assert!(bad(base().args(vec!["x"; MAX_ARGS + 1])).contains("args"));
        assert!(bad(base().args(["x".repeat(MAX_ARG + 1)])).contains("args"));
        assert!(bad(base().throttle_secs(0)).contains("throttle"));
        assert!(bad(base().supervised_env("pitwall")).contains("supervised"));
        assert!(bad(base().supervised_env("1X")).contains("supervised"));
        assert!(bad(base().supervised_env("")).contains("supervised"));
        assert!(bad(base().memory_high_mb(1)).contains("memory"));
        assert!(bad(base().docs_url("http://x")).contains("docs"));
        assert!(bad(base().docs_url("https://x y")).contains("docs"));
    }

    #[test]
    fn hardening_is_for_system_services_and_checked() {
        use crate::hardening::{Hardening, Sandbox};
        let h = || Hardening::new().windows_privileges(["SeChangeNotifyPrivilege"]);
        let err = base().hardening(h()).build().unwrap_err();
        assert_eq!(err.field, "hardening");
        let s = base()
            .kind(Kind::SystemService)
            .hardening(h())
            .build()
            .unwrap();
        assert_eq!(
            s.hardening().unwrap().privileges(),
            ["SeChangeNotifyPrivilege"]
        );
        assert!(base().build().unwrap().hardening().is_none());
        let bad = base()
            .kind(Kind::SystemService)
            .hardening(Hardening::new().sandbox(Sandbox::new()))
            .build();
        assert_eq!(bad.unwrap_err().field, "sandbox");
    }
}
