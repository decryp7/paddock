//! A peer whose calls, answers and pushes are one body type `B` (a protocol enum with `Request`, `Reply` and event
//! variants), for apps whose wire is a single postcard enum (Dyno's app, plugin and helper protocols). It is a thin typed
//! view over [`Peer`]: bodies are encoded with postcard like [`Peer::request`] does, so the frames on the wire are the
//! same as any other peer's. A body that does not decode is answered with [`codes::INVALID`] (a call) or counted and
//! dropped (a push); it never closes the connection.

use crate::error::{IpcError, RemoteError, codes};
use crate::handshake::Connection;
use crate::peer::{CallCtx, Handler, Peer, PeerOptions, decode_request, encode_reply};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Answers incoming calls of a [`BodyPeer`].
pub trait BodyHandler<B>: Send + Sync + 'static {
    /// The answer to `body`, or a typed failure.
    fn handle(&self, body: B) -> Result<B, RemoteError>;
}

impl<B, F> BodyHandler<B> for F
where
    F: Fn(B) -> Result<B, RemoteError> + Send + Sync + 'static,
{
    fn handle(&self, body: B) -> Result<B, RemoteError> {
        self(body)
    }
}

type BodyPush<B> = Arc<dyn Fn(B) + Send + Sync>;

/// How a [`BodyPeer`] runs: the underlying [`PeerOptions`] plus the typed handler and push callback.
pub struct BodyOptions<B> {
    peer: PeerOptions,
    handler: Option<Arc<dyn BodyHandler<B>>>,
    on_push: Option<BodyPush<B>>,
}

impl<B> BodyOptions<B> {
    /// No handler (incoming calls fail with [`codes::UNIMPLEMENTED`]) and pushes ignored. `label` names the other side.
    pub fn new(label: impl Into<String>) -> Self {
        BodyOptions {
            peer: PeerOptions::new(label),
            handler: None,
            on_push: None,
        }
    }

    /// Answer incoming calls with `handler` (a panic in it is contained and answered with [`codes::PANIC`]).
    #[must_use]
    pub fn serve(mut self, handler: impl BodyHandler<B>) -> Self {
        self.handler = Some(Arc::new(handler));
        self
    }

    /// Call `f` for every push, in order, on a thread of its own. When it falls behind by more than the push queue, newer
    /// pushes are dropped and counted ([`BodyPeer::dropped_pushes`]).
    #[must_use]
    pub fn on_push(mut self, f: impl Fn(B) + Send + Sync + 'static) -> Self {
        self.on_push = Some(Arc::new(f));
        self
    }

    /// The options of the underlying peer (concurrency, push queue, clock).
    #[must_use]
    pub fn peer_options(mut self, f: impl FnOnce(PeerOptions) -> PeerOptions) -> Self {
        self.peer = f(self.peer);
        self
    }
}

impl<B> fmt::Debug for BodyOptions<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BodyOptions")
            .field("peer", &self.peer)
            .field("serves", &self.handler.is_some())
            .field("on_push", &self.on_push.is_some())
            .finish()
    }
}

/// Both directions of an authenticated [`Connection`] with one body type. Cheap to clone (a handle on the same connection);
/// the connection closes with [`BodyPeer::close`] or when the other side goes.
pub struct BodyPeer<B> {
    peer: Peer,
    undecodable: Arc<AtomicU64>,
    body: PhantomData<fn(B) -> B>,
}

impl<B> Clone for BodyPeer<B> {
    fn clone(&self) -> Self {
        BodyPeer {
            peer: self.peer.clone(),
            undecodable: self.undecodable.clone(),
            body: PhantomData,
        }
    }
}

impl<B> fmt::Debug for BodyPeer<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BodyPeer")
            .field("peer", &self.peer)
            .finish()
    }
}

/// Adapts a [`BodyHandler`] to the byte-level [`Handler`].
struct Typed<B>(Option<Arc<dyn BodyHandler<B>>>);

impl<B> Handler for Typed<B>
where
    B: Serialize + DeserializeOwned + Send + 'static,
{
    fn call(&self, _ctx: &CallCtx, body: &[u8]) -> Result<Vec<u8>, RemoteError> {
        let Some(handler) = &self.0 else {
            return Err(RemoteError::new(
                codes::UNIMPLEMENTED,
                "this side answers no calls",
            ));
        };
        encode_reply(&handler.handle(decode_request(body)?)?)
    }
}

/// The byte-level [`Handler`] that answers with `handler` over `B` (for [`launch`](crate::launch), whose peer is started
/// there).
pub fn body_handler<B>(handler: impl BodyHandler<B>) -> Arc<dyn Handler>
where
    B: Serialize + DeserializeOwned + Send + 'static,
{
    Arc::new(Typed(Some(Arc::new(handler) as Arc<dyn BodyHandler<B>>)))
}

impl<B> BodyPeer<B>
where
    B: Serialize + DeserializeOwned + Send + 'static,
{
    /// Starts the peer's threads on `conn`.
    pub fn start(conn: Connection, opts: BodyOptions<B>) -> Result<BodyPeer<B>, IpcError> {
        let undecodable = Arc::new(AtomicU64::new(0));
        let mut peer_opts = opts.peer;
        if let Some(f) = opts.on_push {
            let skipped = undecodable.clone();
            peer_opts = peer_opts.on_push(move |bytes| match crate::wire::decode::<B>(&bytes) {
                Ok(body) => f(body),
                Err(_) => {
                    skipped.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
        let peer = Peer::start(conn, Arc::new(Typed(opts.handler)), peer_opts)?;
        Ok(BodyPeer {
            peer,
            undecodable,
            body: PhantomData,
        })
    }

    /// A typed view of a peer that was started elsewhere (a [`Child`](crate::Child)'s, or one with a handler built by
    /// [`body_handler`]). Only calls and pushes go through the view; the peer keeps its own handler and callbacks.
    pub fn wrap(peer: Peer) -> BodyPeer<B> {
        BodyPeer {
            peer,
            undecodable: Arc::new(AtomicU64::new(0)),
            body: PhantomData,
        }
    }

    /// Calls the other side and waits up to `timeout` for its answer. A typed failure is [`IpcError::Remote`]; an answer
    /// that does not decode is [`IpcError::Protocol`].
    pub fn call(&self, body: B, timeout: Duration) -> Result<B, IpcError> {
        self.peer.request(&body, Some(timeout))
    }

    /// Sends an event the other side's push callback receives.
    pub fn push(&self, body: B) -> Result<(), IpcError> {
        self.peer.push_value(&body)
    }

    /// The underlying byte-level peer (ids, limits, the peer pid, streams).
    pub fn peer(&self) -> &Peer {
        &self.peer
    }

    /// Pushes received that did not decode as `B` (dropped).
    pub fn undecodable_pushes(&self) -> u64 {
        self.undecodable.load(Ordering::Relaxed)
    }

    /// Closes the connection. Idempotent.
    pub fn close(&self) {
        self.peer.close();
    }

    /// Whether the connection is closed.
    pub fn is_closed(&self) -> bool {
        self.peer.is_closed()
    }

    /// Why the connection is closed, if it is.
    pub fn closed(&self) -> Option<IpcError> {
        self.peer.closed()
    }

    /// Waits until the connection closes (at most `timeout`; `None`: as long as it takes); returns whether it did.
    pub fn wait_closed(&self, timeout: Option<Duration>) -> bool {
        self.peer.wait_closed(timeout)
    }

    /// Pushes this side dropped because its push callback fell behind.
    pub fn dropped_pushes(&self) -> u64 {
        self.peer.dropped_pushes()
    }
}
