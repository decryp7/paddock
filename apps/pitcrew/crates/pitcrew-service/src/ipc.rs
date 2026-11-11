//! The app ↔ service transport (ADR 0004): a named pipe on Windows, a Unix socket elsewhere, carrying
//! `pitcrew_core::protocol` frames.
//!
//! - Windows: `\\.\pipe\pitcrew`, created as the first instance (nobody can have squatted the name), remote clients
//!   rejected, with the security descriptor [`PIPE_SDDL`]: full access for SYSTEM, Administrators and the pipe's
//!   owner, read/write for interactive users (the logged-on user's app), nothing for anyone else (services, network
//!   logons, other sessions' non-interactive tokens).
//! - Unix: `<socket folder>/pitcrew.sock` (the data folder, or `$XDG_RUNTIME_DIR/pitcrew` on Linux) in a 0700 folder, the socket itself 0600: only the user who runs the agent.
//! - A request that changes what runs is accepted only from an elevated administrator on Windows, any request that changes
//!   something only from an identified caller ([`crate::caller`]); the others get `PermissionDenied` and the supervisor never
//!   sees the request (it logs every such request with the caller's account).
//! - Every connection starts with a hello; frames are bounded by `MAX_FRAME`; at most [`MAX_CONNECTIONS`] at once.
//!   A subscribed connection receives pushed events until either side closes it.

use crate::caller::{authorize, Caller};
use crate::supervisor::Msg;
use pitcrew_core::protocol::{decode_body, encode_frame, frame_len, ErrorCode, FrameError, Request, Response, ServiceError};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Semaphore;

/// The pipe's name on Windows for the installed service (its data folder is `%ProgramData%\Pitcrew`).
pub const PIPE_NAME: &str = r"\\.\pipe\pitcrew";

/// The pipe of the service whose data folder is `data_dir`: the installed service's folder has [`PIPE_NAME`]; any
/// other folder (tests, development) has a name of its own, so it never collides with a service installed on the
/// same machine (a CI runner supervised by Pitcrew runs Pitcrew's tests).
pub fn pipe_name(data_dir: &std::path::Path) -> String {
    let env = |k: &str| std::env::var_os(k).map(std::path::PathBuf::from);
    let installed = pitcrew_core::service_paths::data_dir(pitcrew_core::platform::Os::Windows, &env);
    pipe_name_for(data_dir, installed.as_deref())
}

/// [`pipe_name`] with the installed service's folder given (pure). Folders compare without case and trailing
/// separators, as Windows paths do.
pub fn pipe_name_for(data_dir: &std::path::Path, installed: Option<&std::path::Path>) -> String {
    pitcrew_core::service_paths::pipe_name_for(data_dir, installed)
}
/// The socket's file name in the data folder elsewhere.
pub const SOCKET_FILE: &str = pitcrew_core::service_paths::SOCKET_FILE;
/// Most connections at once.
pub const MAX_CONNECTIONS: usize = 32;
/// The pipe's security descriptor (SDDL): SYSTEM, Administrators and the owner full access; interactive users read and write
/// data with exactly [`pitcrew_core::service_paths::PIPE_CLIENT_ACCESS`] (`0x12019b`): **not** `GRGW`, which includes
/// `FILE_CREATE_PIPE_INSTANCE` and would let any interactive user host `\\.\pipe\pitcrew` and receive an elevated caller's
/// connection. A test keeps the literal and the constant the app opens the pipe with in step.
pub const PIPE_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)(A;;0x12019b;;;IU)";

/// How long a client has to say hello.
const HELLO_TIMEOUT: Duration = Duration::from_secs(30);

async fn read_frame<S: AsyncRead + Unpin>(s: &mut S) -> Result<Request, FrameError> {
    let mut header = [0u8; 4];
    s.read_exact(&mut header).await?;
    let len = frame_len(header)?;
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).await?;
    decode_body(&body)
}

async fn write_frame<S: AsyncWrite + Unpin>(s: &mut S, r: &Response) -> Result<(), FrameError> {
    let bytes = encode_frame(r)?;
    s.write_all(&bytes).await?;
    s.flush().await?;
    Ok(())
}

fn timeout_for(r: &Request) -> Duration {
    match r {
        // Registration may download the runner first.
        Request::Register { .. } => Duration::from_secs(15 * 60),
        Request::Disk { .. } | Request::CleanDisk { .. } | Request::EnginePrune { .. } | Request::CacheClean { .. } => {
            Duration::from_secs(10 * 60)
        }
        _ => Duration::from_secs(120),
    }
}

/// Refusals are logged once per request kind per connection; the rest are counted and summed up when the connection ends (a
/// program that retries a refused request in a loop must not fill the service log).
#[derive(Default)]
struct RefusalLog {
    seen: std::collections::HashMap<&'static str, (u32, String)>,
}

impl RefusalLog {
    fn note(&mut self, tx: &Sender<Msg>, kind: &'static str, caller: &str) {
        let entry = self.seen.entry(kind).or_insert_with(|| (0, caller.to_string()));
        entry.0 += 1;
        if entry.0 == 1 {
            let _ = tx.send(Msg::Audit {
                request: kind.to_string(),
                caller: caller.to_string(),
                allowed: false,
            });
        }
    }

    fn flush(self, tx: &Sender<Msg>) {
        for (kind, (n, caller)) in self.seen {
            if n > 1 {
                let _ = tx.send(Msg::Audit {
                    request: format!("{kind} (and {} more like it)", n - 1),
                    caller,
                    allowed: false,
                });
            }
        }
    }
}

/// Serves one connection. `identify` says who the caller is; it is asked once, when the first request that changes
/// something arrives (the OS can tell only after the client's first bytes).
pub async fn connection<S: AsyncRead + AsyncWrite + Unpin + Send>(stream: S, tx: Sender<Msg>, identify: impl FnOnce() -> Caller + Send) {
    let mut refusals = RefusalLog::default();
    serve_connection(stream, tx.clone(), identify, &mut refusals).await;
    refusals.flush(&tx);
}

async fn serve_connection<S: AsyncRead + AsyncWrite + Unpin + Send>(
    mut stream: S,
    tx: Sender<Msg>,
    identify: impl FnOnce() -> Caller + Send,
    refusals: &mut RefusalLog,
) {
    let mut identify = Some(identify);
    let mut caller: Option<Caller> = None;
    let hello = match tokio::time::timeout(HELLO_TIMEOUT, read_frame(&mut stream)).await {
        Ok(Ok(r)) => r,
        _ => return,
    };
    let Request::Hello { protocol } = hello else {
        let _ = write_frame(
            &mut stream,
            &Response::Error(ServiceError::new(ErrorCode::BadRequest, "say hello first")),
        )
        .await;
        return;
    };
    let answer = pitcrew_core::protocol::answer_hello(protocol, crate::supervisor::SERVICE_VERSION);
    let ok = matches!(answer, Response::Hello { .. });
    if write_frame(&mut stream, &answer).await.is_err() || !ok {
        return;
    }
    loop {
        let req = match read_frame(&mut stream).await {
            Ok(r) => r,
            Err(FrameError::Malformed(m)) => {
                // A request this service does not understand: answer and close (the stream may be out of step).
                let _ = write_frame(
                    &mut stream,
                    &Response::Error(ServiceError::new(ErrorCode::BadRequest, format!("the request is not valid: {m}"))),
                )
                .await;
                return;
            }
            Err(_) => return,
        };
        if req == Request::Subscribe {
            subscribed(stream, tx).await;
            return;
        }
        if req.mutates() {
            // Every request that changes anything is attributed (and logged with the account), so who is asking is read here.
            let who = caller.get_or_insert_with(|| {
                identify
                    .take()
                    .map_or_else(|| Caller::Unreadable("the caller was not asked".into()), |f| f())
            });
            let verdict = authorize(&req, who);
            match &verdict {
                Ok(()) => {
                    let _ = tx.send(Msg::Audit {
                        request: req.kind().to_string(),
                        caller: who.describe(),
                        allowed: true,
                    });
                }
                Err(_) => refusals.note(&tx, req.kind(), &who.describe()),
            }
            if let Err(refusal) = verdict {
                if write_frame(&mut stream, &Response::Error(refusal)).await.is_err() {
                    return;
                }
                continue;
            }
        }
        let wait = timeout_for(&req);
        let (otx, orx) = tokio::sync::oneshot::channel();
        if tx.send(Msg::Request(Box::new(req), otx)).is_err() {
            let _ = write_frame(
                &mut stream,
                &Response::Error(ServiceError::new(ErrorCode::Internal, "the service is stopping")),
            )
            .await;
            return;
        }
        let resp = match tokio::time::timeout(wait, orx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Response::Error(ServiceError::new(
                ErrorCode::Internal,
                "the request was dropped; the service log has details",
            )),
            Err(_) => Response::Error(ServiceError::new(
                ErrorCode::Busy,
                "the service did not answer in time; it goes on in the background",
            )),
        };
        if write_frame(&mut stream, &resp).await.is_err() {
            return;
        }
    }
}

async fn subscribed<S: AsyncRead + AsyncWrite + Unpin + Send>(stream: S, tx: Sender<Msg>) {
    let (mut rd, mut wr) = tokio::io::split(stream);
    let (etx, mut erx) = tokio::sync::mpsc::channel::<Response>(64);
    if write_frame(&mut wr, &Response::Ok).await.is_err() || tx.send(Msg::Subscribe(etx)).is_err() {
        return;
    }
    let mut buf = [0u8; 64];
    loop {
        tokio::select! {
            event = erx.recv() => match event {
                Some(e) => if write_frame(&mut wr, &e).await.is_err() { return },
                None => return,
            },
            // The client sends nothing on a subscription: anything (or the end) closes it.
            _ = rd.read(&mut buf) => return,
        }
    }
}

/// Whose engine this is (Windows; elsewhere the engine is always the user's own).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeMode {
    /// The SYSTEM service: the pipe admits interactive users to read and operate, and a request that changes what runs needs an
    /// elevated administrator ([`PIPE_SDDL`], [`crate::caller`]).
    System,
    /// The **per-user engine**: the same engine as the service, started in a user's own session. Its pipe admits only that user
    /// (and SYSTEM): [`PIPE_SDDL_USER`]; whoever can connect is its owner, who may do anything with their own runners, so no
    /// caller check is asked ([`crate::caller::Caller::Local`]).
    User,
}

/// The per-user engine's pipe: its owner and SYSTEM, nobody else (no interactive-user entry, no Administrators).
pub const PIPE_SDDL_USER: &str = "D:P(A;;GA;;;OW)(A;;GA;;;SY)";

/// Makes this start's pipe name for the per-user engine (128 random bits) and records it in the engine's folder, where only that
/// user's programs (and the app) read it. A hint on top of the pipe's ACL and the client's owner check, not a secret to rely on.
///
/// # Errors
/// The folder or the file could not be written.
pub fn make_user_pipe_name(data_dir: &std::path::Path) -> std::io::Result<String> {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    // `RandomState` is seeded by the OS's random source once per process: a fresh state per half keeps the halves apart.
    let half = |salt: u64| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(nanos);
        h.write_u64(salt);
        h.finish()
    };
    let name = pitcrew_core::service_paths::user_pipe_name((half(1), half(2)));
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(data_dir.join(pitcrew_core::service_paths::USER_ENGINE_PIPE_FILE), &name)?;
    Ok(name)
}

/// The per-user engine's pipe name recorded in `dir`, if the file holds a name this module could have made.
pub fn read_user_pipe_name(dir: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(pitcrew_core::service_paths::USER_ENGINE_PIPE_FILE)).ok()?;
    let text = text.trim();
    pitcrew_core::service_paths::is_user_pipe_name(text).then(|| text.to_string())
}

/// Accepts connections until the runtime shuts down.
pub async fn serve(tx: Sender<Msg>, socket_dir: std::path::PathBuf, mode: ServeMode) -> Result<(), String> {
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    platform::serve(tx, socket_dir, permits, mode).await
}

#[cfg(windows)]
mod platform {
    //! The named pipe with its ACL. The only unsafe code is building the security descriptor and handing it to
    //! the pipe's creation.
    #![allow(unsafe_code)]

    use super::{connection, Caller, Msg, ServeMode, PIPE_SDDL, PIPE_SDDL_USER};
    use std::os::windows::io::AsRawHandle;
    use std::sync::mpsc::Sender;
    use std::sync::Arc;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use tokio::sync::Semaphore;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

    fn create(name: &str, first: bool, mode: ServeMode) -> std::io::Result<NamedPipeServer> {
        let sddl = if mode == ServeMode::System { PIPE_SDDL } else { PIPE_SDDL_USER };
        let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `wide` is NUL-terminated and outlives the call; the descriptor is freed below after the pipe
        // instance (which copies it) has been created.
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(wide.as_ptr()), SDDL_REVISION_1, &mut descriptor, None) }
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        let mut options = ServerOptions::new();
        options
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(super::MAX_CONNECTIONS + 1);
        // SAFETY: `attributes` points at a valid descriptor for the duration of the call.
        let result = unsafe { options.create_with_security_attributes_raw(name, (&mut attributes as *mut SECURITY_ATTRIBUTES).cast()) };
        // SAFETY: allocated by the conversion above and no longer used.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        }
        result
    }

    pub async fn serve(tx: Sender<Msg>, data_dir: std::path::PathBuf, permits: Arc<Semaphore>, mode: ServeMode) -> Result<(), String> {
        let name = match mode {
            ServeMode::System => super::pipe_name(&data_dir),
            ServeMode::User => {
                super::make_user_pipe_name(&data_dir).map_err(|e| format!("the engine's pipe name could not be recorded ({e})"))?
            }
        };
        let mut server = create(&name, true, mode)
            .map_err(|e| format!("the pipe {name} could not be created ({e}); is another Pitcrew service running?"))?;
        loop {
            server.connect().await.map_err(|e| e.to_string())?;
            let connected = server;
            server = create(&name, false, mode).map_err(|e| e.to_string())?;
            let Ok(permit) = permits.clone().acquire_owned().await else {
                return Ok(());
            };
            let tx = tx.clone();
            let raw = connected.as_raw_handle() as usize;
            tokio::spawn(async move {
                // The handle lives as long as `connected`, which `connection` owns until it returns.
                connection(connected, tx, move || match mode {
                    ServeMode::System => crate::caller::level_of_pipe(raw as std::os::windows::io::RawHandle),
                    // The pipe's ACL admits only its owner: nothing more to ask.
                    ServeMode::User => Caller::Local,
                })
                .await;
                drop(permit);
            });
        }
    }
}

#[cfg(unix)]
mod platform {
    use super::{connection, Caller, Msg, SOCKET_FILE};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::mpsc::Sender;
    use std::sync::Arc;
    use tokio::net::UnixListener;
    use tokio::sync::Semaphore;

    pub async fn serve(
        tx: Sender<Msg>,
        socket_dir: std::path::PathBuf,
        permits: Arc<Semaphore>,
        _mode: super::ServeMode,
    ) -> Result<(), String> {
        std::fs::create_dir_all(&socket_dir).map_err(|e| format!("{}: {e}", socket_dir.display()))?;
        let _ = std::fs::set_permissions(&socket_dir, std::fs::Permissions::from_mode(0o700));
        let path = socket_dir.join(SOCKET_FILE);
        // A socket left by a previous run.
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(|e| format!("{}: {e}", path.display()))?;
        loop {
            let (stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
            let Ok(permit) = permits.clone().acquire_owned().await else {
                return Ok(());
            };
            let tx = tx.clone();
            tokio::spawn(async move {
                connection(stream, tx, || Caller::Local).await;
                drop(permit);
            });
        }
    }
}

#[cfg(not(any(windows, unix)))]
mod platform {
    pub async fn serve(
        _: std::sync::mpsc::Sender<super::Msg>,
        _: std::path::PathBuf,
        _: std::sync::Arc<tokio::sync::Semaphore>,
        _: super::ServeMode,
    ) -> Result<(), String> {
        Err("no local transport on this platform".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitcrew_core::protocol::PROTOCOL_VERSION;

    async fn send(c: &mut tokio::io::DuplexStream, r: &Request) {
        c.write_all(&encode_frame(r).unwrap()).await.unwrap();
    }

    async fn recv(c: &mut tokio::io::DuplexStream) -> Response {
        let mut h = [0u8; 4];
        c.read_exact(&mut h).await.unwrap();
        let mut b = vec![0u8; frame_len(h).unwrap()];
        c.read_exact(&mut b).await.unwrap();
        decode_body(&b).unwrap()
    }

    #[tokio::test]
    async fn hello_then_requests_then_a_subscription() {
        let (tx, rx) = std::sync::mpsc::channel::<Msg>();
        // A stand-in supervisor answering everything with Ok and accepting subscribers.
        std::thread::spawn(move || {
            for m in rx {
                match m {
                    Msg::Request(_, reply) => {
                        let _ = reply.send(Response::Ok);
                    }
                    Msg::Subscribe(s) => {
                        let _ = s.try_send(Response::Ok);
                    }
                    _ => {}
                }
            }
        });
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let task = tokio::spawn(connection(server, tx.clone(), || Caller::Local));
        send(
            &mut client,
            &Request::Hello {
                protocol: PROTOCOL_VERSION,
            },
        )
        .await;
        assert!(matches!(recv(&mut client).await, Response::Hello { .. }));
        send(&mut client, &Request::Status).await;
        assert_eq!(recv(&mut client).await, Response::Ok);
        send(&mut client, &Request::Subscribe).await;
        assert_eq!(recv(&mut client).await, Response::Ok, "subscription confirmed");
        assert_eq!(recv(&mut client).await, Response::Ok, "the pushed event");
        drop(client);
        task.await.unwrap();

        // A wrong protocol or no hello closes the connection with a reason.
        let (mut client, server) = tokio::io::duplex(4096);
        let task = tokio::spawn(connection(server, tx.clone(), || Caller::Local));
        send(&mut client, &Request::Hello { protocol: 1 }).await;
        assert!(matches!(
            recv(&mut client).await,
            Response::Error(ServiceError {
                code: ErrorCode::ProtocolMismatch,
                ..
            })
        ));
        task.await.unwrap();
        let (mut client, server) = tokio::io::duplex(4096);
        let task = tokio::spawn(connection(server, tx.clone(), || Caller::Local));
        send(&mut client, &Request::Status).await;
        assert!(matches!(
            recv(&mut client).await,
            Response::Error(ServiceError {
                code: ErrorCode::BadRequest,
                ..
            })
        ));
        task.await.unwrap();

        // Junk after the hello is answered and the connection closed.
        let (mut client, server) = tokio::io::duplex(4096);
        let task = tokio::spawn(connection(server, tx, || Caller::Local));
        send(
            &mut client,
            &Request::Hello {
                protocol: PROTOCOL_VERSION,
            },
        )
        .await;
        let _ = recv(&mut client).await;
        client.write_all(&3u32.to_le_bytes()).await.unwrap();
        client.write_all(b"{x}").await.unwrap();
        assert!(matches!(
            recv(&mut client).await,
            Response::Error(ServiceError {
                code: ErrorCode::BadRequest,
                ..
            })
        ));
        task.await.unwrap();
        assert_eq!(timeout_for(&Request::Status), Duration::from_secs(120));
    }
}

#[cfg(test)]
mod user_pipe_name_tests {
    use super::*;

    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn scratch(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("pitcrew-it-pipename-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn the_engine_records_a_random_pipe_name_and_reads_back_only_a_valid_one() {
        let dir = scratch("roundtrip");
        let first = make_user_pipe_name(&dir).unwrap();
        assert!(pitcrew_core::service_paths::is_user_pipe_name(&first), "{first}");
        assert_eq!(read_user_pipe_name(&dir).as_deref(), Some(first.as_str()));
        let second = make_user_pipe_name(&dir).unwrap();
        assert_ne!(first, second, "a new name at each start");
        assert_eq!(read_user_pipe_name(&dir).as_deref(), Some(second.as_str()));
        // Whatever else is in the file is not a name that is ever opened.
        std::fs::write(dir.join(pitcrew_core::service_paths::USER_ENGINE_PIPE_FILE), r"\\.\pipe\pitcrew").unwrap();
        assert_eq!(read_user_pipe_name(&dir), None);
        assert_eq!(read_user_pipe_name(&scratch("missing")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod pipe_acl_tests {
    use super::PIPE_SDDL;
    use pitcrew_core::service_paths::PIPE_CLIENT_ACCESS;

    /// `FILE_CREATE_PIPE_INSTANCE` (the same bit as `FILE_APPEND_DATA`).
    const CREATE_PIPE_INSTANCE: u32 = 0x4;
    const FILE_READ_DATA: u32 = 0x1;
    const FILE_WRITE_DATA: u32 = 0x2;

    /// The access mask an ACE of `PIPE_SDDL` gives to the account `who` (`IU`), as written.
    fn mask_for(who: &str) -> u32 {
        let ace = PIPE_SDDL
            .split('(')
            .skip(1)
            .map(|a| a.trim_end_matches(')'))
            .find(|a| a.ends_with(&format!(";;;{who}")))
            .unwrap_or_else(|| panic!("no ACE for {who} in {PIPE_SDDL}"));
        let rights = ace.split(';').nth(2).unwrap();
        u32::from_str_radix(rights.trim_start_matches("0x"), 16)
            .unwrap_or_else(|_| panic!("a symbolic right ({rights}) hides what it includes"))
    }

    #[test]
    fn interactive_users_may_read_and_write_but_not_create_a_pipe_instance() {
        let iu = mask_for("IU");
        assert_eq!(
            iu & CREATE_PIPE_INSTANCE,
            0,
            "{iu:#x} lets an interactive user host the pipe: it would receive an elevated caller"
        );
        assert!(iu & FILE_READ_DATA != 0 && iu & FILE_WRITE_DATA != 0, "{iu:#x}");
        assert_eq!(iu, PIPE_CLIENT_ACCESS, "the app opens the pipe with the mask the service grants");
    }

    #[test]
    fn nobody_else_is_in_the_list_and_the_list_is_protected() {
        assert!(PIPE_SDDL.starts_with("D:P("), "protected: nothing inherited");
        assert_eq!(PIPE_SDDL.matches("(A;").count(), 4, "{PIPE_SDDL}");
        for who in ["SY", "BA", "OW"] {
            assert!(PIPE_SDDL.contains(&format!("(A;;GA;;;{who})")), "{who}");
        }
    }
}

#[cfg(test)]
mod pipe_name_tests {
    use super::{pipe_name_for, PIPE_NAME};
    use std::path::Path;

    #[test]
    fn the_installed_folder_keeps_the_pipe_and_any_other_folder_gets_its_own() {
        let installed = Path::new(r"C:\ProgramData\Pitcrew");
        assert_eq!(pipe_name_for(installed, Some(installed)), PIPE_NAME);
        assert_eq!(pipe_name_for(Path::new(r"c:\programdata\pitcrew\"), Some(installed)), PIPE_NAME);
        let test = pipe_name_for(Path::new(r"C:\Temp\pitcrew-transport-42"), Some(installed));
        assert!(
            test.starts_with(&format!("{PIPE_NAME}-")) && test.len() == PIPE_NAME.len() + 17,
            "{test}"
        );
        assert_eq!(
            test,
            pipe_name_for(Path::new(r"C:\Temp\pitcrew-transport-42"), Some(installed)),
            "stable"
        );
        assert_ne!(test, pipe_name_for(Path::new(r"C:\Temp\pitcrew-transport-43"), Some(installed)));
        assert_ne!(
            pipe_name_for(installed, None),
            PIPE_NAME,
            "no ProgramData: never the installed pipe"
        );
    }
}

#[cfg(test)]
mod caller_tests {
    //! The hole (release blocker 2026-11-11): any program of an interactive user could send `AddRunner` or `SaveSettings`
    //! and make the LocalSystem service start a program it chose. A caller that is not an elevated administrator never
    //! gets a request that changes something to the supervisor.
    use super::*;
    use pitcrew_core::config::{RunnerConfig, SessionMode};
    use pitcrew_core::names::RunnerName;
    use pitcrew_core::protocol::PROTOCOL_VERSION;
    use std::sync::Mutex;

    type Seen = Arc<Mutex<Vec<String>>>;
    /// The audit lines the supervisor was given: `allowed|refused <request> <caller>`.
    type Audits = Arc<Mutex<Vec<String>>>;

    async fn send(c: &mut tokio::io::DuplexStream, r: &Request) {
        c.write_all(&encode_frame(r).unwrap()).await.unwrap();
    }

    async fn recv(c: &mut tokio::io::DuplexStream) -> Response {
        let mut h = [0u8; 4];
        c.read_exact(&mut h).await.unwrap();
        let mut b = vec![0u8; frame_len(h).unwrap()];
        c.read_exact(&mut b).await.unwrap();
        decode_body(&b).unwrap()
    }

    /// A stand-in supervisor that records what reached it and answers Ok.
    fn supervisor() -> (Sender<Msg>, Seen, Audits) {
        let (tx, rx) = std::sync::mpsc::channel::<Msg>();
        let seen: Seen = Arc::default();
        let log = seen.clone();
        let audits: Audits = Arc::default();
        let audit_log = audits.clone();
        std::thread::spawn(move || {
            for m in rx {
                match m {
                    Msg::Request(req, reply) => {
                        log.lock().unwrap().push(req.kind().to_string());
                        let _ = reply.send(Response::Ok);
                    }
                    Msg::Audit { request, caller, allowed } => {
                        audit_log
                            .lock()
                            .unwrap()
                            .push(format!("{} {request} {caller}", if allowed { "allowed" } else { "refused" }));
                        if !allowed {
                            log.lock().unwrap().push(format!("denied:{request}"));
                        }
                    }
                    _ => {}
                }
            }
        });
        (tx, seen, audits)
    }

    async fn connect(caller: Caller) -> (tokio::io::DuplexStream, Seen) {
        let (c, seen, _) = connect_audited(caller).await;
        (c, seen)
    }

    async fn connect_audited(caller: Caller) -> (tokio::io::DuplexStream, Seen, Audits) {
        let (tx, seen, audits) = supervisor();
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        tokio::spawn(connection(server, tx, move || caller));
        send(
            &mut client,
            &Request::Hello {
                protocol: PROTOCOL_VERSION,
            },
        )
        .await;
        assert!(matches!(recv(&mut client).await, Response::Hello { .. }));
        (client, seen, audits)
    }

    const SID: &str = "S-1-5-21-1-2-3-1001";

    fn service_runner() -> Request {
        let mut runner: RunnerConfig = toml::from_str(
            "name = \"evil\"\nkind = \"gitea-runner\"\ndir = 'C:\\Users\\someone\\runner'\nbinary_path = 'C:\\Users\\someone\\runner\\run.exe'\nsession = \"service\"\n",
        )
        .unwrap();
        assert_eq!(runner.session, SessionMode::Service);
        runner.name = RunnerName::parse("evil").unwrap();
        Request::AddRunner { runner }
    }

    fn denied(r: &Response) -> bool {
        matches!(
            r,
            Response::Error(ServiceError {
                code: ErrorCode::PermissionDenied,
                ..
            })
        )
    }

    #[tokio::test]
    async fn a_service_runner_from_a_caller_without_administrator_rights_is_refused_and_never_reaches_the_supervisor() {
        for caller in [Caller::Standard(SID.into()), Caller::Unreadable("OpenThreadToken: denied".into())] {
            let (mut c, seen) = connect(caller).await;
            send(&mut c, &service_runner()).await;
            let answer = recv(&mut c).await;
            assert!(denied(&answer), "{answer:?}");
            // The same connection goes on: a read-only request is answered, a second change is refused again.
            send(&mut c, &Request::Status).await;
            assert_eq!(recv(&mut c).await, Response::Ok);
            send(&mut c, &service_runner()).await;
            assert!(denied(&recv(&mut c).await));
            // Messages reach the supervisor in order: once this answer is back, the refusal before it has been logged.
            send(&mut c, &Request::Status).await;
            assert_eq!(recv(&mut c).await, Response::Ok);
            // The second identical refusal is not logged again (coalesced; summed up when the connection ends).
            assert_eq!(*seen.lock().unwrap(), ["denied:AddRunner", "Status", "Status"]);
        }
    }

    #[tokio::test]
    async fn every_request_that_changes_what_runs_is_refused_for_a_standard_caller() {
        let name = RunnerName::parse("a").unwrap();
        let (mut c, seen) = connect(Caller::Standard(SID.into())).await;
        for r in [
            Request::SaveSettings {
                config: pitcrew_core::config::Config::default(),
            },
            Request::RemoveRunner { runner: name.clone() },
            Request::Register {
                runner: name.clone(),
                server_url: "https://git.example.com".into(),
                token: pitcrew_core::protocol::Secret::new("t").unwrap(),
                display_name: None,
            },
        ] {
            assert!(r.requires_admin());
            send(&mut c, &r).await;
            assert!(denied(&recv(&mut c).await), "{r:?}");
        }
        // Ordered delivery: the answer to this read proves every refusal before it has been recorded.
        send(&mut c, &Request::Status).await;
        assert_eq!(recv(&mut c).await, Response::Ok);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 4, "{seen:?}");
        assert!(
            seen[..3].iter().all(|k| k.starts_with("denied:")) && seen[3] == "Status",
            "{seen:?}"
        );
    }

    /// Pause, stop, drain, start and restart of an existing runner are like `sc stop`: open to the pipe's users.
    #[tokio::test]
    async fn operating_an_existing_runner_is_open_to_a_standard_caller() {
        let name = RunnerName::parse("a").unwrap();
        let (mut c, seen) = connect(Caller::Standard(SID.into())).await;
        for r in [
            Request::Start { runner: name.clone() },
            Request::Stop { runner: name.clone() },
            Request::Restart { runner: name.clone() },
            Request::Drain { runner: name.clone() },
            Request::Resume { runner: name },
        ] {
            assert!(r.mutates() && !r.requires_admin(), "{r:?}");
            send(&mut c, &r).await;
            assert_eq!(recv(&mut c).await, Response::Ok, "{r:?}");
        }
        assert_eq!(*seen.lock().unwrap(), ["Start", "Stop", "Restart", "Drain", "Resume"]);
    }

    #[tokio::test]
    async fn an_elevated_administrator_and_a_local_caller_are_served() {
        for caller in [Caller::Elevated(SID.into()), Caller::Local] {
            let (mut c, seen) = connect(caller).await;
            send(&mut c, &service_runner()).await;
            assert_eq!(recv(&mut c).await, Response::Ok);
            assert_eq!(*seen.lock().unwrap(), ["AddRunner"]);
        }
    }

    #[tokio::test]
    async fn a_caller_that_cannot_be_identified_cannot_even_stop_a_runner() {
        let (mut c, seen, audits) = connect_audited(Caller::Unreadable("OpenThreadToken: denied".into())).await;
        send(
            &mut c,
            &Request::Stop {
                runner: RunnerName::parse("a").unwrap(),
            },
        )
        .await;
        assert!(denied(&recv(&mut c).await));
        send(&mut c, &Request::Status).await;
        assert_eq!(recv(&mut c).await, Response::Ok, "reading needs no identity");
        assert_eq!(*seen.lock().unwrap(), ["denied:Stop", "Status"]);
        assert!(
            audits.lock().unwrap()[0].starts_with("refused Stop"),
            "{:?}",
            audits.lock().unwrap()
        );
    }

    #[tokio::test]
    async fn a_refused_request_repeated_in_a_loop_is_logged_once_and_summed_up_at_the_end() {
        let (tx, _seen, audits) = supervisor();
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let tx_again = tx.clone();
        let served = tokio::spawn(connection(server, tx, || Caller::Standard(SID.into())));
        send(
            &mut client,
            &Request::Hello {
                protocol: PROTOCOL_VERSION,
            },
        )
        .await;
        let _ = recv(&mut client).await;
        for _ in 0..5 {
            send(&mut client, &service_runner()).await;
            assert!(denied(&recv(&mut client).await));
        }
        drop(client);
        served.await.unwrap();
        // The stand-in supervisor reads its channel on its own thread: a read-only round trip on a new connection orders after it.
        let (mut again, server_again) = tokio::io::duplex(64 * 1024);
        tokio::spawn(connection(server_again, tx_again, || Caller::Local));
        send(
            &mut again,
            &Request::Hello {
                protocol: PROTOCOL_VERSION,
            },
        )
        .await;
        let _ = recv(&mut again).await;
        send(&mut again, &Request::Status).await;
        assert_eq!(recv(&mut again).await, Response::Ok);
        let audits = audits.lock().unwrap();
        assert_eq!(audits.len(), 2, "{audits:?}");
        assert!(audits[0].starts_with("refused AddRunner ") && audits[0].contains(SID), "{audits:?}");
        assert!(audits[1].contains("AddRunner (and 4 more like it)"), "{audits:?}");
    }

    #[tokio::test]
    async fn every_change_is_logged_with_the_callers_account() {
        let (mut c, _seen, audits) = connect_audited(Caller::Elevated(SID.into())).await;
        send(&mut c, &service_runner()).await;
        assert_eq!(recv(&mut c).await, Response::Ok);
        send(
            &mut c,
            &Request::Stop {
                runner: RunnerName::parse("a").unwrap(),
            },
        )
        .await;
        assert_eq!(recv(&mut c).await, Response::Ok);
        let audits = audits.lock().unwrap();
        assert_eq!(audits.len(), 2, "{audits:?}");
        assert!(audits[0].starts_with("allowed AddRunner") && audits[0].contains(SID), "{audits:?}");
        assert!(audits[1].starts_with("allowed Stop") && audits[1].contains(SID), "{audits:?}");
    }

    #[tokio::test]
    async fn read_only_requests_never_ask_who_the_caller_is() {
        let (tx, _seen, _audits) = supervisor();
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        tokio::spawn(connection(server, tx, || -> Caller {
            panic!("the caller was identified for a read-only request")
        }));
        send(
            &mut client,
            &Request::Hello {
                protocol: PROTOCOL_VERSION,
            },
        )
        .await;
        let _ = recv(&mut client).await;
        for r in [Request::Status, Request::GetSettings, Request::Diagnostics] {
            send(&mut client, &r).await;
            assert_eq!(recv(&mut client).await, Response::Ok);
        }
    }
}
