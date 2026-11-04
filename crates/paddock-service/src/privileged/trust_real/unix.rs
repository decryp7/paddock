//! The Unix real port (see the parent module).
#![cfg_attr(
    not(test),
    deny(
        clippy::let_underscore_must_use,
        clippy::let_underscore_future,
        clippy::let_underscore_untyped
    )
)]

use super::RealTrust;
use super::{
    HeldImage, remove_tree_no_follow, rename_aside_free, sweep_aside_free, walk_no_follow,
};
use crate::privileged::files::FileKind;
use crate::privileged::trust::{Node, Security, Style, TrustPort};
use std::io;

impl TrustPort for RealTrust {
    fn style(&self) -> Style {
        Style::Unix
    }

    fn node(&mut self, path: &str) -> io::Result<Option<Node>> {
        use std::os::unix::fs::MetadataExt;
        let meta = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let t = meta.file_type();
        let kind = if t.is_symlink() {
            FileKind::Symlink
        } else if t.is_dir() {
            FileKind::Dir
        } else if t.is_file() {
            FileKind::File
        } else {
            FileKind::Other
        };
        Ok(Some(Node {
            kind,
            security: Security::Unix {
                owner: meta.uid(),
                mode: meta.mode() & 0o7777,
                write_acl: cfg!(target_os = "macos")
                    && !t.is_symlink()
                    && mac_acl_allows_write(path),
            },
        }))
    }

    fn canonical(&mut self, path: &str) -> io::Result<String> {
        Ok(std::fs::canonicalize(path)?.to_string_lossy().into_owned())
    }

    fn walk(&mut self, path: &str, limit: usize) -> io::Result<Vec<(String, Node)>> {
        walk_no_follow(self, path, limit)
    }

    fn create_dir(&mut self, path: &str) -> io::Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        // One level, create-new, exactly 0755 (the umask cannot widen it; narrowing is undone by `lock_dir`).
        std::fs::DirBuilder::new().mode(0o755).create(path)
    }

    fn lock_dir(&mut self, path: &str) -> io::Result<()> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        // By descriptor of a folder opened without following a link, so the mode lands on the folder that was checked.
        let dir = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
            .open(path)?;
        // As root the folder is made root's too (a moved-aside folder was a user's), so that only the trusted set owns what the
        // helper keeps; a user who is not root cannot give it away and the owner check refuses the folder instead.
        if unsafe { libc::geteuid() } == 0 {
            use std::os::fd::AsRawFd;
            // SAFETY: `dir` is open; -1 leaves the group as it is.
            if unsafe { libc::fchown(dir.as_raw_fd(), 0, libc::gid_t::MAX) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        dir.set_permissions(std::fs::Permissions::from_mode(0o755))
    }

    fn rename_aside(&mut self, path: &str) -> io::Result<String> {
        rename_aside_free(path)
    }

    fn sweep_aside(&mut self, parent: &str, name: &str, keep: usize) -> io::Result<Vec<String>> {
        sweep_aside_free(self, parent, name, keep)
    }

    fn remove_tree(&mut self, path: &str) -> io::Result<()> {
        remove_tree_no_follow(path)
    }
}

#[cfg(target_os = "macos")]
fn mac_acl_allows_write(path: &str) -> bool {
    // `ls -lde` lists the ACL entries under the line of the folder itself; a failure to run it is treated as "has a write
    // entry" so that the rule refuses rather than guesses.
    match std::process::Command::new("/bin/ls")
        .args(["-lde", "--", path])
        .output()
    {
        Ok(o) if o.status.success() => {
            crate::privileged::trust::acl_allows_write(&String::from_utf8_lossy(&o.stdout))
        }
        _ => true,
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn mac_acl_allows_write(_path: &str) -> bool {
    false
}

/// Holds `requested` for a run (see [`HeldImage`]).
pub(super) fn open_image(requested: &str) -> io::Result<HeldImage> {
    open_with(requested, true, "/proc/self/fd")
}

/// [`open_image`] with the owner rule and the descriptor folder as parameters, so tests can use files and folders of their own.
pub(super) fn open_with(
    requested: &str,
    require_root: bool,
    proc_fd_dir: &str,
) -> io::Result<HeldImage> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let resolved = std::fs::canonicalize(requested)?
        .to_string_lossy()
        .into_owned();
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&resolved)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    // The facts of the open descriptor, not of the path.
    if require_root && (meta.uid() != 0 || meta.mode() & 0o022 != 0) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "not owned by root or writable by others",
        ));
    }
    let spawn_path = spawn_path_for(&file, &resolved, proc_fd_dir)?;
    Ok(HeldImage {
        _file: file,
        final_path: resolved,
        spawn_path,
    })
}

/// Linux: the program must be an ELF (a script's interpreter would reopen it by name after the descriptor is closed) and runs
/// through the descriptor; without `/proc/self/fd` the checked path is run and the fallback is logged.
#[cfg(target_os = "linux")]
fn spawn_path_for(file: &std::fs::File, resolved: &str, proc_fd_dir: &str) -> io::Result<String> {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    let mut magic = [0u8; 4];
    (&*file)
        .read_exact(&mut magic)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not an ELF program"))?;
    if magic != *b"\x7fELF" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not an ELF program",
        ));
    }
    let via_fd = format!("{proc_fd_dir}/{}", file.as_raw_fd());
    if std::path::Path::new(&via_fd).exists() {
        Ok(via_fd)
    } else {
        log::warn!(Program = resolved; "HeldImage: /proc/self/fd is not available; running the checked path instead (the protected folder chain is what prevents a swap)");
        Ok(resolved.to_string())
    }
}

#[cfg(not(target_os = "linux"))]
fn spawn_path_for(_file: &std::fs::File, resolved: &str, _proc_fd_dir: &str) -> io::Result<String> {
    Ok(resolved.to_string())
}

#[cfg(all(test, unix))]
mod unix_tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn text(p: &std::path::Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn a_symlink_is_reported_as_a_link_and_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = dir.path().join("link");
        symlink(&target, &link).unwrap();
        let mut port = RealTrust;
        assert_eq!(
            port.node(&text(&link)).unwrap().unwrap().kind,
            FileKind::Symlink
        );
        assert_eq!(
            port.node(&text(&target)).unwrap().unwrap().kind,
            FileKind::Dir
        );
        assert!(
            port.node(&text(&dir.path().join("none")))
                .unwrap()
                .is_none()
        );
        // The rule refuses a chain through it.
        let through = format!("{}/file", text(&link));
        std::fs::write(target.join("file"), b"x").unwrap();
        let e = crate::privileged::trust::verify_chain(
            &mut port,
            &through,
            FileKind::File,
            None,
            false,
        )
        .unwrap_err();
        assert!(
            matches!(
                e.reason,
                crate::privileged::trust::Reason::Link
                    | crate::privileged::trust::Reason::UntrustedOwner(_)
            ),
            "{e}"
        );
    }

    #[test]
    fn remove_tree_removes_links_and_never_what_they_point_at() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("keep.txt"), b"keep").unwrap();
        let tree = dir.path().join("tree");
        std::fs::create_dir_all(tree.join("sub")).unwrap();
        std::fs::write(tree.join("sub/file"), b"x").unwrap();
        symlink(&outside, tree.join("sub/link")).unwrap();
        symlink(outside.join("keep.txt"), tree.join("filelink")).unwrap();
        let mut port = RealTrust;
        port.remove_tree(&text(&tree)).unwrap();
        assert!(!tree.exists());
        assert_eq!(std::fs::read(outside.join("keep.txt")).unwrap(), b"keep");
        // A root that is itself a link goes as the link it is.
        let root_link = dir.path().join("root-link");
        symlink(&outside, &root_link).unwrap();
        port.remove_tree(&text(&root_link)).unwrap();
        assert!(std::fs::symlink_metadata(&root_link).is_err());
        assert!(outside.join("keep.txt").exists());
        port.remove_tree(&text(&dir.path().join("missing")))
            .unwrap();
    }

    #[test]
    fn remove_tree_removes_a_plain_file_and_a_link_to_a_file_by_name_never_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let keep = dir.path().join("keep.txt");
        std::fs::write(&keep, b"keep").unwrap();
        let planted = dir.path().join("planted.txt");
        std::fs::write(&planted, b"x").unwrap();
        let link = dir.path().join("link.txt");
        symlink(&keep, &link).unwrap();
        let mut port = RealTrust;
        port.remove_tree(&text(&planted)).unwrap();
        port.remove_tree(&text(&link)).unwrap();
        assert!(!planted.exists() && std::fs::symlink_metadata(&link).is_err());
        assert_eq!(std::fs::read(&keep).unwrap(), b"keep");
    }

    #[test]
    fn walk_lists_links_without_entering_them_and_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(outside.join("deep")).unwrap();
        let tree = dir.path().join("tree");
        std::fs::create_dir_all(tree.join("a")).unwrap();
        std::fs::write(tree.join("a/f"), b"x").unwrap();
        symlink(&outside, tree.join("a/link")).unwrap();
        let mut port = RealTrust;
        let walked = port.walk(&text(&tree), 100).unwrap();
        let names: Vec<String> = walked
            .iter()
            .map(|(p, _)| p.trim_start_matches(&text(&tree)).to_string())
            .collect();
        assert!(
            names.contains(&"/a".to_string())
                && names.contains(&"/a/f".to_string())
                && names.contains(&"/a/link".to_string()),
            "{names:?}"
        );
        assert!(
            !names.iter().any(|n| n.contains("deep")),
            "a link is not entered: {names:?}"
        );
        assert!(port.walk(&text(&tree), 1).is_err(), "bounded");
    }

    #[test]
    fn create_dir_makes_one_level_exactly_0755_and_never_over_something() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mut port = RealTrust;
        let made = dir.path().join("made");
        port.create_dir(&text(&made)).unwrap();
        assert_eq!(
            std::fs::metadata(&made).unwrap().permissions().mode() & 0o7777,
            0o755
        );
        assert_eq!(
            port.create_dir(&text(&made)).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(
            port.create_dir(&text(&dir.path().join("a/b"))).is_err(),
            "the parent must exist"
        );
        std::fs::set_permissions(&made, std::fs::Permissions::from_mode(0o777)).unwrap();
        port.lock_dir(&text(&made)).unwrap();
        assert_eq!(
            std::fs::metadata(&made).unwrap().permissions().mode() & 0o7777,
            0o755
        );
        let link = dir.path().join("link");
        symlink(&made, &link).unwrap();
        assert!(port.lock_dir(&text(&link)).is_err(), "a link is not locked");
    }

    #[test]
    fn rename_aside_moves_the_entry_and_not_what_a_link_points_at() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        let link = dir.path().join("Dyno");
        symlink(&outside, &link).unwrap();
        let mut port = RealTrust;
        let aside = port.rename_aside(&text(&link)).unwrap();
        assert!(std::fs::symlink_metadata(&link).is_err());
        assert!(
            std::fs::symlink_metadata(&aside)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(outside.exists());
    }

    #[test]
    fn a_held_image_must_be_a_regular_file_owned_by_root() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("prog");
        std::fs::write(&file, b"\x7fELFxxxx").unwrap();
        // Owned by the user running the test, so not trusted (unless the test runs as root).
        let r = HeldImage::open(&text(&file));
        if unsafe { libc::getuid() } != 0 {
            assert_eq!(r.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        }
        assert!(
            HeldImage::open(&text(dir.path())).is_err(),
            "a folder is not a program"
        );
        assert!(HeldImage::open(&text(&dir.path().join("missing"))).is_err());
    }

    #[test]
    fn on_linux_the_held_program_must_be_an_elf_and_runs_through_its_descriptor() {
        let dir = tempfile::tempdir().unwrap();
        let elf = dir.path().join("prog");
        std::fs::write(&elf, b"\x7fELFxxxx").unwrap();
        let script = dir.path().join("script");
        std::fs::write(&script, b"#!/bin/sh\nexit 0\n").unwrap();
        let tiny = dir.path().join("tiny");
        std::fs::write(&tiny, b"\x7f").unwrap();
        let held = open_with(&text(&elf), false, "/proc/self/fd").unwrap();
        assert_eq!(
            std::path::Path::new(held.final_path()),
            std::fs::canonicalize(&elf).unwrap()
        );
        if cfg!(target_os = "linux") {
            assert!(
                held.spawn_path().starts_with("/proc/self/fd/"),
                "{}",
                held.spawn_path()
            );
            for bad in [&script, &tiny] {
                assert_eq!(
                    open_with(&text(bad), false, "/proc/self/fd")
                        .unwrap_err()
                        .kind(),
                    io::ErrorKind::InvalidData
                );
            }
            // No /proc/self/fd: the checked path runs instead (and the fallback is logged).
            let fallback = open_with(&text(&elf), false, "/nonexistent-proc").unwrap();
            assert_eq!(fallback.spawn_path(), fallback.final_path());
        } else {
            assert_eq!(held.spawn_path(), held.final_path());
            open_with(&text(&script), false, "/proc/self/fd").unwrap();
        }
    }
}
