//! Finding an installed family app: its program on `PATH`, then in the usual install places. Pure over an injected
//! environment, operating system and file probe, so every layout is tested on every OS.

use crate::app::AppSpec;
use crate::probe::FileProbe;
use paddock_env::Os;
use std::path::{Path, PathBuf};

/// What the search found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// The app's program is here.
    Program(PathBuf),
    /// Only the pre-rename program is installed: the app must be updated before it can take part.
    NeedsUpdate,
    /// Nothing.
    Missing,
}

fn exe(name: &str, os: Os) -> String {
    if os == Os::Windows {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// The places to look for the program `cli` of `app`, best first, after the directories of `PATH` (which the caller
/// searches first). `env` reads an environment variable.
pub fn install_candidates(
    app: &AppSpec,
    cli: &str,
    os: Os,
    env: &dyn Fn(&str) -> Option<String>,
) -> Vec<PathBuf> {
    let program = exe(cli, os);
    let name = app.name;
    let mut out = Vec::new();
    match os {
        Os::MacOs => {
            out.push(PathBuf::from(format!(
                "/Applications/{name}.app/Contents/MacOS/{program}"
            )));
            if let Some(h) = env("HOME") {
                out.push(
                    PathBuf::from(&h)
                        .join(format!("Applications/{name}.app/Contents/MacOS/{program}")),
                );
                out.push(PathBuf::from(h).join(".cargo/bin").join(&program));
            }
            out.push(PathBuf::from("/usr/local/bin").join(&program));
            out.push(PathBuf::from("/opt/homebrew/bin").join(&program));
        }
        Os::Windows => {
            for var in ["LOCALAPPDATA", "ProgramFiles"] {
                if let Some(base) = env(var) {
                    let base = PathBuf::from(base);
                    out.push(base.join("Programs").join(name).join(&program));
                    out.push(base.join(name).join(&program));
                }
            }
        }
        Os::Linux => {
            if let Some(h) = env("HOME") {
                out.push(PathBuf::from(&h).join(".local/bin").join(&program));
                out.push(PathBuf::from(h).join(".cargo/bin").join(&program));
            }
            out.push(PathBuf::from("/usr/local/bin").join(&program));
            out.push(PathBuf::from("/usr/bin").join(&program));
            out.push(PathBuf::from(format!("/opt/{name}/{program}")));
        }
    }
    out
}

fn find_cli(
    app: &AppSpec,
    cli: &str,
    path: Option<&str>,
    os: Os,
    env: &dyn Fn(&str) -> Option<String>,
    probe: &dyn FileProbe,
) -> Option<PathBuf> {
    let file = exe(cli, os);
    // The list is split by the rules of `os`, not of the host: the discovery is computed (and tested) for every OS everywhere.
    let separator = if os == Os::Windows { ';' } else { ':' };
    let on_path = path
        .into_iter()
        .flat_map(move |p| p.split(separator))
        .filter(|d| !d.is_empty())
        .map(|d| Path::new(d).join(&file));
    on_path
        .chain(install_candidates(app, cli, os, env))
        .find(|p| probe.is_file(p))
}

/// Looks for `app` on this machine. `path` is the `PATH` list.
pub fn discover(
    app: &AppSpec,
    path: Option<&str>,
    os: Os,
    env: &dyn Fn(&str) -> Option<String>,
    probe: &dyn FileProbe,
) -> Found {
    if let Some(p) = find_cli(app, app.cli, path, os, env, probe) {
        return Found::Program(p);
    }
    match app.legacy_cli {
        Some(old) if find_cli(app, old, path, os, env, probe).is_some() => Found::NeedsUpdate,
        _ => Found::Missing,
    }
}
