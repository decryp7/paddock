//! `dev heavy-install`: puts the `heavy` binary of a release (tools/heavy, `.gitea/workflows/heavy-release.yml`) into
//! `<dev-targets>/heavy/bin/heavy`, the one place engineering-playbook's `heavy.sh` wrapper looks after `HEAVY_BIN` and PATH.
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

/// The file name of the binary on a machine: `heavy.exe` on Windows.
pub fn bin_name(os: &str) -> &'static str {
    if os == "windows" { "heavy.exe" } else { "heavy" }
}

/// Whether `v` is a semantic version (`1.2.3`, `1.2.3-rc.1`): the only thing a release tag and a folder name are built from.
pub fn is_semver(v: &str) -> bool {
    let (core, pre) = v.split_once('-').map_or((v, None), |(c, p)| (c, Some(p)));
    let nums: Vec<&str> = core.split('.').collect();
    nums.len() == 3
        && nums.iter().all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        && pre.is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-'))
}

/// A releases base URL is `https`, or plain http on this machine's own loopback (a test's fake server): a token is never sent in the
/// clear to another host.
pub fn check_base(base: &str) -> Result<(), String> {
    if base.starts_with("https://") {
        return Ok(());
    }
    let host = base.strip_prefix("http://").map(|r| r.split(['/', ':']).next().unwrap_or(""));
    match host {
        Some("127.0.0.1" | "localhost") => Ok(()),
        Some(h) if h.starts_with("[") && base.starts_with("http://[::1]") => {
            let _ = h;
            Ok(())
        }
        _ => Err(format!(
            "{RELEASES_ENV}={base:?} is not an https URL (only a loopback address may be plain http)"
        )),
    }
}

/// Puts `new` at `bin` by rename. When the rename is refused and `bin` exists (Windows: a binary that runs cannot be replaced, but it can
/// be renamed), the old one is moved aside to `<bin>.old.<pid>` first and the new one renamed into place; the old one is removed
/// when it can be (a later run sweeps what is left). `rename` is the system's; a test's fake refuses the first call.
pub fn replace_binary(new: &Path, bin: &Path, rename: &dyn Fn(&Path, &Path) -> std::io::Result<()>) -> std::io::Result<()> {
    match rename(new, bin) {
        Ok(()) => Ok(()),
        Err(first) if bin.exists() => {
            let old = bin.with_extension(format!("old.{}", std::process::id()));
            rename(bin, &old).map_err(|_| first)?;
            if let Err(e) = rename(new, bin) {
                let _ = rename(&old, bin);
                return Err(e);
            }
            let _ = std::fs::remove_file(&old);
            Ok(())
        }
        Err(e) => Err(e),
    }
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

/// Writes `text` to a new file only its owner can read, with that mode from the start (never a moment with a wider one).
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)?.write_all(text.as_bytes())
}

/// Creates the work folder, readable by its owner only.
fn private_dir(path: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(path)
}

/// Removes the `.download.<pid>` folders of runs that are gone (killed before they cleaned up) and the binaries a Windows replace moved
/// aside; a folder of a running process stays. Best effort.
fn sweep(dir: &Path) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(pid) = name.strip_prefix(".download.").and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        let written_ms = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        if pid != std::process::id() && paddock_proc::ProcessId::recorded_presence(pid, written_ms) == paddock_proc::Presence::Gone {
            let _ = crate::safe_remove::remove_under(dir, &e.path(), None);
        }
    }
    let bin = dir.join("bin");
    if let Ok(read) = std::fs::read_dir(&bin) {
        for e in read.flatten() {
            if e.file_name().to_string_lossy().contains(".old.") {
                let _ = crate::safe_remove::remove_under(&bin, &e.path(), None);
            }
        }
    }
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
        write_private(&cfg, &format!("header = \"Authorization: token {t}\"\n")).map_err(|e| {
            error(
                &format!("cannot write {}: {e}.", cfg.display()),
                "make the dev-targets folder writable.",
            )
        })?;
        call = call.args(["-K".to_string(), cfg.to_string_lossy().into_owned()]).secret(t);
    }
    if i.base.starts_with("https://") {
        call = call.args(["--proto", "=https"]);
    }
    let ran = call.run(i.env, &mut std::io::sink());
    // The config, and with it the token, is gone as soon as curl has ended, whatever the outcome.
    let _ = std::fs::remove_file(work.join("curl.cfg"));
    let ran = ran.map_err(|e| error(&format!("cannot start curl ({e})."), "install curl."))?;
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
    check_base(i.base).map_err(|e| error(&format!("{e}."), "use the repository's https release URL."))?;
    if !is_semver(i.version) {
        return Err(error(
            &format!("{:?} is not a version like 1.2.3.", i.version),
            "pass --version X.Y.Z.",
        ));
    }
    let bin = i.dir.join("bin").join(bin_name(i.os));
    let record_path = i.dir.join("VERSION");
    if let (Ok(text), true) = (std::fs::read_to_string(&record_path), bin.is_file())
        && let Some(r) = Record::parse(&text)
        && r.version == i.version
        && r.asset == asset
        && sha256_file(&bin).is_ok_and(|s| s == r.sha256)
    {
        return Ok(Done::AlreadyInstalled(bin));
    }
    sweep(i.dir);
    std::fs::create_dir_all(i.dir.join("bin")).map_err(|e| {
        error(
            &format!("cannot create {}: {e}.", i.dir.join("bin").display()),
            "make the dev-targets folder writable (PADDOCK_DEV_TARGETS).",
        )
    })?;
    let work = i.dir.join(format!(".download.{}", std::process::id()));
    private_dir(&work).map_err(|e| {
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
        // The verified file itself is renamed into place (same disk, a folder only this user reaches): what was hashed is what runs.
        replace_binary(&file, &bin, &|a, b| std::fs::rename(a, b)).map_err(|e| {
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
        paddock_env::write_atomic(&record_path, rec.format().as_bytes(), false)
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
    if !is_semver(&version) {
        return Err(CliError::failure(format!(
            "heavy-install: {version:?} is not a version like 1.2.3; pass --version X.Y.Z"
        )));
    }
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
    fn versions_are_semver_and_only_loopback_may_be_plain_http() {
        for ok in ["0.2.0", "10.20.30", "1.0.0-rc.1"] {
            assert!(is_semver(ok), "{ok}");
        }
        for bad in [
            "",
            "1.2",
            "1.2.3.4",
            "v1.2.3",
            "1.2.3+b",
            "../1.2.3",
            "1.2.x",
            "1.2.3-",
            "1.2.3/../x",
        ] {
            assert!(!is_semver(bad), "{bad}");
        }
        for ok in [
            "https://dev.decryptology.net/r",
            "http://127.0.0.1:9/r",
            "http://localhost:1",
            "http://[::1]:5/x",
        ] {
            assert!(check_base(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://dev.decryptology.net/r",
            "ftp://x",
            "http://127.0.0.1.evil.net/r",
            "dev.decryptology.net",
        ] {
            assert!(check_base(bad).is_err(), "{bad}");
        }
    }

    /// Windows: the name of the binary, and replacing one that is in use (its rename over is refused, but it can be renamed away).
    #[test]
    fn a_windows_binary_in_use_is_moved_aside_and_the_new_one_renamed_into_place() {
        assert_eq!(bin_name("windows"), "heavy.exe");
        assert_eq!(bin_name("linux"), "heavy");
        let d = tempfile::tempdir().unwrap();
        let (new, bin) = (d.path().join("new"), d.path().join("heavy.exe"));
        std::fs::write(&new, "new").unwrap();
        std::fs::write(&bin, "old").unwrap();
        // The first rename of anything onto `bin` is refused, as Windows does for a running program.
        let refused = std::cell::Cell::new(true);
        let rename = |a: &Path, b: &Path| {
            if b == bin && refused.replace(false) {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            std::fs::rename(a, b)
        };
        replace_binary(&new, &bin, &rename).unwrap();
        assert_eq!(std::fs::read_to_string(&bin).unwrap(), "new");
        let left: Vec<String> = std::fs::read_dir(d.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["heavy.exe"], "the old one is removed when it can be: {left:?}");
        // When nothing can be renamed, the old binary is untouched and the error is the first one.
        std::fs::write(&new, "newer").unwrap();
        let never = |_: &Path, _: &Path| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert!(replace_binary(&new, &bin, &never).is_err());
        assert_eq!(std::fs::read_to_string(&bin).unwrap(), "new");
    }

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
