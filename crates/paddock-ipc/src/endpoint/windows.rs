//! Windows: an owner-only, local-only named pipe with overlapped I/O, one pipe instance per client.
//!
//! Every operation is started overlapped and waited for at once (`GetOverlappedResult(.., wait = TRUE)` on every
//! path), so the `OVERLAPPED` and the buffer outlive the operation. Overlapped handles let one thread read while
//! another writes; a synchronous pipe handle would serialise them and deadlock a peer. A shared "shutdown" event ends
//! blocked operations from any thread.
#![allow(unsafe_code)]

use crate::token::random_hex;
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_NO_DATA, ERROR_OPERATION_ABORTED, ERROR_PIPE_BUSY,
    ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE, GetLastError,
    HANDLE, INVALID_HANDLE_VALUE, LocalFree, WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects,
};

const BUFFER: u32 = 64 * 1024;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error() -> u32 {
    // SAFETY: reads the calling thread's last-error value.
    unsafe { GetLastError() }
}

/// A manual-reset event.
struct Event(OwnedHandle);

impl Event {
    fn new() -> io::Result<Event> {
        // SAFETY: no security attributes, no name; the returned handle is owned below.
        let h = unsafe { CreateEventW(null(), 1, 0, null()) };
        if h.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `h` is a fresh, valid handle nobody else owns.
        Ok(Event(unsafe { OwnedHandle::from_raw_handle(h) }))
    }

    fn raw(&self) -> HANDLE {
        self.0.as_raw_handle()
    }

    fn set(&self) {
        // SAFETY: the event handle is valid while `self` lives.
        unsafe {
            SetEvent(self.raw());
        }
    }
}

/// How a wait ended.
#[derive(PartialEq, Eq)]
enum Ended {
    Done(u32),
    TimedOut,
    Shutdown,
}

/// Starts an overlapped operation with `start` and waits until it completes, `timeout` passes or `shutdown` is set
/// (then the operation is cancelled and waited for, so it never outlives this call).
fn overlapped(
    pipe: HANDLE,
    event: &Event,
    shutdown: Option<&Event>,
    timeout: Option<Duration>,
    start: impl FnOnce(*mut OVERLAPPED) -> i32,
) -> io::Result<Ended> {
    let mut ov = OVERLAPPED {
        hEvent: event.raw(),
        ..Default::default()
    };
    if start(&mut ov) == 0 {
        let err = last_error();
        if err != ERROR_IO_PENDING {
            // Not started: nothing refers to `ov`.
            return Err(io::Error::from_raw_os_error(err as i32));
        }
    }
    let ms = timeout.map_or(INFINITE, |t| {
        u32::try_from(t.as_millis())
            .unwrap_or(INFINITE - 1)
            .clamp(1, INFINITE - 1)
    });
    let handles = [event.raw(), shutdown.map_or(event.raw(), Event::raw)];
    let count = if shutdown.is_some() { 2 } else { 1 };
    // SAFETY: `handles` holds `count` valid, distinct handles.
    let woke = unsafe { WaitForMultipleObjects(count, handles.as_ptr(), 0, ms) };
    let mut why = Ended::Done(0);
    if woke != WAIT_OBJECT_0 {
        why = if woke == WAIT_OBJECT_0 + 1 {
            Ended::Shutdown
        } else {
            Ended::TimedOut
        };
        // SAFETY: cancels only this operation; `ov` is still alive.
        unsafe {
            CancelIoEx(pipe, &ov);
        }
    }
    let mut n = 0u32;
    // SAFETY: waits (bWait = TRUE) until the operation using `ov` has completed or been cancelled.
    if unsafe { GetOverlappedResult(pipe, &ov, &mut n, 1) } != 0 {
        // Completed, possibly despite a late cancel: keep the data.
        return Ok(Ended::Done(n));
    }
    match last_error() {
        ERROR_OPERATION_ABORTED if why != Ended::Done(0) => Ok(why),
        err => Err(io::Error::from_raw_os_error(err as i32)),
    }
}

fn is_disconnect(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error().map(|c| c as u32),
        Some(ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED | ERROR_NO_DATA)
    )
}

pub(super) struct Listener {
    pipe: Option<OwnedHandle>,
    name: String,
    /// The access list of every instance.
    sddl: String,
}

/// Creates one instance of the pipe `name`. The first instance is created with `FILE_FLAG_FIRST_PIPE_INSTANCE`, so a
/// squatter that created the name earlier makes it fail; later instances (one per accepted client) rely on the owner-only
/// DACL: nobody else may add an instance to this pipe.
fn create_instance(name: &str, first: bool, sddl: &str) -> io::Result<OwnedHandle> {
    let sddl = wide(sddl);
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: `sddl` is NUL-terminated and outlives the call; the descriptor is freed below.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let wname = wide(name);
    let open_mode = PIPE_ACCESS_DUPLEX
        | FILE_FLAG_OVERLAPPED
        | if first {
            FILE_FLAG_FIRST_PIPE_INSTANCE
        } else {
            0
        };
    // SAFETY: valid NUL-terminated name and attributes for the duration of the call.
    let h = unsafe {
        CreateNamedPipeW(
            wname.as_ptr(),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            BUFFER,
            BUFFER,
            0,
            &attributes,
        )
    };
    let created = io::Error::last_os_error();
    // SAFETY: allocated by the conversion above; the pipe copied it.
    unsafe {
        LocalFree(descriptor);
    }
    if h == INVALID_HANDLE_VALUE {
        return Err(created);
    }
    // SAFETY: a fresh, valid handle nobody else owns.
    Ok(unsafe { OwnedHandle::from_raw_handle(h) })
}

impl Listener {
    /// A fresh owner-only pipe with a random name (pipes live in their own namespace, so `_dir` is not used).
    pub(super) fn bind(_dir: Option<&std::path::Path>) -> io::Result<Listener> {
        let name = format!(r"\\.\pipe\paddock-{}", random_hex(12)?);
        let sddl = super::Access::owner_only().sddl;
        let pipe = create_instance(&name, true, &sddl)?;
        Ok(Listener {
            pipe: Some(pipe),
            name,
            sddl,
        })
    }

    /// A pipe with a fixed `name` and the access list `sddl` (a service's endpoint).
    pub(super) fn bind_fixed(name: &str, access: &super::Access) -> io::Result<Listener> {
        let pipe = create_instance(name, true, &access.sddl)?;
        Ok(Listener {
            pipe: Some(pipe),
            name: name.to_string(),
            sddl: access.sddl.clone(),
        })
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn accept(&mut self, timeout: Duration) -> io::Result<(Stream, Option<u32>)> {
        let pipe = self
            .pipe
            .as_ref()
            .ok_or_else(|| io::Error::other("the endpoint already has its connection"))?
            .as_raw_handle();
        let event = Event::new()?;
        // SAFETY (inside `overlapped`): `pipe` is valid while `self.pipe` lives.
        match overlapped(pipe, &event, None, Some(timeout), |ov| unsafe {
            ConnectNamedPipe(pipe, ov)
        }) {
            Ok(Ended::Done(_)) => {}
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "nobody connected in time",
                ));
            }
            // The client connected between creating the pipe and this call.
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_CONNECTED as i32) => {}
            Err(e) => return Err(e),
        }
        let mut pid = 0u32;
        // SAFETY: a connected pipe server handle and a valid out pointer.
        let pid = (unsafe { GetNamedPipeClientProcessId(pipe, &mut pid) } != 0).then_some(pid);
        let connected = self
            .pipe
            .take()
            .ok_or_else(|| io::Error::other("the endpoint already has its connection"))?;
        // The next client needs a fresh instance of the same pipe; until it exists a connecting client sees "busy" and
        // waits. If it cannot be made, later accepts report that, this connection still works.
        self.pipe = create_instance(&self.name, false, &self.sddl).ok();
        Ok((Stream::new(connected)?, pid))
    }
}

pub(super) struct Stream {
    pipe: Arc<OwnedHandle>,
    shutdown: Arc<Event>,
    /// This handle's own event: each concurrent operation needs its own.
    event: Event,
    read_timeout: Option<Duration>,
}

impl Stream {
    fn new(pipe: OwnedHandle) -> io::Result<Stream> {
        Ok(Stream {
            pipe: Arc::new(pipe),
            shutdown: Arc::new(Event::new()?),
            event: Event::new()?,
            read_timeout: None,
        })
    }

    /// The facts of the client, after the first read (the OS refuses to impersonate before).
    pub(super) fn raw_peer(&self) -> Result<crate::identity::RawPeer, crate::identity::PeerError> {
        super::windows_peer::raw_peer(self.pipe.as_raw_handle())
    }

    /// Connects at identification level and trusts the pipe only when SYSTEM owns it; see
    /// [`super::Stream::connect_system`].
    pub(super) fn connect_system(
        name: &str,
        timeout: Duration,
    ) -> Result<Stream, super::SystemConnectError> {
        let pipe = open_pipe(name, timeout).map_err(super::SystemConnectError::Connect)?;
        super::windows_peer::check_owner_is_system(pipe.as_raw_handle())
            .map_err(super::SystemConnectError::Untrusted)?;
        Stream::new(pipe).map_err(super::SystemConnectError::Connect)
    }

    pub(super) fn connect(name: &str, timeout: Duration) -> io::Result<Stream> {
        Stream::new(open_pipe(name, timeout)?)
    }
}

/// Opens the pipe `name`, retrying while it is busy. Identification-level impersonation only: whoever serves the pipe
/// cannot act as us.
fn open_pipe(name: &str, timeout: Duration) -> io::Result<OwnedHandle> {
    {
        let wname = wide(name);
        let deadline = Instant::now() + timeout;
        loop {
            // SAFETY: a valid NUL-terminated name. Identification-level impersonation only: whoever serves the pipe
            // cannot act as us.
            let h = unsafe {
                CreateFileW(
                    wname.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                    null_mut(),
                )
            };
            if h != INVALID_HANDLE_VALUE {
                // SAFETY: a fresh, valid handle nobody else owns.
                return Ok(unsafe { OwnedHandle::from_raw_handle(h) });
            }
            let err = io::Error::last_os_error();
            if err.raw_os_error() != Some(ERROR_PIPE_BUSY as i32) || Instant::now() >= deadline {
                return Err(err);
            }
            // SAFETY: a valid NUL-terminated name.
            unsafe {
                WaitNamedPipeW(wname.as_ptr(), 50);
            }
        }
    }
}

impl Stream {
    pub(super) fn try_clone(&self) -> io::Result<Stream> {
        Ok(Stream {
            pipe: self.pipe.clone(),
            shutdown: self.shutdown.clone(),
            event: Event::new()?,
            read_timeout: self.read_timeout,
        })
    }

    pub(super) fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.read_timeout = timeout;
        Ok(())
    }

    pub(super) fn closer(&self) -> io::Result<Closer> {
        Ok(Closer(self.shutdown.clone()))
    }

    pub(super) fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let pipe = self.pipe.as_raw_handle();
        let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        let ptr = buf.as_mut_ptr();
        // SAFETY (inside `overlapped`): `buf` outlives the operation, which `overlapped` waits for.
        match overlapped(
            pipe,
            &self.event,
            Some(&self.shutdown),
            self.read_timeout,
            |ov| unsafe { ReadFile(pipe, ptr, len, null_mut(), ov) },
        ) {
            Ok(Ended::Done(n)) => Ok(n as usize),
            Ok(Ended::Shutdown) => Ok(0),
            Ok(Ended::TimedOut) => Err(io::Error::new(io::ErrorKind::TimedOut, "no data in time")),
            Err(e) if is_disconnect(&e) => Ok(0),
            Err(e) => Err(e),
        }
    }

    pub(super) fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let pipe = self.pipe.as_raw_handle();
        let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        let ptr = buf.as_ptr();
        // SAFETY (inside `overlapped`): `buf` outlives the operation, which `overlapped` waits for.
        match overlapped(pipe, &self.event, Some(&self.shutdown), None, |ov| unsafe {
            WriteFile(pipe, ptr, len, null_mut(), ov)
        }) {
            Ok(Ended::Done(n)) => Ok(n as usize),
            Ok(_) => Err(io::Error::from(io::ErrorKind::BrokenPipe)),
            Err(e) if is_disconnect(&e) => Err(io::Error::from(io::ErrorKind::BrokenPipe)),
            Err(e) => Err(e),
        }
    }
}

pub(super) struct Closer(Arc<Event>);

impl Closer {
    pub(super) fn close(&self) {
        self.0.set();
    }
}
