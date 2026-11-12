//! Machine facts for the performance section: what the hardware and the OS are, never who owns them (no machine name, user
//! name, serial number or address). The shape and the choice of the data folder's volume are pure; the one place that asks
//! the OS is [`SystemMachine`].

use serde::Serialize;
use std::path::{Path, PathBuf};

/// What kind of disk holds the data folder, where the OS can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiskKind {
    /// A solid-state drive.
    Ssd,
    /// A spinning disk.
    Hdd,
    /// The OS does not say (a network share, a virtual disk, a permission limit).
    Unknown,
}

impl DiskKind {
    /// The word the summary uses.
    pub fn word(self) -> &'static str {
        match self {
            DiskKind::Ssd => "SSD",
            DiskKind::Hdd => "HDD",
            DiskKind::Unknown => "unknown type",
        }
    }
}

/// The volume the data folder is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataDisk {
    /// SSD, HDD or unknown.
    pub kind: DiskKind,
    /// Size, MiB (sizes are in MiB so that no number of 13 digits is written: the text redactor masks such a run as a card
    /// number and breaks the JSON).
    pub total_mib: u64,
    /// Free, MiB.
    pub free_mib: u64,
}

/// Bytes in MiB, rounded down.
pub fn mib(bytes: u64) -> u64 {
    bytes >> 20
}

/// The machine at capture time.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineFacts {
    /// The processor's brand string (`Apple M4 Pro`), empty when the OS does not say.
    pub cpu_model: String,
    /// Logical cores.
    pub cpu_logical_cores: usize,
    /// Physical cores, when known.
    pub cpu_physical_cores: Option<usize>,
    /// Memory installed, MiB.
    pub memory_total_mib: u64,
    /// Memory available to start work with, MiB.
    pub memory_free_mib: u64,
    /// The data folder's volume, when it can be found.
    pub data_disk: Option<DataDisk>,
    /// The OS and its version (`macOS 15.1`).
    pub os: String,
    /// Load average over 1, 5 and 15 minutes; two decimals; none where the OS has no such number (Windows).
    pub load_average: Option<[f64; 3]>,
}

/// Where machine facts come from; a test names its own.
pub trait MachineSource: Send + Sync {
    /// The facts now, with the volume of `data_dir`.
    fn facts(&self, data_dir: &Path) -> MachineFacts;
}

/// One mounted volume, as the OS lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    /// Where it is mounted.
    pub mount: PathBuf,
    /// Its kind.
    pub kind: DiskKind,
    /// Size, bytes.
    pub total_bytes: u64,
    /// Free, bytes.
    pub free_bytes: u64,
}

/// The volume that holds `path`: the one with the longest mount point that is a prefix of it. Both sides are compared by
/// components ([`paddock_env::winpath::starts_with`]: the `\\?\` prefix ignored, names compared ignoring case on Windows, no
/// length limit), so a canonical path matches the volume list's plain mount points however long it is.
pub fn volume_of<'a>(path: &Path, volumes: &'a [Volume]) -> Option<&'a Volume> {
    volume_of_on(path, volumes, paddock_env::Os::current())
}

/// [`volume_of`] with the platform's path rules given, so every OS tests the Windows ones.
fn volume_of_on<'a>(path: &Path, volumes: &'a [Volume], os: paddock_env::Os) -> Option<&'a Volume> {
    volumes
        .iter()
        .filter(|v| paddock_env::winpath::starts_with(path, &v.mount, os))
        .max_by_key(|v| paddock_env::winpath::components(&v.mount, os).len())
}

/// The data disk of `path` among `volumes`.
pub fn data_disk(path: &Path, volumes: &[Volume]) -> Option<DataDisk> {
    volume_of(path, volumes).map(|v| DataDisk {
        kind: v.kind,
        total_mib: mib(v.total_bytes),
        free_mib: mib(v.free_bytes),
    })
}

/// The nearest folder of `path` that exists, canonical, so a not yet created data folder still finds its volume.
fn resolved(path: &Path) -> PathBuf {
    let mut p = path;
    loop {
        if let Ok(c) = paddock_env::winpath::real(p) {
            return c;
        }
        match p.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => p = parent,
            _ => return path.to_path_buf(),
        }
    }
}

/// `x` with two decimals (a load average has no more worth reading, and fewer digits keep the text redactor away).
fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// The real machine, through `sysinfo`.
pub struct SystemMachine;

impl MachineSource for SystemMachine {
    fn facts(&self, data_dir: &Path) -> MachineFacts {
        use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};
        let sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing())
                .with_memory(MemoryRefreshKind::nothing().with_ram()),
        )
        .ok();
        let cpus = sys.as_ref().map(|s| s.cpus()).unwrap_or_default();
        // An error from the OS leaves a fact out (an empty list, no load), never the report.
        let disks = Disks::new_with_refreshed_list().ok();
        let volumes: Vec<Volume> = disks
            .iter()
            .flat_map(|d| d.iter())
            .map(|d| Volume {
                mount: d.mount_point().to_path_buf(),
                kind: match d.kind() {
                    sysinfo::DiskKind::SSD => DiskKind::Ssd,
                    sysinfo::DiskKind::HDD => DiskKind::Hdd,
                    sysinfo::DiskKind::Unknown(_) => DiskKind::Unknown,
                },
                total_bytes: d.total_space(),
                free_bytes: d.available_space(),
            })
            .collect();
        let load = System::load_average().ok();
        MachineFacts {
            cpu_model: cpus
                .first()
                .map(|c| c.brand().trim().to_string())
                .unwrap_or_default(),
            cpu_logical_cores: cpus.len(),
            cpu_physical_cores: System::physical_core_count().ok(),
            memory_total_mib: mib(sys.as_ref().map_or(0, |s| s.total_memory())),
            memory_free_mib: mib(sys.as_ref().map_or(0, |s| s.available_memory())),
            data_disk: data_disk(&resolved(data_dir), &volumes),
            os: System::long_os_version()
                .or_else(|_| System::name())
                .unwrap_or_default(),
            load_average: load.map(|l| [round2(l.one), round2(l.five), round2(l.fifteen)]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vol(mount: &str, kind: DiskKind, free: u64) -> Volume {
        Volume {
            mount: PathBuf::from(mount),
            kind,
            total_bytes: 1 << 20,
            free_bytes: free << 20,
        }
    }

    #[test]
    fn the_volume_is_the_longest_mount_point_above_the_path() {
        let vols = [
            vol("/", DiskKind::Ssd, 1),
            vol("/data", DiskKind::Hdd, 2),
            vol("/data2", DiskKind::Unknown, 3),
        ];
        assert_eq!(
            volume_of(Path::new("/data/app/x"), &vols).map(|v| v.free_bytes >> 20),
            Some(2)
        );
        assert_eq!(
            volume_of(Path::new("/data2"), &vols).map(|v| v.free_bytes >> 20),
            Some(3)
        );
        assert_eq!(
            volume_of(Path::new("/home/x"), &vols).map(|v| v.free_bytes >> 20),
            Some(1),
            "`/data` is not above `/database`"
        );
        assert_eq!(
            volume_of(Path::new("/database"), &vols).map(|v| v.free_bytes >> 20),
            Some(1)
        );
        assert_eq!(volume_of(Path::new("/x"), &[]), None);
        let d = data_disk(Path::new("/data/a"), &vols).unwrap();
        assert_eq!((d.kind, d.total_mib, d.free_mib), (DiskKind::Hdd, 1, 2));
        assert_eq!(data_disk(Path::new("/x"), &[]), None);
    }

    #[test]
    fn disk_kinds_have_words_and_serialise_lowercase() {
        assert_eq!(DiskKind::Ssd.word(), "SSD");
        assert_eq!(DiskKind::Hdd.word(), "HDD");
        assert_eq!(DiskKind::Unknown.word(), "unknown type");
        assert_eq!(serde_json::to_string(&DiskKind::Ssd).unwrap(), "\"ssd\"");
    }

    #[test]
    fn a_folder_that_does_not_exist_yet_resolves_to_its_nearest_existing_parent() {
        let dir = tempfile::tempdir().unwrap();
        // The canonical spelling without Windows' verbatim prefix (`\\?\C:\...`), the form volumes are listed in.
        let want = paddock_env::winpath::real(dir.path()).unwrap();
        let got = resolved(&dir.path().join("not/yet/there"));
        assert!(
            !got.to_string_lossy().starts_with(r"\\?\"),
            "{got:?} keeps the verbatim prefix"
        );
        assert_eq!(got, want);
        assert_eq!(resolved(Path::new("")), PathBuf::new());
    }

    #[test]
    fn sizes_are_mib_and_loads_have_two_decimals() {
        assert_eq!(
            (mib(0), mib((1 << 20) - 1), mib(5 << 20), mib(3 << 40)),
            (0, 0, 5, 3 << 20)
        );
        assert_eq!(round2(3.133_300_781_25), 3.13);
        assert_eq!(round2(0.005), 0.01);
    }

    #[test]
    fn a_verbatim_windows_path_finds_its_plain_volume_however_long_it_is() {
        use paddock_env::Os;
        let vols = [
            vol(r"C:\", DiskKind::Ssd, 1),
            vol(r"D:\", DiskKind::Hdd, 2),
            vol(r"\\srv\share", DiskKind::Unknown, 3),
        ];
        let free =
            |p: &str| volume_of_on(Path::new(p), &vols, Os::Windows).map(|v| v.free_bytes >> 20);
        assert_eq!(free(r"\\?\C:\Users\x"), Some(1));
        assert_eq!(
            free(r"\\?\d:\data"),
            Some(2),
            "drive letters compare ignoring case"
        );
        assert_eq!(free(r"\\?\UNC\srv\share\x"), Some(3));
        // Past the 259 units at which `plain` keeps the prefix.
        let long = format!(r"\\?\D:\{}", vec!["a".repeat(50); 6].join(r"\"));
        assert_eq!(free(&long), Some(2));
        assert_eq!(free(r"E:\x"), None);
    }

    #[test]
    fn the_real_machine_reports_plausible_facts() {
        let dir = tempfile::tempdir().unwrap();
        let f = SystemMachine.facts(dir.path());
        assert!(f.cpu_logical_cores >= 1);
        assert!(f.memory_total_mib > 0 && f.memory_free_mib <= f.memory_total_mib);
        assert!(!f.os.is_empty());
        assert_eq!(f.load_average.is_some(), !cfg!(windows));
        if let Some(l) = f.load_average {
            assert!(l.iter().all(|x| *x >= 0.0));
        }
        let name = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let json = serde_json::to_string(&f).unwrap();
        assert!(!json.contains(&name), "no folder name in the facts");
    }
}
