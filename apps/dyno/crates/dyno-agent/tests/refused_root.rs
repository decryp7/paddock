#![allow(clippy::unwrap_used, clippy::expect_used)]

//! A data folder the user named (`DYNO_DATA_DIR`, here a test's own folder) that other accounts can write is refused at the very start
//! (ADR 0092, revised 2026-11-12): before the log, the instance lock, the crash or run markers, the database or any other file. There
//! is no memory-only mode (settings and Wake-on-LAN passwords would be lost without a word, ER-37): the agent exits with a defined code
//! and the cause and next step. The default per-user folder is not refused (`paths::check_root_with`, tested there). The folders are
//! temporary ones (ER-14); the real start path is the `dyno-agent run` binary itself.

use dyno_agent::paths::Dirs;
use dyno_agent::{Running, StartError};
use dyno_platform::fake::Fake;

fn start(dirs: Dirs) -> Result<Running, StartError> {
    let fake = Fake::new();
    Running::start(dirs, fake.platform(), Box::new(Fake::new()), Box::new(|_| Ok(())))
}

/// Runs `dyno-agent run` on `root`, waiting at most 60 s: a mutation that lets it start would run for ever, and must fail instead of
/// hanging the suite. A watchdog thread ends the child through `paddock_proc` when the run is not over in time; the test then fails on the
/// exit code.
#[cfg(unix)]
#[expect(
    clippy::disallowed_methods,
    reason = "runs the dyno-agent binary under test itself, on a temporary data folder"
)]
fn run_bounded(root: &std::path::Path) -> std::process::Output {
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_dyno-agent"))
        .arg("run")
        .env("DYNO_DATA_DIR", root)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // The child is ours and unreaped, so its pid is its own: the process id (pid and start time) is taken now, and ended through
    // `paddock_proc` if the run is not over in time.
    let id = paddock_proc::ProcessId::of(child.id());
    let (done, finished) = std::sync::mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if finished.recv_timeout(std::time::Duration::from_secs(60)) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
            if let Some(id) = id {
                let _ = paddock_proc::end(id);
            }
        }
    });
    let out = child.wait_with_output().unwrap();
    let _ = done.send(());
    watchdog.join().unwrap();
    out
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn names(dir: &Path) -> Vec<std::ffi::OsString> {
        let mut v: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name()).collect();
        v.sort();
        v
    }

    fn open_root(tmp: &Path) -> std::path::PathBuf {
        let root = tmp.join("data");
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
        root
    }

    #[test]
    fn a_data_folder_the_user_named_and_others_can_write_stops_the_library_start_before_the_lock_and_the_database() {
        let tmp = paddock_testkit::private_tempdir().unwrap();
        let root = open_root(tmp.path());
        std::fs::write(root.join("dyno.db"), b"planted, not a database").unwrap();
        let before = names(&root);
        let Err(StartError::DataFolderRefused(text)) = start(Dirs::at(&root)) else {
            panic!("the start must be refused")
        };
        assert!(
            text.contains("chmod go-w") && text.contains("DYNO_DATA_DIR"),
            "cause and next step: {text}"
        );
        assert_eq!(names(&root), before, "no lock file, no database, nothing was created");
        assert_eq!(std::fs::read(root.join("dyno.db")).unwrap(), b"planted, not a database");
        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o777,
            "never tightened"
        );
    }

    /// The real start path (the binary, `run`) with links planted where the log, the lock and the markers go: none is followed.
    #[test]
    fn the_real_run_with_planted_links_follows_none_and_exits_with_the_refusal_code() {
        let tmp = paddock_testkit::private_tempdir().unwrap();
        let root = open_root(tmp.path());
        let victims = tmp.path().join("victims");
        std::fs::create_dir(&victims).unwrap();
        let mut planted = Vec::new();
        for name in ["agent.lock", "crash-dyno-agent.json", "run-agent.marker", "logs", "dyno.db"] {
            let target = victims.join(name);
            if name == "logs" {
                std::fs::create_dir(&target).unwrap();
            } else {
                std::fs::write(&target, b"victim").unwrap();
            }
            std::os::unix::fs::symlink(&target, root.join(name)).unwrap();
            planted.push(target);
        }
        let victims_before = names(&victims);
        let out = run_bounded(&root);
        assert_eq!(out.status.code(), Some(dyno_agent::cli::EXIT_DATA_FOLDER_REFUSED), "{out:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("did not start") && err.contains("chmod go-w") && err.contains("DYNO_DATA_DIR"),
            "{err}"
        );
        assert_eq!(names(&victims), victims_before, "nothing was created behind a link");
        for t in planted.iter().filter(|t| t.is_file()) {
            assert_eq!(std::fs::read(t).unwrap(), b"victim", "{} was not truncated or written", t.display());
        }
        assert_eq!(
            std::fs::read_dir(victims.join("logs")).unwrap().count(),
            0,
            "no log was written through the link"
        );
    }
}

/// Windows twin: a data folder that Everyone may write to is refused the same way. Owed to the Windows runner or VM (an
/// Administrators-owned `%LOCALAPPDATA%\Dyno`, left by an elevated 1.1.0, is the manual VM check).
#[cfg(windows)]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "a Windows-only test runs icacls on its own temporary folder to set its access list"
)]
fn windows_a_data_folder_everyone_can_write_stops_the_start_before_the_lock_and_says_what_to_do() {
    let tmp = paddock_testkit::private_tempdir().unwrap();
    let root = tmp.path().join("data");
    std::fs::create_dir(&root).unwrap();
    let out = std::process::Command::new("icacls")
        .arg(&root)
        .args(["/grant", "*S-1-1-0:(OI)(CI)F"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let before: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.file_name()).collect();
    let Err(StartError::DataFolderRefused(text)) = start(Dirs::at(&root)) else {
        panic!("the start must be refused")
    };
    assert!(text.contains("did not start") && text.contains("DYNO_DATA_DIR"), "{text}");
    let after: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(before, after, "no lock file or database was created");
    let run = std::process::Command::new(env!("CARGO_BIN_EXE_dyno-agent"))
        .arg("run")
        .env("DYNO_DATA_DIR", &root)
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(dyno_agent::cli::EXIT_DATA_FOLDER_REFUSED));
}

/// ER-27: a refused start leaves its reason in the OS's own logs folder (never the refused folder), because an agent started at
/// login has no standard error and the refused folder is not written to. The test's account folders are temporary ones (ER-14), so the
/// real profile is not touched; the line names the folder, the cause and the next step.
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "runs the dyno-agent binary under test itself, on a temporary data folder and temporary account folders"
)]
fn a_refused_start_leaves_its_reason_in_the_os_logs_folder() {
    let tmp = paddock_testkit::private_tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let root = tmp.path().join("data");
    std::fs::create_dir(&root).unwrap();
    open_to_everyone(&root);
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_dyno-agent"));
    cmd.arg("run").env("DYNO_DATA_DIR", &root).env_remove("DYNO_HOME");
    for var in [
        "HOME",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
    ] {
        cmd.env(var, &home);
    }
    let run = cmd.output().unwrap();
    assert_eq!(run.status.code(), Some(dyno_agent::cli::EXIT_DATA_FOLDER_REFUSED), "{run:?}");
    let logs = logs_under(&home);
    assert_eq!(logs.len(), 1, "one agent log in the OS logs folder: {logs:?}");
    let text = std::fs::read_to_string(&logs[0]).unwrap();
    assert!(
        text.contains("DataFolderRefused")
            && text.contains("a folder the user named")
            && (text.contains("its access list gives other users access") || text.contains("other users can change it")),
        "the line names the code, the kind and the reason class: {text}"
    );
    assert!(text.contains("DYNO_DATA_DIR"), "and the next step: {text}");
    assert!(
        !text.contains(&*root.to_string_lossy()) && !text.contains("S-1-"),
        "and no folder path and no account id: {text}"
    );
    assert!(
        std::fs::read_dir(&root).unwrap().next().is_none(),
        "nothing was written in the refused folder"
    );
}

/// Makes `root` writable by every account (Unix: mode 0777; Windows: an Everyone entry that new files inherit).
#[cfg_attr(
    windows,
    expect(clippy::disallowed_methods, reason = "runs icacls on the test's own temporary folder")
)]
fn open_to_everyone(root: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o777)).unwrap();
    }
    #[cfg(windows)]
    {
        let out = std::process::Command::new("icacls")
            .arg(root)
            .args(["/grant", "*S-1-1-0:(OI)(CI)F"])
            .output()
            .unwrap();
        assert!(out.status.success());
    }
}

/// The agent's dated logs below `dir`.
fn logs_under(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else if p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("dyno-agent") && n.ends_with(".log"))
            {
                found.push(p);
            }
        }
    }
    found.sort();
    found
}
