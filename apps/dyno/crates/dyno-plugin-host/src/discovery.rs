//! Finding plugins: `<plugins dir>/<id>/plugin.toml` and the executable it names, in the same folder.
//!
//! `plugin.toml` is parsed once into a [`Discovered`]; anything wrong with a folder becomes an [`Invalid`] entry with
//! a message saying what to fix, never an error for the whole scan.

use dyno_plugin_api::{valid_id, Contribution, API_VERSION};
use serde::Deserialize;
use std::io::Read;
use std::path::{Path, PathBuf};

/// At most this many plugin folders are read.
pub const MAX_PLUGINS: usize = 64;
/// The largest `plugin.toml` read.
const MAX_MANIFEST: u64 = 64 * 1024;

/// The file a plugin ships next to its executable.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginFile {
    id: String,
    name: String,
    version: String,
    api_version: u32,
    #[serde(default)]
    executable: Option<String>,
    #[serde(default)]
    contributes: Vec<Contribution>,
}

/// A plugin found on disk, ready to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    /// Its id (also its folder name).
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// What it says it contributes.
    pub contributes: Vec<Contribution>,
    /// Its folder (canonical).
    pub dir: PathBuf,
    /// Its executable (canonical, inside `dir`).
    pub executable: PathBuf,
}

/// A plugin folder that cannot be used, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid {
    /// The folder's name.
    pub folder: String,
    /// What is wrong, and what to fix.
    pub error: String,
}

/// Every plugin in `dir`, sorted by id. A missing `dir` has none.
pub fn discover(dir: &Path) -> (Vec<Discovered>, Vec<Invalid>) {
    let mut found = Vec::new();
    let mut invalid = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (found, invalid);
    };
    let mut folders: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();
    folders.sort();
    for (i, folder) in folders.into_iter().enumerate() {
        let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if i >= MAX_PLUGINS {
            invalid.push(Invalid {
                folder: name,
                error: format!("more than {MAX_PLUGINS} plugin folders; this one was not read"),
            });
            continue;
        }
        match read_plugin(&folder) {
            Ok(p) => found.push(p),
            Err(error) => invalid.push(Invalid { folder: name, error }),
        }
    }
    (found, invalid)
}

/// Reads one plugin folder.
pub fn read_plugin(folder: &Path) -> Result<Discovered, String> {
    let folder_name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let manifest_path = folder.join("plugin.toml");
    let mut text = String::new();
    std::fs::File::open(&manifest_path)
        .and_then(|f| f.take(MAX_MANIFEST + 1).read_to_string(&mut text))
        .map_err(|e| format!("cannot read {}: {e}", manifest_path.display()))?;
    if text.len() as u64 > MAX_MANIFEST {
        return Err(format!("plugin.toml is larger than {MAX_MANIFEST} bytes"));
    }
    let file: PluginFile = toml::from_str(&text).map_err(|e| format!("plugin.toml is not valid: {}", e.message()))?;
    if !valid_id(&file.id) {
        return Err(format!(
            "the id {:?} is not valid: use 1 to 64 lowercase letters, digits, '-' or '_'",
            file.id
        ));
    }
    if file.id != folder_name {
        return Err(format!("the id {:?} must match the folder name {folder_name:?}", file.id));
    }
    if file.api_version != API_VERSION {
        return Err(format!(
            "built for plugin API version {}, but this Dyno speaks version {API_VERSION}: update the plugin or Dyno",
            file.api_version
        ));
    }
    let exe_name = file.executable.unwrap_or_else(|| file.id.clone());
    if !valid_file_name(&exe_name) {
        return Err(format!(
            "the executable {exe_name:?} must be a plain file name in the plugin's folder"
        ));
    }
    let mut exe = folder.join(&exe_name);
    if cfg!(windows) && exe.extension().is_none() {
        exe.set_extension("exe");
    }
    let dir = folder.canonicalize().map_err(|e| format!("cannot resolve the folder: {e}"))?;
    let executable = exe
        .canonicalize()
        .map_err(|_| format!("the executable {} is missing", exe.display()))?;
    // A link pointing out of the folder is refused: a plugin is what is in its folder.
    if executable.parent() != Some(dir.as_path()) || !executable.is_file() {
        return Err(format!(
            "the executable {} must be a file inside the plugin's folder",
            exe.display()
        ));
    }
    Ok(Discovered {
        id: file.id,
        name: file.name,
        version: file.version,
        contributes: file.contributes,
        dir,
        executable,
    })
}

fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(root: &Path, name: &str, toml: &str, exe: Option<&str>) {
        let d = root.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("plugin.toml"), toml).unwrap();
        if let Some(exe) = exe {
            std::fs::write(d.join(exe), b"x").unwrap();
        }
    }

    fn exe(name: &str) -> String {
        if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        }
    }

    #[test]
    fn valid_plugins_are_found_and_broken_folders_say_what_to_fix() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        folder(
            root,
            "good",
            "id = \"good\"\nname = \"Good\"\nversion = \"1.0\"\napi_version = 1\ncontributes = [\"sensors\"]\n",
            Some(&exe("good")),
        );
        folder(
            root,
            "named",
            "id = \"named\"\nname = \"N\"\nversion = \"1\"\napi_version = 1\nexecutable = \"bin-x\"\n",
            Some(&exe("bin-x")),
        );
        folder(
            root,
            "mismatch",
            "id = \"other\"\nname = \"M\"\nversion = \"1\"\napi_version = 1\n",
            Some(&exe("other")),
        );
        folder(
            root,
            "old",
            "id = \"old\"\nname = \"O\"\nversion = \"1\"\napi_version = 99\n",
            Some(&exe("old")),
        );
        folder(
            root,
            "noexe",
            "id = \"noexe\"\nname = \"X\"\nversion = \"1\"\napi_version = 1\n",
            None,
        );
        folder(
            root,
            "escape",
            "id = \"escape\"\nname = \"E\"\nversion = \"1\"\napi_version = 1\nexecutable = \"../good/good\"\n",
            None,
        );
        folder(root, "garbage", "this is = = not toml", None);
        folder(
            root,
            "extra",
            "id = \"extra\"\nname = \"E\"\nversion = \"1\"\napi_version = 1\nshell = true\n",
            None,
        );
        folder(
            root,
            "Bad Id",
            "id = \"Bad Id\"\nname = \"B\"\nversion = \"1\"\napi_version = 1\n",
            None,
        );
        std::fs::create_dir_all(root.join("empty")).unwrap();
        std::fs::write(root.join("stray-file.txt"), b"ignored").unwrap();
        folder(
            root,
            "huge",
            &format!(
                "id = \"huge\"\nname = \"{}\"\nversion = \"1\"\napi_version = 1\n",
                "h".repeat(70_000)
            ),
            None,
        );

        let (found, invalid) = discover(root);
        let ids: Vec<_> = found.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["good", "named"]);
        assert_eq!(found[0].contributes, vec![Contribution::Sensors]);
        assert!(found[1].executable.ends_with(exe("bin-x")));
        let why = |f: &str| invalid.iter().find(|i| i.folder == f).map(|i| i.error.clone()).unwrap();
        assert!(why("mismatch").contains("must match the folder"));
        assert!(why("old").contains("update the plugin"));
        assert!(why("noexe").contains("missing"));
        assert!(why("escape").contains("plain file name"));
        assert!(why("garbage").contains("not valid"));
        assert!(why("extra").contains("not valid"));
        assert!(why("Bad Id").contains("not valid"));
        assert!(why("empty").contains("cannot read"));
        assert!(why("huge").contains("larger"));
        assert_eq!(invalid.len(), 9);
    }

    #[test]
    fn a_missing_plugins_folder_has_no_plugins() {
        let (found, invalid) = discover(Path::new("/no/such/dyno/plugins"));
        assert!(found.is_empty() && invalid.is_empty());
    }

    #[test]
    fn file_names_are_plain() {
        assert!(valid_file_name("dyno-plugin-x.exe"));
        assert!(!valid_file_name(".."));
        assert!(!valid_file_name("a/b"));
        assert!(!valid_file_name("a\\b"));
        assert!(!valid_file_name(""));
        assert!(!valid_file_name("c:x"));
    }
}
