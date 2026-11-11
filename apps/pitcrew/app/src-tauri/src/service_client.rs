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

/// Lets a stream through only after `verify` accepted it: the check comes first and the stream is handed on only if it passes, so
/// nothing is ever written to a server that was not checked.
#[cfg_attr(
    not(any(windows, test)),
    allow(dead_code, reason = "only the Windows pipe is checked; the check itself is tested everywhere")
)]
fn checked<S: Stream + 'static>(stream: S, verify: impl FnOnce(&S) -> Result<(), String>) -> Result<Box<dyn Stream>, ClientError> {
    verify(&stream).map_err(ClientError::Unavailable)?;
    Ok(Box::new(stream))
}

/// Which engine a connection is for. Windows has two (ADR 0004, second amendment): the SYSTEM **service** (runners in folders only
/// administrators can change) and the **per-user engine**, the same engine started in the user's own session (runners in folders
/// the user can change). macOS and Linux have one, the user's own service: [`Engine::System`] there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// The service (Windows: SYSTEM, `\\.\pipe\pitcrew`; elsewhere the user's own service).
    System,
    /// The per-user engine (Windows only).
    User,
}

/// What the client says when the per-user engine is not there (not an error to show: nobody may have runners there).
pub const USER_ENGINE_NOT_RUNNING: &str = "your own runners' engine is not running";

/// The pipe an engine listens on (Windows). The per-user engine's name is recorded by the engine in its own folder at each start
/// (a hint, checked for shape; the pipe's owner is checked on connect regardless).
#[cfg(windows)]
fn pipe_of(engine: Engine) -> Result<String, ClientError> {
    let env = |k: &str| std::env::var_os(k).map(std::path::PathBuf::from);
    match engine {
        Engine::System => Ok(r"\\.\pipe\pitcrew".to_string()),
        Engine::User => {
            let dir = pitcrew_core::service_paths::user_engine_dir(&env)
                .ok_or_else(|| ClientError::Unavailable("LOCALAPPDATA is not set".into()))?;
            let recorded = std::fs::read_to_string(dir.join(pitcrew_core::service_paths::USER_ENGINE_PIPE_FILE))
                .map_err(|_| ClientError::Unavailable(USER_ENGINE_NOT_RUNNING.into()))?;
            let recorded = recorded.trim();
            if pitcrew_core::service_paths::is_user_pipe_name(recorded) {
                Ok(recorded.to_string())
            } else {
                Err(ClientError::Unavailable(USER_ENGINE_NOT_RUNNING.into()))
            }
        }
    }
}

/// Connects to an engine. On Windows every connection, reads and the event subscription included, is checked first: the pipe must
/// be owned by SYSTEM or Administrators, or, for the per-user engine's own pipe, by this account ([`crate::pipe_server`]).
/// Elsewhere the socket's folder is the user's own.
fn connect(engine: Engine) -> Result<Box<dyn Stream>, ClientError> {
    #[cfg(windows)]
    {
        const ERROR_PIPE_BUSY: i32 = 231;
        let name = pipe_of(engine)?;
        let started = std::time::Instant::now();
        loop {
            match pitcrew_core::service_paths::open_pipe_options().open(&name) {
                Ok(f) => {
                    return checked(f, |f| {
                        let own = (engine == Engine::User).then(crate::pipe_server::current_user_sid).transpose()?;
                        crate::pipe_server::verify(&crate::pipe_server::PipeOwner(f), own.as_deref())
                    })
                }
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && started.elapsed() < Duration::from_secs(3) => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(ClientError::Unavailable(if engine == Engine::User {
                        USER_ENGINE_NOT_RUNNING.into()
                    } else {
                        "it is not installed or not started. Install it from Settings ▸ Service (Windows asks for permission)".into()
                    }))
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
        if engine == Engine::User {
            return Err(ClientError::Unavailable("only Windows has a per-user engine".into()));
        }
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

/// One connection, one request: connect (checked), hello, request, response.
fn exchange(connect: impl FnOnce() -> Result<Box<dyn Stream>, ClientError>, request: &Request) -> Result<Response, ClientError> {
    let mut s = connect()?;
    hello(&mut s)?;
    write_frame(&mut s, request).map_err(|e| ClientError::Unavailable(e.to_string()))?;
    read_frame::<Response>(&mut s).map_err(|e| ClientError::Unavailable(e.to_string()))
}

/// The client of the local service.
#[derive(Debug, Default)]
pub struct LocalServiceClient;

impl LocalServiceClient {
    /// One request as this process, with whatever rights it has.
    fn request_direct(&self, request: Request) -> Result<Response, ClientError> {
        request_to(Engine::System, request)
    }
}

/// One request to `engine` as this process.
fn request_to(engine: Engine, request: Request) -> Result<Response, ClientError> {
    let timeout = timeout_for(&request);
    with_timeout(timeout, move || exchange(|| connect(engine), &request))
}

/// The client of the per-user engine (Windows): the user's own engine takes every request from its own user, so nothing is
/// elevated and nothing is forwarded.
#[derive(Debug, Default)]
pub struct UserEngineClient;

impl ServiceClient for UserEngineClient {
    fn request(&self, request: Request) -> Result<Response, ClientError> {
        request_to(Engine::User, request)
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

/// The client of the elevated forwarding mode: it never elevates again (a refusal there is the answer).
#[derive(Debug, Default)]
pub struct DirectClient;

impl ServiceClient for DirectClient {
    fn request(&self, request: Request) -> Result<Response, ClientError> {
        LocalServiceClient.request_direct(request)
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
pub fn subscribe(on_connected: impl FnOnce(String), on_event: impl FnMut(Response) -> bool) -> Result<(), ClientError> {
    subscribe_engine(Engine::System, on_connected, on_event)
}

/// [`subscribe`] to one engine.
pub fn subscribe_engine(
    engine: Engine,
    on_connected: impl FnOnce(String),
    mut on_event: impl FnMut(Response) -> bool,
) -> Result<(), ClientError> {
    let mut s = with_timeout(Duration::from_secs(10), move || {
        let mut s = connect(engine)?;
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

    /// A stream that records what is done to it, to show the order: the check, then the first write.
    struct Recording(std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>);

    impl Read for Recording {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().push("read");
            Ok(0)
        }
    }

    impl Write for Recording {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().push("write");
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn nothing_is_written_to_a_pipe_that_the_service_does_not_own() {
        let log: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>> = std::sync::Arc::default();
        // A squatter's pipe: the check fails, so even the hello is never sent, for a read as for a change.
        for request in [
            Request::Status,
            Request::SaveSettings {
                config: pitcrew_core::config::Config::default(),
            },
            Request::Subscribe,
        ] {
            let l = log.clone();
            let squat = Recording(log.clone());
            let result = exchange(
                move || {
                    checked(squat, |_| {
                        l.lock().unwrap().push("check");
                        crate::pipe_server::judge_owner("S-1-5-21-1-2-3-1001", None)
                    })
                },
                &request,
            );
            assert!(
                matches!(&result, Err(ClientError::Unavailable(m)) if m.contains("nothing was sent")),
                "{result:?}"
            );
        }
        assert_eq!(
            *log.lock().unwrap(),
            ["check", "check", "check"],
            "a write or read happened on an unchecked pipe"
        );

        // The control: when the owner is the service's, the first thing that happens after the check is the hello.
        log.lock().unwrap().clear();
        let l = log.clone();
        let service = Recording(log.clone());
        let _ = exchange(
            move || {
                checked(service, |_| {
                    l.lock().unwrap().push("check");
                    crate::pipe_server::judge_owner("S-1-5-18", None)
                })
            },
            &Request::Status,
        );
        assert_eq!(log.lock().unwrap()[..2], ["check", "write"], "{:?}", log.lock().unwrap());
    }

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

    /// Round 6 P1-A on the VM: with the service installed (its folder is `%ProgramData%\Pitcrew`), `pitcrew-service.exe run` as the
    /// standard user is the per-user engine; it records its pipe name in `%LOCALAPPDATA%\Pitcrew\engine`, where `pipe_of` reads it,
    /// and answers there.
    #[cfg(windows)]
    #[test]
    #[ignore = "VM: PITCREW_TEST_EXPECT=standard, the service installed, PITCREW_TEST_SERVICE_EXE names pitcrew-service.exe"]
    #[allow(clippy::disallowed_methods, reason = "the test ends the engine it started, by its child handle")]
    fn the_user_engine_started_beside_the_installed_service_is_found_through_pipe_of() {
        use std::os::windows::process::CommandExt;
        let exe = std::env::var_os("PITCREW_TEST_SERVICE_EXE").expect("PITCREW_TEST_SERVICE_EXE names pitcrew-service.exe");
        let mut child = std::process::Command::new(exe)
            .arg("run")
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .spawn()
            .expect("the engine starts");
        let deadline = std::time::Instant::now() + Duration::from_secs(40);
        let found = loop {
            if let Ok(name) = pipe_of(Engine::User) {
                break Some(name);
            }
            if child.try_wait().ok().flatten().is_some() || std::time::Instant::now() > deadline {
                break None;
            }
            std::thread::sleep(Duration::from_millis(250)); // conformance: hang-guard — polls for the engine's pipe.name, bounded by the 40 s deadline above
        };
        let answer = found.as_ref().map(|_| request_to(Engine::User, Request::Status));
        println!("MEASURED pipe name: {found:?}; status: {answer:?}");
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            found.is_some(),
            "pipe_of never found the engine: it wrote pipe.name somewhere else, or did not start"
        );
        assert!(matches!(answer, Some(Ok(Response::Status { .. }))), "{answer:?}");
    }
}
