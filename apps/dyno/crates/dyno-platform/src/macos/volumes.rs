//! Tidies macOS' volume list. APFS mounts the system volume at `/` (a read-only snapshot of the OS, a few GB) and the
//! user's data at `/System/Volumes/Data` in one shared container, plus helper volumes (VM, Preboot, Update…) that
//! only add noise. People expect one "Macintosh HD" with the real used and free space.

use dyno_core::metrics::DiskSample;

const DATA: &str = "/System/Volumes/Data";

/// Whether a mount is plumbing the user never manages: system helper volumes, snapshot and Time Machine mounts.
fn is_plumbing(mount: &str) -> bool {
    (mount.starts_with("/System/Volumes/") && mount != DATA)
        || mount.starts_with("/Volumes/.timemachine")
        || mount.starts_with("/Volumes/com.apple.")
        || mount.starts_with("/private/var/vm")
        || mount.starts_with("/Library/Developer/CoreSimulator")
}

/// Drops plumbing volumes and shows the Data volume as the startup disk (under the name of `/`).
pub fn tidy(disks: Vec<DiskSample>) -> Vec<DiskSample> {
    let root_name = disks.iter().find(|d| d.mount == "/").map(|d| d.name.clone());
    let has_data = disks.iter().any(|d| d.mount == DATA);
    disks
        .into_iter()
        .filter(|d| !is_plumbing(&d.mount) && !(has_data && d.mount == "/"))
        .map(|mut d| {
            if d.mount == DATA {
                d.name = root_name
                    .clone()
                    .filter(|n| !n.is_empty() && n != "/")
                    .unwrap_or_else(|| "Macintosh HD".into());
                d.id = "/".into();
                d.removable = false;
            }
            d
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(mount: &str, name: &str, total: u64, free: u64) -> DiskSample {
        DiskSample {
            id: mount.into(),
            name: name.into(),
            mount: mount.into(),
            total,
            free,
            removable: false,
        }
    }

    #[test]
    fn shows_one_startup_disk_with_the_data_volumes_space() {
        let out = tidy(vec![
            disk("/", "Macintosh HD", 1000, 300),
            disk("/System/Volumes/VM", "VM", 1000, 300),
            disk("/System/Volumes/Preboot", "Preboot", 1000, 300),
            disk(DATA, "Data", 1000, 300),
            disk("/Volumes/Work", "Work", 4000, 1000),
            disk("/Volumes/.timemachine/NAS/x/TimeMachine", "tm", 1, 1),
        ]);
        let mounts: Vec<&str> = out.iter().map(|d| d.mount.as_str()).collect();
        assert_eq!(mounts, vec![DATA, "/Volumes/Work"]);
        assert_eq!(out[0].name, "Macintosh HD");
        assert_eq!(out[0].id, "/");
        assert_eq!(out[1].name, "Work");
    }

    #[test]
    fn without_a_data_volume_the_root_stays() {
        let out = tidy(vec![disk("/", "Mac", 10, 5), disk("/private/var/vm", "vm", 1, 1)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].mount, "/");
        assert!(tidy(vec![]).is_empty());
    }

    #[test]
    fn an_unnamed_root_gets_the_default_name() {
        let out = tidy(vec![disk("/", "/", 10, 5), disk(DATA, "Data", 10, 5)]);
        assert_eq!(out[0].name, "Macintosh HD");
    }
}
