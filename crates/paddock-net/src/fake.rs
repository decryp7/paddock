//! In-process fakes of the wire and the resolver, for the apps' own tests: nothing opens a socket.

use crate::error::NetError;
use crate::reach::Reach;
use crate::request::Method;
use crate::wire::{Lookup, Wire, WireRequest, WireResponse};
use std::collections::{HashMap, VecDeque};
use std::io::{Cursor, Read};
use std::net::IpAddr;
use std::sync::{Mutex, MutexGuard, PoisonError};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What a [`FakeWire`] answers.
#[derive(Debug, Clone)]
pub struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    fail_after: Option<std::io::ErrorKind>,
}

impl Reply {
    /// An answer with a status.
    pub fn status(status: u16) -> Reply {
        Reply {
            status,
            headers: Vec::new(),
            body: Vec::new(),
            fail_after: None,
        }
    }

    /// A 200 with a body.
    pub fn ok(body: &[u8]) -> Reply {
        Reply {
            body: body.to_vec(),
            ..Reply::status(200)
        }
    }

    /// A redirect to `location`.
    pub fn redirect(status: u16, location: &str) -> Reply {
        Reply::status(status).header("Location", location)
    }

    /// The body breaks with this error after its bytes were read.
    pub fn failing_body(mut self, kind: std::io::ErrorKind) -> Reply {
        self.fail_after = Some(kind);
        self
    }

    /// Adds a header.
    pub fn header(mut self, name: &str, value: &str) -> Reply {
        self.headers
            .push((name.to_ascii_lowercase(), value.to_string()));
        self
    }
}

/// What a [`FakeWire`] saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// The method.
    pub method: Method,
    /// The address.
    pub url: String,
    /// Header names and values.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
    /// The reach the request went out with (the policy's, narrowed to where the first hop's addresses are).
    pub reach: Reach,
}

/// A [`Wire`] that answers from a script (200 with an empty body when it is used up) and records every request. `sends()` is
/// the number of sockets a real wire would have opened.
#[derive(Default)]
pub struct FakeWire {
    script: Mutex<VecDeque<Result<Reply, NetError>>>,
    seen: Mutex<Vec<Seen>>,
}

impl FakeWire {
    /// A wire that answers 200 to everything.
    pub fn new() -> FakeWire {
        FakeWire::default()
    }

    /// The next request is answered with `reply`.
    pub fn then(self, reply: Reply) -> FakeWire {
        lock(&self.script).push_back(Ok(reply));
        self
    }

    /// The next request fails with `error`.
    pub fn then_error(self, error: NetError) -> FakeWire {
        lock(&self.script).push_back(Err(error));
        self
    }

    /// The requests received so far.
    pub fn seen(&self) -> Vec<Seen> {
        lock(&self.seen).clone()
    }

    /// How many requests reached the wire.
    pub fn sends(&self) -> usize {
        lock(&self.seen).len()
    }
}

impl Wire for FakeWire {
    fn send(&self, r: &WireRequest<'_>) -> Result<WireResponse, NetError> {
        lock(&self.seen).push(Seen {
            method: r.method,
            url: r.url.to_string(),
            headers: r
                .headers
                .iter()
                .map(|h| (h.name.clone(), h.value.clone()))
                .collect(),
            body: r.body.to_vec(),
            reach: r.reach,
        });
        let step = lock(&self.script)
            .pop_front()
            .unwrap_or_else(|| Ok(Reply::status(200)));
        step.map(|reply| WireResponse {
            status: reply.status,
            headers: reply.headers,
            body: match reply.fail_after {
                None => Box::new(Cursor::new(reply.body)),
                Some(kind) => Box::new(Cursor::new(reply.body).chain(Failing(kind))),
            },
        })
    }
}

/// A [`Lookup`] with a fixed table; unknown names fail.
#[derive(Default)]
pub struct StaticLookup {
    table: HashMap<String, Vec<IpAddr>>,
    asked: Mutex<Vec<String>>,
}

impl StaticLookup {
    /// An empty table.
    pub fn new() -> StaticLookup {
        StaticLookup::default()
    }

    /// `host` resolves to these addresses.
    pub fn with(mut self, host: &str, addrs: &[&str]) -> StaticLookup {
        self.table.insert(
            host.to_ascii_lowercase(),
            addrs.iter().filter_map(|a| a.parse().ok()).collect(),
        );
        self
    }

    /// The names asked so far.
    pub fn asked(&self) -> Vec<String> {
        lock(&self.asked).clone()
    }
}

impl Lookup for StaticLookup {
    fn lookup(&self, host: &str, _port: u16) -> std::io::Result<Vec<IpAddr>> {
        lock(&self.asked).push(host.to_string());
        self.table
            .get(&host.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no such host"))
    }
}

/// A reader that fails with this error as soon as it is read.
struct Failing(std::io::ErrorKind);

impl Read for Failing {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::new(self.0, "the connection broke"))
    }
}
