//! The app's side of the app ↔ service protocol (ADR 0001, ADR 0004). Commands depend on the [`ServiceClient`]
//! trait, never on a transport, so the transport can be swapped and the commands tested with a fake.
//!
//! The local client opens a connection per request (hello, request, response) — the volume is tiny — and one
//! long-lived subscription for pushed events (`events.rs`). Every exchange has a timeout; a hung service never
//! blocks the UI.

use pitcrew_core::protocol::{read_frame, write_frame, Request, Response, ServiceError, PROTOCOL_VERSION};
use std::io::{Read, Write};
use std::time::Duration;

/// Why a request could not be answered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientError {
    /// The service is not running or not installed.
    #[error("The Pitcrew service is not running: {0}")]
    Unavailable(String),
    /// The app and the service are different versions.
    #[error("{0}")]
    Mismatch(String),
    /// The service answered something that is not part of the protocol.
    #[error("The Pitcrew service gave an unexpected answer: {0}")]
    Protocol(String),
    /// The service refused or failed the request.
    #[error(transparent)]
    Service(#[from] ServiceError),
}

/// Sends one request to the service and returns its response. Implementations must be safe to call from any thread
/// and must time out instead of blocking forever.
pub trait ServiceClient: Send + Sync {
    /// One request, one response.
    fn request(&self, request: Request) -> Result<Response, ClientError>;
}

/// A bidirectional stream to the service.
trait Stream: Read + Write + Send {}
impl<T: Read + Write + Send> Stream for T {}

/// Connects to the service. `require_system`: before anything is sent, check that the program serving the pipe is the SYSTEM
/// service (Windows; the elevated forwarder, which carries a registration token, asks for it). Elsewhere the socket's folder is
/// the user's own and nothing is asked.
fn connect(require_system: bool) -> Result<Box<dyn Stream>, ClientError> {
    let _ = require_system;
    #[cfg(windows)]
    {
        const ERROR_PIPE_BUSY: i32 = 231;
        let started = std::time::Instant::now();
        loop {
            match pitcrew_core::service_paths::open_pipe_options().open(r"\\.\pipe\pitcrew") {
                Ok(f) => {
                    if require_system {
                        crate::pipe_server::require_system_server(&f).map_err(ClientError::Unavailable)?;
                    }
                    return Ok(Box::new(f));
                }
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && started.elapsed() < Duration::from_secs(3) => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(ClientError::Unavailable(
                        "it is not installed or not started. Install it from Settings ▸ Service (Windows asks for permission)".into(),
                    ))
                }
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    return Err(ClientError::Unavailable(
                        "this account may not talk to it (the pipe allows interactive users only)".into(),
                    ))
                }
                Err(e) => return Err(ClientError::Unavailable(e.to_string())),
            }
        }
    }
    #[cfg(unix)]
    {
        let os = if cfg!(target_os = "macos") {
            pitcrew_core::platform::Os::Macos
        } else {
            pitcrew_core::platform::Os::Linux
        };
        let path = pitcrew_core::service_paths::socket_path(os, &|k| std::env::var_os(k).map(std::path::PathBuf::from))
            .ok_or_else(|| ClientError::Unavailable("HOME is not set".into()))?;
        let s = std::os::unix::net::UnixStream::connect(path).map_err(|e| {
            if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused) {
                ClientError::Unavailable(
                    if cfg!(target_os = "macos") {
                        "it is not running. Use Repair service (Settings ▸ Service) to start it again; macOS may ask you to allow it in Login Items"
                    } else {
                        "it is not running. Turn it on in Settings ▸ Service (on Linux it is a systemd user service)"
                    }
                    .into(),
                )
            } else {
                ClientError::Unavailable(e.to_string())
            }
        })?;
        Ok(Box::new(s))
    }
}

fn hello(s: &mut Box<dyn Stream>) -> Result<String, ClientError> {
    write_frame(
        s,
        &Request::Hello {
            protocol: PROTOCOL_VERSION,
        },
    )
    .map_err(|e| ClientError::Unavailable(e.to_string()))?;
    match read_frame::<Response>(s).map_err(|e| ClientError::Unavailable(e.to_string()))? {
        Response::Hello { service_version, .. } => Ok(service_version),
        Response::Error(e) if e.code == pitcrew_core::protocol::ErrorCode::ProtocolMismatch => Err(ClientError::Mismatch(e.message)),
        Response::Error(e) => Err(e.into()),
        other => Err(ClientError::Protocol(format!("{other:?}"))),
    }
}

/// Runs `f` on a thread and waits at most `timeout` for it.
fn with_timeout<T: Send + 'static>(
    timeout: Duration,
    f: impl FnOnce() -> Result<T, ClientError> + Send + 'static,
) -> Result<T, ClientError> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(timeout)
        .map_err(|_| ClientError::Unavailable("it did not answer in time".into()))?
}

/// How long the app waits for an answer.
pub fn timeout_for(r: &Request) -> Duration {
    match r {
        Request::Register { .. } => Duration::from_secs(16 * 60),
        Request::Disk { .. } | Request::CleanDisk { .. } | Request::EnginePrune { .. } | Request::CacheClean { .. } => {
            Duration::from_secs(11 * 60)
        }
        _ => Duration::from_secs(130),
    }
}

/// The client of the local service.
#[derive(Debug, Default)]
pub struct LocalServiceClient;

impl LocalServiceClient {
    /// One request as this process, with whatever rights it has.
    fn request_direct(&self, request: Request) -> Result<Response, ClientError> {
        self.request_checked(request, false)
    }

    /// [`Self::request_direct`], optionally refusing a server that is not the SYSTEM service.
    fn request_checked(&self, request: Request, require_system: bool) -> Result<Response, ClientError> {
        let timeout = timeout_for(&request);
        with_timeout(timeout, move || {
            let mut s = connect(require_system)?;
            hello(&mut s)?;
            write_frame(&mut s, &request).map_err(|e| ClientError::Unavailable(e.to_string()))?;
            read_frame::<Response>(&mut s).map_err(|e| ClientError::Unavailable(e.to_string()))
        })
    }
}

impl ServiceClient for LocalServiceClient {
    /// The service refuses a change from a caller that is not an elevated administrator (Windows, where it runs as LocalSystem).
    /// This asks first as itself and, only when refused, runs the one request elevated: Windows' own prompt, once per change
    /// ([`crate::elevated_request`]).
    fn request(&self, request: Request) -> Result<Response, ClientError> {
        let answer = self.request_direct(request.clone())?;
        if cfg!(windows) && request.requires_admin() && is_permission_denied(&answer) {
            return elevated::request(&request, timeout_for(&request));
        }
        Ok(answer)
    }
}

/// The client of the elevated forwarding mode: it never elevates again (a refusal there is the answer), and it sends nothing
/// to a pipe whose server is not the SYSTEM service.
#[derive(Debug, Default)]
pub struct DirectClient;

impl ServiceClient for DirectClient {
    fn request(&self, request: Request) -> Result<Response, ClientError> {
        LocalServiceClient.request_checked(request, true)
    }
}

/// Whether the service refused for lack of administrator rights.
fn is_permission_denied(r: &Response) -> bool {
    matches!(r, Response::Error(e) if e.code == pitcrew_core::protocol::ErrorCode::PermissionDenied)
}

#[cfg(windows)]
mod elevated {
    use super::*;

    /// Runs `request` through an elevated copy of this program; declining Windows' prompt gives the service's own refusal.
    pub fn request(request: &Request, wait: Duration) -> Result<Response, ClientError> {
        let refused = |why: String| {
            ClientError::Service(ServiceError::new(
                pitcrew_core::protocol::ErrorCode::PermissionDenied,
                format!("Changing the runners needs administrator rights: {why}"),
            ))
        };
        let parent = std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| ClientError::Protocol("LOCALAPPDATA is not set".into()))?
            .join("Pitcrew")
            .join("elevated");
        let handoff = crate::elevated_request::prepare(&parent, request).map_err(ClientError::Protocol)?;
        let exe = std::env::current_exe().map_err(|e| ClientError::Protocol(format!("this program cannot be found: {e}")))?;
        let run =
            crate::service_install::run_elevated(&exe, &crate::elevated_request::parameters(&handoff), wait + Duration::from_secs(60));
        match run {
            Ok(()) => crate::elevated_request::collect(handoff),
            Err(e) if e.code == crate::dto::CommandErrorCode::Cancelled => {
                // Nothing was sent; the folder holds only the request the user just made.
                let _ = crate::elevated_request::collect(handoff);
                Err(refused("Windows' permission prompt was declined".into()))
            }
            Err(e) => {
                let _ = crate::elevated_request::collect(handoff);
                Err(ClientError::Protocol(e.message))
            }
        }
    }
}

#[cfg(not(windows))]
mod elevated {
    use super::*;

    /// Only Windows asks (the macOS and Linux services run as the user and accept their own socket's callers).
    pub fn request(_: &Request, _: Duration) -> Result<Response, ClientError> {
        Err(ClientError::Protocol("elevation is a Windows feature".into()))
    }
}

/// Subscribes to pushed events and calls `on_event` for each until it returns `false` or the connection ends.
/// `on_connected` gets the service's version once the subscription stands.
pub fn subscribe(on_connected: impl FnOnce(String), mut on_event: impl FnMut(Response) -> bool) -> Result<(), ClientError> {
    let mut s = with_timeout(Duration::from_secs(10), || {
        let mut s = connect(false)?;
        let v = hello(&mut s)?;
        write_frame(&mut s, &Request::Subscribe).map_err(|e| ClientError::Unavailable(e.to_string()))?;
        match read_frame::<Response>(&mut s).map_err(|e| ClientError::Unavailable(e.to_string()))? {
            Response::Ok => Ok((s, v)),
            Response::Error(e) => Err(e.into()),
            other => Err(ClientError::Protocol(format!("{other:?}"))),
        }
    })?;
    on_connected(std::mem::take(&mut s.1));
    loop {
        match read_frame::<Response>(&mut s.0) {
            Ok(r) => {
                if !on_event(r) {
                    return Ok(());
                }
            }
            Err(e) => return Err(ClientError::Unavailable(format!("the connection ended ({e})"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_say_what_to_do() {
        assert!(ClientError::Unavailable("pipe not found".into())
            .to_string()
            .starts_with("The Pitcrew service is not running"));
        assert_eq!(timeout_for(&Request::Status), Duration::from_secs(130));
        let r = with_timeout(Duration::from_millis(50), || {
            std::thread::sleep(Duration::from_secs(2));
            Ok(())
        });
        assert!(matches!(r, Err(ClientError::Unavailable(_))));
    }
}
