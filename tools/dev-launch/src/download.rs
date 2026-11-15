//! The prebuilt download (ADR 0022 R0b): fetches the tool published for a hash and compiler target from the Gitea generic
//! package `paddock-dev` (version `<hash>`, file `<name>`, checksum `<name>.sha256`) with `curl`, checks the checksum and that
//! the binary reports the hash it was built for, then publishes it into the cache with an atomic rename. Any miss or doubt is a
//! `::notice::` or `::warning::` and the caller builds: a binary that was not checked is never run.

use crate::proc;
use crate::sha256;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Whether this run downloads: `PADDOCK_DEV_DOWNLOAD=1` yes, `0` no, otherwise only on a CI runner (`GITHUB_ACTIONS=true`).
pub fn enabled() -> bool {
    match std::env::var("PADDOCK_DEV_DOWNLOAD").as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true"),
    }
}

/// A folder name no other run makes: for a scratch folder inside a cache the containers of several runs share.
pub fn unique(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{prefix}{:x}{:x}{}", nanos, std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

/// Marks `path` executable (a no-op where there is no such bit).
pub fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Whether `binary self hash` reports `embedded <hash>` (the binary was built for that hash).
pub fn reports_hash(binary: &Path, hash: &str) -> bool {
    let mut c = proc::command(binary);
    c.args(["self", "hash"]);
    let out = proc::capture(c, Duration::from_secs(60), None);
    out.ok() && out.stdout.lines().any(|l| l == format!("embedded {hash}"))
}

/// What a download needs to know.
pub struct Fetch<'a> {
    /// The content address of the tool.
    pub hash: &'a str,
    /// The file name for this host (`paddock-dev-<host>[.exe]`).
    pub name: &'a str,
    /// The compiler's host triple, for the messages.
    pub host: &'a str,
    /// The cache entry folder the binary goes into.
    pub cache: &'a Path,
    /// A line that explains the hash's inputs, printed on a 404 (ER-27).
    pub explain: &'a dyn Fn(),
}

/// The base URL of the package registry.
fn base() -> String {
    std::env::var("PADDOCK_DEV_PACKAGES")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "https://dev.decryptology.net/api/packages/paddock-build-inputs/generic".to_string())
}

/// The read token: `PADDOCK_DEV_TOKEN`, else the job's `GITHUB_TOKEN`. Never printed, never on a command line.
fn token() -> String {
    ["PADDOCK_DEV_TOKEN", "GITHUB_TOKEN"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default()
}

/// Downloads, checks and caches the published tool. Returns whether `cache/<name>` now holds it.
pub fn prebuilt(f: &Fetch) -> bool {
    let base = base();
    let timeout: u64 = std::env::var("PADDOCK_DEV_DOWNLOAD_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(60);
    if !crate::toolchain::on_path("curl") {
        eprintln!("::notice::./dev cannot download the prebuilt paddock-dev: curl is not installed. Building it.");
        return false;
    }
    let dl: PathBuf = f.cache.join(unique(".download."));
    if std::fs::create_dir_all(&dl).is_err() {
        eprintln!(
            "::notice::./dev cannot write the cache folder {}, so it cannot keep a download. Building it.",
            f.cache.display()
        );
        return false;
    }
    let done = |ok: bool| {
        let _ = std::fs::remove_dir_all(&dl);
        ok
    };
    let tokinfo = if token().is_empty() {
        "no read token is set: export PADDOCK_DEV_TOKEN"
    } else {
        "a read token was sent"
    };
    let t0 = Instant::now();
    let name = f.name;
    for file in [format!("{name}.sha256"), name.to_string()] {
        // The token goes in through curl's config on stdin, so no process list shows it.
        let cfg = format!("header = \"Authorization: token {}\"\n", token());
        let url = format!("{base}/paddock-dev/{}/{file}", f.hash);
        let mut c = proc::command("curl");
        c.args([
            "-sS",
            "-K",
            "-",
            "--connect-timeout",
            "10",
            "--max-time",
            &timeout.to_string(),
            "-o",
        ])
        .arg(dl.join(&file))
        .args(["-w", "%{http_code}", &url]);
        let out = proc::capture(c, Duration::from_secs(timeout + 30), Some(cfg.as_bytes()));
        let code = if out.ok() {
            out.stdout.trim().to_string()
        } else {
            "000".to_string()
        };
        match code.as_str() {
            "200" => {}
            "404" => {
                eprintln!(
                    "::notice::./dev: paddock-dev {} is not published for {} (HTTP 404 for {file}); building it.",
                    f.hash, f.host
                );
                (f.explain)();
                return done(false);
            }
            _ => {
                let said: String = out.stderr.chars().take(300).map(|c| if c == '\n' { ' ' } else { c }).collect();
                eprintln!(
                    "::warning::./dev could not download {url} (HTTP {code} after {} s, timeout {timeout} s, {tokinfo}): {said} Building it instead.",
                    t0.elapsed().as_secs()
                );
                return done(false);
            }
        }
    }
    let want = std::fs::read_to_string(dl.join(format!("{name}.sha256")))
        .unwrap_or_default()
        .replace('\r', "")
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    let got = std::fs::read(dl.join(name)).map(|b| sha256::hex(&b)).unwrap_or_default();
    if want.is_empty() || want != got {
        let shown = if got.is_empty() { "unknown" } else { got.as_str() };
        let said = if want.is_empty() { "nothing" } else { want.as_str() };
        eprintln!(
            "::warning::./dev: the downloaded {name} has checksum {shown}, the published one says {said}: the checksum does not match, so it is not used. Building it instead."
        );
        return done(false);
    }
    make_executable(&dl.join(name));
    if !reports_hash(&dl.join(name), f.hash) {
        eprintln!(
            "::warning::./dev: the downloaded {name} is not for hash {} (it reports another one, or none). Building it instead.",
            f.hash
        );
        return done(false);
    }
    if std::fs::rename(dl.join(name), f.cache.join(name)).is_err() {
        eprintln!(
            "::warning::./dev could not move the downloaded {name} into {}. Building it instead.",
            f.cache.display()
        );
        return done(false);
    }
    let _ = std::fs::remove_dir_all(&dl);
    eprintln!(
        "dev: downloaded paddock-dev {} for {} in {} s, transport checks passed (checksum, embedded hash).",
        f.hash,
        f.host,
        t0.elapsed().as_secs()
    );
    true
}
