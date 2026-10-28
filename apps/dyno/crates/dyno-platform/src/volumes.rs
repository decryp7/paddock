//! The OS side of the disk map's volume list: the mounts, their capacity and what is known about the medium. Everything
//! that decides what a mount *is* lives in `dyno_core::disk::volume`; the parsers here are pure and tested with invented
//! data on every OS, the calls into the OS are small and tested against the machine where they can be.
#![allow(unsafe_code)]

use dyno_core::disk::volume::{MediaTraits, RawMount};
use std::path::Path;

/// macOS: the user's data volume, joined to the sealed system volume at `/` by firmlinks.
pub const MACOS_DATA: &str = "/System/Volumes/Data";

/// Whether a macOS mount is plumbing no one manages: helper volumes, snapshots, Time Machine and VM mounts.
fn is_plumbing(mount: &str) -> bool {
    (mount.starts_with("/System/Volumes/") && mount != MACOS_DATA)
        || mount.starts_with("/Volumes/.timemachine")
        || mount.starts_with("/Volumes/com.apple.")
        || mount.starts_with("/private/var/vm")
        || mount.starts_with("/Library/Developer/CoreSimulator")
}

/// macOS: drops plumbing and shows the Data volume as the startup disk at `/`, scanned from the Data volume's own mount
/// (`/` is the read-only sealed system volume; its folders reach the data through firmlinks).
pub fn tidy_macos(mounts: Vec<RawMount>) -> Vec<RawMount> {
    let has_data = mounts.iter().any(|m| m.mount == MACOS_DATA);
    mounts
        .into_iter()
        .filter(|m| !is_plumbing(&m.mount) && !(has_data && m.mount == "/"))
        .map(|mut m| {
            if m.mount == MACOS_DATA {
                m.mount = "/".into();
                m.scan_root = Some(MACOS_DATA.into());
                m.read_only = false;
            }
            m
        })
        .collect()
}

/// The name macOS shows for the startup disk: the `/Volumes` entry that is a link to `/`.
pub fn macos_startup_name(volumes_dir: &Path) -> Option<String> {
    for e in std::fs::read_dir(volumes_dir).ok()?.flatten() {
        if std::fs::canonicalize(e.path()).is_ok_and(|p| p == Path::new("/")) && e.path().is_symlink() {
            return e.file_name().to_str().map(str::to_string);
        }
    }
    None
}

/// Reads the answer of `diskutil info -plist <mount>` (macOS) into traits.
pub fn parse_diskutil_info(bytes: &[u8]) -> Option<MediaTraits> {
    let v = plist::Value::from_reader(std::io::Cursor::new(bytes)).ok()?;
    let d = v.as_dictionary()?;
    let internal = d.get("Internal").and_then(plist::Value::as_boolean);
    let removable = d.get("RemovableMediaOrExternalDevice").and_then(plist::Value::as_boolean);
    Some(MediaTraits {
        internal,
        removable: removable.filter(|_| internal != Some(true)),
        rotational: d.get("SolidState").and_then(plist::Value::as_boolean).map(|ssd| !ssd),
        bus: d.get("BusProtocol").and_then(plist::Value::as_string).map(str::to_string),
    })
}

/// Linux: removable, rotational and bus of the block device `major:minor`, from sysfs under `sys_root` (`/sys`).
pub fn linux_traits(sys_root: &Path, device: &str) -> MediaTraits {
    let Ok(dir) = std::fs::canonicalize(sys_root.join("dev").join("block").join(device)) else {
        return MediaTraits::default();
    };
    // A partition keeps `removable` and `queue/` in its parent disk's folder.
    let flag = |rel: &str| {
        [dir.join(rel), dir.join("..").join(rel)]
            .iter()
            .find_map(|p| std::fs::read_to_string(p).ok())
            .map(|t| t.trim() == "1")
    };
    // The device path runs through its USB port: `.../usb1/1-1/...`.
    let bus = dir
        .components()
        .any(|c| {
            let n = c.as_os_str().to_string_lossy();
            n.strip_prefix("usb")
                .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
        })
        .then(|| "USB".to_string());
    MediaTraits {
        internal: None,
        removable: flag("removable").filter(|r| *r).or(bus.as_ref().map(|_| true)),
        rotational: flag("queue/rotational"),
        bus,
    }
}

// The field types of `statvfs` differ per OS (32 or 64 bits), so the widening is needed on some and a no-op on others.
#[cfg(unix)]
#[allow(clippy::useless_conversion)]
fn statvfs(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: an all-zero `statvfs` is a valid value to be overwritten; `c` is a NUL-terminated path.
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` and `s` are valid for the duration of the call.
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let unit = u64::from(s.f_frsize);
    Some((
        u64::from(s.f_blocks).saturating_mul(unit),
        u64::from(s.f_bavail).saturating_mul(unit),
    ))
}

/// Capacity and free bytes of the volume mounted at `mount`; `None` when the OS will not say. May block on a dead
/// network mount: callers on a network mount run it on an abandonable thread.
pub fn capacity(mount: &str) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        statvfs(Path::new(mount))
    }
    #[cfg(windows)]
    {
        win::capacity(mount)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = mount;
        None
    }
}

/// Every mount the OS lists (before [`dyno_core::disk::volume::is_listed`] filters them).
pub fn mounts() -> Vec<RawMount> {
    #[cfg(target_os = "macos")]
    {
        tidy_macos(macos_mounts())
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/self/mountinfo")
            .map(|t| dyno_core::disk::volume::parse_mountinfo(&t))
            .unwrap_or_default()
    }
    #[cfg(windows)]
    {
        win::mounts()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        Vec::new()
    }
}

/// What the OS knows about the medium behind `m`.
pub fn media_traits(m: &RawMount) -> MediaTraits {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = crate::process::command("/usr/sbin/diskutil");
        cmd.args(["info", "-plist", m.scan_root.as_deref().unwrap_or(&m.mount)]);
        crate::process::run_with_timeout(cmd, std::time::Duration::from_secs(5))
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| parse_diskutil_info(&o.stdout))
            .unwrap_or_default()
    }
    #[cfg(target_os = "linux")]
    {
        m.device.as_deref().map(|d| linux_traits(Path::new("/sys"), d)).unwrap_or_default()
    }
    #[cfg(windows)]
    {
        win::traits(m)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        let _ = m;
        MediaTraits::default()
    }
}

/// The mounts as `getfsstat` lists them.
#[cfg(target_os = "macos")]
fn macos_mounts() -> Vec<RawMount> {
    read_fsstat(Path::new("/Volumes"), |buf, bytes| {
        // SAFETY: `buf` is null with `bytes` 0 (only the count is asked) or valid for `bytes` bytes, as `read_fsstat` makes it.
        unsafe { libc::getfsstat(buf, bytes, libc::MNT_NOWAIT) }
    })
}

/// Reads the mount table through `fsstat` (`getfsstat`: called once for the count, once to fill a buffer of that many
/// entries). An empty or failing table is an empty list.
#[cfg(target_os = "macos")]
fn read_fsstat(volumes_dir: &Path, fsstat: impl Fn(*mut libc::statfs, i32) -> i32) -> Vec<RawMount> {
    let count = fsstat(std::ptr::null_mut(), 0);
    let Ok(n) = usize::try_from(count) else { return Vec::new() };
    if n == 0 {
        return Vec::new();
    }
    // SAFETY: `statfs` is plain data; zero is a valid bit pattern.
    let mut buf: Vec<libc::statfs> = vec![unsafe { std::mem::zeroed() }; n];
    let bytes = i32::try_from(n * std::mem::size_of::<libc::statfs>()).unwrap_or(i32::MAX);
    let got = fsstat(buf.as_mut_ptr(), bytes);
    let got = usize::try_from(got).unwrap_or(0).min(n);
    mounts_from_statfs(&buf[..got], volumes_dir)
}

#[cfg(target_os = "macos")]
fn mounts_from_statfs(entries: &[libc::statfs], volumes_dir: &Path) -> Vec<RawMount> {
    let text = |a: &[libc::c_char]| {
        let b: Vec<u8> = a.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
        String::from_utf8_lossy(&b).into_owned()
    };
    let mut out: Vec<RawMount> = entries
        .iter()
        .map(|s| RawMount {
            source: text(&s.f_mntfromname),
            mount: text(&s.f_mntonname),
            fs: text(&s.f_fstypename),
            read_only: s.f_flags & (libc::MNT_RDONLY as u32) != 0,
            hidden: s.f_flags & (libc::MNT_DONTBROWSE as u32) != 0 && text(&s.f_mntonname) != MACOS_DATA,
            ..RawMount::default()
        })
        .collect();
    // The startup disk carries the name of the `/Volumes` link that points at `/`.
    if let Some(root) = out.iter_mut().find(|m| m.mount == MACOS_DATA) {
        root.name = Some(macos_startup_name(volumes_dir).unwrap_or_else(|| "Macintosh HD".into()));
    }
    out
}

#[cfg(windows)]
mod win {
    use super::{MediaTraits, RawMount};
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::NetworkManagement::WNet::WNetGetConnectionW;
    use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW};

    // The `GetDriveTypeW` results (winbase.h).
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    const DRIVE_REMOTE: u32 = 4;
    const DRIVE_RAMDISK: u32 = 6;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn text(buf: &[u16]) -> String {
        String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap_or(buf.len())])
    }

    pub(super) fn capacity(mount: &str) -> Option<(u64, u64)> {
        let root = wide(mount);
        let (mut free, mut total) = (0u64, 0u64);
        // SAFETY: `root` is NUL-terminated and both out pointers are valid for the call.
        unsafe { GetDiskFreeSpaceExW(PCWSTR(root.as_ptr()), Some(&mut free), Some(&mut total), None) }.ok()?;
        Some((total, free))
    }

    pub(super) fn mounts() -> Vec<RawMount> {
        let mut buf = vec![0u16; 512];
        // SAFETY: the buffer is valid for its length.
        let n = unsafe { GetLogicalDriveStringsW(Some(&mut buf)) } as usize;
        if n == 0 || n > buf.len() {
            return Vec::new();
        }
        buf[..n]
            .split(|c| *c == 0)
            .filter(|d| !d.is_empty())
            .filter_map(|d| {
                let mount = String::from_utf16_lossy(d);
                let root = wide(&mount);
                // SAFETY: `root` is NUL-terminated.
                let kind = unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) };
                if kind != DRIVE_FIXED && kind != DRIVE_REMOVABLE && kind != DRIVE_REMOTE && kind != DRIVE_RAMDISK {
                    return None;
                }
                let mut fs = [0u16; 64];
                // SAFETY: `root` is NUL-terminated; the buffer is valid for its length.
                let fs_name = unsafe { GetVolumeInformationW(PCWSTR(root.as_ptr()), None, None, None, None, Some(&mut fs)) }
                    .map(|()| text(&fs))
                    .unwrap_or_default();
                let mut source = mount.clone();
                if kind == DRIVE_REMOTE {
                    let local = wide(mount.trim_end_matches('\\'));
                    let mut remote = [0u16; 512];
                    let mut len = remote.len() as u32;
                    // SAFETY: `local` is NUL-terminated; `remote` is valid for `len` units.
                    if unsafe { WNetGetConnectionW(PCWSTR(local.as_ptr()), Some(PWSTR(remote.as_mut_ptr())), &mut len) }.0 == 0 {
                        source = text(&remote);
                    }
                }
                Some(RawMount {
                    source,
                    mount,
                    // A mapped drive is a network volume whatever its file system says.
                    fs: if kind == DRIVE_REMOTE {
                        "smb".into()
                    } else {
                        fs_name.to_ascii_lowercase()
                    },
                    ..RawMount::default()
                })
            })
            .collect()
    }

    pub(super) fn traits(m: &RawMount) -> MediaTraits {
        let root = wide(&m.mount);
        // SAFETY: `root` is NUL-terminated.
        let kind = unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) };
        MediaTraits {
            internal: (kind == DRIVE_FIXED).then_some(true),
            removable: (kind == DRIVE_REMOVABLE).then_some(true),
            ..MediaTraits::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(mount: &str, fs: &str) -> RawMount {
        RawMount {
            source: "/dev/x".into(),
            mount: mount.into(),
            fs: fs.into(),
            read_only: mount == "/",
            ..RawMount::default()
        }
    }

    #[test]
    fn macos_shows_one_startup_disk_from_the_data_volume() {
        let out = tidy_macos(vec![
            raw("/", "apfs"),
            raw("/System/Volumes/VM", "apfs"),
            raw("/System/Volumes/Preboot", "apfs"),
            raw(MACOS_DATA, "apfs"),
            raw("/Volumes/Work", "exfat"),
            raw("/Volumes/.timemachine/NAS/x/TimeMachine", "apfs"),
        ]);
        let mounts: Vec<&str> = out.iter().map(|m| m.mount.as_str()).collect();
        assert_eq!(mounts, vec!["/", "/Volumes/Work"]);
        assert_eq!(out[0].scan_root.as_deref(), Some(MACOS_DATA));
        assert!(!out[0].read_only, "the Data volume is writable");
        assert_eq!(out[1].scan_root, None);
        let alone = tidy_macos(vec![raw("/", "apfs"), raw("/private/var/vm", "apfs")]);
        assert_eq!(alone.len(), 1);
        assert_eq!(alone[0].scan_root, None);
    }

    #[test]
    fn diskutil_answers_are_read() {
        let plist = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Internal</key><false/>
<key>RemovableMediaOrExternalDevice</key><true/>
<key>SolidState</key><true/>
<key>BusProtocol</key><string>USB</string>
</dict></plist>"#;
        let t = parse_diskutil_info(plist).unwrap();
        assert_eq!(t.internal, Some(false));
        assert_eq!(t.removable, Some(true));
        assert_eq!(t.rotational, Some(false));
        assert_eq!(t.bus.as_deref(), Some("USB"));
        let internal = br#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict>
<key>Internal</key><true/><key>RemovableMediaOrExternalDevice</key><true/></dict></plist>"#;
        let t = parse_diskutil_info(internal).unwrap();
        assert_eq!((t.internal, t.removable), (Some(true), None), "an internal disk is never removable");
        assert!(parse_diskutil_info(b"not a plist").is_none());
        assert!(parse_diskutil_info(br#"<?xml version="1.0"?><plist version="1.0"><string>x</string></plist>"#).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn linux_traits_come_from_sysfs() {
        let t = tempfile::tempdir().unwrap();
        let disk = t.path().join("devices/pci/usb1/block/sdb");
        std::fs::create_dir_all(disk.join("sdb1")).unwrap();
        std::fs::create_dir_all(disk.join("queue")).unwrap();
        std::fs::write(disk.join("removable"), "1\n").unwrap();
        std::fs::write(disk.join("queue/rotational"), "1\n").unwrap();
        std::fs::create_dir_all(t.path().join("dev/block")).unwrap();
        std::os::unix::fs::symlink(disk.join("sdb1"), t.path().join("dev/block/8:17")).unwrap();
        let tr = linux_traits(t.path(), "8:17");
        assert_eq!(tr.removable, Some(true));
        assert_eq!(tr.rotational, Some(true));
        assert_eq!(tr.bus.as_deref(), Some("USB"));

        let nvme = t.path().join("devices/pci/nvme/block/nvme0n1");
        std::fs::create_dir_all(nvme.join("queue")).unwrap();
        std::fs::write(nvme.join("removable"), "0\n").unwrap();
        std::fs::write(nvme.join("queue/rotational"), "0\n").unwrap();
        std::os::unix::fs::symlink(&nvme, t.path().join("dev/block/259:0")).unwrap();
        let tr = linux_traits(t.path(), "259:0");
        assert_eq!((tr.removable, tr.rotational, tr.bus), (None, Some(false), None));

        assert_eq!(
            linux_traits(t.path(), "1:1"),
            MediaTraits::default(),
            "an unknown device says nothing"
        );
    }

    #[test]
    fn the_startup_disk_name_is_the_link_to_root() {
        #[cfg(unix)]
        {
            let t = tempfile::tempdir().unwrap();
            std::os::unix::fs::symlink("/", t.path().join("Macintosh HD")).unwrap();
            std::fs::create_dir(t.path().join("Work")).unwrap();
            assert_eq!(macos_startup_name(t.path()).as_deref(), Some("Macintosh HD"));
            let empty = tempfile::tempdir().unwrap();
            assert_eq!(macos_startup_name(empty.path()), None);
        }
        assert_eq!(macos_startup_name(Path::new("no-such-folder-for-dyno-tests")), None);
    }

    #[test]
    fn capacity_is_read_for_a_folder_of_this_run_and_refused_for_a_missing_mount() {
        let t = tempfile::tempdir().unwrap();
        let (total, free) = capacity(&t.path().to_string_lossy()).unwrap();
        assert!(total > 0 && free <= total);
        assert!(capacity("no-such-mount-for-dyno-tests").is_none());
    }

    /// The one test that lists this machine's real mounts (ER-14: unit tests never depend on them). Run it by hand with
    /// `cargo test -p dyno-platform -- --ignored real_mounts`; it only reads the mount table, it runs no program.
    #[test]
    #[ignore = "reads this machine's real mount table"]
    fn real_mounts_are_listed() {
        assert!(!mounts().is_empty());
    }

    #[cfg(target_os = "macos")]
    mod fsstat {
        use super::super::*;

        fn entry(from: &str, on: &str, fs: &str, flags: u32) -> libc::statfs {
            // SAFETY: `statfs` is plain data; zero is a valid bit pattern.
            let mut s: libc::statfs = unsafe { std::mem::zeroed() };
            let put = |dst: &mut [libc::c_char], text: &str| {
                for (d, b) in dst.iter_mut().zip(text.bytes()) {
                    *d = b as libc::c_char;
                }
            };
            put(&mut s.f_mntfromname, from);
            put(&mut s.f_mntonname, on);
            put(&mut s.f_fstypename, fs);
            s.f_flags = flags;
            s
        }

        /// A `getfsstat` over a fixed table: the count when asked without a buffer, the entries otherwise.
        fn table(entries: Vec<libc::statfs>) -> impl Fn(*mut libc::statfs, i32) -> i32 {
            move |buf, _| {
                if buf.is_null() {
                    return entries.len() as i32;
                }
                for (i, e) in entries.iter().enumerate() {
                    // SAFETY: `read_fsstat` passes a buffer of `entries.len()` entries.
                    unsafe { *buf.add(i) = *e };
                }
                entries.len() as i32
            }
        }

        #[test]
        fn an_empty_or_failing_mount_table_is_an_empty_list() {
            let none = tempfile::tempdir().unwrap();
            assert!(read_fsstat(none.path(), |_, _| 0).is_empty());
            assert!(read_fsstat(none.path(), |_, _| -1).is_empty());
        }

        #[test]
        fn a_table_that_shrinks_between_the_two_calls_is_read_as_far_as_it_goes() {
            let one = vec![entry("/dev/disk3s1", "/Volumes/usb", "exfat", 0)];
            let calls = std::cell::Cell::new(0);
            let none = tempfile::tempdir().unwrap();
            let list = read_fsstat(none.path(), |buf, bytes| {
                calls.set(calls.get() + 1);
                if buf.is_null() {
                    return 3;
                }
                table(one.clone())(buf, bytes)
            });
            assert_eq!(list.len(), 1);
            assert_eq!(calls.get(), 2);
        }

        #[test]
        fn entries_carry_their_names_flags_and_the_startup_name() {
            let none = tempfile::tempdir().unwrap();
            let list = read_fsstat(
                none.path(),
                table(vec![
                    entry("/dev/disk3s5", MACOS_DATA, "apfs", libc::MNT_DONTBROWSE as u32),
                    entry("/dev/disk4s1", "/Volumes/usb", "exfat", libc::MNT_RDONLY as u32),
                    entry("/dev/disk3s6", "/System/Volumes/VM", "apfs", libc::MNT_DONTBROWSE as u32),
                ]),
            );
            assert_eq!(list.len(), 3);
            assert_eq!(
                list[0].name.as_deref(),
                Some("Macintosh HD"),
                "no link in the folder: the usual name"
            );
            assert!(!list[0].hidden, "the data volume stays visible");
            assert!(list[1].read_only && !list[1].hidden);
            assert!(list[2].hidden);
            assert_eq!(
                (list[1].source.as_str(), list[1].mount.as_str(), list[1].fs.as_str()),
                ("/dev/disk4s1", "/Volumes/usb", "exfat")
            );
        }
    }
}
