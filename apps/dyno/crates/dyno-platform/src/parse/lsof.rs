//! `lsof -F` field output (one field per line, the first character names it): `p` pid, `c` command, `f` file
//! descriptor, `t` type, `n` name, `P` protocol, `T` TCP/TPI info such as `TST=ESTABLISHED`.

use dyno_core::process::{Connection, Pid};

/// One open file of one process.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LsofFile {
    /// Owning process.
    pub pid: Pid,
    /// The process's command name.
    pub command: String,
    /// File descriptor (`cwd`, `txt`, `3u`…).
    pub fd: String,
    /// Type (`REG`, `DIR`, `IPv4`, `IPv6`…).
    pub kind: String,
    /// Name: a path, or `local->remote` for a socket.
    pub name: String,
    /// Protocol for sockets (`TCP`, `UDP`).
    pub protocol: String,
    /// TCP state (`ESTABLISHED`, `LISTEN`), when given.
    pub state: String,
}

impl LsofFile {
    /// Whether this is a network socket.
    pub fn is_socket(&self) -> bool {
        matches!(self.kind.as_str(), "IPv4" | "IPv6") && !self.protocol.is_empty()
    }

    /// Whether this is a file on disk (regular file or directory, excluding the program text and cwd).
    pub fn is_path(&self) -> bool {
        matches!(self.kind.as_str(), "REG" | "DIR") && self.name.starts_with('/') && !matches!(self.fd.as_str(), "txt" | "cwd" | "rtd")
    }

    /// The socket as a connection.
    pub fn connection(&self) -> Connection {
        let (local, remote) = match self.name.split_once("->") {
            Some((l, r)) => (l.to_string(), r.to_string()),
            None => (self.name.clone(), String::new()),
        };
        let six = if self.kind == "IPv6" { "6" } else { "" };
        Connection {
            protocol: format!("{}{six}", self.protocol),
            local,
            remote,
            state: self.state.clone(),
        }
    }
}

/// Parses `lsof -F` output into files. Lines it doesn't know are skipped; never panics.
pub fn parse_fields(text: &str) -> Vec<LsofFile> {
    let mut out = Vec::new();
    let (mut pid, mut command) = (0, String::new());
    let mut cur: Option<LsofFile> = None;
    for line in text.lines() {
        let mut chars = line.chars();
        let Some(tag) = chars.next() else { continue };
        let value = chars.as_str();
        match tag {
            'p' => {
                out.extend(cur.take());
                pid = value.trim().parse().unwrap_or(0);
                command.clear();
            }
            'c' => command = value.to_string(),
            'f' => {
                out.extend(cur.take());
                cur = Some(LsofFile {
                    pid,
                    command: command.clone(),
                    fd: value.to_string(),
                    ..LsofFile::default()
                });
            }
            't' | 'n' | 'P' | 'T' => {
                let file = cur.get_or_insert_with(|| LsofFile {
                    pid,
                    command: command.clone(),
                    ..LsofFile::default()
                });
                match tag {
                    't' => file.kind = value.to_string(),
                    'n' => file.name = value.to_string(),
                    'P' => file.protocol = value.to_string(),
                    _ => {
                        if let Some(st) = value.strip_prefix("ST=") {
                            file.state = st.to_string();
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out.extend(cur);
    // Records that only named a process (`-F pc` without files) still count once.
    out.retain(|f| f.pid != 0);
    out
}

/// The distinct processes (pid, command) in `lsof -F` output, in order of appearance.
pub fn processes(files: &[LsofFile]) -> Vec<(Pid, String)> {
    let mut seen: Vec<(Pid, String)> = Vec::new();
    for f in files {
        if !seen.iter().any(|(p, _)| *p == f.pid) {
            seen.push((f.pid, f.command.clone()));
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "p123\ncSafari\nfcwd\ntDIR\nn/\nftxt\ntREG\nn/Applications/Safari.app/Contents/MacOS/Safari\n\
f12u\ntREG\nn/Users/me/doc.txt\nf20u\ntIPv4\nPTCP\nn10.0.0.2:50123->17.1.2.3:443\nTST=ESTABLISHED\nTQR=0\n\
f21u\ntIPv6\nPTCP\nn*:8080\nTST=LISTEN\nf22u\ntIPv4\nPUDP\nn*:5353\np456\ncvim\nf3u\ntREG\nn/Users/me/doc.txt\n";

    #[test]
    fn parses_files_and_sockets() {
        let files = parse_fields(SAMPLE);
        assert_eq!(files.len(), 7);
        let paths: Vec<&str> = files.iter().filter(|f| f.is_path()).map(|f| f.name.as_str()).collect();
        assert_eq!(paths, ["/Users/me/doc.txt", "/Users/me/doc.txt"]);
        let conns: Vec<Connection> = files.iter().filter(|f| f.is_socket()).map(LsofFile::connection).collect();
        assert_eq!(conns.len(), 3);
        assert_eq!(conns[0].protocol, "TCP");
        assert_eq!(conns[0].remote, "17.1.2.3:443");
        assert_eq!(conns[0].state, "ESTABLISHED");
        assert_eq!(conns[1].protocol, "TCP6");
        assert_eq!(conns[1].local, "*:8080");
        assert_eq!(conns[1].remote, "");
        assert_eq!(conns[2].protocol, "UDP");
        assert_eq!(processes(&files), vec![(123, "Safari".to_string()), (456, "vim".to_string())]);
    }

    #[test]
    fn tolerates_partial_and_odd_output() {
        assert!(parse_fields("").is_empty());
        assert!(parse_fields("\n\nzzz\n").is_empty());
        let f = parse_fields("p7\ncsh\nn/tmp/x\n");
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].pid, f[0].name.as_str(), f[0].command.as_str()), (7, "/tmp/x", "sh"));
        assert!(parse_fields("pnotanumber\nf1\n").is_empty());
        assert_eq!(parse_fields("p9\ncé\nf1\n")[0].command, "é");
    }
}
