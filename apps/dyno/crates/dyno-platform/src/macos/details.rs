//! macOS process details: nice value and background class, open files, sockets and mapped libraries from `libproc` and the kernel socket lists (never `lsof`, ADR 0026),
//! and the code signature from `codesign`. Everything runs with a timeout; failures become unavailable fields.

use crate::parse::mac_tools::parse_codesign;
use crate::processes::DetailsProbe;
use crate::unix_proc::get_nice;
use dyno_core::process::{ListOrReason, Priority, ProcessDetails, ProcessRef, SignatureInfo};
use std::path::Path;
use std::time::Duration;

const TOOL_TIMEOUT: Duration = Duration::from_secs(8);

/// Details through macOS' own tools.
pub struct MacDetails;

impl MacDetails {
    /// The probe.
    pub fn new() -> Self {
        MacDetails
    }
}

impl Default for MacDetails {
    fn default() -> Self {
        Self::new()
    }
}

fn signature(exe: &Path) -> SignatureInfo {
    let mut cmd = crate::process::command("/usr/bin/codesign");
    cmd.args(["-dv", "--verbose=2"]).arg(exe);
    match crate::process::run_with_timeout(cmd, TOOL_TIMEOUT) {
        // codesign writes its report to stderr.
        Ok(out) => parse_codesign(&String::from_utf8_lossy(&out.stderr), out.status.success()),
        Err(e) => SignatureInfo::Unknown(e.to_string()),
    }
}

impl DetailsProbe for MacDetails {
    fn enrich(&mut self, target: ProcessRef, exe: Option<&Path>, details: &mut ProcessDetails) {
        let pid = target.pid;
        details.priority = libc::pid_t::try_from(pid)
            .ok()
            .and_then(|p| get_nice(p).ok())
            .map(Priority::from_nice);
        details.efficiency_mode = super::background::is_background(pid);
        details.affinity = None;
        details.signature = match exe {
            Some(e) => signature(e),
            None => SignatureInfo::Unknown("The program file is not known".into()),
        };
        // Never `lsof`: it asks the kernel about every socket and pipe descriptor of the process, which can mark another
        // program's socket `SS_DRAINING` (ADR 0026). Files and libraries come from `libproc` (descriptors listed from the
        // table, only file descriptors asked about), connections from the kernel's socket lists.
        let fds = super::libproc::fds(pid);
        if fds.is_empty() {
            let why = "macOS lists open files and connections only for your own processes".to_string();
            details.open_files = ListOrReason::Unavailable(why.clone());
            details.connections = ListOrReason::Unavailable(why.clone());
            details.modules = ListOrReason::Unavailable(why);
            return;
        }
        details.handles = Some(u32::try_from(fds.len()).unwrap_or(u32::MAX));
        details.open_files = ListOrReason::capped(
            fds.iter()
                .filter(|(_, t)| super::libproc::is_vnode(*t))
                .filter_map(|(fd, _)| super::libproc::vnode_path(pid, *fd))
                .collect(),
        );
        details.connections = match super::pcblist::connections_of(pid) {
            Ok(c) => ListOrReason::capped(c),
            Err(e) => ListOrReason::Unavailable(e.to_string()),
        };
        details.modules = ListOrReason::capped(
            super::libproc::mapped_files(pid)
                .into_iter()
                .filter(|p| p.contains(".dylib"))
                .collect(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processes::SysinfoProcesses;
    use dyno_core::ports::ProcessSource;

    #[test]
    fn describes_the_test_process_itself() {
        let mut src = SysinfoProcesses::new(Box::new(MacDetails::new()));
        let me = std::process::id();
        let mine = src.processes().into_iter().find(|p| p.pid == me).unwrap();
        let d = src.details(mine.reference()).unwrap();
        // The process's own nice value, not an assumed 0: a loaded Mac (or a tool that throttles busy processes) may have
        // lowered this test's priority, which made `Normal` fail at random.
        let nice = get_nice(libc::pid_t::try_from(me).unwrap()).unwrap();
        assert_eq!(d.priority, Some(Priority::from_nice(nice)));
        assert_eq!(d.efficiency_mode, Some(false));
        assert!(!matches!(d.signature, SignatureInfo::Signed(ref s) if s.is_empty()));
        match &d.open_files {
            ListOrReason::Items { items, .. } => assert!(items.iter().all(|p| p.starts_with('/'))),
            ListOrReason::Unavailable(why) => assert!(!why.is_empty()),
        }
        assert!(matches!(d.modules, ListOrReason::Items { .. } | ListOrReason::Unavailable(_)));
    }

    #[test]
    fn a_listening_socket_of_a_dummy_shows_as_a_connection() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut src = SysinfoProcesses::new(Box::new(MacDetails::new()));
        let me = std::process::id();
        let mine = src.processes().into_iter().find(|p| p.pid == me).unwrap();
        let d = src.details(mine.reference()).unwrap();
        let ListOrReason::Items { items, .. } = d.connections else {
            panic!("own sockets should be listed")
        };
        assert!(
            items.iter().any(|c| c.local.ends_with(&format!(":{port}")) && c.state == "LISTEN"),
            "{items:?}"
        );
        drop(listener);
    }

    #[test]
    fn signatures_of_system_and_unsigned_files() {
        assert!(matches!(
            signature(Path::new("/bin/ls")),
            SignatureInfo::Signed(_) | SignatureInfo::Invalid(_)
        ));
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("plain");
        std::fs::write(&f, b"not a program").unwrap();
        assert!(!matches!(signature(&f), SignatureInfo::Signed(_)));
    }
}
