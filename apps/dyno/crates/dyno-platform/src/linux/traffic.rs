//! Per-program network traffic on Linux without elevation: the kernel's `sock_diag` answers (any user may ask) give the
//! cumulative bytes of every TCP socket, and `/proc/<pid>/fd` says which programs hold which socket, as far as the
//! user's rights reach (other users' programs are not attributed). UDP has no byte counters and is not counted.

use super::paths::{read_trim, HostPaths};
use super::sys;
use crate::parse::sock_diag::{counters_by_process, parse_diag_chunk, socket_inode, tcp_dump_request, DiagSocket, MAX_DIAG_SOCKETS};
use dyno_core::network_traffic::TrafficCounter;
use dyno_core::ports::{PlatformError, ProcessTraffic};
use std::collections::HashMap;

/// Most descriptors read per process (a bound against a process holding millions).
const MAX_FDS_PER_PROCESS: usize = 65_536;

const AF_INET: u8 = 2;
const AF_INET6: u8 = 10;

/// Reads traffic through `sock_diag` and `/proc`.
#[derive(Debug, Clone)]
pub struct LinuxTraffic {
    paths: HostPaths,
}

impl LinuxTraffic {
    /// A reader of this machine.
    pub fn new(paths: HostPaths) -> Self {
        LinuxTraffic { paths }
    }

    fn sockets(family: u8) -> Result<Vec<DiagSocket>, PlatformError> {
        let mut sockets = Vec::new();
        let mut refusal = None;
        sys::sock_diag_dump(&tcp_dump_request(family, 1), |buf| {
            let chunk = parse_diag_chunk(buf);
            sockets.extend(chunk.sockets);
            refusal = chunk.error.filter(|e| *e != 0);
            chunk.done || refusal.is_some() || sockets.len() >= MAX_DIAG_SOCKETS
        })?;
        match refusal {
            Some(errno) => Err(PlatformError::Failed(format!(
                "The kernel refused the socket statistics ({}).",
                std::io::Error::from_raw_os_error(errno)
            ))),
            None => Ok(sockets),
        }
    }

    /// Inode of every socket held by a program this user may inspect, with its pid and name.
    fn owners(&self) -> HashMap<u64, (u32, String)> {
        let mut owners = HashMap::new();
        let Ok(dir) = std::fs::read_dir(&self.paths.proc) else {
            return owners;
        };
        for entry in dir.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
                continue;
            };
            // Another user's processes refuse to list their descriptors: skipped silently.
            let Ok(fds) = std::fs::read_dir(self.paths.pid(pid).join("fd")) else {
                continue;
            };
            let mut name = None;
            for fd in fds.flatten().take(MAX_FDS_PER_PROCESS) {
                let Ok(link) = std::fs::read_link(fd.path()) else { continue };
                let Some(inode) = link.to_str().and_then(socket_inode) else {
                    continue;
                };
                let name = name.get_or_insert_with(|| read_trim(&self.paths.pid(pid).join("comm")).unwrap_or_else(|| format!("pid {pid}")));
                owners.insert(inode, (pid, name.clone()));
            }
        }
        owners
    }
}

impl ProcessTraffic for LinuxTraffic {
    fn counters(&self) -> Result<Vec<TrafficCounter>, PlatformError> {
        let mut sockets = Self::sockets(AF_INET).map_err(|e| {
            PlatformError::Unsupported(format!(
                "Traffic per program needs the kernel's socket statistics, which are not available here ({e})."
            ))
        })?;
        // A machine without IPv6 refuses the second question; the first answer still counts.
        if let Ok(v6) = Self::sockets(AF_INET6) {
            sockets.extend(v6);
        }
        Ok(counters_by_process(&sockets, &self.owners()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners_come_from_socket_links_of_visible_processes() {
        let dir = tempfile::tempdir().unwrap();
        let p = HostPaths::under(dir.path());
        let fd = p.pid(41).join("fd");
        std::fs::create_dir_all(&fd).unwrap();
        std::fs::write(p.pid(41).join("comm"), "alpha\n").unwrap();
        std::os::unix::fs::symlink("socket:[900]", fd.join("3")).unwrap();
        std::os::unix::fs::symlink("pipe:[5]", fd.join("4")).unwrap();
        // A process whose descriptors cannot be listed, and entries that are not pids.
        std::fs::create_dir_all(p.pid(42)).unwrap();
        std::fs::create_dir_all(p.proc.join("self")).unwrap();
        std::fs::write(p.proc.join("meminfo"), "x").unwrap();
        let owners = LinuxTraffic::new(p).owners();
        assert_eq!(owners.len(), 1);
        assert_eq!(owners[&900], (41, "alpha".to_string()));
        assert!(LinuxTraffic::new(HostPaths::under(&dir.path().join("none"))).owners().is_empty());
    }

    #[test]
    fn this_machine_answers_with_the_bytes_of_a_connection_the_test_made() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        client.write_all(&[7u8; 4096]).unwrap();
        let mut got = [0u8; 4096];
        server.read_exact(&mut got).unwrap();
        match LinuxTraffic::new(HostPaths::host()).counters() {
            Ok(c) => {
                // Some container runtimes answer sock_diag but do not expose socket inodes through /proc/<pid>/fd, so no socket can be
                // matched to a program there; the parsers are covered by fixtures. A match, when there is one, must be right.
                let Some(me) = c.iter().find(|c| c.pid == std::process::id()) else {
                    eprintln!("sock_diag answered but this process's sockets were not attributed (container /proc)");
                    return;
                };
                assert!(me.bytes_out >= 4096 && me.bytes_in >= 4096, "{me:?}");
            }
            // A sandbox without netlink says why instead of failing.
            Err(e) => assert!(matches!(e, PlatformError::Unsupported(_)), "{e:?}"),
        }
    }
}
