//! Which drives the disk test measures and where each one's scratch folder goes (ADR 0030 section 1). Pure: the volume list
//! comes in, targets go out, with the refusal (read-only, network, optical, RAM disk) already decided and worded. Numbers come
//! back marked with their drive, so a comparison never mixes two drives.

use dyno_core::bench::disk::{drive_slug, refusal};
use dyno_core::bench::{BenchStatus, RunPlan};
use dyno_core::disk::volume::{Volume, VolumeLocation};
use std::path::{Path, PathBuf};

/// One drive to measure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// What the person calls it.
    pub name: String,
    /// Where it is mounted.
    pub mount: PathBuf,
    /// The system drive: its scratch folder goes to the temp folder, which is on the volume the system itself writes to.
    pub is_system: bool,
    /// Where the system drive's scratch folder goes.
    pub scratch_root: PathBuf,
    /// A spinning disk (`None`: not known; treated as solid state).
    pub rotational: Option<bool>,
    /// The file system.
    pub fs: String,
    /// The bus of an external drive, when known.
    pub bus: Option<String>,
    /// A network drive (the person agreed; results are marked).
    pub network: bool,
    /// Capacity as listed.
    pub capacity: Option<u64>,
    /// Free bytes as listed.
    pub free: Option<u64>,
    /// A folder the person chose on this drive (already checked to be on it); the scratch folder goes inside.
    pub folder_root: Option<PathBuf>,
    /// Why it is not tested, in words for the person.
    pub refusal: Option<String>,
}

impl Target {
    /// The scratch folder for `run_id`.
    pub fn folder(&self, run_id: &str) -> PathBuf {
        match &self.folder_root {
            Some(root) => crate::bench_scratch::folder_path(root, false, &self.scratch_root, run_id),
            None => crate::bench_scratch::folder_path(&self.mount, self.is_system, &self.scratch_root, run_id),
        }
    }
}

/// Whether `mount` is the system drive. On a Mac the root is a read-only snapshot; the writable data volume is the same drive.
pub fn is_system_mount(mount: &str) -> bool {
    #[cfg(windows)]
    {
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        mount
            .trim_end_matches(['\\', '/'])
            .eq_ignore_ascii_case(system.trim_end_matches(['\\', '/']))
    }
    #[cfg(not(windows))]
    {
        mount == "/" || mount == "/System/Volumes/Data"
    }
}

fn target_of(v: &Volume, allow_network: bool, scratch_root: &Path) -> Target {
    Target {
        name: v.name.clone(),
        mount: PathBuf::from(&v.mount),
        is_system: is_system_mount(&v.mount),
        scratch_root: scratch_root.to_path_buf(),
        rotational: v.rotational,
        fs: v.fs.clone(),
        bus: match &v.location {
            VolumeLocation::External { bus } => bus.clone(),
            _ => None,
        },
        network: matches!(v.location, VolumeLocation::Network { .. }),
        capacity: v.capacity,
        free: v.free,
        folder_root: None,
        refusal: refusal(v, allow_network),
    }
}

/// The drives of a plan, in the order asked. No drive chosen: the system drive (or, where the list has none, the scratch root's
/// own volume). A chosen id that is not in the list is a refused target, not a silent skip.
pub fn resolve(plan: &RunPlan, volumes: &[Volume], scratch_root: &Path) -> Vec<Target> {
    let mut targets = resolve_drives(plan, volumes, scratch_root);
    for t in &mut targets {
        let chosen = plan.folders.iter().find(|f| f.volume == t.mount.to_string_lossy());
        if let (Some(f), None) = (chosen, &t.refusal) {
            match folder_on_volume(Path::new(&f.folder), &t.mount) {
                Ok(p) => t.folder_root = Some(p),
                Err(why) => t.refusal = Some(why),
            }
        }
    }
    targets
}

/// The folder the person chose, canonicalised, if it is a folder on the volume mounted at `mount` (the same device) and not a
/// link out of it.
pub fn folder_on_volume(folder: &Path, mount: &Path) -> Result<PathBuf, String> {
    let canon = std::fs::canonicalize(folder).map_err(|e| format!("The folder you chose cannot be used ({e})."))?;
    if !canon.is_dir() {
        return Err("The folder you chose is not a folder.".into());
    }
    if same_device(&canon, mount) {
        Ok(canon)
    } else {
        Err(format!(
            "The folder you chose is not on {}. Choose a folder on that drive.",
            mount.display()
        ))
    }
}

fn same_device(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        matches!((std::fs::metadata(a), std::fs::metadata(b)), (Ok(x), Ok(y)) if x.dev() == y.dev())
    }
    #[cfg(windows)]
    {
        matches!((dyno_platform::fileid::file_facts(a), dyno_platform::fileid::file_facts(b)), (Ok(x), Ok(y)) if x.volume == y.volume)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (a, b);
        false
    }
}

fn resolve_drives(plan: &RunPlan, volumes: &[Volume], scratch_root: &Path) -> Vec<Target> {
    if plan.drives.is_empty() {
        return vec![match volumes.iter().find(|v| is_system_mount(&v.mount)) {
            Some(v) => target_of(v, plan.allow_network, scratch_root),
            None => Target {
                name: "System drive".into(),
                mount: scratch_root.to_path_buf(),
                is_system: true,
                scratch_root: scratch_root.to_path_buf(),
                rotational: None,
                fs: String::new(),
                bus: None,
                network: false,
                capacity: None,
                free: None,
                folder_root: None,
                refusal: None,
            },
        }];
    }
    plan.drives
        .iter()
        .map(|id| match volumes.iter().find(|v| &v.id == id || &v.mount == id) {
            Some(v) => target_of(v, plan.allow_network, scratch_root),
            None => Target {
                name: id.clone(),
                mount: PathBuf::from(id),
                is_system: false,
                scratch_root: scratch_root.to_path_buf(),
                rotational: None,
                fs: String::new(),
                bus: None,
                network: false,
                capacity: None,
                free: None,
                folder_root: None,
                refusal: Some(format!("{id} is not connected any more.")),
            },
        })
        .collect()
}

/// What is known about the drive, in words, kept with its result: the name, file system, bus and kind of medium, and the space it
/// had free at the start. The model is not among them: the volume list does not carry it.
pub fn facts(t: Option<&Target>) -> Vec<String> {
    let Some(t) = t else { return Vec::new() };
    let mut v = vec![format!("Drive: {}", t.name)];
    if !t.fs.is_empty() {
        v.push(format!("File system: {}", t.fs));
    }
    if let Some(b) = &t.bus {
        v.push(format!("Connection: {b}"));
    }
    match t.rotational {
        Some(true) => v.push("Medium: spinning disk".into()),
        Some(false) => v.push("Medium: solid state".into()),
        None => {}
    }
    if let Some(f) = t.free {
        v.push(format!("Free at the start: {} GB", f / 1_000_000_000));
    }
    v
}

/// Puts the drive into a finished result: ids get the drive's slug (`disk-<slug>-seq1m-t1-read`) so two drives never share an id,
/// labels get its name, and a network drive says so.
pub fn mark(status: BenchStatus, t: &Target) -> BenchStatus {
    let BenchStatus::Done { mut metrics } = status else {
        return status;
    };
    let slug = drive_slug(&t.mount.to_string_lossy());
    let note = if t.network { " (network)" } else { "" };
    for m in &mut metrics {
        if let Some(rest) = m.id.strip_prefix("disk-") {
            m.id = format!("disk-{slug}-{rest}");
        }
        // The result card is titled with the drive (`KindResult::device`), so the label does not repeat it; a network drive is marked.
        m.label = format!("{}{note}", m.label);
    }
    BenchStatus::Done { metrics }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dyno_core::bench::{plan_with, BenchKind, BenchMetric};
    use dyno_core::disk::volume::NetworkProtocol;

    fn vol(mount: &str, name: &str, location: VolumeLocation) -> Volume {
        Volume {
            id: mount.into(),
            name: name.into(),
            mount: mount.into(),
            fs: "apfs".into(),
            location,
            capacity: Some(500_000_000_000),
            free: Some(200_000_000_000),
            rotational: Some(false),
            read_only: false,
            reachable: true,
            whole_volume: true,
            measured_at: None,
        }
    }

    fn plan(drives: &[&str], network: bool) -> RunPlan {
        let ids: Vec<String> = drives.iter().map(|d| d.to_string()).collect();
        plan_with(&[BenchKind::Disk], None, "", None, true)
            .unwrap()
            .with_drives(&ids, network)
            .unwrap()
    }

    #[test]
    fn no_drive_chosen_means_the_system_drive() {
        let vols = [
            vol("/Volumes/USB", "USB", VolumeLocation::External { bus: None }),
            vol(&sys(), "Macintosh HD", VolumeLocation::Internal),
        ];
        let t = resolve(&plan(&[], false), &vols, Path::new("/tmp"));
        assert_eq!(t.len(), 1);
        assert_eq!(
            (t[0].name.as_str(), t[0].is_system, t[0].refusal.clone()),
            ("Macintosh HD", true, None)
        );
        assert_eq!(t[0].folder("9"), Path::new("/tmp/.dyno-bench-9"));
    }

    #[cfg(not(windows))]
    fn sys() -> String {
        "/".into()
    }

    #[cfg(windows)]
    fn sys() -> String {
        std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into())
    }

    #[test]
    fn without_a_list_the_scratch_roots_own_volume_is_the_system_drive() {
        let t = resolve(&plan(&[], false), &[], Path::new("/tmp"));
        assert_eq!((t[0].name.as_str(), t[0].is_system, t[0].capacity), ("System drive", true, None));
    }

    #[test]
    fn chosen_drives_come_in_order_each_with_its_own_folder_and_the_refusals_decided() {
        let net = VolumeLocation::Network {
            protocol: NetworkProtocol::Smb,
            host: "nas".into(),
        };
        let mut ro = vol("/Volumes/RO", "RO", VolumeLocation::Internal);
        ro.read_only = true;
        let vols = [
            vol("/Volumes/USB", "USB", VolumeLocation::External { bus: Some("USB".into()) }),
            vol("/Volumes/Share", "Share", net),
            ro,
        ];
        let t = resolve(
            &plan(&["/Volumes/Share", "/Volumes/USB", "/Volumes/RO", "/Volumes/Gone"], false),
            &vols,
            Path::new("/tmp"),
        );
        assert_eq!(
            t.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["Share", "USB", "RO", "/Volumes/Gone"]
        );
        assert!(t[0].refusal.as_ref().unwrap().contains("network drive"));
        assert!(t[1].refusal.is_none());
        assert_eq!(t[1].folder("5"), Path::new("/Volumes/USB/.dyno-bench-5"));
        assert!(t[2].refusal.as_ref().unwrap().contains("read-only"));
        assert!(t[3].refusal.as_ref().unwrap().contains("not connected"));
        // With the person's agreement the network drive is tested and marked.
        let t = resolve(&plan(&["/Volumes/Share"], true), &vols, Path::new("/tmp"));
        assert!(t[0].refusal.is_none() && t[0].network);
    }

    #[test]
    fn a_chosen_folder_is_used_only_when_it_is_on_that_drive() {
        let d = tempfile::tempdir().unwrap();
        let mount = d.path().join("vol");
        let inside = mount.join("bench");
        std::fs::create_dir_all(&inside).unwrap();
        let v = [vol(&mount.to_string_lossy(), "Chosen", VolumeLocation::External { bus: None })];
        let mut p = plan(&[&mount.to_string_lossy()], false);
        p.folders = vec![dyno_core::bench::DiskFolder {
            volume: mount.to_string_lossy().into_owned(),
            folder: inside.to_string_lossy().into_owned(),
        }];
        let t = resolve(&p, &v, Path::new("/tmp"));
        assert_eq!(t[0].folder_root.as_deref(), Some(std::fs::canonicalize(&inside).unwrap().as_path()));
        assert!(t[0].folder("4").starts_with(std::fs::canonicalize(&inside).unwrap()));
        // A folder that does not exist, or is a file, is refused with words.
        p.folders[0].folder = d.path().join("nope").to_string_lossy().into_owned();
        assert!(resolve(&p, &v, Path::new("/tmp"))[0]
            .refusal
            .as_ref()
            .unwrap()
            .contains("cannot be used"));
        let file = d.path().join("f");
        std::fs::write(&file, b"x").unwrap();
        p.folders[0].folder = file.to_string_lossy().into_owned();
        assert!(resolve(&p, &v, Path::new("/tmp"))[0].refusal.is_some());
    }

    #[test]
    fn what_is_known_about_a_drive_is_kept_with_its_result() {
        let mut v = vol("/Volumes/USB", "USB", VolumeLocation::External { bus: Some("USB".into()) });
        v.rotational = Some(true);
        let t = resolve(&plan(&["/Volumes/USB"], false), &[v], Path::new("/tmp"));
        assert_eq!(
            facts(Some(&t[0])),
            [
                "Drive: USB",
                "File system: apfs",
                "Connection: USB",
                "Medium: spinning disk",
                "Free at the start: 200 GB"
            ]
        );
        assert!(facts(None).is_empty());
    }

    #[test]
    fn a_result_carries_its_drive_in_ids_and_labels() {
        let t = Target {
            name: "USB".into(),
            mount: PathBuf::from("/Volumes/USB"),
            is_system: false,
            scratch_root: PathBuf::new(),
            rotational: None,
            fs: String::new(),
            bus: None,
            network: true,
            capacity: None,
            free: None,
            folder_root: None,
            refusal: None,
        };
        let done = BenchStatus::Done {
            metrics: vec![BenchMetric::higher(
                "disk-seq1m-t1-read",
                "Sequential 1 MiB, 1 thread · read",
                5.0,
                "MB/s",
            )],
        };
        let BenchStatus::Done { metrics } = mark(done, &t) else { panic!() };
        assert!(
            metrics[0].id.starts_with("disk-volumes-usb-") && metrics[0].id.ends_with("-seq1m-t1-read"),
            "{}",
            metrics[0].id
        );
        assert_eq!(metrics[0].label, "Sequential 1 MiB, 1 thread · read (network)");
        let failed = BenchStatus::Failed { reason: "x".into() };
        assert_eq!(mark(failed.clone(), &t), failed);
    }
}
