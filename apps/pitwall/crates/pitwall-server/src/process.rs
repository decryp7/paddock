//! The one place child processes are started (clippy forbids `Command::new` elsewhere): no console window on
//! Windows, an empty environment plus an allow-list, below-normal priority, killed when Pitwall drops the handle.

use std::ffi::OsStr;
use std::path::Path;
use tokio::process::Command;

/// Environment variables an engine inherits (nothing else: no tokens, proxies or user variables).
pub const KEPT_ENV: &[&str] = &[
    "PATH",
    "SystemRoot",
    "SYSTEMROOT",
    "windir",
    "TEMP",
    "TMP",
    "TMPDIR",
    "HOME",
    "USERPROFILE",
    "LANG",
    "LC_ALL",
    "LD_LIBRARY_PATH",
    "DYLD_LIBRARY_PATH",
    "XDG_RUNTIME_DIR",
];

/// A command for `program` with the sanitised environment.
#[allow(clippy::disallowed_methods)] // the single allowed place
pub fn command(program: impl AsRef<OsStr>, cwd: Option<&Path>) -> Command {
    let mut c = Command::new(program);
    c.env_clear();
    for k in KEPT_ENV {
        if let Some(v) = std::env::var_os(k) {
            c.env(k, v);
        }
    }
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    c.kill_on_drop(true).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS
        c.creation_flags(0x0800_0000 | 0x0000_4000);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn environment_is_scrubbed() {
        std::env::set_var("PITWALL_SECRET_TEST", "x");
        let out = if cfg!(windows) {
            command("cmd", None).args(["/C", "set"]).output().await
        } else {
            command("/usr/bin/env", None).output().await
        }
        .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(!text.contains("PITWALL_SECRET_TEST"));
    }
}
