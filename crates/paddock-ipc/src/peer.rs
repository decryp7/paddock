//! One connection between two processes after the handshake: either side calls the other with [`Peer::call`] (one
//! answer) or [`Peer::stream`] (items with credits, then one answer), sends events with [`Peer::push`], and answers the
//! other side's calls with its [`Handler`]. Debrief's peer (calls both ways, streams, credits, cancel) on threads, plus
//! Dyno's pushes; no async runtime.
//!
//! Threads: a reader dispatches incoming frames (answers to waiting callers, calls to a thread per call, pushes to the
//! push thread) and never blocks on them, so a handler may call back into the other side; a writer sends queued frames.
//!
//! Failure semantics: a typed error is [`IpcError::Remote`] (the other side is fine); a broken connection is
//! [`IpcError::Closed`]; a deadline the callee ignored (it did not end the call within [`PeerOptions::cancel_grace`] after
//! the cancel) is [`IpcError::Timeout`]; one frame over the limit fails that call only ([`IpcError::TooLarge`], or a
//! `tooLarge` failure the other side receives); a frame that breaks the protocol (garbled, a repeated handshake, more
//! items than credits, a length beyond any limit) closes the connection and every call on it fails.

use crate::error::{IpcError, RemoteError, codes, panic_message};
use crate::frame::{self, Frame, FrameError, FrameHead, FrameKind, FrameLimit};
use crate::handshake::Connection;
use paddock_jobs::CancelToken;
use paddock_jobs::time::{Clock, SystemClock};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fmt;
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

/// How long a callee has to end a call after a cancel before the call counts as hung.
pub const CANCEL_GRACE: Duration = Duration::from_secs(2);
/// Items a stream may have in flight before the caller grants more (see [`Peer::stream`]).
pub const DEFAULT_CREDIT: u32 = 4;

/// Answers the other side's calls. Runs on a thread per call; a panic is contained and answered with [`codes::PANIC`].
pub trait Handler: Send + Sync + 'static {
    /// The answer to `body`, or a typed failure.
    fn call(&self, ctx: &CallCtx, body: &[u8]) -> Result<Vec<u8>, RemoteError>;
}

/// A handler that refuses every call (the side that only calls).
pub struct NoCalls;

impl Handler for NoCalls {
    fn call(&self, _ctx: &CallCtx, _body: &[u8]) -> Result<Vec<u8>, RemoteError> {
        Err(RemoteError::new(
            codes::UNIMPLEMENTED,
            "this side answers no calls",
        ))
    }
}

impl<F> Handler for F
where
    F: Fn(&CallCtx, &[u8]) -> Result<Vec<u8>, RemoteError> + Send + Sync + 'static,
{
    fn call(&self, ctx: &CallCtx, body: &[u8]) -> Result<Vec<u8>, RemoteError> {
        self(ctx, body)
    }
}

/// A handler over typed requests and replies (postcard): a request that does not decode is [`codes::INVALID`].
pub fn typed<Req, Rep, F>(f: F) -> impl Handler
where
    Req: DeserializeOwned,
    Rep: Serialize,
    F: Fn(&CallCtx, Req) -> Result<Rep, RemoteError> + Send + Sync + 'static,
{
    move |ctx: &CallCtx, body: &[u8]| -> Result<Vec<u8>, RemoteError> {
        encode_reply(&f(ctx, decode_request(body)?)?)
    }
}

type PushFn = Arc<dyn Fn(Vec<u8>) + Send + Sync>;

/// How a peer runs.
#[derive(Clone)]
pub struct PeerOptions {
    /// Names the other side in messages ("plugin \"x\"", "the agent").
    pub label: String,
    /// Calls of the other side handled at once; more wait in [`PeerOptions::call_queue`], and past that are refused with
    /// [`codes::BUSY`].
    pub max_concurrent_calls: usize,
    /// Calls over [`PeerOptions::max_concurrent_calls`] that wait their turn, first come first served (0: none wait, they
    /// are refused at once). A waiting call can be cancelled and ends with the connection.
    pub call_queue: usize,
    /// The most request bytes that wait in the call queue at once; a call that would exceed it is refused with
    /// [`codes::BUSY`] (default 16 MiB, the default frame limit, so one connection cannot park an unbounded amount).
    pub call_queue_bytes: usize,
    /// How long a callee has to end a call after a cancel ([`CANCEL_GRACE`]).
    pub cancel_grace: Duration,
    /// Pushes waiting for the push callback; newer ones are dropped and counted when it falls behind.
    pub push_queue: usize,
    /// The clock call timeouts and the cancel grace run on (a `FakeClock` in tests).
    pub clock: Arc<dyn Clock>,
    on_push: Option<PushFn>,
}

impl fmt::Debug for PeerOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PeerOptions")
            .field("label", &self.label)
            .field("max_concurrent_calls", &self.max_concurrent_calls)
            .field("call_queue", &self.call_queue)
            .field("call_queue_bytes", &self.call_queue_bytes)
            .field("cancel_grace", &self.cancel_grace)
            .field("push_queue", &self.push_queue)
            .field("on_push", &self.on_push.is_some())
            .finish()
    }
}

impl PeerOptions {
    /// 64 concurrent calls, [`CANCEL_GRACE`], a push queue of 64 and pushes ignored.
    pub fn new(label: impl Into<String>) -> PeerOptions {
        PeerOptions {
            label: label.into(),
            max_concurrent_calls: 64,
            call_queue: 0,
            call_queue_bytes: 16 << 20,
            cancel_grace: CANCEL_GRACE,
            push_queue: 64,
            clock: Arc::new(SystemClock),
            on_push: None,
        }
    }

    /// Calls `f` for every push, in order, on a thread of its own (a panic in it is contained).
    pub fn on_push(mut self, f: impl Fn(Vec<u8>) + Send + Sync + 'static) -> PeerOptions {
        self.on_push = Some(Arc::new(f));
        self
    }
}

/// What a call's caller receives.
enum Event {
    Item(Vec<u8>),
    Reply(Vec<u8>),
    Fail(RemoteError),
    Closed(IpcError),
}

/// A call this side made and is waiting for.
struct Pending {
    tx: mpsc::Sender<Event>,
    /// Cancelled when an event is posted, so the caller's wait on the clock ends at once.
    arrived: CancelToken,
    streams: bool,
    granted: u64,
    received: u64,
}

impl Pending {
    fn post(&self, event: Event) {
        let _ = self.tx.send(event);
        self.arrived.cancel();
    }
}

/// A call of the other side this side is handling.
struct Served {
    state: Mutex<ServedState>,
    cv: Condvar,
}

struct ServedState {
    credit: u64,
    streams: bool,
    cancelled: bool,
}

enum Out {
    Frame(Vec<u8>),
    Shutdown,
}

struct Inner {
    id: u64,
    opts: PeerOptions,
    limit: FrameLimit,
    peer_pid: Option<u32>,
    out: mpsc::Sender<Out>,
    closer: crate::endpoint::Closer,
    pending: Mutex<HashMap<u64, Pending>>,
    served: Mutex<HashMap<u64, Arc<Served>>>,
    /// Running calls and the calls waiting for a free slot (lock before `served`).
    gate: Mutex<Gate>,
    next: AtomicU64,
    dropped_pushes: AtomicU64,
    closed: Mutex<Option<IpcError>>,
    closed_cv: Condvar,
}

/// The calls running now and those waiting for a slot.
#[derive(Default)]
struct Gate {
    running: usize,
    /// Request bytes waiting in `waiting`.
    queued_bytes: usize,
    waiting: std::collections::VecDeque<(u64, Vec<u8>, Arc<Served>)>,
}

static NEXT_PEER: AtomicU64 = AtomicU64::new(1);
/// Why a call ended when its connection went without saying why (not expected: every close posts its reason).
const GONE: &str = "the connection is gone";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // Every guarded value here stays consistent if a holder panicked.
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// A live connection (cheap to clone). Dropping a clone does not close it: call [`Peer::close`], or it closes when the
/// other side goes away.
#[derive(Clone)]
pub struct Peer {
    inner: Arc<Inner>,
}

impl fmt::Debug for Peer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Peer")
            .field("label", &self.inner.opts.label)
            .finish()
    }
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) -> Result<(), IpcError> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(f)
        .map(drop)
        .map_err(|e| IpcError::Spawn(format!("cannot start an IPC thread: {e}")))
}

impl Peer {
    /// Starts the reader, the writer and (with a push callback) the push thread on an authenticated connection.
    pub fn start(
        conn: Connection,
        handler: Arc<dyn Handler>,
        opts: PeerOptions,
    ) -> Result<Peer, IpcError> {
        let peer_pid = conn.peer_pid();
        let mut reader = conn.stream;
        let mut writer = reader.try_clone()?;
        let closer = reader.closer()?;
        let (out_tx, out_rx) = mpsc::channel();
        let (push_tx, push_rx) = mpsc::sync_channel::<Vec<u8>>(opts.push_queue.max(1));
        let on_push = opts.on_push.clone();
        let peer = Peer {
            inner: Arc::new(Inner {
                id: NEXT_PEER.fetch_add(1, Ordering::Relaxed),
                opts,
                limit: conn.limit,
                peer_pid,
                out: out_tx,
                closer,
                pending: Mutex::new(HashMap::new()),
                served: Mutex::new(HashMap::new()),
                gate: Mutex::new(Gate::default()),
                next: AtomicU64::new(1),
                dropped_pushes: AtomicU64::new(0),
                closed: Mutex::new(None),
                closed_cv: Condvar::new(),
            }),
        };
        if let Some(f) = on_push {
            spawn("paddock-ipc-push", move || {
                while let Ok(body) = push_rx.recv() {
                    let _ = catch_unwind(AssertUnwindSafe(|| f(body)));
                }
            })?;
        }
        let w = peer.clone();
        spawn("paddock-ipc-writer", move || {
            w.write_loop(&mut writer, &out_rx)
        })?;
        let r = peer.clone();
        let pushes = r.inner.opts.on_push.is_some().then_some(push_tx);
        spawn("paddock-ipc-reader", move || {
            r.read_loop(&mut reader, &handler, pushes.as_ref())
        })?;
        Ok(peer)
    }

    /// This connection's number, unique in the process (subscriptions and logs key on it).
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    /// The label of the other side.
    pub fn label(&self) -> &str {
        &self.inner.opts.label
    }

    /// The largest frame this connection carries.
    pub fn limit(&self) -> FrameLimit {
        self.inner.limit
    }

    /// The other side's process id, when the handshake learned it (host side).
    pub fn peer_pid(&self) -> Option<u32> {
        self.inner.peer_pid
    }

    /// Why the connection is closed, if it is.
    pub fn closed(&self) -> Option<IpcError> {
        lock(&self.inner.closed).clone()
    }

    /// Whether the connection is closed.
    pub fn is_closed(&self) -> bool {
        self.closed().is_some()
    }

    /// Waits until the connection closes (at most `timeout`; `None`: as long as it takes); returns whether it did.
    pub fn wait_closed(&self, timeout: Option<Duration>) -> bool {
        let g = lock(&self.inner.closed);
        match timeout {
            None => {
                let _g = self
                    .inner
                    .closed_cv
                    .wait_while(g, |c| c.is_none())
                    .unwrap_or_else(|p| p.into_inner());
                true
            }
            Some(t) => {
                let (g, _) = self
                    .inner
                    .closed_cv
                    .wait_timeout_while(g, t, |c| c.is_none())
                    .unwrap_or_else(|p| p.into_inner());
                g.is_some()
            }
        }
    }

    /// Closes the connection: pending calls fail, served calls see a cancel, the other side sees the end of the stream.
    pub fn close(&self) {
        self.mark_closed(IpcError::Closed(format!(
            "the connection to {} was closed by this side",
            self.label()
        )));
        let _ = self.inner.out.send(Out::Shutdown);
        self.inner.closer.close();
    }

    /// Pushes this side dropped because its push callback fell behind.
    pub fn dropped_pushes(&self) -> u64 {
        self.inner.dropped_pushes.load(Ordering::Relaxed)
    }

    fn mark_closed(&self, why: IpcError) {
        {
            let mut c = lock(&self.inner.closed);
            if c.is_some() {
                return;
            }
            *c = Some(why.clone());
        }
        self.inner.closed_cv.notify_all();
        for (_, p) in lock(&self.inner.pending).drain() {
            p.post(Event::Closed(why.clone()));
        }
        for (_, s) in lock(&self.inner.served).drain() {
            lock(&s.state).cancelled = true;
            s.cv.notify_all();
        }
    }

    /// Why the connection closed (every close records its reason).
    fn closed_reason(&self) -> IpcError {
        self.closed()
            .unwrap_or_else(|| IpcError::Closed(GONE.into()))
    }

    fn send(&self, f: &Frame) -> Result<(), IpcError> {
        if self.is_closed() {
            return Err(self.closed_reason());
        }
        let bytes = frame::encode(f, self.inner.limit).map_err(IpcError::from)?;
        self.inner
            .out
            .send(Out::Frame(bytes))
            .map_err(|_| self.closed_reason())
    }

    fn begin(&self, body: Vec<u8>, credit: u32) -> Result<(u64, mpsc::Receiver<Event>), IpcError> {
        let id = self.inner.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        lock(&self.inner.pending).insert(
            id,
            Pending {
                tx,
                arrived: CancelToken::new(),
                streams: credit > 0,
                granted: u64::from(credit),
                received: 0,
            },
        );
        if let Err(e) = self.send(&Frame::Call { id, credit, body }) {
            lock(&self.inner.pending).remove(&id);
            return Err(e);
        }
        Ok((id, rx))
    }

    fn cancel(&self, id: u64) {
        let _ = self.send(&Frame::Cancel { id });
    }

    /// The next event of call `id`, waiting at most `timeout` on the peer's clock (`None`: as long as the connection
    /// lives); `Ok(None)` when the time passed first. Event-driven: posting an event ends the wait at once.
    fn wait_event(
        &self,
        id: u64,
        rx: &mpsc::Receiver<Event>,
        timeout: Option<Duration>,
    ) -> Result<Option<Event>, IpcError> {
        if let Ok(e) = rx.try_recv() {
            return Ok(Some(e));
        }
        let Some(t) = timeout else {
            return rx.recv().map(Some).map_err(|_| self.closed_reason());
        };
        let arrived = CancelToken::new();
        if let Some(p) = lock(&self.inner.pending).get_mut(&id) {
            p.arrived = arrived.clone();
        }
        // An event posted before the new token was in place is already in the channel.
        if let Ok(e) = rx.try_recv() {
            return Ok(Some(e));
        }
        let _ = self.inner.opts.clock.wait(t, &arrived);
        // Nothing posted: the time passed (a sender gone without an event cannot happen; it reads as a timeout).
        Ok(rx.try_recv().ok())
    }

    /// Calls the other side and waits for its answer (at most `timeout`; `None` waits as long as the connection lives).
    /// After the timeout a cancel is sent; a callee that does not end the call within [`PeerOptions::cancel_grace`], or
    /// that ends it with a cancellation, makes this [`IpcError::Timeout`]. A request over the frame limit fails at once
    /// with [`IpcError::TooLarge`] and nothing is sent.
    pub fn call(&self, body: Vec<u8>, timeout: Option<Duration>) -> Result<Vec<u8>, IpcError> {
        let (id, rx) = self.begin(body, 0)?;
        let event = match self.wait_event(id, &rx, timeout)? {
            Some(e) => e,
            None => {
                self.cancel(id);
                let late = self.wait_event(id, &rx, Some(self.inner.opts.cancel_grace));
                lock(&self.inner.pending).remove(&id);
                return match late {
                    Ok(Some(Event::Reply(b))) => Ok(b),
                    Ok(Some(Event::Closed(e))) | Err(e) => Err(e),
                    _ => Err(IpcError::Timeout),
                };
            }
        };
        match event {
            Event::Reply(b) => Ok(b),
            Event::Fail(e) => Err(IpcError::Remote(e)),
            Event::Closed(e) => Err(e),
            // `deliver` refuses items for calls that do not stream before they get here.
            Event::Item(_) => Err(IpcError::Protocol(GONE.into())),
        }
    }

    /// [`Peer::call`] with a typed request and reply (postcard).
    pub fn request<Req: Serialize, Rep: DeserializeOwned>(
        &self,
        req: &Req,
        timeout: Option<Duration>,
    ) -> Result<Rep, IpcError> {
        let body = crate::wire::encode(req).map_err(IpcError::Protocol)?;
        let reply = self.call(body, timeout)?;
        crate::wire::decode(&reply)
            .map_err(|e| IpcError::Protocol(format!("{} answered with {e}", self.label())))
    }

    /// Starts a streamed call: the callee sends items (at most `credit` ahead of what this side took) and then one answer.
    pub fn stream(&self, body: Vec<u8>, credit: u32) -> Result<CallStream, IpcError> {
        let credit = credit.max(1);
        let (id, rx) = self.begin(body, credit)?;
        Ok(CallStream {
            peer: self.clone(),
            id,
            rx,
            window: credit,
            outstanding: credit,
            done: false,
        })
    }

    /// [`Peer::stream`] with a typed request.
    pub fn stream_request<Req: Serialize>(
        &self,
        req: &Req,
        credit: u32,
    ) -> Result<CallStream, IpcError> {
        let body = crate::wire::encode(req).map_err(IpcError::Protocol)?;
        self.stream(body, credit)
    }

    /// Sends an event the other side's push callback receives (no answer). An event over the frame limit fails here.
    pub fn push(&self, body: Vec<u8>) -> Result<(), IpcError> {
        self.send(&Frame::Push { body })
    }

    /// [`Peer::push`] with a typed event.
    pub fn push_value<T: Serialize>(&self, event: &T) -> Result<(), IpcError> {
        self.push(crate::wire::encode(event).map_err(IpcError::Protocol)?)
    }

    fn deliver(&self, id: u64, event: Event, last: bool) -> Result<(), IpcError> {
        let mut pending = lock(&self.inner.pending);
        let Some(p) = pending.get_mut(&id) else {
            // A late answer to a call that timed out or was cancelled.
            return Ok(());
        };
        if let Event::Item(_) = event {
            if !p.streams {
                let e = IpcError::Protocol(format!(
                    "{} sent an item for a call that does not stream",
                    self.label()
                ));
                p.post(Event::Closed(e.clone()));
                return Err(e);
            }
            p.received += 1;
            if p.received > p.granted {
                return Err(IpcError::Protocol(format!(
                    "{} sent more items than it was granted ({} of {})",
                    self.label(),
                    p.received,
                    p.granted
                )));
            }
        }
        p.post(event);
        if last {
            pending.remove(&id);
        }
        Ok(())
    }

    fn grant(&self, id: u64, n: u32) {
        match lock(&self.inner.pending).get_mut(&id) {
            Some(p) => p.granted += u64::from(n),
            None => return,
        }
        let _ = self.send(&Frame::Credit { id, n });
    }

    /// Answers a call this side cannot run now with `busy`.
    fn refuse_call(&self, id: u64, message: &str) {
        let _ = self.send(&Frame::Fail {
            id,
            error: RemoteError::new(codes::BUSY, message),
        });
    }

    fn serve_call(&self, handler: &Arc<dyn Handler>, id: u64, credit: u32, body: Vec<u8>) {
        let opts = &self.inner.opts;
        let started_now = {
            let mut gate = lock(&self.inner.gate);
            let mut served = lock(&self.inner.served);
            if served.len() >= opts.max_concurrent_calls.saturating_add(opts.call_queue) {
                drop(served);
                drop(gate);
                return self.refuse_call(id, "too many calls are running; try again");
            }
            let call = Arc::new(Served {
                state: Mutex::new(ServedState {
                    credit: u64::from(credit),
                    streams: credit > 0,
                    cancelled: false,
                }),
                cv: Condvar::new(),
            });
            served.insert(id, call.clone());
            if gate.running < opts.max_concurrent_calls {
                gate.running += 1;
                Some(call)
            } else if gate.queued_bytes.saturating_add(body.len()) > opts.call_queue_bytes {
                served.remove(&id);
                drop(served);
                drop(gate);
                return self.refuse_call(id, "too many calls are waiting; try again");
            } else {
                gate.queued_bytes += body.len();
                gate.waiting.push_back((id, body.clone(), call));
                None
            }
        };
        let Some(first) = started_now else {
            return;
        };
        let (peer, handler) = (self.clone(), handler.clone());
        let started = spawn("paddock-ipc-call", move || {
            let mut next = Some((id, body, first));
            while let Some((id, body, served)) = next {
                peer.run_call(&handler, id, body, served);
                next = peer.next_waiting();
            }
        });
        if let Err(e) = started {
            lock(&self.inner.served).remove(&id);
            lock(&self.inner.gate).running -= 1;
            self.refuse_call(id, &e.to_string());
        }
    }

    /// The next waiting call that is still wanted (the connection did not end it); frees the slot when there is none.
    fn next_waiting(&self) -> Option<(u64, Vec<u8>, Arc<Served>)> {
        let mut gate = lock(&self.inner.gate);
        while let Some((id, body, served)) = gate.waiting.pop_front() {
            gate.queued_bytes -= body.len();
            if !lock(&self.inner.served).contains_key(&id) {
                continue;
            }
            // The caller gave up while the call waited: it is answered as cancelled and never runs.
            if lock(&served.state).cancelled {
                lock(&self.inner.served).remove(&id);
                let _ = self.send(&Frame::Fail {
                    id,
                    error: CallCtx::cancelled_error(),
                });
                continue;
            }
            return Some((id, body, served));
        }
        gate.running -= 1;
        None
    }

    fn run_call(&self, handler: &Arc<dyn Handler>, id: u64, body: Vec<u8>, served: Arc<Served>) {
        let ctx = CallCtx {
            peer: self.clone(),
            id,
            served,
        };
        let result =
            catch_unwind(AssertUnwindSafe(|| handler.call(&ctx, &body))).unwrap_or_else(|p| {
                Err(RemoteError::new(
                    codes::PANIC,
                    format!("it panicked: {}", panic_message(p.as_ref())),
                ))
            });
        lock(&self.inner.served).remove(&id);
        let reply = match result {
            Ok(body) => Frame::Reply { id, body },
            Err(error) => Frame::Fail { id, error },
        };
        if let Err(IpcError::TooLarge { len, max }) = self.send(&reply) {
            // The answer does not fit in a frame: this call fails, the connection stays.
            let _ = self.send(&Frame::Fail {
                id,
                error: RemoteError::new(
                    codes::TOO_LARGE,
                    format!(
                        "the answer could not be sent: {len} bytes exceed the limit of {max} bytes"
                    ),
                ),
            });
        }
    }

    /// One frame over the limit was skipped: fail the call it belonged to, and nothing else.
    fn on_skipped(&self, len: usize, max: usize, head: Option<FrameHead>) -> Result<(), IpcError> {
        let too_large = IpcError::TooLarge { len, max };
        match head {
            Some(FrameHead {
                kind: FrameKind::Call,
                id: Some(id),
            }) => {
                let _ = self.send(&Frame::Fail {
                    id,
                    error: RemoteError::new(
                        codes::TOO_LARGE,
                        format!(
                            "the request of {len} bytes exceeds this side's limit of {max} bytes"
                        ),
                    ),
                });
                Ok(())
            }
            Some(FrameHead {
                kind: FrameKind::Reply | FrameKind::Item | FrameKind::Fail,
                id: Some(id),
            }) => {
                let streams = lock(&self.inner.pending).remove(&id).map(|p| {
                    p.post(Event::Closed(too_large));
                    p.streams
                });
                if streams == Some(true) {
                    self.cancel(id);
                }
                Ok(())
            }
            Some(FrameHead {
                kind: FrameKind::Push,
                ..
            }) => {
                self.inner.dropped_pushes.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            _ => Err(IpcError::Protocol(format!(
                "{}: {}",
                self.label(),
                FrameError::TooLarge { len, max }
            ))),
        }
    }

    fn on_frame(
        &self,
        handler: &Arc<dyn Handler>,
        pushes: Option<&mpsc::SyncSender<Vec<u8>>>,
        f: Frame,
    ) -> Result<(), IpcError> {
        match f {
            Frame::Call { id, credit, body } => {
                self.serve_call(handler, id, credit, body);
                Ok(())
            }
            Frame::Reply { id, body } => self.deliver(id, Event::Reply(body), true),
            Frame::Item { id, body } => self.deliver(id, Event::Item(body), false),
            Frame::Fail { id, error } => self.deliver(id, Event::Fail(error), true),
            Frame::Cancel { id } => {
                if let Some(s) = lock(&self.inner.served).get(&id) {
                    lock(&s.state).cancelled = true;
                    s.cv.notify_all();
                }
                Ok(())
            }
            Frame::Credit { id, n } => {
                if let Some(s) = lock(&self.inner.served).get(&id) {
                    let mut st = lock(&s.state);
                    st.credit = st.credit.saturating_add(u64::from(n));
                    s.cv.notify_all();
                }
                Ok(())
            }
            Frame::Push { body } => {
                if let Some(tx) = pushes
                    && tx.try_send(body).is_err()
                {
                    self.inner.dropped_pushes.fetch_add(1, Ordering::Relaxed);
                }
                Ok(())
            }
            Frame::Hello { .. } | Frame::Welcome { .. } => Err(IpcError::Protocol(format!(
                "{} repeated the handshake on an open connection",
                self.label()
            ))),
        }
    }

    fn write_loop(&self, w: &mut impl Write, rx: &mpsc::Receiver<Out>) {
        while let Ok(out) = rx.recv() {
            match out {
                Out::Frame(bytes) => {
                    if let Err(e) = w.write_all(&bytes) {
                        self.mark_closed(IpcError::Closed(format!(
                            "the connection to {} broke ({e})",
                            self.label()
                        )));
                        break;
                    }
                }
                Out::Shutdown => break,
            }
        }
        self.inner.closer.close();
    }

    fn read_loop(
        &self,
        r: &mut impl std::io::Read,
        handler: &Arc<dyn Handler>,
        pushes: Option<&mpsc::SyncSender<Vec<u8>>>,
    ) {
        let why = loop {
            let step = match frame::read_frame(r, self.inner.limit) {
                Ok(f) => self.on_frame(handler, pushes, f),
                Err(FrameError::Skipped { len, max, head }) => self.on_skipped(len, max, head),
                Err(FrameError::Closed) => {
                    break IpcError::Closed(format!("{} closed the connection", self.label()));
                }
                Err(FrameError::Io(e)) => {
                    break IpcError::Closed(format!(
                        "the connection to {} broke ({e})",
                        self.label()
                    ));
                }
                Err(e) => break IpcError::Protocol(format!("{}: {e}", self.label())),
            };
            if let Err(e) = step {
                break e;
            }
        };
        self.mark_closed(why);
        let _ = self.inner.out.send(Out::Shutdown);
        self.inner.closer.close();
    }
}

/// What a streamed call yields.
#[derive(Debug, PartialEq, Eq)]
pub enum StreamEvent {
    /// One item.
    Item(Vec<u8>),
    /// The call's answer; the stream is complete.
    Done(Vec<u8>),
    /// Nothing arrived within the timeout given to [`CallStream::next`]; the call is still running.
    Idle,
}

/// The caller's side of a streamed call. Dropping it before the end cancels the call.
pub struct CallStream {
    peer: Peer,
    id: u64,
    rx: mpsc::Receiver<Event>,
    window: u32,
    /// Items granted but not taken yet.
    outstanding: u32,
    done: bool,
}

impl CallStream {
    /// The next item or the answer, waiting at most `timeout` (`None`: as long as the connection lives).
    pub fn next(&mut self, timeout: Option<Duration>) -> Result<StreamEvent, IpcError> {
        if self.done {
            return Err(IpcError::Protocol("the stream is already complete".into()));
        }
        let Some(got) = self.peer.wait_event(self.id, &self.rx, timeout)? else {
            return Ok(StreamEvent::Idle);
        };
        match got {
            Event::Item(b) => {
                self.outstanding = self.outstanding.saturating_sub(1);
                if self.outstanding <= self.window / 2 {
                    let n = self.window - self.outstanding;
                    self.outstanding = self.window;
                    self.peer.grant(self.id, n);
                }
                Ok(StreamEvent::Item(b))
            }
            Event::Reply(b) => {
                self.done = true;
                Ok(StreamEvent::Done(b))
            }
            Event::Fail(e) => {
                self.done = true;
                Err(IpcError::Remote(e))
            }
            Event::Closed(e) => {
                self.done = true;
                Err(e)
            }
        }
    }

    /// Asks the callee to stop; [`CallStream::next`] then yields its answer (usually a cancellation error).
    pub fn cancel(&mut self) {
        if !self.done {
            self.peer.cancel(self.id);
        }
    }

    /// Cancels and waits up to the peer's cancel grace for the callee to end the call; `Err(Timeout)` if it did not.
    pub fn cancel_and_wait(&mut self) -> Result<(), IpcError> {
        self.cancel();
        let grace = self.peer.inner.opts.cancel_grace;
        while !self.done {
            match self.next(Some(grace)) {
                Ok(StreamEvent::Idle) => return Err(IpcError::Timeout),
                Ok(_) | Err(IpcError::Remote(_)) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Whether the answer (or a failure) arrived.
    pub fn is_done(&self) -> bool {
        self.done
    }
}

impl Drop for CallStream {
    fn drop(&mut self) {
        if !self.done {
            self.peer.cancel(self.id);
            lock(&self.peer.inner.pending).remove(&self.id);
        }
    }
}

/// What a [`Handler`] gets for one call.
pub struct CallCtx {
    peer: Peer,
    id: u64,
    served: Arc<Served>,
}

impl CallCtx {
    /// The connection, to call back into the caller's side (e.g. to read a host file).
    pub fn peer(&self) -> &Peer {
        &self.peer
    }

    /// Whether the caller cancelled the call (or the connection closed).
    pub fn is_cancelled(&self) -> bool {
        lock(&self.served.state).cancelled
    }

    /// A probe of [`CallCtx::is_cancelled`] that outlives the borrow, for work on other threads.
    pub fn cancel_probe(&self) -> CancelProbe {
        CancelProbe(self.served.clone())
    }

    /// The call as cancelled, for handlers that check [`CallCtx::is_cancelled`].
    pub fn cancelled_error() -> RemoteError {
        RemoteError::new(codes::CANCELLED, "the call was cancelled")
    }

    /// Sends one item of a streamed answer, waiting until the caller has granted credit for it. Fails when the call was
    /// cancelled, the connection closed, or the caller did not ask for a stream.
    pub fn send_item(&self, body: Vec<u8>) -> Result<(), RemoteError> {
        {
            let st = lock(&self.served.state);
            if !st.streams {
                return Err(RemoteError::new(
                    codes::INVALID,
                    "this call does not stream",
                ));
            }
            let mut st = self
                .served
                .cv
                .wait_while(st, |s| !s.cancelled && s.credit == 0)
                .unwrap_or_else(|p| p.into_inner());
            if st.cancelled {
                return Err(Self::cancelled_error());
            }
            st.credit -= 1;
        }
        self.peer
            .send(&Frame::Item { id: self.id, body })
            .map_err(|e| e.to_remote())
    }

    /// [`CallCtx::send_item`] with a typed item.
    pub fn send<T: Serialize>(&self, item: &T) -> Result<(), RemoteError> {
        let body = crate::wire::encode(item).map_err(|e| RemoteError::new(codes::INTERNAL, e))?;
        self.send_item(body)
    }
}

/// Whether a served call was cancelled (or its connection closed); see [`CallCtx::cancel_probe`].
#[derive(Clone)]
pub struct CancelProbe(Arc<Served>);

impl CancelProbe {
    /// Whether the call was cancelled.
    pub fn is_cancelled(&self) -> bool {
        lock(&self.0.state).cancelled
    }

    /// Blocks until the call is cancelled (or its connection closes).
    pub fn wait(&self) {
        let st = lock(&self.0.state);
        let _st = self
            .0
            .cv
            .wait_while(st, |s| !s.cancelled)
            .unwrap_or_else(|p| p.into_inner());
    }
}

/// Decodes a typed request for a handler; a body that does not decode is [`codes::INVALID`].
pub fn decode_request<T: DeserializeOwned>(body: &[u8]) -> Result<T, RemoteError> {
    crate::wire::decode(body).map_err(|e| RemoteError::new(codes::INVALID, e))
}

/// Encodes a typed reply for a handler.
pub fn encode_reply<T: Serialize>(reply: &T) -> Result<Vec<u8>, RemoteError> {
    crate::wire::encode(reply).map_err(|e| RemoteError::new(codes::INTERNAL, e))
}
