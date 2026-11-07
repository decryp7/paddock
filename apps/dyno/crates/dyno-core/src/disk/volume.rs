//! Volumes for the disk map: what a mounted volume is (internal, external or network), how the OS's raw mount list is
//! turned into the picker's list, and the parsers for the Linux mount table and macOS `diskutil` answers. Pure: the OS
//! calls that fill [`RawMount`] live in `dyno-platform`.

use serde::{Deserialize, Serialize};

/// The protocol of a network volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NetworkProtocol {
    /// SMB / CIFS.
    Smb,
    /// NFS.
    Nfs,
    /// AFP.
    Afp,
    /// WebDAV.
    WebDav,
    /// sshfs.
    Sshfs,
    /// Another network file system.
    Other,
}

/// Where a volume lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum VolumeLocation {
    /// Built into the machine.
    Internal,
    /// USB, Thunderbolt, SD and other removable media.
    External {
        /// The bus ("USB"), when known.
        bus: Option<String>,
    },
    /// A share on another machine.
    Network {
        /// Its protocol.
        protocol: NetworkProtocol,
        /// The server's name.
        host: String,
    },
}

/// How old a volume's figures are against the refresh interval, for a view that must say so (the rule lives here, not in the UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Freshness {
    /// Read within twice the interval (or never stamped).
    #[default]
    Fresh,
    /// Older than twice the interval: the view says "as of 14:05".
    Late,
    /// Older than five times the interval: the view greys the row and says why.
    Stale,
}

/// The freshness of figures measured at `measured_at` (ms since 1970) when it is `now_ms` and the figures are meant to be read
/// every `interval_secs`. Never-stamped figures are fresh; a clock that went back counts as zero age.
pub fn freshness(measured_at: Option<i64>, now_ms: i64, interval_secs: u32) -> Freshness {
    let Some(at) = measured_at else { return Freshness::Fresh };
    let age = u64::try_from(now_ms.saturating_sub(at)).unwrap_or(0);
    let interval = u64::from(interval_secs.max(1)) * 1000;
    if age > 5 * interval {
        Freshness::Stale
    } else if age > 2 * interval {
        Freshness::Late
    } else {
        Freshness::Fresh
    }
}

/// A volume the disk map can scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    /// Stable id: the mount point.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Where it is mounted.
    pub mount: String,
    /// File system ("apfs", "ntfs", "smbfs").
    pub fs: String,
    /// Internal, external or network.
    pub location: VolumeLocation,
    /// Capacity in bytes; `None` when the volume cannot say.
    pub capacity: Option<u64>,
    /// Free bytes; `None` when the volume cannot say.
    pub free: Option<u64>,
    /// Whether it is a spinning disk (`None` when unknown).
    pub rotational: Option<bool>,
    /// Whether it is mounted read-only.
    pub read_only: bool,
    /// False when a network volume did not answer within the limit.
    pub reachable: bool,
    /// Whether the volume's used space is a fixed scale for a scan. False for a network share: the OS reports the
    /// whole server volume, not the share, so a share is scanned like a folder.
    pub whole_volume: bool,
    /// When capacity and free were read, in milliseconds since 1970 (the agent's wall clock); `None` before the agent stamped it. A
    /// volume that stopped answering keeps its last figures and this time, so a view can say "as of 14:05".
    #[serde(default)]
    pub measured_at: Option<i64>,
    /// How old those figures are against the refresh interval, set by the agent when it hands the volume out.
    #[serde(default)]
    pub freshness: Freshness,
}

impl Volume {
    /// Bytes in use, when capacity and free space are known.
    pub fn used(&self) -> Option<u64> {
        Some(self.capacity?.saturating_sub(self.free?))
    }
}

/// One mount as the OS lists it, before it is classified.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawMount {
    /// The device or remote source (`/dev/disk3s1`, `//nas/share`, `nas:/export`, `\\nas\share`).
    pub source: String,
    /// Where it is mounted.
    pub mount: String,
    /// The file system type name.
    pub fs: String,
    /// Mounted read-only.
    pub read_only: bool,
    /// The OS says this mount is not for browsing (macOS `MNT_DONTBROWSE`).
    pub hidden: bool,
    /// Linux `major:minor` of the device, for sysfs lookups.
    pub device: Option<String>,
    /// Where the files really are when that is not the mount point (macOS: the Data volume behind `/`, ADR disk map §9.14).
    pub scan_root: Option<String>,
    /// The volume's own name when the mount point doesn't carry it (macOS startup disk).
    pub name: Option<String>,
}

/// What the platform knows about the medium behind a mount.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaTraits {
    /// Built-in (`Some(true)`) or external (`Some(false)`), when the OS says.
    pub internal: Option<bool>,
    /// Removable media.
    pub removable: Option<bool>,
    /// A spinning disk.
    pub rotational: Option<bool>,
    /// The bus, "USB" or "Thunderbolt".
    pub bus: Option<String>,
    /// What tells this disk from another mounted at the same place later: the volume UUID (macOS, Linux) or the volume GUID
    /// path (Windows). `None` where the OS gives none (network shares), which is never read as a different disk.
    pub identity: Option<String>,
}

/// File systems that are not storage a person manages: pseudo, memory and loop file systems.
const PSEUDO: &[&str] = &[
    "proc",
    "sysfs",
    "tmpfs",
    "devtmpfs",
    "devfs",
    "cgroup",
    "cgroup2",
    "devpts",
    "mqueue",
    "debugfs",
    "tracefs",
    "securityfs",
    "pstore",
    "bpf",
    "configfs",
    "fusectl",
    "hugetlbfs",
    "autofs",
    "binfmt_misc",
    "squashfs",
    "nsfs",
    "ramfs",
    "efivarfs",
    "rpc_pipefs",
    "overlay",
    "fdescfs",
    "volfs",
    "synthfs",
];

/// The network protocol of a file system type name, or `None` for a local file system.
pub fn network_protocol(fs: &str) -> Option<NetworkProtocol> {
    let f = fs.to_ascii_lowercase();
    Some(match f.as_str() {
        "smbfs" | "cifs" | "smb3" | "smb" => NetworkProtocol::Smb,
        "nfs" | "nfs3" | "nfs4" => NetworkProtocol::Nfs,
        "afpfs" | "afp" => NetworkProtocol::Afp,
        "webdav" | "davfs" | "davfs2" => NetworkProtocol::WebDav,
        "fuse.sshfs" | "sshfs" => NetworkProtocol::Sshfs,
        "ftp" | "ftpfs" | "9p" | "ceph" | "glusterfs" | "fuse.glusterfs" | "lustre" => NetworkProtocol::Other,
        _ => return None,
    })
}

/// The server in a remote source: `//user@host/share`, `host:/path`, `\\host\share` or `user@host:/path`.
pub fn host_of(source: &str) -> String {
    let s = source.trim();
    let rest = s
        .strip_prefix("//")
        .or_else(|| s.strip_prefix("\\\\"))
        .map(|r| r.split(['/', '\\']).next().unwrap_or(""));
    let host = match rest {
        Some(r) => r,
        None => s.split(':').next().unwrap_or(""),
    };
    host.rsplit('@').next().unwrap_or("").to_string()
}

/// A display name for a mount point: its last folder, or the whole thing for a drive letter or the root.
pub fn name_of_mount(mount: &str) -> String {
    let trimmed = mount.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() || (trimmed.len() == 2 && trimmed.ends_with(':')) {
        return mount.to_string();
    }
    trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed).to_string()
}

/// The macOS user data volume, which the startup disk is shown as.
pub const MACOS_DATA_MOUNT: &str = "/System/Volumes/Data";

/// Whether a macOS mount is plumbing no one manages: the OS's helper volumes, snapshots, Time Machine, VM and simulator mounts.
/// The one place that says so (the disk map, the sampler and the weekly report all use it).
pub fn is_plumbing_mount(mount: &str) -> bool {
    (mount.starts_with("/System/Volumes/") && mount != MACOS_DATA_MOUNT)
        || mount.starts_with("/Volumes/.timemachine")
        || mount.starts_with("/Volumes/com.apple.")
        || mount.starts_with("/private/var/vm")
        || mount.starts_with("/Library/Developer/CoreSimulator")
}

/// Whether a raw mount belongs in the picker at all.
pub fn is_listed(m: &RawMount) -> bool {
    !m.hidden && !PSEUDO.contains(&m.fs.to_ascii_lowercase().as_str()) && !m.mount.is_empty()
}

/// Classifies one raw mount. `traits` is what the platform could tell about the medium; capacity comes from the caller.
pub fn classify(m: &RawMount, traits: &MediaTraits, capacity: Option<(u64, u64)>, reachable: bool) -> Volume {
    let (location, whole_volume) = match network_protocol(&m.fs) {
        Some(protocol) => (
            VolumeLocation::Network {
                protocol,
                host: host_of(&m.source),
            },
            false,
        ),
        None => {
            let external = traits.internal == Some(false) || traits.removable == Some(true);
            if external {
                (VolumeLocation::External { bus: traits.bus.clone() }, true)
            } else {
                (VolumeLocation::Internal, true)
            }
        }
    };
    // A volume that reports zero blocks cannot say how big it is.
    let capacity = capacity.filter(|(total, _)| *total > 0);
    Volume {
        id: m.mount.clone(),
        name: m.name.clone().unwrap_or_else(|| name_of_mount(&m.mount)),
        mount: m.mount.clone(),
        fs: m.fs.clone(),
        location,
        capacity: capacity.map(|(t, _)| t),
        free: capacity.map(|(t, f)| f.min(t)),
        rotational: traits.rotational,
        read_only: m.read_only,
        reachable,
        whole_volume,
        measured_at: None,
        freshness: Default::default(),
    }
}

/// Parses `/proc/self/mountinfo`: one [`RawMount`] per line, lines it cannot read are skipped.
pub fn parse_mountinfo(text: &str) -> Vec<RawMount> {
    text.lines().filter_map(parse_mountinfo_line).collect()
}

fn parse_mountinfo_line(line: &str) -> Option<RawMount> {
    let (head, tail) = line.split_once(" - ")?;
    let fields: Vec<&str> = head.split(' ').collect();
    let (device, mount, options) = (fields.get(2)?, fields.get(4)?, fields.get(5)?);
    let mut t = tail.split(' ');
    let fs = t.next()?;
    let source = t.next().unwrap_or("");
    Some(RawMount {
        source: unescape_octal(source),
        mount: unescape_octal(mount),
        fs: fs.to_string(),
        read_only: options.split(',').any(|o| o == "ro"),
        hidden: false,
        device: Some((*device).to_string()),
        scan_root: None,
        name: None,
    })
}

/// Undoes the `\040` style escapes of mount tables (space, tab, newline, backslash).
fn unescape_octal(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let octal = (b[i] == b'\\')
            .then(|| s.get(i + 1..i + 4))
            .flatten()
            .and_then(|d| u8::from_str_radix(d, 8).ok());
        match octal {
            Some(v) => {
                out.push(v);
                i += 4;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn freshness_is_late_after_twice_and_stale_after_five_times_the_interval() {
        use super::{freshness, Freshness::*};
        let at = Some(1_000_000);
        // (age in seconds, interval 60 s)
        for (age, want) in [(0, Fresh), (120, Fresh), (121, Late), (300, Late), (301, Stale), (86_400, Stale)] {
            assert_eq!(freshness(at, 1_000_000 + age * 1000, 60), want, "age {age} s");
        }
        assert_eq!(freshness(None, i64::MAX, 60), Fresh, "never stamped");
        assert_eq!(freshness(at, 0, 60), Fresh, "a clock that went back");
        assert_eq!(freshness(at, 1_000_000 + 21_000, 10), Late, "a 10 s interval is late after 20 s");
    }

    use super::*;

    fn raw(mount: &str, fs: &str, source: &str) -> RawMount {
        RawMount {
            source: source.into(),
            mount: mount.into(),
            fs: fs.into(),
            ..RawMount::default()
        }
    }

    #[test]
    fn network_file_systems_are_recognised() {
        for (fs, p) in [
            ("smbfs", NetworkProtocol::Smb),
            ("CIFS", NetworkProtocol::Smb),
            ("nfs4", NetworkProtocol::Nfs),
            ("afpfs", NetworkProtocol::Afp),
            ("davfs2", NetworkProtocol::WebDav),
            ("fuse.sshfs", NetworkProtocol::Sshfs),
            ("9p", NetworkProtocol::Other),
        ] {
            assert_eq!(network_protocol(fs), Some(p), "{fs}");
        }
        assert_eq!(network_protocol("apfs"), None);
        assert_eq!(network_protocol("ntfs"), None);
    }

    #[test]
    fn the_server_is_read_from_every_source_spelling() {
        assert_eq!(host_of("//alex@nas.local/Media"), "nas.local");
        assert_eq!(host_of("//nas.local/Media"), "nas.local");
        assert_eq!(host_of("nas.local:/export/media"), "nas.local");
        assert_eq!(host_of("alex@nas.local:/srv"), "nas.local");
        assert_eq!(host_of("\\\\nas\\share"), "nas");
        assert_eq!(host_of(""), "");
    }

    #[test]
    fn mount_names() {
        assert_eq!(name_of_mount("/Volumes/Work"), "Work");
        assert_eq!(name_of_mount("/Volumes/Work/"), "Work");
        assert_eq!(name_of_mount("/"), "/");
        assert_eq!(name_of_mount("C:\\"), "C:\\");
        assert_eq!(name_of_mount("D:"), "D:");
        assert_eq!(name_of_mount("/mnt/data"), "data");
    }

    #[test]
    fn pseudo_and_hidden_mounts_are_not_listed() {
        assert!(is_listed(&raw("/", "ext4", "/dev/sda1")));
        assert!(!is_listed(&raw("/proc", "proc", "proc")));
        assert!(!is_listed(&raw("/run", "tmpfs", "tmpfs")));
        assert!(!is_listed(&raw("/snap/core/1", "squashfs", "/dev/loop1")));
        assert!(!is_listed(&raw("", "ext4", "x")));
        let mut hidden = raw("/System/Volumes/VM", "apfs", "/dev/disk3s6");
        hidden.hidden = true;
        assert!(!is_listed(&hidden));
    }

    #[test]
    fn classification_gives_kind_capacity_and_scale() {
        let internal = classify(&raw("/", "apfs", "/dev/disk3s1"), &MediaTraits::default(), Some((1000, 400)), true);
        assert_eq!(internal.location, VolumeLocation::Internal);
        assert_eq!(
            (internal.capacity, internal.free, internal.used()),
            (Some(1000), Some(400), Some(600))
        );
        assert!(internal.whole_volume && internal.reachable);

        let usb = classify(
            &raw("/Volumes/Photos", "exfat", "/dev/disk5s1"),
            &MediaTraits {
                internal: Some(false),
                bus: Some("USB".into()),
                rotational: Some(true),
                ..MediaTraits::default()
            },
            Some((500, 600)),
            true,
        );
        assert_eq!(usb.location, VolumeLocation::External { bus: Some("USB".into()) });
        assert_eq!(usb.name, "Photos");
        assert_eq!((usb.free, usb.rotational), (Some(500), Some(true)), "free is never above capacity");

        let removable = classify(
            &raw("E:\\", "fat32", "E:"),
            &MediaTraits {
                removable: Some(true),
                ..MediaTraits::default()
            },
            None,
            true,
        );
        assert_eq!(removable.location, VolumeLocation::External { bus: None });
        assert_eq!((removable.capacity, removable.used()), (None, None));

        let named = RawMount {
            name: Some("Macintosh HD".into()),
            ..raw("/", "apfs", "/dev/disk3s1")
        };
        assert_eq!(classify(&named, &MediaTraits::default(), None, true).name, "Macintosh HD");

        let share = classify(
            &raw("/Volumes/Media", "smbfs", "//alex@nas.local/Media"),
            &MediaTraits::default(),
            Some((0, 0)),
            false,
        );
        assert_eq!(
            share.location,
            VolumeLocation::Network {
                protocol: NetworkProtocol::Smb,
                host: "nas.local".into()
            }
        );
        assert!(!share.whole_volume && !share.reachable);
        assert_eq!(share.capacity, None, "zero blocks means not reported");
    }

    #[test]
    fn the_linux_mount_table_is_parsed() {
        let text = "\
22 1 259:2 / / rw,relatime shared:1 - ext4 /dev/nvme0n1p2 rw,errors=remount-ro
30 22 8:17 / /mnt/My\\040Disk ro,nosuid master:5 - exfat /dev/sdb1 ro
41 22 0:38 / /mnt/nas rw - cifs //nas.local/media rw,vers=3.0
50 22 0:5 / /proc rw - proc proc rw
bad line without separator
7 8 9 - ext4
";
        let m = parse_mountinfo(text);
        assert_eq!(m.len(), 4, "the unreadable lines are skipped");
        assert_eq!(m[0].mount, "/");
        assert_eq!(m[0].device.as_deref(), Some("259:2"));
        assert!(!m[0].read_only);
        assert_eq!(m[1].mount, "/mnt/My Disk");
        assert!(m[1].read_only);
        assert_eq!(m[2].source, "//nas.local/media");
        assert_eq!(m[2].fs, "cifs");
        assert_eq!(m.iter().filter(|x| is_listed(x)).count(), 3);
    }

    #[test]
    fn octal_escapes() {
        assert_eq!(unescape_octal("a\\040b\\011c\\134d"), "a b\tc\\d");
        assert_eq!(unescape_octal("trail\\"), "trail\\");
        assert_eq!(unescape_octal("\\9zz"), "\\9zz");
        assert_eq!(unescape_octal("\\777"), "\\777", "above a byte stays as written");
        assert_eq!(unescape_octal("plain"), "plain");
    }

    #[test]
    fn the_wire_shape_is_camel_case() {
        let v = classify(&raw("/Volumes/M", "smbfs", "//h/s"), &MediaTraits::default(), Some((10, 5)), true);
        let j = serde_json::to_value(&v).unwrap();
        assert_eq!(j["location"], serde_json::json!({"network": {"protocol": "smb", "host": "h"}}));
        assert_eq!(j["wholeVolume"], false);
        assert_eq!(serde_json::to_value(VolumeLocation::Internal).unwrap(), "internal");
    }
}
