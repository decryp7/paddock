//! The CI home (`ci-home.sh`), the job's temp folder, the job's slot and `path` (a kept folder, created and renewed).

use super::table::Layout;
use super::{Ctx, R, Sinks, Stop};
use paddock_env::winpath;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// `s1`, `s2`, ...: a slot name (the script's `^s[1-9][0-9]*$`).
pub fn valid_slot(s: &str) -> bool {
    s.strip_prefix('s')
        .is_some_and(|n| !n.is_empty() && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The slot number of a valid slot name.
pub fn slot_number(slot: &str) -> u32 {
    slot.trim_start_matches('s').parse().unwrap_or(0)
}

fn is_drive_path(p: &str) -> bool {
    let b = p.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'/' || b[2] == b'\\')
}

/// A Git-bash style `/c/dir` (or `/c`) as a drive path `C:/dir`; anything else as it is. Only on Windows (`windows`), where Git bash
/// hands native programs such paths; `windows` is a parameter so every OS tests it.
pub fn from_git_bash(p: &str, windows: bool) -> String {
    let b = p.as_bytes();
    if windows && (b.len() == 2 || b.len() >= 3 && b[2] == b'/') && b[0] == b'/' && b[1].is_ascii_alphabetic() {
        // `/c` is the drive's root `C:/`, never the drive-relative `C:`.
        let rest = if b.len() == 2 { "/" } else { &p[2..] };
        return format!("{}:{rest}", (b[1] as char).to_ascii_uppercase());
    }
    p.to_string()
}

/// `p` as one string with `/` separators and no `\\?\` prefix (what `cygpath -m` gives; the same on Unix). Programs on every
/// OS take it, and so do Git bash's.
pub fn mixed(p: &Path) -> String {
    let s = p.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    if cfg!(windows) { s.replace('\\', "/") } else { s.to_string() }
}

/// The path as the operating system's own programs take it (`cygpath -w`: backslashes on Windows, as it is elsewhere).
pub fn native(p: &str) -> String {
    if cfg!(windows) { p.replace('/', "\\") } else { p.to_string() }
}

/// The job's temp folder: the runner's `RUNNER_TEMP` without a trailing separator.
pub fn job_temp(ctx: &Ctx) -> R<String> {
    let t = ctx.need("RUNNER_TEMP", "RUNNER_TEMP is not set (the per-job temp folder of the runner)")?;
    let t = from_git_bash(t, ctx.windows);
    Ok(t.trim_end_matches(['/', '\\']).to_string())
}

/// Is this job in a Linux container?
pub fn is_linux(ctx: &Ctx) -> R<bool> {
    Ok(ctx.need("RUNNER_OS", "RUNNER_OS is not set")? == "Linux")
}

/// The job's slot: `PADDOCK_CI_SLOT`, or `s1` in a Linux container.
pub fn slot(ctx: &Ctx) -> R<String> {
    let mut s = ctx.get("PADDOCK_CI_SLOT").unwrap_or("").to_string();
    if s.is_empty() && is_linux(ctx)? {
        s = "s1".to_string();
    }
    if !valid_slot(&s) {
        return Err(Stop::die(
            "this job holds no slot: run the job-isolation action before any step that uses a kept folder",
        ));
    }
    Ok(s)
}

/// The CI home (`ci-home.sh`): `PADDOCK_CI_HOME` (absolute, must exist) or `~/.paddock-ci` (created); canonical, no trailing slash.
pub fn ci_home(ctx: &Ctx) -> R<String> {
    let dir: PathBuf = if let Some(h) = ctx.get("PADDOCK_CI_HOME") {
        let h = from_git_bash(h, ctx.windows);
        if !(h.starts_with('/') || is_drive_path(&h)) {
            return Err(Stop::line(1, format!("::error::PADDOCK_CI_HOME ({h}) is not an absolute path.")));
        }
        if !Path::new(&h).is_dir() {
            return Err(Stop::line(
                1,
                format!(
                    "::error::PADDOCK_CI_HOME ({h}) does not exist: is its disk mounted? Mount it (or create the folder once by hand); the job stops here rather than fill the internal disk."
                ),
            ));
        }
        PathBuf::from(h)
    } else {
        let home = ctx.need("HOME", "HOME is not set")?;
        let d = Path::new(home).join(".paddock-ci");
        std::fs::create_dir_all(&d).map_err(|e| Stop::die(format!("cannot create {}: {e}", d.display())))?;
        d
    };
    let real = winpath::real(&dir).map_err(|e| Stop::die(format!("cannot resolve {}: {e}", dir.display())))?;
    Ok(mixed(&real).trim_end_matches('/').to_string())
}

/// The first ten hex digits of the SHA-256 of the canonical source root: the `{src}` of a layout (amendment §1). The root is
/// `pwd -P` (links resolved, `/` separators), on Windows `cygpath -m` with the drive letter lower-cased; the text is hashed as is,
/// without a trailing newline.
pub fn source_key(dir: &Path, windows: bool) -> R<String> {
    // `/c/x`, `C:\x` and `c:/x` are one folder: Git bash callers may pass either.
    let real = winpath::real(dir).map_err(|e| {
        Stop::die(format!(
            "--source {} cannot be resolved: {e}. Pass the workspace folder.",
            dir.display()
        ))
    })?;
    Ok(source_key_of(&mixed(&real), windows))
}

/// A path text as the key is made from it: Git-bash spelling made a drive path (`windows`), `\` as `/`, a `\\?\` prefix dropped, no
/// trailing `/`, the drive letter lower-cased. Pure, so every OS tests it.
pub fn normalize_root(root: &str, windows: bool) -> String {
    let mut text = from_git_bash(root, windows);
    if windows {
        text = text.replace('\\', "/");
    }
    let text = text.strip_prefix("//?/").unwrap_or(&text);
    let mut text = text.trim_end_matches('/').to_string();
    if is_drive_path(&format!("{text}/")) {
        text.replace_range(0..1, &text[0..1].to_ascii_lowercase());
    }
    text
}

/// [`source_key`] of a path text (see [`normalize_root`]).
pub fn source_key_of(root: &str, windows: bool) -> String {
    let text = normalize_root(root, windows);
    let digest = Sha256::digest(text.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    hex[..10].to_string()
}

/// The owner record `./dev disk` reads: made once (kind `slot`, owner `ci:<run>/<job>`), later only touched. Failures are ignored.
fn record_owner(ctx: &Ctx, dir: &Path) {
    use crate::disk::record::{Kind, Record, read, touch, write};
    if matches!(read(dir), Ok(Some(_))) {
        touch(dir);
        return;
    }
    let job = |k: &str| ctx.get(k).unwrap_or("").to_string();
    let mut r = Record::new(Kind::Slot, &format!("ci:{}/{}", job("GITHUB_RUN_ID"), job("GITHUB_JOB")));
    r.branch = job("GITHUB_REF_NAME");
    r.head = job("GITHUB_SHA");
    let _ = write(dir, &r);
}

/// Sets a folder's time stamp to now (the orphan sweep removes only folders no job asked for in the last days). Failures are ignored.
fn touch(dir: &Path) {
    #[cfg(not(windows))]
    {
        if let Ok(f) = std::fs::File::open(dir) {
            let _ = f.set_modified(std::time::SystemTime::now());
        }
    }
    #[cfg(windows)]
    {
        // A folder cannot be opened like a file here: making and removing an entry renews its time stamp.
        let mark = dir.join(".paddock-touch");
        if std::fs::write(&mark, b"").is_ok() {
            let _ = std::fs::remove_file(&mark);
        }
    }
}

/// The layout's name, source and slot filled in: `target-{name}-{src}-{slot}` -> `target-x-0123456789-s1`.
pub fn fill_layout(layout: &str, name: &str, src: &str, slot: &str) -> String {
    layout.replace("{name}", name).replace("{src}", src).replace("{slot}", slot)
}

/// The kept-folders line of a kind.
fn find_layout(ctx: &Ctx, kind: &str) -> R<Layout> {
    ctx.kept()?
        .layouts
        .into_iter()
        .find(|l| l.kind == kind)
        .ok_or_else(|| Stop::die(format!("no kept folder '{kind}' in kept-folders.txt")))
}

/// `path <kind> [<name>] [--source <dir>]`: a kept folder, created, its time stamp renewed. Prints only the path.
pub fn cmd_path(ctx: &Ctx, args: &[String], io: &mut Sinks<'_>) -> R<()> {
    let (mut pos, mut source, mut it) = (Vec::new(), None, args.iter());
    while let Some(a) = it.next() {
        if a == "--source" {
            source = Some(it.next().ok_or_else(|| Stop::die("--source needs a folder"))?.clone());
        } else if let Some(v) = a.strip_prefix("--source=") {
            source = Some(v.to_string());
        } else {
            pos.push(a.as_str());
        }
    }
    let kind = *pos
        .first()
        .ok_or_else(|| Stop::die("usage: dev ci isolation path <kind> [<name>] [--source <dir>]"))?;
    let name = pos.get(1).copied().unwrap_or("");
    let lay = find_layout(ctx, kind)?;
    if lay.layout.contains("{name}") {
        if name.is_empty() {
            return Err(Stop::die(format!("the kept folder '{kind}' needs a name")));
        }
        if name == "." || name == ".." || !name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
            return Err(Stop::die(format!(
                "the name '{name}' may only hold letters, digits, '.', '_' and '-'"
            )));
        }
    }
    let src = if lay.layout.contains("{src}") {
        let dir = source.ok_or_else(|| {
            Stop::die(format!(
                "the kept folder '{kind}' is keyed by its source: pass --source <the workspace folder> (ADR 0025 amendment)"
            ))
        })?;
        source_key(Path::new(&from_git_bash(&dir, ctx.windows)), ctx.windows)?
    } else {
        String::new()
    };
    let slot_name = if lay.is_slot() { slot(ctx)? } else { String::new() };
    let rel = fill_layout(&lay.layout, name, &src, &slot_name);
    let home = ci_home(ctx)?;
    let target = Path::new(&home).join(&rel);
    // One-time migration: the folder of the old layout (before slots) becomes slot s1's, so it stays warm and is not kept twice.
    // A layout keyed by its source has no old layout: an old folder cannot be attributed to a root.
    if lay.is_slot() && slot_name == "s1" && !target.exists() && !lay.layout.contains("{src}") {
        let old = lay.layout.replace("-{slot}", "").replace("{name}", name);
        let from = Path::new(&home).join(&old);
        let is_plain_dir = std::fs::symlink_metadata(&from).is_ok_and(|m| m.is_dir());
        if old != rel && is_plain_dir {
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::rename(&from, &target).is_ok() {
                io.say(format!("job-isolation: moved {old} to {rel} (kept folders are per slot now)"));
            }
        }
    }
    std::fs::create_dir_all(&target).map_err(|e| Stop::die(format!("cannot create {}: {e}", target.display())))?;
    touch(&target);
    record_owner(ctx, &target);
    io.value(format!("{home}/{rel}"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ci_isolation::testkit::{ctx, go};

    #[test]
    fn slot_names() {
        assert!(valid_slot("s1") && valid_slot("s12"));
        assert!(!valid_slot("s0") && !valid_slot("s") && !valid_slot("s01") && !valid_slot("1") && !valid_slot("s1x"));
    }

    #[test]
    fn the_source_key_is_ten_hex_digits_of_the_sha256_of_the_root_text() {
        // sha256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(source_key_of("abc", false), "ba7816bf8f");
        assert_eq!(source_key_of("abc/", false), "ba7816bf8f", "no trailing slash");
        assert_eq!(
            source_key_of("C:/Users/x", false),
            source_key_of("c:/Users/x", false),
            "the drive letter is lower-cased"
        );
        assert_ne!(source_key_of("/a/b", false), source_key_of("/a/c", false));
    }

    #[test]
    fn git_bash_and_native_spellings_of_one_windows_folder_give_one_key() {
        let k = source_key_of("c:/x", true);
        for spelling in ["/c/x", "C:\\x", "c:/x", "C:/x", "C:\\x\\", "\\\\?\\C:\\x", "/C/x/"] {
            assert_eq!(source_key_of(spelling, true), k, "{spelling}");
        }
        assert_ne!(source_key_of("/c/y", true), k);
        // Off Windows `/c/x` is just a folder of that name.
        assert_eq!(from_git_bash("/c/x", false), "/c/x");
        assert_eq!(from_git_bash("/c/x", true), "C:/x");
        assert_eq!(from_git_bash("/cc/x", true), "/cc/x", "only a one-letter first part is a drive");
        assert_eq!(from_git_bash("/usr", true), "/usr");
        assert_eq!(from_git_bash("/c", true), "C:/", "a drive's root, not the drive-relative C:");
        assert_eq!(from_git_bash("/c/", true), "C:/");
        assert_eq!(from_git_bash("/c", false), "/c");
        assert_eq!(normalize_root("/c/x", true), "c:/x");
    }

    #[test]
    fn path_needs_a_source_for_a_src_layout_and_gives_two_folders_for_two_roots() {
        let d = tempfile::tempdir().unwrap();
        let tables = d.path().join("tables");
        std::fs::create_dir(&tables).unwrap();
        std::fs::write(
            tables.join("kept-folders.txt"),
            "target target-{name}-{src}-{slot} slot\nsrc src/{name}-{slot} slot\n",
        )
        .unwrap();
        let ci = d.path().join("ci");
        std::fs::create_dir(&ci).unwrap();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let mut c = ctx(&[
            ("PADDOCK_CI_HOME", ci.to_str().unwrap()),
            ("PADDOCK_CI_SLOT", "s1"),
            ("RUNNER_OS", "macOS"),
        ]);
        c.tables = tables;
        let (code, _, err) = go(&c, &["path", "target", "x"]);
        assert_eq!(code, 1);
        assert!(err.contains("--source"), "{err}");
        let (ca, pa, _) = go(&c, &["path", "target", "x", "--source", a.to_str().unwrap()]);
        let (cb, pb, _) = go(&c, &["path", "target", "x", "--source", b.to_str().unwrap()]);
        assert_eq!((ca, cb), (0, 0));
        assert_ne!(pa, pb, "two roots, two folders");
        let real = mixed(&winpath::real(&ci).unwrap());
        let ka = source_key(&a, false).unwrap();
        assert_eq!(pa.trim(), format!("{real}/target-x-{ka}-s1"));
        assert!(Path::new(pa.trim()).is_dir());
        // A layout without {src} ignores --source.
        let (code, p, _) = go(&c, &["path", "src", "x", "--source", a.to_str().unwrap()]);
        assert_eq!(code, 0);
        assert_eq!(p.trim(), format!("{real}/src/x-s1"));
    }

    #[test]
    fn the_unslotted_migration_skips_a_layout_keyed_by_its_source() {
        let d = tempfile::tempdir().unwrap();
        let tables = d.path().join("tables");
        std::fs::create_dir(&tables).unwrap();
        std::fs::write(tables.join("kept-folders.txt"), "target target-{name}-{src}-{slot} slot\n").unwrap();
        let ci = d.path().join("ci");
        // Looks like the unslotted layout of the layout above (`target-x-<src>`): it must not be taken over.
        std::fs::create_dir_all(ci.join("target-x-0123456789")).unwrap();
        let mut c = ctx(&[
            ("PADDOCK_CI_HOME", ci.to_str().unwrap()),
            ("PADDOCK_CI_SLOT", "s1"),
            ("RUNNER_OS", "macOS"),
        ]);
        c.tables = tables;
        let src = d.path().join("w");
        std::fs::create_dir(&src).unwrap();
        let (code, p, err) = go(&c, &["path", "target", "x", "--source", src.to_str().unwrap()]);
        assert_eq!(code, 0, "{err}");
        assert!(!err.contains("moved"), "{err}");
        assert!(ci.join("target-x-0123456789").is_dir() && Path::new(p.trim()).is_dir());
    }
}
