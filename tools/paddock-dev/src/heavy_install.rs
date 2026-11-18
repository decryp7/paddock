//! `dev heavy-install`: puts the `heavy` binary of a release (tools/heavy, `.gitea/workflows/heavy-release.yml`) into
//! `<dev-targets>/heavy/bin/heavy`, the one place engineering-rules' `heavy.sh` wrapper looks after `HEAVY_BIN` and PATH.
//!
//! It downloads the asset for this OS and CPU and the release's `SHA256SUMS` from the user's own Gitea
//! (`PADDOCK_HEAVY_RELEASES`, default `https://dev.decryptology.net/decryp7/paddock/releases/download`, then `heavy-v<version>/<file>`),
//! refuses a checksum that does not match (an `::error::` line, nothing installed), publishes the binary with an atomic rename and
//! records the version beside it (`<dev-targets>/heavy/VERSION`). A rerun of the same version that finds the binary unchanged
//! downloads nothing. The token (`PADDOCK_DEV_TOKEN`, else `GITHUB_TOKEN`, as `./dev` itself reads) reaches curl through a
//! private config file and is never printed. Nothing here touches `~/.cargo` or `~/.rustup`: the download is not a `cargo install`.

use crate::ci_setup_rust::sha256_file;
use crate::ci_tool::{NETWORK_ENV, ToolCall, tail};
use clap::Args;
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where the releases are (a test points it at a fake server).
pub const RELEASES_ENV: &str = "PADDOCK_HEAVY_RELEASES";
/// The user's own Gitea.
pub const DEFAULT_RELEASES: &str = "https://dev.decryptology.net/decryp7/paddock/releases/download";

/// Arguments of `dev heavy-install`.
#[derive(Args, Debug)]
pub struct InstallArgs {
    /// The version to install (default: the version of this checkout's tools/heavy/Cargo.toml).
    #[arg(long)]
    pub version: Option<String>,
}

/// The Rust target the release has a binary for, from `std::env::consts` names; `Err` for any other machine.
pub fn target_of(os: &str, arch: &str) -> Result<&'static str, String> {
    match (os, arch) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        _ => Err(format!(
            "there is no heavy release for {os} on {arch} (macOS arm64, Linux x86_64 and arm64, Windows x86_64 have one)"
        )),
    }
}

/// The release asset of a version for a machine: `heavy-<version>-<target>` (`.exe` on Windows), as the release workflow names it.
pub fn asset_name(version: &str, os: &str, arch: &str) -> Result<String, String> {
    let exe = if os == "windows" { ".exe" } else { "" };
    Ok(format!("heavy-{version}-{}{exe}", target_of(os, arch)?))
}

/// The checksum `SHA256SUMS` (`<hex>  <file>` per line, `sha256sum`'s output) lists for `asset`.
pub fn find_sum(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hex, name) = l.trim_end_matches('\r').split_once(char::is_whitespace)?;
        (name.trim_start().trim_start_matches('*') == asset).then(|| hex.to_ascii_lowercase())
    })
}

/// What `VERSION` records: the version, the binary's checksum and the asset it came from.
#[derive(Debug, PartialEq, Eq)]
pub struct Record {
    /// The installed version.
    pub version: String,
    /// The checksum of the installed binary.
    pub sha256: String,
    /// The release asset it came from.
    pub asset: String,
}

impl Record {
    fn format(&self) -> String {
        format!("{}\n{}\n{}\n", self.version, self.sha256, self.asset)
    }
    fn parse(text: &str) -> Option<Record> {
        let mut l = text.lines();
        Some(Record {
            version: l.next()?.to_string(),
            sha256: l.next()?.to_string(),
            asset: l.next()?.to_string(),
        })
    }
}

/// What a run did.
#[derive(Debug, PartialEq, Eq)]
pub enum Done {
    /// The same version was there and unchanged: nothing was downloaded.
    AlreadyInstalled(PathBuf),
    /// Installed (downloaded, checked, renamed into place).
    Installed(PathBuf),
}

/// What one install needs; the machine's parts are arguments so a test can name them.
pub struct Install<'a> {
    /// `<dev-targets>/heavy`.
    pub dir: &'a Path,
    /// The version to install.
    pub version: &'a str,
    /// The releases' base URL.
    pub base: &'a str,
    /// `std::env::consts::OS`.
    pub os: &'a str,
    /// `std::env::consts::ARCH`.
    pub arch: &'a str,
    /// The read token, when there is one.
    pub token: Option<&'a str>,
    /// The caller's environment (curl gets the network variables).
    pub env: &'a HashMap<String, String>,
}

fn error(cause: &str, next: &str) -> String {
    format!("::error::heavy-install: {cause} Next step: {next}")
}

fn download(i: &Install, work: &Path, file: &str) -> Result<PathBuf, String> {
    let url = format!("{}/heavy-v{}/{file}", i.base.trim_end_matches('/'), i.version);
    let dest = work.join(file);
    let mut call = ToolCall::new("curl")
        .args(["-fsS", "--retry", "2", "--connect-timeout", "10", "--max-time", "300", "-o"])
        .args([dest.to_string_lossy().into_owned(), url.clone()])
        .pass(NETWORK_ENV.iter().copied())
        .timeout(Duration::from_secs(330));
    if let Some(t) = i.token {
        // The token goes in through a config file only this user reads (so no process list shows it) and is masked if curl echoes it.
        let cfg = work.join("curl.cfg");
        std::fs::write(&cfg, format!("header = \"Authorization: token {t}\"\n"))
            .map_err(|e| format!("cannot write {}: {e}", cfg.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o600));
        }
        call = call.args(["-K".to_string(), cfg.to_string_lossy().into_owned()]).secret(t);
    }
    let ran = call
        .run(i.env, &mut std::io::sink())
        .map_err(|e| error(&format!("cannot start curl ({e})."), "install curl."))?;
    if !ran.success {
        return Err(error(
            &format!("cannot download {url} ({}): {}", ran.how, tail(&ran.stderr, 3).trim()),
            "check that the tag exists and has this asset (heavy-release.yml), that PADDOCK_DEV_TOKEN can read the repository, and run it again.",
        ));
    }
    Ok(dest)
}

/// Installs, or finds the same version already installed. `Err` is the whole `::error::` line.
pub fn install(i: &Install) -> Result<Done, String> {
    let asset = asset_name(i.version, i.os, i.arch).map_err(|e| {
        error(
            &format!("{e}."),
            "build it from tools/heavy (`cargo install --path tools/heavy --locked --root <dir>`).",
        )
    })?;
    let bin = i.dir.join("bin").join(if i.os == "windows" { "heavy.exe" } else { "heavy" });
    let record_path = i.dir.join("VERSION");
    if let (Ok(text), true) = (std::fs::read_to_string(&record_path), bin.is_file())
        && let Some(r) = Record::parse(&text)
        && r.version == i.version
        && r.asset == asset
        && sha256_file(&bin).is_ok_and(|s| s == r.sha256)
    {
        return Ok(Done::AlreadyInstalled(bin));
    }
    std::fs::create_dir_all(i.dir.join("bin")).map_err(|e| {
        error(
            &format!("cannot create {}: {e}.", i.dir.join("bin").display()),
            "make the dev-targets folder writable (PADDOCK_DEV_TARGETS).",
        )
    })?;
    let work = i.dir.join(format!(".download.{}", std::process::id()));
    std::fs::create_dir_all(&work).map_err(|e| {
        error(
            &format!("cannot create {}: {e}.", work.display()),
            "make the dev-targets folder writable.",
        )
    })?;
    let result = (|| {
        let sums = download(i, &work, "SHA256SUMS")?;
        let sums = std::fs::read_to_string(&sums).map_err(|e| error(&format!("cannot read SHA256SUMS: {e}."), "run it again."))?;
        let want = find_sum(&sums, &asset).ok_or_else(|| {
            error(
                &format!("SHA256SUMS of heavy-v{} does not list {asset}.", i.version),
                "the release was built without this OS; check heavy-release.yml's matrix.",
            )
        })?;
        let file = download(i, &work, &asset)?;
        let got = sha256_file(&file).map_err(|e| error(&format!("cannot read the download: {e}."), "run it again."))?;
        if got != want {
            return Err(error(
                &format!("the downloaded {asset} has checksum {got}, SHA256SUMS says {want}: it is not installed."),
                "run it again; if it repeats, the release asset is damaged: request the tag again.",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| error(&format!("cannot make it executable: {e}."), "run it again."))?;
        }
        let part = bin.with_extension(format!("part.{}", std::process::id()));
        std::fs::copy(&file, &part).map_err(|e| {
            error(
                &format!("cannot copy into {}: {e}.", part.display()),
                "make the dev-targets folder writable.",
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755));
        }
        std::fs::rename(&part, &bin).map_err(|e| {
            let _ = std::fs::remove_file(&part);
            error(
                &format!("cannot put heavy into {}: {e}.", bin.display()),
                "stop the heavy runs that use it (Windows) and run it again.",
            )
        })?;
        let rec = Record {
            version: i.version.to_string(),
            sha256: got,
            asset: asset.clone(),
        };
        let tmp = i.dir.join(format!("VERSION.part.{}", std::process::id()));
        std::fs::write(&tmp, rec.format())
            .and_then(|()| std::fs::rename(&tmp, &record_path))
            .map_err(|e| error(&format!("cannot record the version: {e}."), "run it again."))?;
        Ok(Done::Installed(bin.clone()))
    })();
    let _ = crate::safe_remove::remove_under(i.dir, &work, None);
    result
}

/// `dev heavy-install`, against the real machine.
pub fn command(a: &InstallArgs) -> Result<Report, CliError> {
    let env: HashMap<String, String> = crate::process_env::vars().collect();
    let cwd = std::env::current_dir().map_err(|e| CliError::failure(format!("current folder: {e}")))?;
    let targets = crate::places::dev_targets(&cwd, &|k| env.get(k).cloned())?;
    let version = match &a.version {
        Some(v) => v.clone(),
        None => checkout_version(&cwd)?,
    };
    let base = env
        .get(RELEASES_ENV)
        .cloned()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_RELEASES.to_string());
    let token = env
        .get("PADDOCK_DEV_TOKEN")
        .or_else(|| env.get("GITHUB_TOKEN"))
        .filter(|t| !t.is_empty());
    let dir = targets.join("heavy");
    let done = install(&Install {
        dir: &dir,
        version: &version,
        base: &base,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        token: token.map(String::as_str),
        env: &env,
    })
    .map_err(|line| {
        eprintln!("{line}");
        CliError::failure("heavy-install failed")
    })?;
    let (bin, what) = match done {
        Done::AlreadyInstalled(b) => (b, "already installed"),
        Done::Installed(b) => (b, "installed"),
    };
    Ok(Report::new(
        json!({ "ok": true, "version": version, "path": bin.display().to_string(), "result": what }),
        format!("heavy {version} {what}: {}\n", bin.display()),
    ))
}

/// The version in this checkout's `tools/heavy/Cargo.toml`.
fn checkout_version(from: &Path) -> Result<String, CliError> {
    let root = crate::worktree::git::Git::at(from)
        .text(&["rev-parse", "--show-toplevel"])
        .map(|t| PathBuf::from(t.trim()))
        .map_err(|e| CliError::failure(format!("heavy-install: {e}; run it in a Paddock checkout or pass --version")))?;
    let text = std::fs::read_to_string(root.join("tools/heavy/Cargo.toml"))
        .map_err(|e| CliError::failure(format!("heavy-install: cannot read tools/heavy/Cargo.toml ({e}); pass --version")))?;
    text.parse::<toml::Table>()
        .ok()
        .and_then(|t| t.get("package")?.get("version")?.as_str().map(str::to_string))
        .ok_or_else(|| CliError::failure("heavy-install: tools/heavy/Cargo.toml has no package version; pass --version"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_asset_of_each_supported_machine_and_the_checksum_line() {
        assert_eq!(asset_name("0.2.0", "macos", "aarch64").unwrap(), "heavy-0.2.0-aarch64-apple-darwin");
        assert_eq!(
            asset_name("0.2.0", "linux", "x86_64").unwrap(),
            "heavy-0.2.0-x86_64-unknown-linux-gnu"
        );
        assert_eq!(
            asset_name("0.2.0", "linux", "aarch64").unwrap(),
            "heavy-0.2.0-aarch64-unknown-linux-gnu"
        );
        assert_eq!(
            asset_name("0.2.0", "windows", "x86_64").unwrap(),
            "heavy-0.2.0-x86_64-pc-windows-msvc.exe"
        );
        assert!(asset_name("0.2.0", "freebsd", "x86_64").is_err());
        let sums = "AB12  heavy-0.2.0-a\r\ncd34 *heavy-0.2.0-b\n";
        assert_eq!(find_sum(sums, "heavy-0.2.0-a").as_deref(), Some("ab12"));
        assert_eq!(find_sum(sums, "heavy-0.2.0-b").as_deref(), Some("cd34"));
        assert_eq!(find_sum(sums, "heavy-0.2.0-c"), None);
        assert_eq!(
            Record::parse(
                &Record {
                    version: "1".into(),
                    sha256: "x".into(),
                    asset: "a".into()
                }
                .format()
            )
            .unwrap()
            .version,
            "1"
        );
    }
}
