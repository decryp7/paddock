//! Hands files, folders and web addresses to the OS: the program associated with a file, the file manager, the browser. The
//! command lines are pure functions of the platform ([`Os`]) so every variant is tested on any machine; a [`Launcher`] runs one
//! (no shell, each argument separate, a reaper stops it after [`OPENER_TIMEOUT`]). Nothing that would run on open is opened
//! ([`runs_on_open`]), and only web addresses of a few schemes are ([`check_url`]).

use crate::path::Result;
use paddock_commands::CommandError;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long an opener helper (file manager, `open`, `xdg-open`) may run before it is stopped.
pub const OPENER_TIMEOUT: Duration = Duration::from_secs(30);
/// How often the reaper looks at its child.
const POLL: Duration = Duration::from_millis(20);
/// The longest web address accepted from the webview, in bytes.
pub const MAX_URL_LEN: usize = 2048;
/// The URL schemes that may be opened.
pub const URL_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

/// The platform a command line is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// Windows.
    Windows,
    /// macOS.
    Mac,
    /// Linux and the other Unixes.
    Linux,
}

impl Os {
    /// The platform this build runs on.
    pub const CURRENT: Os = if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Mac
    } else {
        Os::Linux
    };
}

/// A program and its arguments, each passed as its own argument (never through a shell).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The program.
    pub program: OsString,
    /// Its arguments.
    pub args: Vec<OsString>,
}

impl Invocation {
    fn new(program: impl AsRef<OsStr>, args: impl IntoIterator<Item = OsString>) -> Self {
        Invocation {
            program: program.as_ref().to_os_string(),
            args: args.into_iter().collect(),
        }
    }

    /// The command to run it (no console window on Windows).
    pub fn command(&self) -> std::process::Command {
        let mut cmd = paddock_proc::command(&self.program);
        cmd.args(&self.args);
        cmd
    }
}

/// `path` as the OS tools expect it: Windows verbatim paths lose their prefix, which Explorer and editors do not understand.
/// A verbatim spelling only exists in Windows paths, so the rule is the Windows one on every host (a table of any OS tests it).
pub fn plain_path(path: &Path) -> PathBuf {
    paddock_env::winpath::plain(path, paddock_env::Os::Windows).into_owned()
}

/// The file manager showing `file` selected (Explorer, Finder; on Linux its folder, as file managers have no common
/// "select" argument).
pub fn reveal(os: Os, file: &Path) -> Invocation {
    let file = plain_path(file);
    match os {
        Os::Windows => {
            let mut arg = OsString::from("/select,");
            arg.push(&file);
            Invocation::new("explorer", [arg])
        }
        Os::Mac => Invocation::new("open", [OsString::from("-R"), file.into_os_string()]),
        Os::Linux => Invocation::new(
            "xdg-open",
            [file.parent().unwrap_or(&file).as_os_str().to_os_string()],
        ),
    }
}

/// `file` (or a folder: the file manager on it) opened with the program the OS associates with it.
pub fn open_default(os: Os, file: &Path) -> Invocation {
    Invocation::new(opener_program(os), [plain_path(file).into_os_string()])
}

fn opener_program(os: Os) -> &'static str {
    match os {
        Os::Windows => "explorer",
        Os::Mac => "open",
        Os::Linux => "xdg-open",
    }
}

/// The browser (or mail program) on a checked address ([`check_url`]).
pub fn open_url_with(os: Os, url: &str) -> Invocation {
    Invocation::new(opener_program(os), [OsString::from(url)])
}

/// `editor` (VS Code's `code` launcher) opening `file` at `line`: `code -g <file>:<line>`.
pub fn open_at_line(editor: &Path, file: &Path, line: u32) -> Invocation {
    let mut target = plain_path(file).into_os_string();
    target.push(format!(":{line}"));
    Invocation::new(editor, [OsString::from("-g"), target])
}

/// Extensions whose "default program" runs them (or changes the system): never opened with the OS association, on any
/// platform, as an opened file may come from anywhere.
pub const RUNS_ON_OPEN: &[&str] = &[
    "exe",
    "com",
    "bat",
    "cmd",
    "ps1",
    "psm1",
    "psd1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "msi",
    "msp",
    "mst",
    "scr",
    "hta",
    "cpl",
    "msc",
    "jar",
    "lnk",
    "url",
    "reg",
    "inf",
    "scf",
    "pif",
    "application",
    "appref-ms",
    "gadget",
    "settingcontent-ms",
    "library-ms",
    "app",
    "command",
    "tool",
    "pkg",
    "mpkg",
    "dmg",
    "workflow",
    "terminal",
    "webloc",
    "sh",
    "bash",
    "zsh",
    "csh",
    "ksh",
    "run",
    "desktop",
    "appimage",
    "deb",
    "rpm",
    "py",
    "pyw",
    "pl",
    "rb",
];

/// True when opening `file` with its default program could run it (see [`RUNS_ON_OPEN`]).
pub fn runs_on_open(file: &Path) -> bool {
    file.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| RUNS_ON_OPEN.contains(&e.to_ascii_lowercase().as_str()))
}

/// Where VS Code's command-line launcher is: on `PATH`, else its usual install locations. `var` reads an environment variable
/// and `is_file` checks a candidate (injected for tests).
pub fn find_vscode(
    os: Os,
    var: &dyn Fn(&str) -> Option<OsString>,
    is_file: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let name = if os == Os::Windows {
        "code.cmd"
    } else {
        "code"
    };
    let on_path = var("PATH")
        .map(|p| {
            // A relative PATH entry would depend on the working directory: never used.
            std::env::split_paths(&p)
                .filter(|dir| dir.is_absolute() || (os != Os::Windows && dir.has_root()))
                .map(|dir| dir.join(name))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let installed: Vec<PathBuf> = match os {
        Os::Windows => [
            ("LOCALAPPDATA", r"Programs\Microsoft VS Code\bin\code.cmd"),
            ("ProgramFiles", r"Microsoft VS Code\bin\code.cmd"),
            ("ProgramFiles(x86)", r"Microsoft VS Code\bin\code.cmd"),
        ]
        .iter()
        .filter_map(|(v, rel)| var(v).map(|base| PathBuf::from(base).join(rel)))
        .collect(),
        Os::Mac => {
            let app = "Visual Studio Code.app/Contents/Resources/app/bin/code";
            let mut v = vec![PathBuf::from("/Applications").join(app)];
            v.extend(var("HOME").map(|h| PathBuf::from(h).join("Applications").join(app)));
            v
        }
        Os::Linux => vec![
            PathBuf::from("/usr/bin/code"),
            PathBuf::from("/usr/share/code/bin/code"),
            PathBuf::from("/snap/bin/code"),
        ],
    };
    on_path.into_iter().chain(installed).find(|p| is_file(p))
}

/// VS Code's launcher on this machine, if it is installed (the search of [`find_vscode`] with the real environment and file system).
pub fn installed_vscode() -> Option<PathBuf> {
    find_vscode(Os::CURRENT, &|v| std::env::var_os(v), &|p| p.is_file())
}

/// The largest line number accepted for "open at line".
pub const MAX_LINE: u32 = 10_000_000;

/// A web address the webview wants opened: at most [`MAX_URL_LEN`] bytes, no spaces or control characters, one of
/// [`URL_SCHEMES`]; `http` and `https` need a host. Returns the address unchanged.
pub fn check_url(raw: &str) -> Result<&str> {
    let bad = |why: &str| {
        Err(
            CommandError::invalid(format!("this web address cannot be opened ({why})"))
                .field("url"),
        )
    };
    if raw.is_empty() || raw.len() > MAX_URL_LEN {
        return bad("empty or too long");
    }
    if raw.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return bad("it has spaces or control characters");
    }
    let Some((scheme, rest)) = raw.split_once(':') else {
        return bad("it has no scheme");
    };
    let scheme = scheme.to_ascii_lowercase();
    if !URL_SCHEMES.contains(&scheme.as_str()) {
        return bad("only http, https and mailto addresses are opened");
    }
    let host_ok = if scheme == "mailto" {
        !rest.is_empty()
    } else {
        rest.strip_prefix("//")
            .and_then(|r| r.split(['/', '?', '#']).next())
            .is_some_and(|authority| {
                authority
                    .rsplit('@')
                    .next()
                    .is_some_and(|host| !host.is_empty())
            })
    };
    if !host_ok {
        return bad("it names no host");
    }
    Ok(raw)
}

/// Runs an [`Invocation`] without waiting for it. A port, so tests record instead of launching.
pub trait Launcher: Send + Sync {
    /// Starts `invocation`; fails only when it cannot be started.
    fn start(&self, invocation: &Invocation) -> std::io::Result<()>;
}

/// The real launcher: a detached child with its output discarded and a reaper thread.
pub struct ProcessLauncher {
    timeout: Duration,
}

impl Default for ProcessLauncher {
    fn default() -> Self {
        ProcessLauncher {
            timeout: OPENER_TIMEOUT,
        }
    }
}

impl ProcessLauncher {
    /// A launcher whose children are stopped after `timeout`.
    pub fn with_timeout(timeout: Duration) -> Self {
        ProcessLauncher { timeout }
    }

    /// Starts `invocation`; the returned handle ends when the child exited or was stopped (callers may ignore it).
    pub fn spawn(&self, invocation: &Invocation) -> std::io::Result<std::thread::JoinHandle<()>> {
        spawn_detached(invocation.command(), self.timeout)
    }
}

impl Launcher for ProcessLauncher {
    fn start(&self, invocation: &Invocation) -> std::io::Result<()> {
        self.spawn(invocation).map(drop)
    }
}

/// Starts `cmd` without blocking the caller. Its output is discarded; a background thread reaps it and stops it if it is still
/// running after `timeout`. Fails only when the process cannot be started or the thread cannot be created.
pub fn spawn_detached(
    mut cmd: std::process::Command,
    timeout: Duration,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    use std::process::Stdio;
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + timeout;
    std::thread::Builder::new()
        .name("paddock-opener".into())
        .spawn(move || {
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if !status.success() {
                            log::debug!(Status = i64::from(status.code().unwrap_or(-1)); "Opener helper exited with an error");
                        }
                        return;
                    }
                    Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
                    _ => {
                        log::warn!("Opener helper timed out; stopping it");
                        let _ = child.kill();
                        let _ = child.wait();
                        return;
                    }
                }
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_commands::ErrorCode;

    fn args(i: &Invocation) -> Vec<String> {
        i.args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn reveal_selects_the_file_per_platform() {
        let w = reveal(Os::Windows, Path::new(r"\\?\C:\logs\a b.log"));
        assert_eq!(
            (w.program.to_str(), args(&w)),
            (
                Some("explorer"),
                vec![r"/select,C:\logs\a b.log".to_string()]
            )
        );
        let m = reveal(Os::Mac, Path::new("/logs/a.log"));
        assert_eq!(
            (m.program.to_str(), args(&m)),
            (
                Some("open"),
                vec!["-R".to_string(), "/logs/a.log".to_string()]
            )
        );
        let l = reveal(Os::Linux, Path::new("/logs/a.log"));
        assert_eq!(
            (l.program.to_str(), args(&l)),
            (Some("xdg-open"), vec!["/logs".to_string()])
        );
        assert_eq!(
            args(&reveal(Os::Linux, Path::new("/"))),
            vec!["/".to_string()]
        );
    }

    #[test]
    fn a_hostile_name_stays_one_argument() {
        let name = r#"C:\logs\x" & calc & ".log"#;
        for (os, program) in [
            (Os::Windows, "explorer"),
            (Os::Mac, "open"),
            (Os::Linux, "xdg-open"),
        ] {
            let i = open_default(os, Path::new(name));
            assert_eq!(
                (i.program.to_str(), args(&i)),
                (Some(program), vec![name.to_string()])
            );
            assert_eq!(
                open_url_with(os, "https://a.example/x").program.to_str(),
                Some(program)
            );
        }
        let cmd = open_default(Os::Linux, Path::new("/a.log")).command();
        assert_eq!(cmd.get_program(), "xdg-open");
        assert_eq!(
            cmd.get_args().collect::<Vec<_>>(),
            vec![OsStr::new("/a.log")]
        );
    }

    #[test]
    fn an_editor_opens_at_a_line() {
        let i = open_at_line(
            Path::new("/usr/bin/code"),
            Path::new(r"\\?\C:\src\a.rs"),
            42,
        );
        assert_eq!(
            args(&i),
            vec!["-g".to_string(), r"C:\src\a.rs:42".to_string()]
        );
    }

    #[test]
    fn programs_that_run_on_open_are_never_opened() {
        for f in [
            "a.exe",
            "A.BAT",
            "x.command",
            "y.Sh",
            "p.ps1",
            "q.desktop",
            "w.webloc",
            "t.js",
            "m.msi",
            "app.app",
        ] {
            assert!(runs_on_open(Path::new(f)), "{f}");
        }
        for f in ["a.log", "a.txt", "noext", "a.json", "dir.d", "x.jpg"] {
            assert!(!runs_on_open(Path::new(f)), "{f}");
        }
    }

    #[test]
    fn vscode_is_found_on_path_or_in_its_usual_places() {
        let vars = |pairs: Vec<(&'static str, OsString)>| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone())
        };
        let nothing = |_: &Path| false;
        // On PATH (an absolute entry only).
        let path = std::env::join_paths(["relative/bin", "/opt/bin"]).unwrap();
        let on_path = |p: &Path| p == Path::new("/opt/bin/code");
        assert_eq!(
            find_vscode(Os::Linux, &vars(vec![("PATH", path.clone())]), &on_path),
            Some(PathBuf::from("/opt/bin/code"))
        );
        assert_eq!(
            find_vscode(
                Os::Linux,
                &vars(vec![(
                    "PATH",
                    std::env::join_paths(["relative/bin"]).unwrap()
                )]),
                &|p: &Path| p == Path::new("relative/bin/code")
            ),
            None
        );
        assert_eq!(
            find_vscode(Os::Linux, &vars(vec![]), &|p: &Path| p
                == Path::new("/snap/bin/code")),
            Some(PathBuf::from("/snap/bin/code"))
        );
        let mac = |p: &Path| {
            p.ends_with("Applications/Visual Studio Code.app/Contents/Resources/app/bin/code")
                && p.starts_with("/Users/me")
        };
        assert!(find_vscode(Os::Mac, &vars(vec![("HOME", "/Users/me".into())]), &mac).is_some());
        assert!(
            find_vscode(Os::Mac, &vars(vec![]), &|p: &Path| p
                .starts_with("/Applications"))
            .is_some()
        );
        let win = find_vscode(
            Os::Windows,
            &vars(vec![("LOCALAPPDATA", r"C:\Users\me\AppData\Local".into())]),
            &|p: &Path| p.to_string_lossy().contains("Programs"),
        );
        assert!(win.is_some());
        assert_eq!(find_vscode(Os::Windows, &vars(vec![]), &nothing), None);
        let _ = Os::CURRENT;
    }

    #[test]
    fn only_web_addresses_of_a_few_schemes_open() {
        for ok in [
            "https://example.org/a?b=c#d",
            "HTTP://example.org",
            "http://user@host:8080/x",
            "mailto:a@example.org",
        ] {
            assert_eq!(check_url(ok).unwrap(), ok);
        }
        for bad in [
            "",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,x",
            "ftp://x",
            "https://",
            "https:///x",
            "https:x",
            "mailto:",
            "no scheme",
            "example.org",
            "https://a b",
            "https://a\nb",
            "-oProxyCommand=x",
            "https://?x",
            "ssh://x",
        ] {
            let e = check_url(bad).unwrap_err();
            assert_eq!(e.code, ErrorCode::Invalid, "{bad:?}");
        }
        let long = format!("https://a.example/{}", "x".repeat(MAX_URL_LEN));
        assert!(check_url(&long).is_err());
    }

    #[cfg(unix)]
    fn shell(script: &str) -> Invocation {
        Invocation::new("sh", [OsString::from("-c"), OsString::from(script)])
    }

    #[cfg(unix)]
    #[test]
    fn a_finished_child_is_reaped_and_a_hung_one_is_stopped() {
        let quick = ProcessLauncher::default().spawn(&shell("exit 0")).unwrap();
        quick.join().unwrap();
        let failing = ProcessLauncher::default().spawn(&shell("exit 3")).unwrap();
        failing.join().unwrap();
        let started = Instant::now();
        let hung = ProcessLauncher::with_timeout(Duration::from_millis(60))
            .spawn(&shell("exec sleep 30"))
            .unwrap();
        hung.join().unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the reaper stopped the child at its timeout"
        );
        assert!(ProcessLauncher::default().start(&shell("exit 0")).is_ok());
    }

    #[test]
    fn a_program_that_does_not_exist_is_an_error_not_a_panic() {
        let missing = Invocation::new("/no/such/program-for-paddock-files", []);
        assert!(ProcessLauncher::default().start(&missing).is_err());
    }
}
