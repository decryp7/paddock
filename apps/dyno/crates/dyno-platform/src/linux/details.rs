//! The Linux parts of a process's details, read from `/proc/<pid>`: priority, affinity, threads, descriptors, open
//! files, connections, mapped libraries, the owning package and what started it. Everything about another user's
//! process that the kernel hides becomes an unavailable field with the reason, never an error.

use super::packages::PackageOwners;
use super::paths::{home_dir, read_text, HostPaths};
use super::tools::Tools;
use crate::parse::desktop_entry;
use crate::parse::linux_net::{parse_table, Proto};
use crate::parse::linux_proc::{cpus_allowed, maps_files, socket_inode, stat_nice, status_field, user_service};
use crate::processes::DetailsProbe;
use dyno_core::process::{Connection, ListOrReason, Priority, ProcessDetails, ProcessRef, MAX_DETAIL_ITEMS};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Descriptors read per process (a server can hold hundreds of thousands).
const MAX_FDS: usize = 8_192;
/// The reason for things only the owner (or root) may read.
pub const NEEDS_OWNER: &str = "Needs the same user or root";
/// Nice values from this up count as efficiency mode (what Dyno sets is 10).
const EFFICIENCY_NICE: i32 = 10;

/// The details probe for Linux.
pub struct LinuxDetails {
    paths: HostPaths,
    autostart: Vec<PathBuf>,
    packages: PackageOwners,
}

impl LinuxDetails {
    /// A probe reading `paths`; `autostart_dirs` are searched for the entry that started a program.
    pub fn new(paths: HostPaths, autostart_dirs: Vec<PathBuf>, tools: Arc<dyn Tools>) -> Self {
        LinuxDetails {
            paths,
            autostart: autostart_dirs,
            packages: PackageOwners::new(tools),
        }
    }

    /// The usual autostart folders of `$HOME` and the system.
    pub fn default_autostart_dirs() -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = home_dir().map(|h| vec![h.join(".config/autostart")]).unwrap_or_default();
        v.push(PathBuf::from("/etc/xdg/autostart"));
        v
    }
}

/// The descriptors of a process: `(fd name, link target)`; or why they can't be listed.
fn descriptors(base: &Path) -> Result<(Vec<String>, usize), String> {
    let dir = match std::fs::read_dir(base.join("fd")) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Err(NEEDS_OWNER.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err("The process has exited".to_string()),
        Err(e) => return Err(format!("Could not list descriptors: {e}")),
    };
    let mut total = 0usize;
    let mut targets = Vec::new();
    for entry in dir.flatten() {
        total += 1;
        if targets.len() < MAX_FDS {
            if let Ok(t) = std::fs::read_link(entry.path()) {
                targets.push(t.to_string_lossy().into_owned());
            }
        }
    }
    Ok((targets, total))
}

/// Regular files and folders among descriptor targets: absolute paths outside `/dev`, `/proc` and `/sys`.
fn file_targets(targets: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    targets
        .iter()
        .filter(|t| t.starts_with('/') && !["/dev/", "/proc/", "/sys/"].iter().any(|p| t.starts_with(p)))
        .filter(|t| seen.insert((*t).clone()))
        .cloned()
        .collect()
}

impl LinuxDetails {
    fn connections(&self, base: &Path, targets: &[String]) -> Vec<Connection> {
        let inodes: HashSet<u64> = targets.iter().filter_map(|t| socket_inode(t)).collect();
        if inodes.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for proto in Proto::ALL {
            // The process's own network namespace first (containers), then the host's.
            let text =
                read_text(&base.join("net").join(proto.file())).or_else(|| read_text(&self.paths.proc.join("net").join(proto.file())));
            for row in text.map(|t| parse_table(&t, proto)).unwrap_or_default() {
                if inodes.contains(&row.inode) {
                    out.push(Connection {
                        protocol: proto.label().to_string(),
                        local: row.local,
                        remote: row.remote,
                        state: row.state.to_string(),
                    });
                }
            }
        }
        out
    }

    fn startup_source(&self, base: &Path, exe: Option<&Path>) -> Option<String> {
        if let Some(exe) = exe {
            let exe_name = exe.file_name()?.to_str()?.to_string();
            for dir in &self.autostart {
                let Ok(entries) = std::fs::read_dir(dir) else { continue };
                for e in entries.flatten().take(500) {
                    if e.path().extension().and_then(|x| x.to_str()) != Some("desktop") {
                        continue;
                    }
                    let Some(entry) = read_text(&e.path()).map(|t| desktop_entry::parse(&t)) else {
                        continue;
                    };
                    let Some(program) = desktop_entry::exec_program(&entry.exec) else {
                        continue;
                    };
                    let same = if program.contains('/') {
                        Path::new(&program) == exe
                    } else {
                        program == exe_name
                    };
                    if same && entry.enabled() {
                        let name = if entry.name.is_empty() {
                            e.file_name().to_string_lossy().into_owned()
                        } else {
                            entry.name
                        };
                        return Some(format!("Autostart entry \"{name}\""));
                    }
                }
            }
        }
        let unit = read_text(&base.join("cgroup")).and_then(|t| user_service(&t))?;
        Some(format!("systemd user service {unit}"))
    }
}

impl DetailsProbe for LinuxDetails {
    fn enrich(&mut self, target: ProcessRef, exe: Option<&Path>, details: &mut ProcessDetails) {
        let base = self.paths.pid(target.pid);
        let status = read_text(&base.join("status")).unwrap_or_default();
        let nice = read_text(&base.join("stat")).and_then(|t| stat_nice(&t));
        details.priority = nice.map(Priority::from_nice);
        details.efficiency_mode = nice.map(|n| n >= EFFICIENCY_NICE);
        details.affinity = cpus_allowed(&status);
        if let Some(n) = status_field(&status, "Threads").and_then(|v| v.parse::<u32>().ok()) {
            details.threads = Some(n);
        }
        match descriptors(&base) {
            Ok((targets, total)) => {
                details.handles = Some(u32::try_from(total).unwrap_or(u32::MAX));
                details.open_files = ListOrReason::capped(file_targets(&targets));
                details.connections = ListOrReason::capped(self.connections(&base, &targets));
            }
            Err(reason) => {
                details.open_files = ListOrReason::Unavailable(reason.clone());
                details.connections = ListOrReason::Unavailable(reason);
            }
        }
        details.modules = match read_text(&base.join("maps")) {
            Some(t) => {
                let mut files = maps_files(&t);
                files.truncate(MAX_DETAIL_ITEMS + 1);
                ListOrReason::capped(files)
            }
            None => ListOrReason::Unavailable(NEEDS_OWNER.to_string()),
        };
        details.signature = self.packages.signature(exe.and_then(Path::to_str));
        details.startup_source = self.startup_source(&base, exe);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tools::fake::FakeTools;
    use super::*;
    use dyno_core::process::{Measured, Owner, ProcessSample, RunState, SignatureInfo};
    use dyno_core::time::Timestamp;
    use std::os::unix::fs::symlink;

    fn blank(pid: u32) -> ProcessDetails {
        ProcessDetails {
            process: ProcessSample {
                pid,
                parent: None,
                name: "x".into(),
                exe: None,
                user: Measured::Value(String::new()),
                cpu: Measured::Value(0.0),
                memory: Measured::Value(0),
                disk_read_bps: Measured::Value(0.0),
                disk_write_bps: Measured::Value(0.0),
                started: Measured::Value(Timestamp(0)),
                owner: Owner::Own,
                state: RunState::Running,
            },
            command_line: vec![],
            cwd: None,
            priority: None,
            affinity: None,
            efficiency_mode: None,
            threads: None,
            handles: None,
            parents: vec![],
            signature: SignatureInfo::Unknown("n/a".into()),
            open_files: ListOrReason::Unavailable("n/a".into()),
            connections: ListOrReason::Unavailable("n/a".into()),
            modules: ListOrReason::Unavailable("n/a".into()),
            startup_source: None,
        }
    }

    fn w(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    /// A fake `/proc/4242` for a program that has files, a listening socket and a connection open.
    fn fake_process(root: &Path) {
        w(root, "proc/4242/status", "Name:\tapp\nThreads:\t7\nCpus_allowed:\t0000000f\n");
        w(
            root,
            "proc/4242/stat",
            "4242 (app) S 1 4242 4242 0 -1 4194304 1 0 0 0 1 1 0 0 20 12 7 0 100 1000 100 18446744073709551615",
        );
        w(
            root,
            "proc/4242/maps",
            "1000-2000 r-xp 00000000 08:01 55 /usr/bin/app\n2000-3000 r-xp 00000000 08:01 56 /usr/lib/libz.so.1\n",
        );
        w(
            root,
            "proc/4242/cgroup",
            "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app.service\n",
        );
        let fd = root.join("proc/4242/fd");
        std::fs::create_dir_all(&fd).unwrap();
        symlink("/home/me/notes.txt", fd.join("3")).unwrap();
        symlink("/home/me/notes.txt", fd.join("4")).unwrap();
        symlink("/dev/null", fd.join("5")).unwrap();
        symlink("socket:[11111]", fd.join("6")).unwrap();
        symlink("socket:[22222]", fd.join("7")).unwrap();
        symlink("anon_inode:[eventfd]", fd.join("8")).unwrap();
        let head = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n";
        w(
            root,
            "proc/net/tcp",
            &format!("{head}   0: 0100007F:0277 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 11111 1\n   1: 0F01A8C0:C350 22D8B85D:01BB 01 00000000:00000000 02:000A7C4E 00000000  1000        0 99999 2\n"),
        );
        w(
            root,
            "proc/4242/net/udp",
            &format!("{head}   0: 0F01A8C0:0044 00000000:0000 07 00000000:00000000 00:00000000 00000000     0        0 22222 2\n"),
        );
    }

    fn probe(root: &Path, autostart: Vec<PathBuf>, tools: Arc<FakeTools>) -> LinuxDetails {
        LinuxDetails::new(HostPaths::under(root), autostart, tools)
    }

    #[test]
    fn fills_priority_threads_files_connections_and_modules() {
        let d = tempfile::tempdir().unwrap();
        fake_process(d.path());
        let mut p = probe(d.path(), vec![], Arc::new(FakeTools::with("dpkg", "app: /usr/bin/app\n")));
        let mut det = blank(4242);
        p.enrich(
            ProcessRef {
                pid: 4242,
                started: Some(Timestamp(0)),
            },
            Some(Path::new("/usr/bin/app")),
            &mut det,
        );
        assert_eq!(det.priority, Some(Priority::BelowNormal));
        assert_eq!(det.efficiency_mode, Some(true));
        assert_eq!(det.affinity, Some(0xf));
        assert_eq!(det.threads, Some(7));
        assert_eq!(det.handles, Some(6));
        assert_eq!(
            det.open_files,
            ListOrReason::Items {
                items: vec!["/home/me/notes.txt".into()],
                truncated: false
            }
        );
        let ListOrReason::Items { items, .. } = &det.connections else {
            panic!("{:?}", det.connections)
        };
        assert_eq!(items.len(), 2);
        assert!(items.contains(&Connection {
            protocol: "TCP".into(),
            local: "127.0.0.1:631".into(),
            remote: String::new(),
            state: "LISTEN".into()
        }));
        assert!(
            items.iter().any(|c| c.protocol == "UDP" && c.local == "192.168.1.15:68"),
            "the process's own namespace file is used: {items:?}"
        );
        assert_eq!(
            det.modules,
            ListOrReason::Items {
                items: vec!["/usr/bin/app".into(), "/usr/lib/libz.so.1".into()],
                truncated: false
            }
        );
        assert!(matches!(&det.signature, SignatureInfo::Signed(m) if m.contains("package app")));
        assert_eq!(det.startup_source.as_deref(), Some("systemd user service app.service"));
    }

    #[test]
    fn what_the_kernel_hides_is_a_reason_not_an_error() {
        let d = tempfile::tempdir().unwrap();
        let mut p = probe(d.path(), vec![], Arc::new(FakeTools::default()));
        let mut det = blank(77);
        p.enrich(
            ProcessRef {
                pid: 77,
                started: Some(Timestamp(0)),
            },
            None,
            &mut det,
        );
        assert_eq!(det.open_files, ListOrReason::Unavailable("The process has exited".into()));
        assert_eq!(det.modules, ListOrReason::Unavailable(NEEDS_OWNER.into()));
        assert!(det.priority.is_none() && det.affinity.is_none());
        assert!(matches!(det.signature, SignatureInfo::Unknown(_)));
        // A readable fd directory that denies access.
        use std::os::unix::fs::PermissionsExt;
        w(d.path(), "proc/78/status", "Threads:\t1\n");
        std::fs::create_dir_all(d.path().join("proc/78/fd")).unwrap();
        std::fs::set_permissions(d.path().join("proc/78/fd"), std::fs::Permissions::from_mode(0o000)).unwrap();
        let mut det = blank(78);
        p.enrich(
            ProcessRef {
                pid: 78,
                started: Some(Timestamp(0)),
            },
            None,
            &mut det,
        );
        // Root can read anything; elsewhere the reason is the documented one.
        if super::super::sys::effective_uid() != 0 {
            assert_eq!(det.open_files, ListOrReason::Unavailable(NEEDS_OWNER.into()));
            assert_eq!(det.connections, ListOrReason::Unavailable(NEEDS_OWNER.into()));
        }
        std::fs::set_permissions(d.path().join("proc/78/fd"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn lists_are_capped() {
        let d = tempfile::tempdir().unwrap();
        let mut maps = String::new();
        for i in 0..(MAX_DETAIL_ITEMS + 20) {
            maps.push_str(&format!("1000-2000 r-xp 00000000 08:01 {} /usr/lib/lib{i}.so\n", i + 1));
        }
        w(d.path(), "proc/9/maps", &maps);
        let mut det = blank(9);
        probe(d.path(), vec![], Arc::new(FakeTools::default())).enrich(
            ProcessRef {
                pid: 9,
                started: Some(Timestamp(0)),
            },
            None,
            &mut det,
        );
        let ListOrReason::Items { items, truncated } = det.modules else {
            panic!()
        };
        assert_eq!((items.len(), truncated), (MAX_DETAIL_ITEMS, true));
    }

    #[test]
    fn the_autostart_entry_that_started_a_program_is_named() {
        let d = tempfile::tempdir().unwrap();
        let auto = d.path().join("autostart");
        w(
            &auto,
            "tray.desktop",
            "[Desktop Entry]\nName=Sync Tray\nExec=/opt/sync/bin/tray --minimized\n",
        );
        w(&auto, "off.desktop", "[Desktop Entry]\nName=Off\nExec=other\nHidden=true\n");
        w(&auto, "short.desktop", "[Desktop Entry]\nName=Short\nExec=env A=1 shortprog %U\n");
        w(&auto, "notes.txt", "x");
        let p = probe(d.path(), vec![auto, d.path().join("missing")], Arc::new(FakeTools::default()));
        let base = d.path().join("proc/1");
        assert_eq!(
            p.startup_source(&base, Some(Path::new("/opt/sync/bin/tray"))).as_deref(),
            Some("Autostart entry \"Sync Tray\"")
        );
        assert_eq!(
            p.startup_source(&base, Some(Path::new("/usr/bin/shortprog"))).as_deref(),
            Some("Autostart entry \"Short\"")
        );
        assert_eq!(
            p.startup_source(&base, Some(Path::new("/usr/bin/other"))),
            None,
            "a disabled entry did not start it"
        );
        assert_eq!(p.startup_source(&base, None), None);
    }

    #[test]
    fn probes_a_live_process_of_this_machine() {
        let mut p = LinuxDetails::new(HostPaths::host(), vec![], Arc::new(FakeTools::default()));
        let mut det = blank(std::process::id());
        let exe = std::env::current_exe().unwrap();
        p.enrich(
            ProcessRef {
                pid: std::process::id(),
                started: Some(Timestamp(0)),
            },
            Some(&exe),
            &mut det,
        );
        assert!(det.priority.is_some());
        assert!(det.threads.unwrap_or(0) >= 1);
        assert!(det.affinity.unwrap_or(0) != 0);
        assert!(det.handles.unwrap_or(0) >= 3);
        let ListOrReason::Items { items, .. } = &det.modules else {
            panic!("{:?}", det.modules)
        };
        assert!(items.iter().any(|m| m.contains("libc")), "{items:?}");
    }
}
