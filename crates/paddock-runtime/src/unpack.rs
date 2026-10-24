use crate::{Result, RuntimeError};
use paddock_download::phases;
use paddock_jobs::{JobContext, Phase};
use std::io::{Read, Write};
use std::path::{Component, Path};

/// Bounds for one archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnpackLimits {
    /// The most entries (files, folders, links).
    pub max_entries: usize,
    /// The most bytes the files may expand to, counted as they are written (a header that lies does not help).
    pub max_expanded: u64,
}

impl Default for UnpackLimits {
    /// 100,000 entries and 4 GiB: a Chrome for Testing build has about 600 files and 600 MB.
    fn default() -> Self {
        UnpackLimits {
            max_entries: 100_000,
            max_expanded: 4 << 30,
        }
    }
}

const BUF: usize = 256 << 10;

fn archive_err(why: impl Into<String>) -> RuntimeError {
    RuntimeError::Archive(why.into())
}

/// Unpacks a zip into `dest` (created) as the job's `unpack` phase, in bytes of the expanded size. Refused, before
/// anything is written: a path that leaves `dest` (absolute, `..`), more entries than the limit, a declared size over the
/// limit. While writing: an entry that holds more than it declares (the zip reader fails it). A symbolic link (macOS frameworks link
/// `Versions/Current`) is recreated only when its target is relative and stays inside `dest`. Unix permissions are
/// kept. Cancel is checked between files and between buffers. Returns the bytes written.
pub fn unpack_zip(
    archive: &Path,
    dest: &Path,
    limits: &UnpackLimits,
    ctx: &JobContext,
) -> Result<u64> {
    let file = std::fs::File::open(archive).map_err(|e| RuntimeError::io("open", archive, e))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| archive_err(format!("it is not a valid zip file ({e})")))?;
    if zip.len() > limits.max_entries {
        return Err(archive_err(format!(
            "it has more than {} entries",
            limits.max_entries
        )));
    }
    let mut total = 0u64;
    for i in 0..zip.len() {
        let f = zip
            .by_index_raw(i)
            .map_err(|e| archive_err(e.to_string()))?;
        if f.enclosed_name().is_none() {
            return Err(archive_err("it contains a path that leaves its folder"));
        }
        total = total.saturating_add(f.size());
        if total > limits.max_expanded {
            return Err(archive_err("it expands to more than the allowed size"));
        }
    }
    std::fs::create_dir_all(dest).map_err(|e| RuntimeError::io("create", dest, e))?;
    ctx.phase(Phase::bytes(
        phases::UNPACK,
        phases::UNPACK_LABEL,
        Some(total),
    ));
    let mut done = 0u64;
    let mut buf = vec![0u8; BUF];
    for i in 0..zip.len() {
        ctx.cancel_token().check()?;
        let mut f = zip.by_index(i).map_err(|e| archive_err(e.to_string()))?;
        // Checked in the first pass; the same entry gives the same name.
        let rel = f
            .enclosed_name()
            .ok_or_else(|| archive_err("it contains a path that leaves its folder"))?;
        let out = dest.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| RuntimeError::io("create", &out, e))?;
            continue;
        }
        let parent = out.parent().unwrap_or(dest);
        std::fs::create_dir_all(parent).map_err(|e| RuntimeError::io("create", parent, e))?;
        if f.is_symlink() {
            let mut target = String::new();
            (&mut f)
                .take(4096)
                .read_to_string(&mut target)
                .map_err(|e| archive_err(format!("a link cannot be read ({e})")))?;
            link_inside(dest, &out, &target)?;
            continue;
        }
        // The zip reader never yields more than an entry declares (a header that lies fails its read), so the declared
        // sizes checked above bound what is written.
        let mut o = std::fs::File::create(&out).map_err(|e| RuntimeError::io("create", &out, e))?;
        loop {
            let n = f
                .read(&mut buf)
                .map_err(|e| archive_err(format!("it is damaged ({e})")))?;
            if n == 0 {
                break;
            }
            done += n as u64;
            o.write_all(&buf[..n])
                .map_err(|e| RuntimeError::io("write", &out, e))?;
            ctx.set(done);
            ctx.cancel_token().check()?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Only the permission bits; never set-user-id or set-group-id from a download.
            let mode = f.unix_mode().unwrap_or(0o644) & 0o777;
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode))
                .map_err(|e| RuntimeError::io("set permissions of", &out, e))?;
        }
    }
    Ok(done)
}

/// Creates the link `link` -> `target` when `target` is relative and resolves inside `root`.
fn link_inside(root: &Path, link: &Path, target: &str) -> Result<()> {
    let unsafe_link = || archive_err("it contains a link that leaves its folder");
    let t = Path::new(target);
    if target.is_empty()
        || target.contains('\0')
        || t.is_absolute()
        || target.starts_with(['/', '\\'])
    {
        return Err(unsafe_link());
    }
    let parent = link.parent().unwrap_or(root);
    let mut depth = parent
        .strip_prefix(root)
        .map_err(|_| unsafe_link())?
        .components()
        .count();
    for c in t.components() {
        // Roots and prefixes were refused above (absolute, or starting with a separator).
        match c {
            Component::ParentDir => depth = depth.checked_sub(1).ok_or_else(unsafe_link)?,
            Component::CurDir => {}
            _ => depth += 1,
        }
    }
    make_link(target, link)
}

#[cfg(unix)]
fn make_link(target: &str, link: &Path) -> Result<()> {
    let _ = std::fs::remove_file(link);
    std::os::unix::fs::symlink(target, link)
        .map_err(|e| RuntimeError::io("create the link", link, e))
}

/// Windows builds have no links (Chrome for Testing's Windows zip has none); one is skipped.
#[cfg(not(unix))]
fn make_link(_target: &str, _link: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_jobs::JobRegistry;
    use std::sync::Arc;
    use zip::write::SimpleFileOptions;

    struct NoSink;
    impl paddock_jobs::EventSink for NoSink {
        fn emit(&self, _: &paddock_jobs::JobSnapshot) {}
    }

    fn ctx() -> (JobRegistry, JobContext) {
        let r = JobRegistry::new(Arc::new(NoSink));
        let c = r.register("t", "t", None);
        (r, c)
    }

    fn zip_of(path: &Path, files: &[(&str, &[u8])], links: &[(&str, &str)]) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, body) in files {
            if name.ends_with('/') {
                z.add_directory(*name, SimpleFileOptions::default())
                    .unwrap();
            } else {
                z.start_file(*name, SimpleFileOptions::default().unix_permissions(0o4755))
                    .unwrap();
                z.write_all(body).unwrap();
            }
        }
        for (name, target) in links {
            z.add_symlink(*name, *target, SimpleFileOptions::default())
                .unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn files_folders_links_and_permissions_are_unpacked() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.zip");
        zip_of(
            &a,
            &[
                ("app/", b""),
                ("app/Versions/1/lib", b"hello"),
                ("bin/run", b"x"),
            ],
            &[
                ("app/Versions/Current", "1"),
                ("app/here", "./Versions/1/lib"),
            ],
        );
        let (_r, c) = ctx();
        let out = d.path().join("out");
        assert_eq!(
            unpack_zip(&a, &out, &UnpackLimits::default(), &c).unwrap(),
            6
        );
        assert_eq!(
            std::fs::read(out.join("app/Versions/Current/lib")).unwrap(),
            b"hello"
        );
        assert!(out.join("app/here").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(out.join("bin/run"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o7777, 0o755, "set-user-id is dropped");
        }
    }

    #[test]
    fn unsafe_paths_and_links_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let (_r, c) = ctx();
        let slip = d.path().join("slip.zip");
        zip_of(&slip, &[("../evil", b"x")], &[]);
        let e = unpack_zip(&slip, &d.path().join("o1"), &UnpackLimits::default(), &c).unwrap_err();
        assert!(e.to_string().contains("leaves its folder"), "{e}");
        assert!(
            !d.path().join("o1").exists(),
            "nothing is written before the checks pass"
        );
        for (i, target) in ["/etc", "../../..", "a/../../..", "", "\\x"]
            .iter()
            .enumerate()
        {
            let z = d.path().join(format!("l{i}.zip"));
            zip_of(&z, &[("a/f", b"x")], &[("a/link", target)]);
            let e = unpack_zip(
                &z,
                &d.path().join(format!("ol{i}")),
                &UnpackLimits::default(),
                &c,
            )
            .unwrap_err();
            assert!(e.to_string().contains("link that leaves"), "{target}: {e}");
        }
        assert!(
            link_inside(Path::new("/r"), Path::new("/other/l"), "x").is_err(),
            "a link outside the root"
        );
    }

    #[test]
    fn limits_are_enforced() {
        let d = tempfile::tempdir().unwrap();
        let (_r, c) = ctx();
        let a = d.path().join("a.zip");
        zip_of(&a, &[("a", b"12345"), ("b", b"12345")], &[]);
        let few = UnpackLimits {
            max_entries: 1,
            ..UnpackLimits::default()
        };
        assert!(
            unpack_zip(&a, &d.path().join("o1"), &few, &c)
                .unwrap_err()
                .to_string()
                .contains("more than 1 entries")
        );
        let small = UnpackLimits {
            max_expanded: 9,
            ..UnpackLimits::default()
        };
        assert!(
            unpack_zip(&a, &d.path().join("o2"), &small, &c)
                .unwrap_err()
                .to_string()
                .contains("more than the allowed")
        );
        std::fs::write(&a, b"not a zip").unwrap();
        assert!(
            unpack_zip(&a, &d.path().join("o3"), &UnpackLimits::default(), &c)
                .unwrap_err()
                .to_string()
                .contains("not a valid zip")
        );
        assert!(matches!(
            unpack_zip(
                &d.path().join("none.zip"),
                &d.path().join("o4"),
                &UnpackLimits::default(),
                &c
            ),
            Err(RuntimeError::Io { .. })
        ));
    }

    /// A zip whose headers declare fewer bytes than the entry holds (a zip bomb's trick) is refused while writing.
    #[test]
    fn an_entry_larger_than_it_declares_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.zip");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&a).unwrap());
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        z.start_file("a", stored).unwrap();
        z.write_all(b"0123456789").unwrap();
        z.finish().unwrap();
        let mut bytes = std::fs::read(&a).unwrap();
        // The uncompressed size: offset 22 of the local header, offset 24 of the central directory entry.
        for (sig, off) in [
            ([0x50u8, 0x4b, 0x03, 0x04], 22usize),
            ([0x50, 0x4b, 0x01, 0x02], 24),
        ] {
            let at = bytes.windows(4).position(|w| w == sig).unwrap();
            bytes[at + off..at + off + 4].copy_from_slice(&5u32.to_le_bytes());
        }
        std::fs::write(&a, bytes).unwrap();
        let (_r, c) = ctx();
        let e = unpack_zip(&a, &d.path().join("o"), &UnpackLimits::default(), &c).unwrap_err();
        assert!(matches!(e, RuntimeError::Archive(_)), "{e}");
    }

    #[test]
    fn a_cancelled_job_stops_unpacking() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.zip");
        zip_of(&a, &[("a", b"1"), ("b", b"2")], &[]);
        let (_r, c) = ctx();
        c.cancel_token().cancel();
        assert!(matches!(
            unpack_zip(&a, &d.path().join("o"), &UnpackLimits::default(), &c),
            Err(RuntimeError::Cancelled(_))
        ));
    }
}
