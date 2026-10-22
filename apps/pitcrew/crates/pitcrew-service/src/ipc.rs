//! The app ↔ service transport (ADR 0004): a named pipe on Windows, a Unix socket elsewhere, carrying
//! `pitcrew_core::protocol` frames.
//!
//! - Windows: `\\.\pipe\pitcrew`, created as the first instance (nobody can have squatted the name), remote clients
//!   rejected, with the security descriptor [`PIPE_SDDL`]: full access for SYSTEM, Administrators and the pipe's
//!   owner, read/write for interactive users (the logged-on user's app), nothing for anyone else (services, network
//!   logons, other sessions' non-interactive tokens).
//! - Unix: `<socket folder>/pitcrew.sock` (the data folder, or `$XDG_RUNTIME_DIR/pitcrew` on Linux) in a 0700 folder, the socket itself 0600: only the user who runs the agent.
//! - Every connection starts with a hello; frames are bounded by `MAX_FRAME`; at most [`MAX_CONNECTIONS`] at once.
//!   A subscribed connection receives pushed events until either side closes it.

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
    let norm = |p: &std::path::Path| p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase();
    let dir = norm(data_dir);
    if installed.is_some_and(|i| norm(i) == dir) {
        return PIPE_NAME.to_string();
    }
    // FNV-1a: stable across builds and platforms, no dependency.
    let hash = dir.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{PIPE_NAME}-{hash:016x}")
}
/// The socket's file name in the data folder elsewhere.
pub const SOCKET_FILE: &str = pitcrew_core::service_paths::SOCKET_FILE;
/// Most connections at once.
pub const MAX_CONNECTIONS: usize = 32;
/// The pipe's security descriptor (SDDL): SYSTEM, Administrators and the owner full access; interactive users
/// read and write.
pub const PIPE_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)(A;;GRGW;;;IU)";

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
        Request::Disk { .. } | Request::CleanDisk { .. } => Duration::from_secs(10 * 60),
        _ => Duration::from_secs(120),
    }
}

/// Serves one connection.
pub async fn connection<S: AsyncRead + AsyncWrite + Unpin + Send>(mut stream: S, tx: Sender<Msg>) {
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

/// Accepts connections until the runtime shuts down.
pub async fn serve(tx: Sender<Msg>, socket_dir: std::path::PathBuf) -> Result<(), String> {
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    platform::serve(tx, socket_dir, permits).await
}

#[cfg(windows)]
mod platform {
    //! The named pipe with its ACL. The only unsafe code is building the security descriptor and handing it to
    //! the pipe's creation.
    #![allow(unsafe_code)]

    use super::{connection, Msg, PIPE_SDDL};
    use std::sync::mpsc::Sender;
    use std::sync::Arc;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use tokio::sync::Semaphore;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

    fn create(name: &str, first: bool) -> std::io::Result<NamedPipeServer> {
        let wide: Vec<u16> = PIPE_SDDL.encode_utf16().chain(std::iter::once(0)).collect();
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

    pub async fn serve(tx: Sender<Msg>, data_dir: std::path::PathBuf, permits: Arc<Semaphore>) -> Result<(), String> {
        let name = super::pipe_name(&data_dir);
        let mut server =
            create(&name, true).map_err(|e| format!("the pipe {name} could not be created ({e}); is another Pitcrew service running?"))?;
        loop {
            server.connect().await.map_err(|e| e.to_string())?;
            let connected = server;
            server = create(&name, false).map_err(|e| e.to_string())?;
            let Ok(permit) = permits.clone().acquire_owned().await else {
                return Ok(());
            };
            let tx = tx.clone();
            tokio::spawn(async move {
                connection(connected, tx).await;
                drop(permit);
            });
        }
    }
}

#[cfg(unix)]
mod platform {
    use super::{connection, Msg, SOCKET_FILE};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::mpsc::Sender;
    use std::sync::Arc;
    use tokio::net::UnixListener;
    use tokio::sync::Semaphore;

    pub async fn serve(tx: Sender<Msg>, socket_dir: std::path::PathBuf, permits: Arc<Semaphore>) -> Result<(), String> {
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
                connection(stream, tx).await;
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
        let task = tokio::spawn(connection(server, tx.clone()));
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
        let task = tokio::spawn(connection(server, tx.clone()));
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
        let task = tokio::spawn(connection(server, tx.clone()));
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
        let task = tokio::spawn(connection(server, tx));
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
