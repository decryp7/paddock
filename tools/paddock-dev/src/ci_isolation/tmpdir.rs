//! The job's own temp folder and socket folder (`job-tmpdir.sh`): the one implementation. The `job-tmpdir` action runs it in a job
//! without job-isolation (a Linux container); job-isolation's `acquire` and `release` run it on the host runners.
//!
//! `setup` writes `PADDOCK_JOB_TEMP` and `TMPDIR` (`TMP` and `TEMP` on Windows) = the runner's per-job folder, which the runner deletes
//! after the job; on Unix also `PADDOCK_SOCKET_BASE`, a short private folder (`<root>/pd-XXXXXXXX`, mode 700, its `.owner` naming the
//! job's folder) for sockets whose path in the temp folder would be too long. On macOS, when the runner's work folder is on an external
//! volume the folder is `$PADDOCK_INTERNAL_TMP_ROOT` (default `~/gitea-runner/tmp`)`/<the runner's job hash>` on the internal disk instead
//! (TCC, Removable Volumes). `remove` removes the socket folder and the internal job folder; `sweep` removes `pd-*` folders whose owner
//! folder is gone and internal job folders whose runner folder is gone. The folder check: a real folder, owned by this user, directly
//! under the root and named `pd-*`; anything else fails. `PADDOCK_SOCKET_ROOT` replaces `/tmp` in tests.

use super::home::job_temp;
use super::{Ctx, R, Sinks, Stop};
use std::io::Write;
use std::path::Path;

fn die(msg: impl AsRef<str>) -> Stop {
    Stop::die_as("job-tmpdir", msg)
}

fn socket_root(ctx: &Ctx) -> String {
    let r = ctx.get("PADDOCK_SOCKET_ROOT").unwrap_or("/tmp");
    r.strip_suffix('/').unwrap_or(r).to_string()
}

fn internal_root(ctx: &Ctx) -> String {
    let r = match ctx.get("PADDOCK_INTERNAL_TMP_ROOT") {
        Some(r) => r.to_string(),
        None => format!("{}/gitea-runner/tmp", ctx.get("HOME").unwrap_or("/tmp")),
    };
    r.trim_end_matches('/').to_string()
}

/// The user id of this process (`id -u`: no `unsafe` needed).
#[cfg(unix)]
fn my_uid() -> Option<u32> {
    static UID: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    *UID.get_or_init(|| {
        let out = paddock_proc::command("id").arg("-u").output().ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    })
}

fn owned_by_me(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        my_uid().is_some_and(|u| u == meta.uid())
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        true
    }
}

/// A folder of this mechanism that may be used or removed: a real folder (not a link) of this user, directly under the root, `pd-*`.
fn ours(ctx: &Ctx, b: &str) -> bool {
    if b.is_empty() {
        return false;
    }
    let Ok(meta) = std::fs::symlink_metadata(b) else { return false };
    if !meta.is_dir() || !owned_by_me(&meta) {
        return false;
    }
    let root = socket_root(ctx);
    b.strip_prefix(&format!("{root}/"))
        .is_some_and(|rest| rest.starts_with("pd-") && !rest.contains('/'))
}

/// On macOS: does `diskutil` say the volume holding the folder is not internal?
fn volume_is_external(folder: &str) -> bool {
    let Ok(df) = paddock_proc::command("df").arg("-P").arg(folder).output() else {
        return false;
    };
    let text = String::from_utf8_lossy(&df.stdout).into_owned();
    let Some(line) = text.lines().nth(1) else { return false };
    // The mount point is everything after the fifth field (it may hold spaces).
    let mut rest = line;
    for _ in 0..5 {
        rest = rest.trim_start().split_once(char::is_whitespace).map_or("", |(_, r)| r);
    }
    let mount = rest.trim();
    if mount.is_empty() {
        return false;
    }
    let Ok(info) = paddock_proc::command("diskutil").args(["info", "-plist", mount]).output() else {
        return false;
    };
    if !info.status.success() {
        return false;
    }
    let plist = String::from_utf8_lossy(&info.stdout).into_owned();
    let mut lines = plist.lines();
    while let Some(l) = lines.next() {
        if l.contains("<key>Internal</key>") && lines.next().is_some_and(|n| n.contains("<false/>")) {
            return true;
        }
    }
    false
}

fn append_env(file: &str, lines: &[String]) -> R<()> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .map_err(|e| Stop::die(format!("cannot write {file}: {e}")))?;
    for l in lines {
        writeln!(f, "{l}").map_err(|e| Stop::die(format!("cannot write {file}: {e}")))?;
    }
    Ok(())
}

/// A random name of eight letters and digits.
fn random_suffix() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    h.write_u32(std::process::id());
    let mut n = h.finish();
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    (0..8)
        .map(|_| {
            let c = ALPHABET[(n % ALPHABET.len() as u64) as usize] as char;
            n /= ALPHABET.len() as u64;
            c
        })
        .collect()
}

/// Atomic and private: the folder is made with mode 700 under a name nobody else holds.
fn make_private_dir(root: &str) -> R<String> {
    for _ in 0..64 {
        let path = format!("{root}/pd-{}", random_suffix());
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            b.mode(0o700);
        }
        match b.create(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(die(format!("cannot make a socket folder under {root}: {e}"))),
        }
    }
    Err(die(format!("cannot find a free pd-* name under {root}")))
}

/// On a macOS host with an external work folder: the per-job folder on the internal disk.
fn internal_job_temp(ctx: &Ctx, t: &str) -> R<String> {
    let iroot = internal_root(ctx);
    let hash = Path::new(t)
        .parent()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let d = format!("{iroot}/{hash}");
    let made = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&iroot)?;
        set_private(&iroot)?;
        std::fs::create_dir_all(&d)?;
        set_private(&d)
    })();
    if made.is_err() {
        return Err(die(format!(
            "cannot make the internal job folder {d}; make {iroot} writable (or set PADDOCK_INTERNAL_TMP_ROOT)"
        )));
    }
    std::fs::write(format!("{iroot}/{hash}.owner"), format!("{t}\n"))
        .map_err(|e| die(format!("cannot write the owner file of {d}: {e}")))?;
    Ok(d)
}

fn set_private(p: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        Ok(())
    }
}

/// `setup`: the lines of the job's temp and socket folders for `$GITHUB_ENV`.
pub fn setup(ctx: &Ctx, io: &mut Sinks<'_>) -> R<()> {
    let t = job_temp(ctx)?;
    if !Path::new(&t).is_dir() {
        return Err(die(format!("the per-job temp folder {t} of the runner does not exist")));
    }
    let os = ctx.get("RUNNER_OS").unwrap_or("");
    let env_file = ctx.need("GITHUB_ENV", "GITHUB_ENV is not set")?;
    if os == "Windows" {
        let mixed = t.replace('\\', "/");
        let native = t.replace('/', "\\");
        append_env(
            env_file,
            &[
                format!("PADDOCK_JOB_TEMP={mixed}"),
                format!("TMPDIR={mixed}"),
                format!("TMP={native}"),
                format!("TEMP={native}"),
            ],
        )?;
        io.say(format!("job-tmpdir: temp folders in {t}"));
        return Ok(());
    }
    let mut t = t;
    if os == "macOS" {
        if volume_is_external(&t) {
            let ext = t.clone();
            t = internal_job_temp(ctx, &ext)?;
            io.say(format!("job-tmpdir: temp folder {t} on the internal disk, not {ext}: that volume is external and macOS (TCC, Removable Volumes) denies disclaimed processes and launchctl jobs there"));
        } else {
            io.say(format!(
                "job-tmpdir: temp folder {t} is the runner's own: its volume is internal (or diskutil could not tell)"
            ));
        }
    }
    let root = socket_root(ctx);
    let base = if let Some(b) = ctx.get("PADDOCK_SOCKET_BASE") {
        if !ours(ctx, b) {
            return Err(die(format!(
                "PADDOCK_SOCKET_BASE={b} is not a folder of this job (a real folder of this user under {root}, named pd-*)"
            )));
        }
        b.to_string()
    } else {
        let b = make_private_dir(&root)?;
        std::fs::write(format!("{b}/.owner"), format!("{t}\n")).map_err(|e| die(format!("cannot write {b}/.owner: {e}")))?;
        b
    };
    append_env(
        env_file,
        &[
            format!("PADDOCK_JOB_TEMP={t}"),
            format!("TMPDIR={t}"),
            format!("PADDOCK_SOCKET_BASE={base}"),
        ],
    )?;
    io.say(format!("job-tmpdir: temp folders in {t}; sockets in {base}"));
    Ok(())
}

/// `remove`: the socket folder (after the folder check) and the internal job folder.
pub fn remove(ctx: &Ctx, io: &mut Sinks<'_>) -> R<()> {
    if ctx.get("RUNNER_OS") == Some("Windows") {
        return Ok(());
    }
    let base = ctx.get("PADDOCK_SOCKET_BASE").unwrap_or("");
    let j = ctx.get("PADDOCK_JOB_TEMP").unwrap_or("");
    let iroot = internal_root(ctx);
    let is_internal = !j.is_empty()
        && ctx.get("RUNNER_OS") == Some("macOS")
        && std::fs::symlink_metadata(j).is_ok_and(|m| m.is_dir())
        && Path::new(j).parent().is_some_and(|p| p.to_string_lossy() == iroot)
        && Some(j) != ctx.get("RUNNER_TEMP");
    if is_internal {
        let _ = std::fs::remove_dir_all(j);
        let owner = format!("{iroot}/{}.owner", Path::new(j).file_name().and_then(|n| n.to_str()).unwrap_or(""));
        let _ = std::fs::remove_file(owner);
        io.say(format!("job-tmpdir: removed the internal job folder {j}"));
    }
    if base.is_empty() {
        return Ok(());
    }
    if std::fs::symlink_metadata(base).is_err() {
        return Ok(());
    }
    if !ours(ctx, base) {
        return Err(die(format!(
            "will not remove {base}: not a folder of this job (a real folder of this user under {}, named pd-*)",
            socket_root(ctx)
        )));
    }
    std::fs::remove_dir_all(base).map_err(|e| die(format!("cannot remove {base}: {e}")))
}

fn first_line(path: &Path) -> String {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| t.lines().next().map(str::to_string))
        .unwrap_or_default()
}

/// `sweep`: socket folders whose owner folder is gone (a job killed before its remove) and internal job folders whose runner folder is gone.
pub fn sweep(ctx: &Ctx, io: &mut Sinks<'_>) -> R<()> {
    if ctx.get("RUNNER_OS") == Some("Windows") {
        return Ok(());
    }
    let iroot = internal_root(ctx);
    if let Ok(entries) = std::fs::read_dir(&iroot) {
        let mut files: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "owner") && p.is_file())
            .collect();
        files.sort();
        for f in files {
            let owner = first_line(&f);
            let d = f.with_extension("");
            if !owner.is_empty() && !Path::new(&owner).exists() {
                if std::fs::symlink_metadata(&d).is_ok_and(|m| m.is_dir()) {
                    let _ = std::fs::remove_dir_all(&d);
                }
                let _ = std::fs::remove_file(&f);
                io.say(format!(
                    "job-tmpdir: removed the internal job folder {} (the runner folder {owner} is gone)",
                    d.display()
                ));
            }
        }
    }
    let root = socket_root(ctx);
    if let Ok(entries) = std::fs::read_dir(&root) {
        let mut dirs: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.starts_with("pd-"))
            .map(|n| format!("{root}/{n}"))
            .collect();
        dirs.sort();
        for b in dirs {
            if !ours(ctx, &b) {
                continue;
            }
            let owner_file = Path::new(&b).join(".owner");
            if !owner_file.is_file() {
                continue;
            }
            let owner = first_line(&owner_file);
            if !owner.is_empty() && !Path::new(&owner).exists() {
                let _ = std::fs::remove_dir_all(&b);
                io.say(format!("job-tmpdir: removed {b} (the job folder {owner} is gone)"));
            }
        }
    }
    Ok(())
}

/// `tmpdir setup|remove|sweep`.
pub fn command(ctx: &Ctx, mode: Option<&str>, io: &mut Sinks<'_>) -> R<()> {
    match mode {
        Some("setup") => setup(ctx, io),
        Some("remove") => remove(ctx, io),
        Some("sweep") => sweep(ctx, io),
        other => Err(die(format!(
            "usage: dev ci isolation tmpdir setup, remove or sweep, not {}",
            other.unwrap_or("")
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_isolation::testkit::{ctx, go};

    fn job(d: &Path) -> (String, String, String) {
        let temp = d.join("work/0123456789abcdef/tmp");
        std::fs::create_dir_all(&temp).unwrap();
        let env_file = d.join("work/0123456789abcdef/env");
        std::fs::write(&env_file, "").unwrap();
        let sockets = d.join("sockets");
        std::fs::create_dir_all(&sockets).unwrap();
        (
            temp.display().to_string(),
            env_file.display().to_string(),
            sockets.display().to_string(),
        )
    }

    #[test]
    fn setup_makes_a_private_socket_folder_naming_its_job_and_remove_deletes_only_it() {
        let d = tempfile::tempdir().unwrap();
        let (temp, env_file, sockets) = job(d.path());
        let c = ctx(&[
            ("RUNNER_OS", "Linux"),
            ("RUNNER_TEMP", &temp),
            ("GITHUB_ENV", &env_file),
            ("PADDOCK_SOCKET_ROOT", &sockets),
        ]);
        let (code, _, err) = go(&c, &["tmpdir", "setup"]);
        assert_eq!(code, 0, "{err}");
        let lines = std::fs::read_to_string(&env_file).unwrap();
        let base = lines
            .lines()
            .find_map(|l| l.strip_prefix("PADDOCK_SOCKET_BASE="))
            .unwrap()
            .to_string();
        assert!(
            lines.contains(&format!("TMPDIR={temp}")) && lines.contains(&format!("PADDOCK_JOB_TEMP={temp}")),
            "{lines}"
        );
        assert!(base.starts_with(&format!("{sockets}/pd-")) && Path::new(&base).is_dir());
        assert_eq!(first_line(&Path::new(&base).join(".owner")), temp);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&base).unwrap().permissions().mode() & 0o777, 0o700);
        }
        let c2 = ctx(&[
            ("RUNNER_OS", "Linux"),
            ("RUNNER_TEMP", &temp),
            ("PADDOCK_SOCKET_ROOT", &sockets),
            ("PADDOCK_SOCKET_BASE", &base),
        ]);
        assert_eq!(go(&c2, &["tmpdir", "remove"]).0, 0);
        assert!(!Path::new(&base).exists());
    }

    #[test]
    fn a_socket_base_outside_the_root_is_neither_used_nor_removed() {
        let d = tempfile::tempdir().unwrap();
        let (temp, env_file, sockets) = job(d.path());
        let other = d.path().join("elsewhere");
        std::fs::create_dir(&other).unwrap();
        let o = other.display().to_string();
        let c = ctx(&[
            ("RUNNER_OS", "Linux"),
            ("RUNNER_TEMP", &temp),
            ("GITHUB_ENV", &env_file),
            ("PADDOCK_SOCKET_ROOT", &sockets),
            ("PADDOCK_SOCKET_BASE", &o),
        ]);
        let (code, _, err) = go(&c, &["tmpdir", "setup"]);
        assert_eq!(code, 1);
        assert!(err.contains("is not a folder of this job"), "{err}");
        let (code, _, err) = go(&c, &["tmpdir", "remove"]);
        assert_eq!(code, 1);
        assert!(err.contains("will not remove"), "{err}");
        assert!(other.is_dir());
    }

    #[test]
    fn sweep_removes_pd_folders_whose_job_folder_is_gone_and_keeps_live_ones() {
        let d = tempfile::tempdir().unwrap();
        let (temp, _, sockets) = job(d.path());
        let dead = format!("{sockets}/pd-killedjb");
        std::fs::create_dir(&dead).unwrap();
        std::fs::write(format!("{dead}/.owner"), format!("{}/long-gone\n", d.path().display())).unwrap();
        let live = format!("{sockets}/pd-livejob1");
        std::fs::create_dir(&live).unwrap();
        std::fs::write(format!("{live}/.owner"), format!("{temp}\n")).unwrap();
        let no_owner = format!("{sockets}/pd-noowner1");
        std::fs::create_dir(&no_owner).unwrap();
        let c = ctx(&[("RUNNER_OS", "Linux"), ("RUNNER_TEMP", &temp), ("PADDOCK_SOCKET_ROOT", &sockets)]);
        let (code, _, err) = go(&c, &["tmpdir", "sweep"]);
        assert_eq!(code, 0, "{err}");
        assert!(!Path::new(&dead).exists() && Path::new(&live).exists() && Path::new(&no_owner).exists());
    }

    #[test]
    fn windows_gets_mixed_and_native_forms_and_no_socket_folder() {
        let d = tempfile::tempdir().unwrap();
        let (temp, env_file, sockets) = job(d.path());
        let c = ctx(&[
            ("RUNNER_OS", "Windows"),
            ("RUNNER_TEMP", &temp),
            ("GITHUB_ENV", &env_file),
            ("PADDOCK_SOCKET_ROOT", &sockets),
        ]);
        assert_eq!(go(&c, &["tmpdir", "setup"]).0, 0);
        let lines = std::fs::read_to_string(&env_file).unwrap();
        assert!(
            lines.contains("TMP=") && lines.contains("TEMP=") && !lines.contains("PADDOCK_SOCKET_BASE"),
            "{lines}"
        );
    }

    #[test]
    fn an_unknown_mode_is_refused() {
        let (code, _, err) = go(&ctx(&[]), &["tmpdir", "nope"]);
        assert_eq!(code, 1);
        assert!(err.contains("::error::job-tmpdir: usage"), "{err}");
    }
}
