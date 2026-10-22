//! What a plugin process may do (ADR 0008 §5), enforced by the **launcher** before the plugin runs its first
//! instruction, so a binary that never cooperates is confined all the same. Pitwall is local only: a plugin gets
//! **no network** unless its manifest asks for it *and* the user enabled it ([`SandboxSpec::network`]).
//!
//! | | Windows | Linux | macOS |
//! |---|---|---|---|
//! | network | AppContainer without `internetClient` / `privateNetworkClientServer` (no sockets to anywhere, loopback included) | seccomp: `socket(AF_INET/AF_INET6)` and `io_uring_setup` fail; plus a private network namespace (`unshare(CLONE_NEWUSER\|CLONE_NEWNET)`) where unprivileged user namespaces are allowed | `sandbox-exec` profile `(deny network*)`, only the host's socket allowed |
//! | files | AppContainer: only its own folder (read) and data folder (write) | not confined (TODO: landlock) | not confined |
//! | child processes | job object: one process | not limited (a child inherits the network block) | `(deny process-exec*)` but the plugin itself |
//! | memory | job object process memory cap | host watches resident memory and stops it | host watches resident memory and stops it |
//! | lifetime | job object kill-on-close | stdin lifeline | stdin lifeline |
//!
//! Where the network block cannot be applied (no AppContainer, no seccomp, no `sandbox-exec`), the plugin is **not
//! started**: the launch fails with a message saying why (fail closed). Pure parts (the seccomp program, the
//! macOS profile, the notes) are unit-tested on every platform.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The sandbox of one plugin process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxSpec {
    /// A stable name of at most 64 characters from `A–Z a–z 0–9 . -` (the AppContainer profile on Windows).
    pub name: String,
    /// Memory cap in bytes (see the table in the module docs).
    pub memory_limit: Option<u64>,
    /// May open network sockets. Only true when the manifest asks for it and the user enabled it.
    pub network: bool,
    /// May start other programs (Windows, macOS; Linux does not limit this).
    pub children: bool,
    /// Folders the plugin may read (its own folder; an engine's model folder). Windows only (see module docs).
    pub read: Vec<PathBuf>,
    /// Folders the plugin may write (its private data folder). Windows only (see module docs).
    pub write: Vec<PathBuf>,
}

impl SandboxSpec {
    /// The default for a plugin: no network, no child processes, 512 MiB, its folder readable.
    pub fn locked(name: impl Into<String>, folder: impl Into<PathBuf>) -> SandboxSpec {
        SandboxSpec {
            name: name.into(),
            memory_limit: Some(512 << 20),
            network: false,
            children: false,
            read: vec![folder.into()],
            write: Vec::new(),
        }
    }

    /// Whether `name` is a valid [`SandboxSpec::name`].
    pub fn valid_name(name: &str) -> bool {
        !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    }

    /// What this platform enforces of the spec, as status lines for the user (shown with the plugin).
    pub fn enforcement(&self) -> Vec<Note> {
        enforcement(self, Platform::current())
    }
}

/// One line about how a plugin is sandboxed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    /// Whether this is something the sandbox does *not* enforce here.
    pub gap: bool,
    /// The line, for the user.
    pub message: String,
}

impl Note {
    fn enforced(m: impl Into<String>) -> Note {
        Note {
            gap: false,
            message: m.into(),
        }
    }

    fn gap(m: impl Into<String>) -> Note {
        Note {
            gap: true,
            message: m.into(),
        }
    }
}

/// The platforms the notes describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    Windows,
    Linux,
    MacOs,
    Other,
}

impl Platform {
    pub(crate) fn current() -> Platform {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "linux") {
            Platform::Linux
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Other
        }
    }
}

/// The notes of `spec` on `platform` (pure, so every platform's text is tested everywhere).
pub(crate) fn enforcement(spec: &SandboxSpec, platform: Platform) -> Vec<Note> {
    let mut notes = Vec::new();
    let network_granted = || Note::gap("network access was granted by the user");
    match platform {
        Platform::Windows => {
            notes.push(if spec.network {
                Note::enforced("AppContainer with the internet and private network capabilities (loopback stays blocked)")
            } else {
                Note::enforced("network blocked: AppContainer without network capabilities")
            });
            notes.push(Note::enforced("files confined to the plugin's folders (AppContainer)"));
            notes.push(if spec.children {
                Note::gap("may start other programs")
            } else {
                Note::enforced("cannot start other programs (job object)")
            });
            if let Some(m) = spec.memory_limit {
                notes.push(Note::enforced(format!("memory capped at {} MB (job object)", m.div_ceil(1 << 20))));
            }
        }
        Platform::Linux => {
            notes.push(if spec.network {
                network_granted()
            } else {
                Note::enforced("network blocked: seccomp denies internet sockets (and a private network namespace where allowed)")
            });
            notes.push(Note::gap("files are not confined on Linux yet"));
            notes.push(Note::gap("child processes are not limited on Linux (they inherit the network block)"));
            if let Some(m) = spec.memory_limit {
                notes.push(Note::enforced(format!("memory capped at {} MB (watched by Pitwall)", m.div_ceil(1 << 20))));
            }
        }
        Platform::MacOs => {
            notes.push(if spec.network {
                network_granted()
            } else {
                Note::enforced("network blocked: sandbox-exec profile denies network access")
            });
            notes.push(Note::gap("files are not confined on macOS yet"));
            notes.push(if spec.children {
                Note::gap("may start other programs")
            } else {
                Note::enforced("cannot start other programs (sandbox-exec profile)")
            });
            if let Some(m) = spec.memory_limit {
                notes.push(Note::enforced(format!("memory capped at {} MB (watched by Pitwall)", m.div_ceil(1 << 20))));
            }
        }
        Platform::Other => {
            notes.push(if spec.network {
                network_granted()
            } else {
                Note::gap("this platform has no network sandbox: plugins without network permission do not start")
            });
        }
    }
    notes
}

/// The macOS sandbox profile (SBPL) a plugin starts under through `/usr/bin/sandbox-exec`: everything allowed but
/// the network (except the host's socket `socket`) and starting programs (except the plugin `exe` itself). Pure, so
/// it is tested on every platform.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn macos_profile(spec: &SandboxSpec, exe: &Path, socket: &Path) -> String {
    let mut rules = vec!["(version 1)".to_string(), "(allow default)".to_string()];
    if !spec.network {
        rules.push("(deny network*)".into());
        // The connection to Pitwall is a Unix socket, which the network rules cover too.
        rules.push(format!("(allow network-outbound (remote unix-socket (path-literal {})))", sbpl_quote(socket)));
    }
    if !spec.children {
        rules.push("(deny process-exec*)".into());
        rules.push(format!("(allow process-exec (literal {}))", sbpl_quote(exe)));
    }
    let mut p = rules.join("\n");
    p.push('\n');
    p
}

/// A path as an SBPL string literal.
fn sbpl_quote(p: &Path) -> String {
    format!("\"{}\"", p.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\""))
}

/// The seccomp program a plugin without the network starts under on Linux (pure, so it is unit-tested on every
/// platform): `socket(AF_INET | AF_INET6, ..)` and `io_uring_setup` (whose socket operations bypass seccomp) fail with
/// `EACCES`; a system call of another architecture kills the process.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) mod bpf {
    /// One BPF instruction (the layout of `struct sock_filter`).
    #[repr(C)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Insn {
        /// Opcode.
        pub code: u16,
        /// Jump if true.
        pub jt: u8,
        /// Jump if false.
        pub jf: u8,
        /// Constant.
        pub k: u32,
    }

    const LD_W_ABS: u16 = 0x20; // BPF_LD | BPF_W | BPF_ABS
    const JEQ_K: u16 = 0x15; // BPF_JMP | BPF_JEQ | BPF_K
    const RET_K: u16 = 0x06; // BPF_RET | BPF_K
    pub(crate) const RET_ALLOW: u32 = 0x7fff_0000;
    pub(crate) const RET_KILL: u32 = 0x8000_0000; // SECCOMP_RET_KILL_PROCESS
    const RET_ERRNO: u32 = 0x0005_0000;
    const EACCES: u32 = 13;
    // Offsets in `struct seccomp_data`.
    const NR: u32 = 0;
    const ARCH: u32 = 4;
    const ARG0: u32 = 16;
    const AF_INET: u32 = 2;
    const AF_INET6: u32 = 10;

    #[cfg(target_arch = "aarch64")]
    pub(crate) const ARCH_ID: u32 = 0xC000_00B7; // AUDIT_ARCH_AARCH64
    /// socket, io_uring_setup.
    #[cfg(target_arch = "aarch64")]
    pub(crate) const SYS: (u32, u32) = (198, 425);
    #[cfg(not(target_arch = "aarch64"))]
    pub(crate) const ARCH_ID: u32 = 0xC000_003E; // AUDIT_ARCH_X86_64
    #[cfg(not(target_arch = "aarch64"))]
    pub(crate) const SYS: (u32, u32) = (41, 425);

    fn stmt(code: u16, k: u32) -> Insn {
        Insn { code, jt: 0, jf: 0, k }
    }

    fn jeq(k: u32, jt: u8, jf: u8) -> Insn {
        Insn { code: JEQ_K, jt, jf, k }
    }

    /// The program (see the module docs).
    pub fn deny_network() -> Vec<Insn> {
        let (socket, io_uring_setup) = SYS;
        let deny = RET_ERRNO | EACCES;
        vec![
            stmt(LD_W_ABS, ARCH),
            jeq(ARCH_ID, 1, 0),
            stmt(RET_K, RET_KILL),
            stmt(LD_W_ABS, NR),
            // io_uring_setup -> deny
            jeq(io_uring_setup, 0, 1),
            stmt(RET_K, deny),
            // nr != socket -> allow; else arg0 == AF_INET || AF_INET6 -> deny, else allow
            jeq(socket, 0, 4),
            stmt(LD_W_ABS, ARG0),
            jeq(AF_INET, 1, 0),
            jeq(AF_INET6, 0, 1),
            stmt(RET_K, deny),
            stmt(RET_K, RET_ALLOW),
        ]
    }

    /// Runs the program on (arch, nr, arg0), as the kernel would (tests).
    #[cfg(test)]
    pub(crate) fn run(p: &[Insn], arch: u32, nr: u32, arg0: u32) -> u32 {
        let mut acc = 0u32;
        let mut pc = 0usize;
        loop {
            let i = p[pc];
            match i.code {
                LD_W_ABS => {
                    acc = match i.k {
                        ARCH => arch,
                        NR => nr,
                        ARG0 => arg0,
                        k => panic!("offset {k}"),
                    };
                    pc += 1;
                }
                JEQ_K => pc += 1 + if acc == i.k { i.jt as usize } else { i.jf as usize },
                RET_K => return i.k,
                c => panic!("code {c}"),
            }
        }
    }

    #[cfg(test)]
    pub(crate) const DENY: u32 = RET_ERRNO | EACCES;
    #[cfg(test)]
    pub(crate) const INET: (u32, u32) = (AF_INET, AF_INET6);
}

/// Linux: confines `cmd` before it runs: `no_new_privs`, a private user and network namespace when the kernel lets an
/// unprivileged process create one, and the seccomp network filter (required: the spawn fails without it). Applied
/// in the forked child between `fork` and `exec`; everything is prepared before the fork. Returns the pipe the child
/// writes one status byte to ([`NETNS_YES`] / [`NETNS_NO`]); read it with [`LinuxReport::note`] after the spawn.
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
pub(crate) fn contain_before_exec(cmd: &mut std::process::Command, spec: &SandboxSpec) -> Result<Option<LinuxReport>, String> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::process::CommandExt;
    if spec.network {
        return Ok(None);
    }
    let filter = bpf::deny_network();
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: pipe2 fills two descriptors it opened; each is owned once below.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
        return Err(format!("cannot create the sandbox status pipe: {}", std::io::Error::last_os_error()));
    }
    // SAFETY: both descriptors were just opened by pipe2 and are owned by nothing else.
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    let status_fd = write.as_raw_fd();
    // SAFETY: the hook runs in the forked (single-threaded) child before exec and only makes system calls on data
    // prepared above (the BPF program and the status pipe); it allocates nothing.
    unsafe {
        cmd.pre_exec(move || {
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let netns = libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET) == 0;
            let code: u8 = if netns { NETNS_YES } else { NETNS_NO };
            libc::write(status_fd, (&code as *const u8).cast(), 1);
            let fprog = libc::sock_fprog {
                len: filter.len() as u16,
                filter: filter.as_ptr() as *mut libc::sock_filter,
            };
            if libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &fprog as *const libc::sock_fprog) != 0 {
                return Err(std::io::Error::other(format!(
                    "the network sandbox (seccomp) is not available: {}",
                    std::io::Error::last_os_error()
                )));
            }
            Ok(())
        });
    }
    Ok(Some(LinuxReport { read, write: Some(write) }))
}

/// What a contained child reported from between `fork` and `exec` (Linux).
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
pub(crate) struct LinuxReport {
    read: std::os::fd::OwnedFd,
    write: Option<std::os::fd::OwnedFd>,
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
impl LinuxReport {
    /// The child's note; call once the spawn returned (the child wrote its status before it ran the program).
    pub(crate) fn note(mut self) -> Note {
        use std::os::fd::AsRawFd;
        drop(self.write.take());
        let mut b = [0u8; 1];
        // SAFETY: reads at most one byte into `b` from a descriptor this report owns.
        let n = unsafe { libc::read(self.read.as_raw_fd(), b.as_mut_ptr().cast(), 1) };
        netns_note((n == 1).then_some(b[0]))
    }
}

/// Status bytes a contained child writes between `fork` and `exec` (Linux): whether it got a network namespace.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const NETNS_YES: u8 = 1;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) const NETNS_NO: u8 = 0;

/// The note for what a Linux child reported (`None`: it reported nothing).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn netns_note(status: Option<u8>) -> Note {
    match status {
        Some(NETNS_YES) => Note::enforced("private network namespace (no interfaces)"),
        _ => Note::gap("no private network namespace (unprivileged user namespaces are not allowed here); seccomp alone blocks the network"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seccomp_program_denies_internet_sockets_and_io_uring_only() {
        let p = bpf::deny_network();
        let (socket, io_uring) = bpf::SYS;
        let (inet, inet6) = bpf::INET;
        assert_eq!(bpf::run(&p, bpf::ARCH_ID, socket, inet), bpf::DENY);
        assert_eq!(bpf::run(&p, bpf::ARCH_ID, socket, inet6), bpf::DENY);
        assert_eq!(bpf::run(&p, bpf::ARCH_ID, io_uring, 0), bpf::DENY);
        assert_eq!(bpf::run(&p, bpf::ARCH_ID, socket, 1), bpf::RET_ALLOW, "AF_UNIX stays allowed (the host connection)");
        assert_eq!(bpf::run(&p, bpf::ARCH_ID, 0, inet), bpf::RET_ALLOW, "another call with the same argument");
        assert_eq!(bpf::run(&p, bpf::ARCH_ID ^ 1, socket, 1), bpf::RET_KILL, "another architecture");
        assert_eq!(std::mem::size_of::<bpf::Insn>(), 8);
    }

    #[test]
    fn the_macos_profile_denies_the_network_but_the_host_socket() {
        let exe = Path::new("/p/x/bin \"q\"");
        let sock = Path::new("/private/var/folders/a/pitwall-plugin-1/w.sock");
        let locked = macos_profile(&SandboxSpec::locked("pitwall.plugin.x", "/p/x"), exe, sock);
        assert!(locked.starts_with("(version 1)\n(allow default)\n"), "{locked}");
        for rule in [
            "(deny network*)",
            r#"(allow network-outbound (remote unix-socket (path-literal "/private/var/folders/a/pitwall-plugin-1/w.sock")))"#,
            "(deny process-exec*)",
            r#"(allow process-exec (literal "/p/x/bin \"q\""))"#,
        ] {
            assert!(locked.contains(rule), "{rule} in {locked}");
        }
        // The allow comes after the deny (in SBPL the last matching rule wins).
        assert!(locked.find("(deny network*)") < locked.find("(allow network-outbound"));
        let open = SandboxSpec {
            network: true,
            children: true,
            ..SandboxSpec::locked("n", "/p")
        };
        assert_eq!(macos_profile(&open, exe, sock), "(version 1)\n(allow default)\n");
    }

    #[test]
    fn every_platform_says_what_it_enforces() {
        let locked = SandboxSpec::locked("pitwall.plugin.x", "/p");
        for platform in [Platform::Windows, Platform::Linux, Platform::MacOs] {
            let notes = enforcement(&locked, platform);
            assert!(notes[0].message.starts_with("network blocked") && !notes[0].gap, "{platform:?}: {notes:?}");
            assert!(notes.iter().any(|n| n.message.contains("512 MB")), "{platform:?}");
            let open = SandboxSpec {
                network: true,
                children: true,
                memory_limit: None,
                ..locked.clone()
            };
            let notes = enforcement(&open, platform);
            assert!(!notes[0].message.starts_with("network blocked"), "{platform:?}");
            assert!(!notes.iter().any(|n| n.message.contains("memory")));
        }
        assert!(enforcement(&locked, Platform::Other)[0].gap);
        assert!(enforcement(&SandboxSpec { network: true, ..locked.clone() }, Platform::Other)[0].gap);
        assert!(!locked.enforcement().is_empty());
        assert!(netns_note(Some(NETNS_YES)).message.contains("namespace") && !netns_note(Some(NETNS_YES)).gap);
        assert!(netns_note(Some(NETNS_NO)).gap && netns_note(None).gap);
    }

    #[test]
    fn names_and_json() {
        assert!(SandboxSpec::valid_name("pitwall.plugin.a-b") && !SandboxSpec::valid_name("a b"));
        assert!(!SandboxSpec::valid_name("") && !SandboxSpec::valid_name(&"x".repeat(65)));
        let s = SandboxSpec::locked("n", "/p");
        assert_eq!(serde_json::from_str::<SandboxSpec>(&serde_json::to_string(&s).unwrap()).unwrap(), s);
    }
}
