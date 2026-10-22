//! Finding installed plugins: every folder `<plugins_dir>/<id>` with a valid `pitwall-plugin.toml` whose id is the
//! folder's name, whose contract this host speaks, whose memory request is within the host's limits, and whose
//! executable is a file inside that folder (symbolic links that leave it are refused). Anything else is a
//! [`Problem`] with a message that says what to fix; one bad plugin never hides the others.

use crate::config::HostConfig;
use pitwall_plugin_api::{Kind, Manifest, CONTRACT_VERSION, MANIFEST_FILE};
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};

/// A plugin that can be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    /// Its manifest.
    pub manifest: Manifest,
    /// Its folder (canonical).
    pub folder: PathBuf,
    /// Its executable (canonical, inside `folder`).
    pub exe: PathBuf,
    /// Its memory cap (bytes): what it asked for, or the host's default.
    pub memory_limit: u64,
}

/// A folder that is not a usable plugin, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// The folder.
    pub folder: PathBuf,
    /// What is wrong and what to fix.
    pub message: String,
}

/// The plugins found and the folders that are not plugins.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Discovery {
    /// Usable plugins, sorted by id.
    pub plugins: Vec<Discovered>,
    /// Unusable folders.
    pub problems: Vec<Problem>,
}

/// Scans `config.plugins_dir` (a missing folder has no plugins).
pub fn discover(config: &HostConfig) -> Discovery {
    let mut out = Discovery::default();
    let entries = match std::fs::read_dir(&config.plugins_dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return out,
        Err(e) => {
            out.problems.push(Problem {
                folder: config.plugins_dir.clone(),
                message: format!("cannot read the plugins folder: {e}"),
            });
            return out;
        }
    };
    let mut folders: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    folders.sort();
    for folder in folders {
        if out.plugins.len() >= config.max_plugins {
            out.problems.push(Problem {
                folder,
                message: format!("more than {} plugins are installed; this one is ignored", config.max_plugins),
            });
            continue;
        }
        match inspect(config, &folder) {
            Ok(d) => out.plugins.push(d),
            Err(message) => out.problems.push(Problem { folder, message }),
        }
    }
    out.plugins.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    out
}

fn read_bounded(path: &Path, max: usize) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot read {MANIFEST_FILE}: {e}"))?;
    let mut text = String::new();
    file.take(max as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|e| format!("cannot read {MANIFEST_FILE}: {e}"))?;
    if text.len() > max {
        return Err(format!("{MANIFEST_FILE} is larger than {max} bytes"));
    }
    Ok(text)
}

/// `path` resolved (links, `..`), without Windows' `\\?\` prefix where the path does not need it (child processes
/// and their working folder are given plain paths).
fn resolve(path: &Path) -> std::io::Result<PathBuf> {
    let p = path.canonicalize()?;
    #[cfg(windows)]
    {
        let text = p.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            if rest.get(1..3) == Some(":\\") && rest.len() < 250 {
                return Ok(PathBuf::from(rest));
            }
        }
    }
    Ok(p)
}

/// One folder as a plugin.
fn inspect(config: &HostConfig, folder: &Path) -> Result<Discovered, String> {
    let folder = resolve(folder).map_err(|e| format!("cannot open the folder: {e}"))?;
    let manifest_path = folder.join(MANIFEST_FILE);
    if !manifest_path.is_file() {
        return Err(format!("no {MANIFEST_FILE} in this folder"));
    }
    let manifest = Manifest::parse(&read_bounded(&manifest_path, pitwall_plugin_api::manifest::MAX_MANIFEST_BYTES)?).map_err(|e| e.to_string())?;
    let folder_name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if manifest.id != folder_name {
        return Err(format!(
            "the manifest's id \"{}\" is not the folder's name \"{folder_name}\"; rename the folder or fix the id",
            manifest.id
        ));
    }
    if let Some(why) = CONTRACT_VERSION.mismatch(manifest.contract, &format!("plugin \"{}\"", manifest.id)) {
        return Err(why);
    }
    let max_mb = if manifest.has(Kind::Engine) {
        config.max_engine_memory_mb
    } else {
        config.max_memory_mb
    };
    let mb = manifest.memory_mb.unwrap_or(config.default_memory_mb.min(max_mb));
    if mb > max_mb {
        return Err(format!(
            "it asks for {mb} MB of memory but Pitwall allows plugins of its kind at most {max_mb} MB; raise the limit in the settings or ask its author"
        ));
    }
    let exe = folder.join(manifest.executable_file());
    let exe = resolve(&exe).map_err(|_| format!("its executable \"{}\" is not in the plugin's folder", manifest.executable_file()))?;
    if !exe.is_file() || !exe.starts_with(&folder) {
        return Err(format!(
            "its executable \"{}\" must be a file inside the plugin's folder",
            manifest.executable_file()
        ));
    }
    Ok(Discovered {
        manifest,
        folder,
        exe,
        memory_limit: mb.saturating_mul(1 << 20),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str, extra: &str) -> String {
        format!("id='{id}'\nname='N'\nversion='1'\ncontract='1.0'\nkinds=['filter']\nexecutable='p'\n{extra}")
    }

    fn plugin(root: &Path, folder: &str, manifest: &str, exe: bool) {
        let d = root.join(folder);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(MANIFEST_FILE), manifest).unwrap();
        if exe {
            std::fs::write(d.join(format!("p{}", std::env::consts::EXE_SUFFIX)), b"x").unwrap();
        }
    }

    #[test]
    fn good_plugins_are_found_and_bad_folders_say_why() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        plugin(r, "good", &manifest("good", "memory_mb = 64"), true);
        plugin(r, "also-good", &manifest("also-good", ""), true);
        plugin(r, "renamed", &manifest("other", ""), true);
        plugin(r, "no-exe", &manifest("no-exe", ""), false);
        plugin(r, "greedy", &manifest("greedy", "memory_mb = 9000"), true);
        plugin(r, "future", &manifest("future", "").replace("1.0", "2.0"), true);
        plugin(r, "broken", "not = = toml", true);
        std::fs::create_dir_all(r.join("empty")).unwrap();
        std::fs::write(r.join("stray-file.txt"), b"ignored").unwrap();
        let config = HostConfig::new(r, r.join("data"));
        let d = discover(&config);
        let ids: Vec<&str> = d.plugins.iter().map(|p| p.manifest.id.as_str()).collect();
        assert_eq!(ids, ["also-good", "good"]);
        assert_eq!(d.plugins[1].memory_limit, 64 << 20);
        assert_eq!(d.plugins[0].memory_limit, 512 << 20, "the host's default");
        assert!(d.plugins[0].exe.starts_with(&d.plugins[0].folder));
        let why = |name: &str| {
            d.problems
                .iter()
                .find(|p| p.folder.file_name().is_some_and(|f| f == name))
                .map(|p| p.message.clone())
                .unwrap_or_else(|| panic!("no problem for {name}: {:?}", d.problems))
        };
        assert!(why("renamed").contains("not the folder's name"));
        assert!(why("no-exe").contains("is not in the plugin's folder"));
        assert!(why("greedy").contains("at most 4096 MB"));
        assert!(why("future").contains("needs a newer Pitwall"));
        assert!(why("broken").contains(MANIFEST_FILE));
        assert!(why("empty").contains("no pitwall-plugin.toml"));
        assert_eq!(d.problems.len(), 6);
    }

    #[test]
    fn engines_may_ask_for_more_and_limits_hold() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        plugin(r, "engine", &manifest("engine", "memory_mb = 9000").replace("['filter']", "['engine']"), true);
        plugin(r, "huge", &"#".repeat(70_000), true);
        let mut config = HostConfig::new(r, r.join("data"));
        let d = discover(&config);
        assert_eq!(d.plugins.len(), 1);
        assert!(d.problems[0].message.contains("larger than"));
        config.max_plugins = 0;
        assert!(discover(&config).problems.iter().any(|p| p.message.contains("more than 0 plugins")));
        // A missing folder has no plugins; a file where the folder should be is a problem.
        assert_eq!(discover(&HostConfig::new(r.join("missing"), r)), Discovery::default());
        std::fs::write(r.join("file"), b"x").unwrap();
        assert_eq!(discover(&HostConfig::new(r.join("file"), r)).problems.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn an_executable_that_links_outside_the_folder_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        plugin(r, "sneaky", &manifest("sneaky", ""), false);
        std::fs::write(r.join("outside"), b"x").unwrap();
        std::os::unix::fs::symlink(r.join("outside"), r.join("sneaky").join("p")).unwrap();
        let d = discover(&HostConfig::new(r, r.join("data")));
        assert!(d.plugins.is_empty());
        assert!(d.problems[0].message.contains("inside the plugin's folder"), "{:?}", d.problems);
    }
}
