//! What an endpoint has not read yet, measured on its own handle (ADR 0122 §3), and the act gate that holds the endpoint
//! until the client's bytes are there.
//!
//! The endpoint's own accepted handle is the only place that sees its unread input on every OS (ADR 0122, "Common practice":
//! on macOS the writer's side shows nothing). [`queued_bytes`] asks the OS: `FIONREAD` on a Unix socket (Linux `SIOCINQ` is the
//! same request) or a loopback TCP socket, `PeekNamedPipe`'s `lpTotalBytesAvail` on a Windows pipe, and `ioctlsocket(FIONREAD)`
//! on a Windows TCP socket. Winsock says its count is "normally" the whole queue, not always, so a Windows TCP count is only a
//! lower bound: more than zero means input is queued, never how much.
//!
//! [`QueueGate`] is what an endpoint's test-only act gate calls (`Host::with_act_gate`, `ServerConfig::with_act_gate`, and the
//! gates of `paddock-mcp` and `pitcrew-service`): after `accept` and before the endpoint reads or refuses, it waits, bounded,
//! until the queue holds more than the rig asked for, records what it saw as [`Evidence`] and lets the endpoint go on. The gate
//! runs before the endpoint reads, so the endpoint has no read buffer yet: the evidence is the OS queue alone (`os_queue=<n>`).

use super::Evidence;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How often [`wait_queued`] asks the OS again while the queue is below the bound: the OS has no event for "more than n bytes
/// are queued" (a readable event says only "more than zero"), so the wait polls. It is the poll's interval, not a limit.
const POLL: Duration = Duration::from_millis(1);

/// The OS handle of one side of a connection, as [`queued_bytes`] reads it. Plain integers, so a handle crosses threads (an
/// async endpoint hands it to a blocking task). The handle is only read; the caller keeps the connection open meanwhile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueHandle {
    /// A Unix socket or a TCP socket's file descriptor.
    #[cfg(unix)]
    Fd(std::os::fd::RawFd),
    /// A Windows named pipe's handle (either end), as an integer.
    #[cfg(windows)]
    Pipe(usize),
    /// A Windows TCP socket.
    #[cfg(windows)]
    Socket(u64),
}

impl QueueHandle {
    /// The handle of a `paddock-ipc` stream: a Unix socket, or a named pipe on Windows.
    pub fn of_stream(stream: &crate::endpoint::Stream) -> QueueHandle {
        stream.queue_handle()
    }

    /// The handle of a TCP stream.
    pub fn of_tcp(stream: &std::net::TcpStream) -> QueueHandle {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            QueueHandle::Fd(stream.as_raw_fd())
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawSocket;
            QueueHandle::Socket(stream.as_raw_socket())
        }
    }

    /// The handle of a Unix socket.
    #[cfg(unix)]
    pub fn of_unix(stream: &std::os::unix::net::UnixStream) -> QueueHandle {
        use std::os::fd::AsRawFd;
        QueueHandle::Fd(stream.as_raw_fd())
    }

    /// A handle the OS refuses (the probe's self-test of "not measured").
    pub fn invalid() -> QueueHandle {
        #[cfg(unix)]
        {
            QueueHandle::Fd(-1)
        }
        #[cfg(windows)]
        {
            // The null handle: never a pipe (-1 would be the current process's pseudo-handle).
            QueueHandle::Pipe(0)
        }
    }

    /// Whether the count is only a lower bound (Windows TCP, see the module documentation).
    pub fn lower_bound_only(self) -> bool {
        #[cfg(windows)]
        {
            matches!(self, QueueHandle::Socket(_))
        }
        #[cfg(unix)]
        {
            false
        }
    }
}

/// The bytes queued on `handle` that its side has not read yet, as the OS says; `None` when the OS could not tell (an invalid
/// or closed handle): **not measured**, never zero (ER-33). For a Windows TCP socket it is a lower bound
/// ([`QueueHandle::lower_bound_only`]).
pub fn queued_bytes(handle: QueueHandle) -> Option<usize> {
    query(handle).ok()
}

/// [`queued_bytes`] with the OS's error.
fn query(handle: QueueHandle) -> std::io::Result<usize> {
    match handle {
        #[cfg(unix)]
        QueueHandle::Fd(fd) => {
            let mut n: libc::c_int = 0;
            // SAFETY: FIONREAD writes one int through the pointer, which points at `n`; an invalid descriptor fails with EBADF
            // and writes nothing.
            #[allow(unsafe_code)]
            let r = unsafe { libc::ioctl(fd, libc::FIONREAD, &mut n as *mut libc::c_int) };
            if r != 0 {
                return Err(std::io::Error::last_os_error());
            }
            usize::try_from(n).map_err(|_| std::io::Error::other(format!("FIONREAD answered {n}")))
        }
        #[cfg(windows)]
        QueueHandle::Pipe(h) => {
            use windows_sys::Win32::System::Pipes::PeekNamedPipe;
            let mut avail: u32 = 0;
            // SAFETY: no buffer is passed (size 0), so the call only writes the count through `avail`; an invalid handle fails
            // and writes nothing.
            #[allow(unsafe_code)]
            let ok = unsafe {
                PeekNamedPipe(
                    h as windows_sys::Win32::Foundation::HANDLE,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut avail,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(avail as usize)
        }
        #[cfg(windows)]
        QueueHandle::Socket(s) => {
            use windows_sys::Win32::Networking::WinSock::{FIONREAD, WSAGetLastError, ioctlsocket};
            let mut n: u32 = 0;
            // SAFETY: FIONREAD writes one u32 through the pointer, which points at `n`; an invalid socket fails and writes
            // nothing.
            #[allow(unsafe_code)]
            let r = unsafe { ioctlsocket(s as usize, FIONREAD, &mut n) };
            if r != 0 {
                // SAFETY: no arguments; reads this thread's last Winsock error.
                #[allow(unsafe_code)]
                let code = unsafe { WSAGetLastError() };
                return Err(std::io::Error::from_raw_os_error(code));
            }
            Ok(n as usize)
        }
    }
}

/// Waits at most `bound` until more than `more_than` bytes are queued on `handle` (for a Windows TCP socket: more than zero,
/// its count being a lower bound). Held: the bound was met (`os_queue=<n>`); missed: the bound passed below it; not measured:
/// the OS could not tell.
pub fn wait_queued(handle: QueueHandle, more_than: usize, bound: Duration) -> Evidence {
    let want = if handle.lower_bound_only() {
        0
    } else {
        more_than
    };
    let started = Instant::now();
    loop {
        let n = match query(handle) {
            Ok(n) => n,
            Err(e) => {
                return Evidence::NotMeasured(format!(
                    "the OS could not tell what is queued on the endpoint's handle: {e} ({:?})",
                    e.kind()
                ));
            }
        };
        if n > want {
            return Evidence::Held(format!("os_queue={n} (more than {want})"));
        }
        if started.elapsed() >= bound {
            return Evidence::Missed(format!(
                "os_queue={n} after {} ms, not more than {want}",
                bound.as_millis()
            ));
        }
        std::thread::sleep(POLL);
    }
}

/// The rig's side of an act gate (ADR 0122 §2, §3): armed for the endpoint's next act, it holds the endpoint until more than the
/// armed count is queued on its handle (at most its bound) and keeps the evidence for the rig. Unarmed, it lets every act
/// through at once (the connections a rig opens before the one it measures).
#[derive(Debug)]
pub struct QueueGate {
    bound: Duration,
    state: Mutex<GateState>,
    seen: Condvar,
}

#[derive(Debug, Default)]
struct GateState {
    /// The count the next act must exceed, while armed.
    armed: Option<usize>,
    /// What the armed act saw.
    evidence: Option<Evidence>,
}

impl QueueGate {
    /// A gate that waits at most `bound` for the queue ([`super::raw::READ_GUARD`] in the endings rigs; a positive control
    /// whose client writes nothing gives a short one, since the miss is certain).
    pub fn new(bound: Duration) -> QueueGate {
        QueueGate {
            bound,
            state: Mutex::new(GateState::default()),
            seen: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Measures the endpoint's next act: it must see more than `more_than` bytes queued (busy: 0, the hello; a client that
    /// keeps writing: its first frame's length).
    pub fn arm(&self, more_than: usize) {
        let mut s = self.lock();
        s.armed = Some(more_than);
        s.evidence = None;
    }

    /// The endpoint's side, called by its act gate with its accepted handle before it reads or refuses: waits when armed,
    /// returns at once otherwise.
    pub fn hold(&self, handle: QueueHandle) {
        let Some(more_than) = self.lock().armed.take() else {
            return;
        };
        let evidence = wait_queued(handle, more_than, self.bound);
        self.lock().evidence = Some(evidence);
        self.seen.notify_all();
    }

    /// What the armed act saw, waiting at most `guard` for the endpoint to reach its gate; not measured when it never did.
    pub fn evidence(&self, guard: Duration) -> Evidence {
        let s = self.lock();
        let (mut s, _) = self
            .seen
            .wait_timeout_while(s, guard, |s| s.evidence.is_none())
            .unwrap_or_else(|p| p.into_inner());
        s.evidence.take().unwrap_or_else(|| {
            Evidence::NotMeasured(format!(
                "the endpoint did not reach its act gate within {} s",
                guard.as_secs()
            ))
        })
    }
}
