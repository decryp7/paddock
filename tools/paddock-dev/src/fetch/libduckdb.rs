//! `dev fetch libduckdb`: the official prebuilt libduckdb that Debrief's store worker links, fetched against its pinned SHA-256
//! and unpacked, with the lines that point a build at it. Replaces Debrief's `cargo xtask libduckdb` (and, once CI switches in
//! R2, `fetch-libduckdb.sh`, `app/scripts/libduckdb.mjs` and `scripts/ci/libduckdb-env.sh`).
//!
//! - **The pins** (version, release assets, SHA-256) are read from `crates/debrief-store-duck/scripts/fetch-libduckdb.sh`,
//!   which CI still runs: one list, so the tool and CI cannot disagree. They move into a `paddock-runtime` catalogue when CI
//!   calls the tool (R2).
//! - **The archive** is checked against its pin before the unpacked folder appears (`<cache>/<version>/<asset>/`, with a
//!   `.verified` marker), as one job: download, verify, unpack.
//! - **`--cargo-config`** writes `.cargo/libduckdb.toml`, the machine-local file Debrief's committed `.cargo/config.toml`
//!   includes, so plain `cargo build` and `cargo test` link this library.

use super::error::FetchError;
use super::runner::Runner;
use paddock_archive::{Options, extract_job};
use paddock_download::{Integrity, Request, Sha256Digest};
use paddock_jobs::JobError;
use std::path::{Component, Path, PathBuf};

/// Debrief's folder, relative to the repository root.
pub const APP: &str = "apps/debrief";
/// The pin list, relative to the app.
pub const PIN_SCRIPT: &str = "crates/debrief-store-duck/scripts/fetch-libduckdb.sh";
/// The machine-local Cargo file this command owns, next to the committed `config.toml`.
pub const LOCAL_CONFIG: &str = "libduckdb.toml";
/// Largest archive accepted (the official ones are about 20 to 40 MB).
const MAX_ARCHIVE: u64 = 512 * 1024 * 1024;

/// One pinned release asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    /// The asset name (`libduckdb-linux-amd64.zip`).
    pub asset: String,
    /// Lower-case hex SHA-256 of the archive.
    pub sha256: String,
}

/// The pins of `fetch-libduckdb.sh`: its `VERSION=` and every `ASSET=...; SHA=...` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pins {
    /// The DuckDB release.
    pub version: String,
    /// The pinned assets.
    pub assets: Vec<Pin>,
}

impl Pins {
    /// Parses the pins out of the fetch script's text.
    pub fn parse(script: &str) -> Result<Pins, FetchError> {
        let version = script
            .lines()
            .find_map(|l| l.strip_prefix("VERSION="))
            .map(str::trim)
            .filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'))
            .ok_or_else(|| FetchError::failed("fetch-libduckdb.sh names no VERSION."))?
            .to_owned();
        let assets: Vec<Pin> = script.lines().filter_map(parse_pin_line).collect();
        if assets.is_empty() {
            return Err(FetchError::failed("fetch-libduckdb.sh pins no assets."));
        }
        Ok(Pins { version, assets })
    }

    /// The pinned asset for a Rust `std::env::consts::OS` and `ARCH`, or an error naming what is pinned.
    pub fn asset_for(&self, os: &str, arch: &str) -> Result<&Pin, FetchError> {
        let cpu = match arch {
            "x86_64" => Some("amd64"),
            "aarch64" => Some("arm64"),
            _ => None,
        };
        let name = match (os, cpu) {
            ("macos", _) => Some("libduckdb-osx-universal.zip".to_owned()),
            ("windows", Some(cpu)) => Some(format!("libduckdb-windows-{cpu}.zip")),
            ("linux", Some(cpu)) => Some(format!("libduckdb-linux-{cpu}.zip")),
            _ => None,
        };
        name.and_then(|n| self.assets.iter().find(|p| p.asset == n)).ok_or_else(|| {
            let pinned: Vec<&str> = self.assets.iter().map(|p| p.asset.as_str()).collect();
            FetchError::failed(format!("No pinned libduckdb for {os}-{arch} (pinned: {}).", pinned.join(", ")))
        })
    }

    /// The release download URL of `pin`.
    pub fn url(&self, pin: &Pin) -> String {
        format!("https://github.com/duckdb/duckdb/releases/download/v{}/{}", self.version, pin.asset)
    }
}

/// `ASSET=libduckdb-....zip; SHA=<64 hex>` anywhere in a line.
fn parse_pin_line(line: &str) -> Option<Pin> {
    let rest = &line[line.find("ASSET=")? + "ASSET=".len()..];
    let (asset, rest) = rest.split_once("; SHA=")?;
    let sha: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
    let valid_asset = asset.starts_with("libduckdb-")
        && asset.ends_with(".zip")
        && asset.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    (valid_asset && sha.len() == 64).then(|| Pin {
        asset: asset.to_owned(),
        sha256: sha.to_ascii_lowercase(),
    })
}

/// The folder an asset unpacks into: `<cache>/<version>/<asset without .zip>`.
pub fn unpack_dir(cache: &Path, version: &str, pin: &Pin) -> PathBuf {
    let stem = pin.asset.strip_suffix(".zip").unwrap_or(&pin.asset);
    cache.join(version).join(stem)
}

/// The cache folder when none is given: `<CARGO_TARGET_DIR or app/target>/libduckdb`. A relative `CARGO_TARGET_DIR` is
/// relative to the app, as Cargo reads it there.
pub fn default_cache(app: &Path, env_target: Option<&str>) -> PathBuf {
    match env_target.filter(|t| !t.is_empty()) {
        Some(t) => app.join(t),
        None => app.join("target"),
    }
    .join("libduckdb")
}

/// What a fetch left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// The DuckDB release.
    pub version: String,
    /// The asset used.
    pub asset: String,
    /// The folder holding the library and headers.
    pub dir: PathBuf,
    /// True when the folder was already verified and nothing was downloaded.
    pub cached: bool,
}

/// Fetches (when not cached), verifies and unpacks the pinned library for `os` and `arch` into `cache`.
pub fn fetch(runner: &Runner, pins: &Pins, os: &str, arch: &str, cache: &Path) -> Result<Fetched, FetchError> {
    let pin = pins.asset_for(os, arch)?.clone();
    let dir = unpack_dir(cache, &pins.version, &pin);
    let fetched = |cached| Fetched {
        version: pins.version.clone(),
        asset: pin.asset.clone(),
        dir: dir.clone(),
        cached,
    };
    if dir.join(".verified").is_file() {
        return Ok(fetched(true));
    }
    let digest = Sha256Digest::from_hex(&pin.sha256)
        .map_err(|e| FetchError::failed(format!("The pinned SHA-256 of {} is not valid: {e}.", pin.asset)))?;
    let (url, folder) = (pins.url(&pin), dir.clone());
    let downloader = runner.downloader().clone();
    let name = pin.asset.clone();
    let subject = format!("libduckdb {}", pins.version);
    runner.run("fetch", "Fetching libduckdb", Some(&subject), move |ctx| {
        let zip = folder.with_file_name(format!("{name}.download"));
        let request = Request::new(&url, &zip, Integrity::Sha256(digest))?.max_bytes(MAX_ARCHIVE);
        downloader.download(ctx, &request)?;
        if let Some(parent) = folder.parent() {
            std::fs::create_dir_all(parent)?;
        }
        extract_job(ctx, &zip, &folder, &Options::default())?;
        std::fs::write(folder.join(".verified"), b"")?;
        let _ = std::fs::remove_file(&zip);
        Ok::<(), JobError>(())
    })?;
    Ok(fetched(false))
}

/// The shell the printed lines are for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    /// POSIX sh and bash: `eval "$(./dev fetch libduckdb)"`.
    Sh,
    /// PowerShell: `./dev fetch libduckdb --shell powershell | Invoke-Expression`.
    PowerShell,
}

impl Shell {
    /// PowerShell on Windows, sh elsewhere.
    pub fn host() -> Shell {
        if cfg!(windows) { Shell::PowerShell } else { Shell::Sh }
    }

    /// Parses `sh` or `powershell`.
    pub fn parse(name: &str) -> Option<Shell> {
        match name {
            "sh" => Some(Shell::Sh),
            "powershell" => Some(Shell::PowerShell),
            _ => None,
        }
    }
}

fn slashes(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// The lines that point a build and the loader at the library: the two variables the `duckdb` crate reads and the loader path
/// the executable needs (`PATH` on Windows, `LD_LIBRARY_PATH` on Linux, `DYLD_LIBRARY_PATH` on macOS).
pub fn export_lines(shell: Shell, os: &str, dir: &Path) -> Vec<String> {
    let d = slashes(dir);
    let loader = match os {
        "windows" => Some("PATH"),
        "linux" => Some("LD_LIBRARY_PATH"),
        "macos" => Some("DYLD_LIBRARY_PATH"),
        _ => None,
    };
    match shell {
        Shell::Sh => {
            let mut v = vec![format!("export DUCKDB_LIB_DIR='{d}' DUCKDB_INCLUDE_DIR='{d}'")];
            v.extend(loader.map(|l| format!("export {l}='{d}':\"${{{l}:-}}\"")));
            v
        }
        Shell::PowerShell => {
            let sep = if os == "windows" { ';' } else { ':' };
            let mut v = vec![format!("$env:DUCKDB_LIB_DIR = '{d}'"), format!("$env:DUCKDB_INCLUDE_DIR = '{d}'")];
            v.extend(loader.map(|l| format!("$env:{l} = '{d}{sep}' + $env:{l}")));
            v
        }
    }
}

/// How `.cargo/libduckdb.toml` names a folder: relative to the workspace when it lies inside it (portable, no machine path),
/// absolute otherwise. Separators are always `/`, which Cargo and the linkers accept on Windows too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigPath {
    /// Inside the workspace.
    Relative(String),
    /// Anywhere else.
    Absolute(String),
}

impl ConfigPath {
    /// Names `dir` relative to `root` when it is inside it.
    pub fn new(root: &Path, dir: &Path) -> ConfigPath {
        match dir.strip_prefix(root) {
            Ok(rel) if rel.components().all(|c| matches!(c, Component::Normal(_))) => ConfigPath::Relative(slashes(rel)),
            _ => ConfigPath::Absolute(slashes(dir)),
        }
    }

    fn toml(&self) -> String {
        match self {
            ConfigPath::Relative(p) => format!("{{ value = {}, relative = true }}", toml_string(p)),
            ConfigPath::Absolute(p) => format!("{{ value = {} }}", toml_string(p)),
        }
    }
}

fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The whole of `.cargo/libduckdb.toml`. The command owns the file and rewrites it on every run.
pub fn render_local_config(version: &str, asset: &str, lib_dir: &ConfigPath) -> String {
    let value = lib_dir.toml();
    format!(
        "# Written by `./dev fetch libduckdb --cargo-config` (libduckdb {version}, {asset}); rerun it to update, delete the file\n\
         # to stop using it. Machine-local: git-ignored, included by .cargo/config.toml. A DUCKDB_LIB_DIR set in the\n\
         # environment takes precedence over these defaults.\n\
         [env]\n\
         DUCKDB_LIB_DIR = {value}\n\
         DUCKDB_INCLUDE_DIR = {value}\n"
    )
}

/// Whether the committed `.cargo/config.toml` includes the local file (optionally), so Cargo reads it.
pub fn config_includes_local(config: &str) -> bool {
    let Ok(doc) = config.parse::<toml_edit::DocumentMut>() else {
        return false;
    };
    let Some(includes) = doc.get("include").and_then(toml_edit::Item::as_array) else {
        return false;
    };
    includes.iter().any(|i| match i {
        toml_edit::Value::String(p) => p.value() == LOCAL_CONFIG,
        toml_edit::Value::InlineTable(t) => t.get("path").and_then(toml_edit::Value::as_str) == Some(LOCAL_CONFIG),
        _ => false,
    })
}

/// Writes `<app>/.cargo/libduckdb.toml` for `fetched`; refuses when the committed config does not include it.
pub fn write_cargo_config(app: &Path, fetched: &Fetched) -> Result<PathBuf, FetchError> {
    let cargo_dir = app.join(".cargo");
    let config = std::fs::read_to_string(cargo_dir.join("config.toml")).unwrap_or_default();
    if !config_includes_local(&config) {
        return Err(FetchError::with_hint(
            format!("{}/.cargo/config.toml does not include {LOCAL_CONFIG}.", app.display()),
            format!("Add `include = [{{ path = \"{LOCAL_CONFIG}\", optional = true }}]` at its top."),
        ));
    }
    let local = cargo_dir.join(LOCAL_CONFIG);
    let text = render_local_config(&fetched.version, &fetched.asset, &ConfigPath::new(app, &fetched.dir));
    std::fs::write(&local, text).map_err(|e| FetchError::io(local.display(), &e))?;
    Ok(local)
}

/// Reads the pins of the checkout at `root`.
pub fn read_pins(root: &Path) -> Result<Pins, FetchError> {
    let path = root.join(APP).join(PIN_SCRIPT);
    let script = std::fs::read_to_string(&path).map_err(|e| FetchError::io(path.display(), &e))?;
    Pins::parse(&script)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{FakeNet, runner, sha256_hex, zip_of};
    use super::*;

    const SCRIPT: &str = "set -eu\n\nVERSION=1.5.6\nCACHE=x\n\
        case x in\n  MINGW*-x86_64)\n    ASSET=libduckdb-windows-amd64.zip; SHA=44cf59583f9951d2cb09b1bf115a63ecb2d8901e363903029d86c7d8683fe96a ;;\n\
          Linux-aarch64)\n    ASSET=libduckdb-linux-arm64.zip; SHA=B72ED9F05003F5E9D2015F7CEADA6416B377D9DD33169CDDE3C9E33897856EEE ;;\n\
          Darwin-*)\n    ASSET=libduckdb-osx-universal.zip; SHA=e0bc007d9b0094c0970ac1847a8601d10aad07cbd2910ce586ec810f77b638d6 ;;\n\
          Bad)\n    ASSET=libduckdb-x.zip; SHA=abc ;;\n    ASSET=../evil.zip; SHA=e0bc007d9b0094c0970ac1847a8601d10aad07cbd2910ce586ec810f77b638d6 ;;\nesac\n";

    #[test]
    fn parses_the_version_and_every_well_formed_pin() {
        let pins = Pins::parse(SCRIPT).unwrap();
        assert_eq!(pins.version, "1.5.6");
        let names: Vec<&str> = pins.assets.iter().map(|p| p.asset.as_str()).collect();
        assert_eq!(
            names,
            [
                "libduckdb-windows-amd64.zip",
                "libduckdb-linux-arm64.zip",
                "libduckdb-osx-universal.zip"
            ]
        );
        assert_eq!(
            pins.assets[1].sha256,
            "b72ed9f05003f5e9d2015f7ceada6416b377d9dd33169cdde3c9e33897856eee"
        );
        assert_eq!(
            pins.url(&pins.assets[0]),
            "https://github.com/duckdb/duckdb/releases/download/v1.5.6/libduckdb-windows-amd64.zip"
        );
    }

    #[test]
    fn a_script_without_version_or_pins_is_refused() {
        assert_eq!(
            Pins::parse("ASSET=libduckdb-a.zip; SHA=00").unwrap_err().to_string(),
            "fetch-libduckdb.sh names no VERSION."
        );
        assert_eq!(
            Pins::parse("VERSION=1.0\n").unwrap_err().to_string(),
            "fetch-libduckdb.sh pins no assets."
        );
        assert!(Pins::parse("VERSION=1.0;rm -rf /\nASSET=libduckdb-a.zip; SHA=0").is_err());
    }

    #[test]
    fn the_committed_script_pins_every_supported_host() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pins = read_pins(&root).unwrap();
        for (os, arch) in [
            ("windows", "x86_64"),
            ("windows", "aarch64"),
            ("linux", "x86_64"),
            ("linux", "aarch64"),
            ("macos", "x86_64"),
            ("macos", "aarch64"),
        ] {
            assert!(pins.asset_for(os, arch).is_ok(), "{os}-{arch}");
        }
        assert!(read_pins(Path::new("/nonexistent-paddock-root")).is_err());
    }

    #[test]
    fn picks_the_asset_for_the_host_or_names_what_is_pinned() {
        let pins = Pins::parse(SCRIPT).unwrap();
        assert_eq!(pins.asset_for("windows", "x86_64").unwrap().asset, "libduckdb-windows-amd64.zip");
        assert_eq!(pins.asset_for("macos", "aarch64").unwrap().asset, "libduckdb-osx-universal.zip");
        let err = pins.asset_for("linux", "x86_64").unwrap_err().to_string();
        assert!(
            err.starts_with("No pinned libduckdb for linux-x86_64 (pinned: libduckdb-windows-amd64.zip"),
            "{err}"
        );
        assert!(pins.asset_for("freebsd", "x86_64").is_err());
        assert!(pins.asset_for("windows", "riscv64").is_err());
    }

    #[test]
    fn folders_follow_the_target_dir() {
        let pin = Pin {
            asset: "libduckdb-linux-amd64.zip".into(),
            sha256: String::new(),
        };
        assert_eq!(
            unpack_dir(Path::new("t"), "1.5.6", &pin),
            Path::new("t").join("1.5.6").join("libduckdb-linux-amd64")
        );
        let app = Path::new("app");
        assert_eq!(default_cache(app, None), app.join("target").join("libduckdb"));
        assert_eq!(default_cache(app, Some("")), app.join("target").join("libduckdb"));
        assert_eq!(default_cache(app, Some("other")), app.join("other").join("libduckdb"));
    }

    #[test]
    fn fetches_verifies_unpacks_and_then_reuses() {
        let zip = zip_of(&[("duckdb.h", b"/* header */", 0o644), ("libduckdb.so", b"lib", 0o755)]);
        let script = format!(
            "VERSION=1.5.6\n  Linux-x86_64)\n    ASSET=libduckdb-linux-amd64.zip; SHA={} ;;\n",
            sha256_hex(&zip)
        );
        let pins = Pins::parse(&script).unwrap();
        let net = FakeNet::new();
        net.serve(&pins.url(&pins.assets[0]), zip);
        let work = tempfile::tempdir().unwrap();
        let cache = work.path().join("libduckdb");
        let r = runner(&net, 1);
        let first = fetch(&r, &pins, "linux", "x86_64", &cache).unwrap();
        assert!(!first.cached);
        assert_eq!(first.dir, cache.join("1.5.6").join("libduckdb-linux-amd64"));
        assert_eq!(std::fs::read(first.dir.join("duckdb.h")).unwrap(), b"/* header */");
        assert!(first.dir.join(".verified").is_file());
        assert!(!cache.join("1.5.6").join("libduckdb-linux-amd64.zip.download").exists());
        let asked = net.seen().len();
        assert!(fetch(&r, &pins, "linux", "x86_64", &cache).unwrap().cached);
        assert_eq!(net.seen().len(), asked);
    }

    #[test]
    fn a_tampered_archive_is_refused_and_leaves_no_folder() {
        let zip = zip_of(&[("duckdb.h", b"x", 0o644)]);
        let script = format!(
            "VERSION=1.5.6\n    ASSET=libduckdb-linux-amd64.zip; SHA={} ;;\n",
            sha256_hex(b"something else")
        );
        let pins = Pins::parse(&script).unwrap();
        let net = FakeNet::new();
        net.serve(&pins.url(&pins.assets[0]), zip);
        let work = tempfile::tempdir().unwrap();
        let err = fetch(&runner(&net, 1), &pins, "linux", "x86_64", work.path()).unwrap_err();
        assert!(matches!(err, FetchError::Failed { .. }), "{err:?}");
        assert!(!work.path().join("1.5.6").join("libduckdb-linux-amd64").exists());
        let bad = Pins {
            version: "1".into(),
            assets: vec![Pin {
                asset: "libduckdb-linux-amd64.zip".into(),
                sha256: "zz".into(),
            }],
        };
        assert!(
            fetch(&runner(&net, 1), &bad, "linux", "x86_64", work.path())
                .unwrap_err()
                .to_string()
                .contains("not valid")
        );
    }

    #[test]
    fn export_lines_per_shell_and_os() {
        let d = Path::new("/c/lib");
        assert_eq!(
            export_lines(Shell::Sh, "linux", d),
            [
                "export DUCKDB_LIB_DIR='/c/lib' DUCKDB_INCLUDE_DIR='/c/lib'",
                "export LD_LIBRARY_PATH='/c/lib':\"${LD_LIBRARY_PATH:-}\""
            ]
        );
        assert!(export_lines(Shell::Sh, "macos", d)[1].starts_with("export DYLD_LIBRARY_PATH="));
        assert!(export_lines(Shell::Sh, "windows", d)[1].starts_with("export PATH="));
        assert_eq!(export_lines(Shell::Sh, "freebsd", d).len(), 1);
        let ps = export_lines(Shell::PowerShell, "windows", Path::new("C:\\x\\lib"));
        assert_eq!(
            ps,
            [
                "$env:DUCKDB_LIB_DIR = 'C:/x/lib'",
                "$env:DUCKDB_INCLUDE_DIR = 'C:/x/lib'",
                "$env:PATH = 'C:/x/lib;' + $env:PATH"
            ]
        );
        assert!(export_lines(Shell::PowerShell, "linux", d)[2].contains("/c/lib:' + $env:LD_LIBRARY_PATH"));
        assert_eq!(Shell::parse("sh"), Some(Shell::Sh));
        assert_eq!(Shell::parse("powershell"), Some(Shell::PowerShell));
        assert_eq!(Shell::parse("fish"), None);
        assert_eq!(Shell::host(), if cfg!(windows) { Shell::PowerShell } else { Shell::Sh });
    }

    #[test]
    fn config_paths_are_workspace_relative_when_inside_it() {
        let root = Path::new("/work/repo").to_path_buf();
        let inside = root.join("target").join("libduckdb").join("1.5.6").join("libduckdb-windows-amd64");
        assert_eq!(
            ConfigPath::new(&root, &inside),
            ConfigPath::Relative("target/libduckdb/1.5.6/libduckdb-windows-amd64".into())
        );
        let outside = Path::new("/work/elsewhere").join("libduckdb");
        assert!(
            matches!(ConfigPath::new(&root, &outside), ConfigPath::Absolute(p) if !p.contains('\\') && p.ends_with("elsewhere/libduckdb"))
        );
        assert!(matches!(
            ConfigPath::new(&root, &root.join("..").join("x")),
            ConfigPath::Absolute(_)
        ));
    }

    #[test]
    fn the_local_config_is_valid_toml_with_both_variables() {
        for path in [
            ConfigPath::Relative("target/libduckdb/1.5.6/a".into()),
            ConfigPath::Absolute("C:/Users/x \"y\"/a\\b\u{1}".into()),
        ] {
            let text = render_local_config("1.5.6", "libduckdb-windows-amd64.zip", &path);
            let doc: toml_edit::DocumentMut = text.parse().unwrap();
            for key in ["DUCKDB_LIB_DIR", "DUCKDB_INCLUDE_DIR"] {
                let entry = doc["env"][key].as_inline_table().unwrap();
                match &path {
                    ConfigPath::Relative(p) => {
                        assert_eq!(entry.get("value").and_then(toml_edit::Value::as_str), Some(p.as_str()));
                        assert_eq!(entry.get("relative").and_then(toml_edit::Value::as_bool), Some(true));
                    }
                    ConfigPath::Absolute(p) => {
                        assert_eq!(entry.get("value").and_then(toml_edit::Value::as_str), Some(p.as_str()));
                        assert!(entry.get("relative").is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn recognises_the_include_of_the_local_config() {
        assert!(config_includes_local(
            "include = [{ path = \"libduckdb.toml\", optional = true }]\n[alias]\nx = \"y\"\n"
        ));
        assert!(config_includes_local("include = [\"other.toml\", \"libduckdb.toml\"]\n"));
        assert!(!config_includes_local("include = [\"other.toml\", 3]\n"));
        assert!(!config_includes_local("include = \"libduckdb.toml\"\n"));
        assert!(!config_includes_local("[alias]\nxtask = \"run -p xtask --\"\n"));
        assert!(!config_includes_local("not toml ["));
        assert!(!config_includes_local(""));
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(APP);
        let committed = std::fs::read_to_string(root.join(".cargo").join("config.toml")).unwrap();
        assert!(
            config_includes_local(&committed),
            "Debrief's committed .cargo/config.toml includes the local file"
        );
    }

    #[test]
    fn writes_the_local_config_only_when_cargo_reads_it() {
        let app = tempfile::tempdir().unwrap();
        let fetched = Fetched {
            version: "1.5.6".into(),
            asset: "libduckdb-linux-amd64.zip".into(),
            dir: app.path().join("target/libduckdb/1.5.6/libduckdb-linux-amd64"),
            cached: false,
        };
        let err = write_cargo_config(app.path(), &fetched).unwrap_err();
        assert!(matches!(&err, FetchError::Failed { hint: Some(h), .. } if h.contains("optional = true")));
        std::fs::create_dir_all(app.path().join(".cargo")).unwrap();
        std::fs::write(
            app.path().join(".cargo/config.toml"),
            "include = [{ path = \"libduckdb.toml\", optional = true }]\n",
        )
        .unwrap();
        let path = write_cargo_config(app.path(), &fetched).unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(
            text.contains("DUCKDB_LIB_DIR = { value = \"target/libduckdb/1.5.6/libduckdb-linux-amd64\", relative = true }"),
            "{text}"
        );
        std::fs::remove_dir_all(app.path().join(".cargo")).unwrap();
        std::fs::create_dir_all(app.path().join(".cargo").join(LOCAL_CONFIG)).unwrap();
        std::fs::write(app.path().join(".cargo/config.toml"), "include = [\"libduckdb.toml\"]\n").unwrap();
        assert!(
            write_cargo_config(app.path(), &fetched).is_err(),
            "a directory where the file belongs"
        );
    }
}
