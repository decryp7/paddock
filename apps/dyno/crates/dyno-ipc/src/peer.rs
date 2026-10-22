//! A connection after the handshake: calls with timeouts, incoming calls for a handler, and pushes, both ways.
//!
//! Threads: a reader dispatches incoming frames (replies to the waiting callers, calls to the handler's workers,
//! pushes to the push callback), a writer sends queued frames, and a few workers run the handler. Every queue is
//! bounded. A handler that panics answers that call with a `panic` failure; the connection stays up. When either side
//! closes (or the stream fails, or garbage arrives), every waiting caller gets the reason and the threads end.

use crate::endpoint::{Closer, Stream};
use crate::error::IpcError;
use crate::frame::{encode, read_frame, FrameLimit};
use crate::host::Connection;
use crate::queue::{PushError, Queue};
use crate::wire::{codes, Frame, RemoteError};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// How long [`Peer::push`] waits for room in the outgoing queue.
const PUSH_TIMEOUT: Duration = Duration::from_secs(5);

/// Answers incoming calls. Runs on the peer's worker threads.
pub trait Handler<B>: Send + Sync + 'static {
    /// The reply to `body`, or a typed failure.
    fn handle(&self, body: B) -> Result<B, RemoteError>;
}

impl<B, F> Handler<B> for F
where
    F: Fn(B) -> Result<B, RemoteError> + Send + Sync + 'static,
{
    fn handle(&self, body: B) -> Result<B, RemoteError> {
        self(body)
    }
}

type PushFn<B> = Arc<dyn Fn(B) + Send + Sync>;

/// How a [`Peer`] behaves; built with [`PeerOptions::new`] and its builder methods.
pub struct PeerOptions<B> {
    handler: Option<Arc<dyn Handler<B>>>,
    on_push: Option<PushFn<B>>,
    workers: usize,
    queue: usize,
    max_in_flight: usize,
}

impl<B> Default for PeerOptions<B> {
    fn default() -> Self {
        PeerOptions {
            handler: None,
            on_push: None,
            workers: 4,
            queue: 64,
            max_in_flight: 256,
        }
    }
}

impl<B> PeerOptions<B> {
    /// No handler (incoming calls fail with `unsupported`), pushes ignored, 4 workers, queues of 64.
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer incoming calls with `handler`.
    pub fn serve(mut self, handler: impl Handler<B>) -> Self {
        self.handler = Some(Arc::new(handler));
        self
    }

    /// Call `f` for every push, on a thread of its own. When `f` falls behind by more than the queue size, newer
    /// pushes are dropped and counted ([`Peer::dropped_pushes`]).
    pub fn on_push(mut self, f: impl Fn(B) + Send + Sync + 'static) -> Self {
        self.on_push = Some(Arc::new(f));
        self
    }

    /// The number of threads running the handler (at least 1), so a slow call does not hold up the others.
    pub fn workers(mut self, n: usize) -> Self {
        self.workers = n.max(1);
        self
    }

    /// The size of each queue (outgoing frames, incoming calls, pushes); at least 1.
    pub fn queue(mut self, n: usize) -> Self {
        self.queue = n.max(1);
        self
    }
}

struct Inner<B> {
    out: Queue<Vec<u8>>,
    calls: Queue<(u64, B)>,
    pushes: Queue<B>,
    pending: Mutex<HashMap<u64, SyncSender<Result<B, IpcError>>>>,
    next_id: AtomicU64,
    closed: Mutex<Option<IpcError>>,
    closed_cv: Condvar,
    closer: Closer,
    limit: FrameLimit,
    max_in_flight: usize,
    dropped_pushes: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // Every guarded value here stays consistent if a holder panicked.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl<B> Inner<B> {
    fn reason(&self) -> Option<IpcError> {
        lock(&self.closed).clone()
    }

    fn shut(&self, reason: IpcError) {
        {
            let mut closed = lock(&self.closed);
            if closed.is_none() {
                *closed = Some(reason);
            }
        }
        self.closed_cv.notify_all();
        self.closer.close();
        self.out.close();
        self.calls.close();
        self.pushes.close();
        let reason = self.reason().unwrap_or(IpcError::Closed);
        let waiting: Vec<_> = lock(&self.pending).drain().map(|(_, tx)| tx).collect();
        for tx in waiting {
            let _ = tx.try_send(Err(reason.clone()));
        }
    }

    fn resolve(&self, id: u64, result: Result<B, IpcError>) {
        // A reply for a caller that gave up is dropped.
        if let Some(tx) = lock(&self.pending).remove(&id) {
            let _ = tx.try_send(result);
        }
    }

    fn forget(&self, id: u64) {
        lock(&self.pending).remove(&id);
    }
}

/// Both directions of an authenticated [`Connection`]. Dropping it closes the connection.
pub struct Peer<B> {
    inner: Arc<Inner<B>>,
}

impl<B> Peer<B>
where
    B: Serialize + DeserializeOwned + Send + 'static,
{
    /// Starts the peer's threads on `conn`.
    pub fn start(conn: Connection, opts: PeerOptions<B>) -> Result<Peer<B>, IpcError> {
        let reader = conn.stream;
        let writer = reader.try_clone()?;
        let inner = Arc::new(Inner {
            out: Queue::new(opts.queue),
            calls: Queue::new(opts.queue),
            pushes: Queue::new(opts.queue),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            closed: Mutex::new(None),
            closed_cv: Condvar::new(),
            closer: reader.closer()?,
            limit: conn.limit,
            max_in_flight: opts.max_in_flight,
            dropped_pushes: AtomicU64::new(0),
        });
        let peer = Peer { inner: inner.clone() };
        let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| {
            std::thread::Builder::new()
                .name(name.into())
                .spawn(f)
                .map(drop)
                .map_err(IpcError::from)
        };
        {
            let inner = inner.clone();
            spawn("dyno-ipc-writer", Box::new(move || write_loop(&inner, writer)))?;
        }
        if let Some(handler) = opts.handler.clone() {
            for _ in 0..opts.workers {
                let (inner, handler) = (inner.clone(), handler.clone());
                spawn("dyno-ipc-worker", Box::new(move || work_loop(&inner, &*handler)))?;
            }
        }
        if let Some(f) = opts.on_push.clone() {
            let inner = inner.clone();
            spawn(
                "dyno-ipc-push",
                Box::new(move || {
                    while let Some(body) = inner.pushes.pop() {
                        let _ = catch_unwind(AssertUnwindSafe(|| f(body)));
                    }
                }),
            )?;
        }
        let (serves, pushes) = (opts.handler.is_some(), opts.on_push.is_some());
        spawn("dyno-ipc-reader", Box::new(move || read_loop(&inner, reader, serves, pushes)))?;
        Ok(peer)
    }

    /// Calls the other side and waits up to `timeout` for its reply. A typed failure is [`IpcError::Remote`]; the
    /// connection ending first is [`IpcError::Closed`] (or why it ended).
    pub fn call(&self, body: B, timeout: Duration) -> Result<B, IpcError> {
        let started = Instant::now();
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = sync_channel(1);
        {
            let mut pending = lock(&self.inner.pending);
            if let Some(reason) = self.inner.reason() {
                return Err(reason);
            }
            if pending.len() >= self.inner.max_in_flight {
                return Err(IpcError::Protocol(format!(
                    "more than {} calls in flight",
                    self.inner.max_in_flight
                )));
            }
            pending.insert(id, tx);
        }
        let bytes = match encode(&Frame::Call { id, body }, self.inner.limit) {
            Ok(b) => b,
            Err(e) => {
                self.inner.forget(id);
                return Err(e);
            }
        };
        match self.inner.out.push(bytes, Some(timeout)) {
            Ok(()) => {}
            Err(PushError::Full) => {
                self.inner.forget(id);
                return Err(IpcError::Timeout(timeout));
            }
            Err(PushError::Closed) => {
                self.inner.forget(id);
                return Err(self.inner.reason().unwrap_or(IpcError::Closed));
            }
        }
        match rx.recv_timeout(timeout.saturating_sub(started.elapsed())) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                self.inner.forget(id);
                Err(IpcError::Timeout(timeout))
            }
            Err(RecvTimeoutError::Disconnected) => Err(self.inner.reason().unwrap_or(IpcError::Closed)),
        }
    }

    /// Sends an unsolicited event (waits up to 5 s for room in the outgoing queue).
    pub fn push(&self, body: B) -> Result<(), IpcError> {
        let bytes = encode(&Frame::Push { body }, self.inner.limit)?;
        match self.inner.out.push(bytes, Some(PUSH_TIMEOUT)) {
            Ok(()) => Ok(()),
            Err(PushError::Full) => Err(IpcError::Timeout(PUSH_TIMEOUT)),
            Err(PushError::Closed) => Err(self.inner.reason().unwrap_or(IpcError::Closed)),
        }
    }
}

impl<B> Peer<B> {
    /// Closes the connection; waiting callers get [`IpcError::Closed`]. Idempotent.
    pub fn close(&self) {
        self.inner.shut(IpcError::Closed);
    }

    /// Whether the connection has ended.
    pub fn is_closed(&self) -> bool {
        self.inner.reason().is_some()
    }

    /// Why the connection ended, once it has.
    pub fn closed_reason(&self) -> Option<IpcError> {
        self.inner.reason()
    }

    /// Waits until the connection ends, up to `timeout` (`None`: forever). Returns whether it ended.
    pub fn wait_closed(&self, timeout: Option<Duration>) -> bool {
        let deadline = timeout.map(|t| Instant::now() + t);
        let mut closed = lock(&self.inner.closed);
        while closed.is_none() {
            closed = match deadline {
                None => self.inner.closed_cv.wait(closed).unwrap_or_else(|e| e.into_inner()),
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return false;
                    }
                    self.inner.closed_cv.wait_timeout(closed, left).unwrap_or_else(|e| e.into_inner()).0
                }
            };
        }
        true
    }

    /// How many pushes were dropped because the push callback fell behind.
    pub fn dropped_pushes(&self) -> u64 {
        self.inner.dropped_pushes.load(Ordering::Relaxed)
    }
}

impl<B> Drop for Peer<B> {
    fn drop(&mut self) {
        self.close();
    }
}

fn write_loop<B>(inner: &Inner<B>, mut stream: Stream) {
    while let Some(bytes) = inner.out.pop() {
        if let Err(e) = stream.write_all(&bytes) {
            inner.shut(e.into());
            return;
        }
    }
}

fn work_loop<B: Serialize + 'static>(inner: &Inner<B>, handler: &dyn Handler<B>) {
    while let Some((id, body)) = inner.calls.pop() {
        let result = catch_unwind(AssertUnwindSafe(|| handler.handle(body))).unwrap_or_else(|panic| {
            Err(RemoteError::new(
                codes::PANIC,
                format!("the handler panicked: {}", panic_message(&*panic)),
            ))
        });
        let frame = match result {
            Ok(body) => Frame::Reply { id, body },
            Err(error) => Frame::Fail { id, error },
        };
        let bytes = encode(&frame, inner.limit).or_else(|e| {
            // Too large: say so, with the message cut to fit if the limit is tiny.
            let mut message = e.to_string();
            loop {
                let fail: Frame<B> = Frame::Fail {
                    id,
                    error: RemoteError::new(codes::TOO_LARGE, message.clone()),
                };
                match encode(&fail, inner.limit) {
                    Err(_) if !message.is_empty() => {
                        let mut cut = message.len() / 2;
                        while !message.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        message.truncate(cut);
                    }
                    result => return result,
                }
            }
        });
        // A limit too small even for an empty failure leaves the caller to its timeout.
        if let Ok(bytes) = bytes {
            if inner.out.push(bytes, None).is_err() {
                return;
            }
        }
    }
}

fn read_loop<B: Serialize + DeserializeOwned>(inner: &Inner<B>, mut stream: Stream, serves: bool, pushes: bool) {
    loop {
        let frame = match read_frame::<Frame<B>>(&mut stream, inner.limit) {
            Ok(Some(frame)) => frame,
            Ok(None) => return inner.shut(IpcError::Closed),
            Err(e) => return inner.shut(e),
        };
        match frame {
            Frame::Reply { id, body } => inner.resolve(id, Ok(body)),
            Frame::Fail { id, error } => inner.resolve(id, Err(IpcError::Remote(error))),
            Frame::Call { id, body } if serves => {
                if inner.calls.push((id, body), None).is_err() {
                    return;
                }
            }
            Frame::Call { id, .. } => {
                let fail: Frame<B> = Frame::Fail {
                    id,
                    error: RemoteError::new(codes::UNSUPPORTED, "this side answers no calls"),
                };
                let Ok(bytes) = encode(&fail, inner.limit) else { return };
                if inner.out.push(bytes, None).is_err() {
                    return;
                }
            }
            Frame::Push { body } if pushes => {
                if inner.pushes.try_push(body) == Err(PushError::Full) {
                    inner.dropped_pushes.fetch_add(1, Ordering::Relaxed);
                }
            }
            Frame::Push { .. } => {}
            Frame::Hello { .. } | Frame::Welcome { .. } => {
                return inner.shut(IpcError::Protocol("a handshake frame after the handshake".into()));
            }
        }
    }
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}
