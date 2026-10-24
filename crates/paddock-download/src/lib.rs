//! Downloads for the Paddock apps: segmented Range downloads with per-host caps, 429 and Retry-After back-off, resume, and one
//! in-order SHA-256 while writing, reported as job phases.
//!
//! Rule 15, ADR 0008 §5, ADR 0012 §3; plan S8. Ported from Debrief's model downloader (`debrief-ai::models::{download,
//! segmented}`), with the hash moved into the write path: bytes that arrive ahead of the hash wait in a bounded buffer instead of
//! being read back from the file.
//!
//! ```no_run
//! # #[cfg(feature = "http")]
//! # fn demo(jobs: &paddock_jobs::JobRegistry, pool: &paddock_jobs::JobPool) -> Result<(), paddock_download::DownloadError> {
//! use paddock_download::{Downloader, HttpConfig, Integrity, Request, Sha256Digest};
//! use std::sync::Arc;
//!
//! let downloader = Arc::new(Downloader::http(HttpConfig::default())?); // one per app: the host caps hold across downloads
//! let req = Request::new(
//!     "https://example.com/model.gguf",
//!     "/data/models/model.gguf",
//!     Integrity::Sha256(Sha256Digest::from_hex(&"ab".repeat(32))?),
//! )?;
//! let ctx = jobs.register("download", "Downloading the model", Some("model.gguf"));
//! let d = downloader.clone();
//! // The command returns the job id at once; the work runs on the pool and reports through job events.
//! let _handle = pool.spawn_job(ctx, move |ctx| Ok(d.download(ctx, &req)?));
//! # Ok(()) }
//! # fn main() {}
//! ```
//!
//! - **Phases** ([`phases`]): `connect` (items, 1), `resume` (bytes of the kept prefix, only when resuming), `download` (bytes;
//!   total from `Content-Range`/`Content-Length`, a growing count when the host does not say), `verify` (items, 1: the digest
//!   computed while writing is compared, then one rename). Each phase starts at 0 on its own scale and never goes backwards.
//! - **Segmented** when the host answers the `bytes=0-0` probe with 206 and the file is at least
//!   [`Config::segment_threshold`]: [`Config::connections`] connections (default 4) take ranges in file order; an idle
//!   connection takes over the far half of the lowest slow range. Writes are positioned; one in-order
//!   [`paddock_jobs::HashingWriter`] takes the bytes at its frontier, and ranges ahead of it wait in memory, at most
//!   [`Config::max_buffer`]. The file is never read back.
//! - **Per-host caps** ([`HostCaps`], default 4 per host across all downloads of the downloader) and **back-off**: 429 and 503
//!   lower the download's connections (down to one) and hold every request to that host for `Retry-After` (seconds or a date),
//!   at least [`Config::backoff`], at most [`Config::max_wait`]; failures retry with exponential back-off.
//! - **Resume**: `<name>.part.json` keeps the written and hashed prefix (saved about every second and on every stop); a later run
//!   of the same request keeps it when the size, the pinned checksum and the host's strong ETag (or Last-Modified) still match,
//!   hashes that prefix once and fetches the rest. Range requests carry `If-Range`, so a file that changed is refused.
//! - **No byte ranges**: one stream; a dropped connection is retried from the start and the bytes already written are skipped.
//! - **Bounded**: [`Config::max_bytes`] (or [`Request::max_bytes`]) is checked against the announced size before writing and
//!   against the bytes while streaming; a pinned [`Request::size`] must match.
//! - **Cancel** is noticed within [`paddock_jobs::CANCEL_LATENCY_TARGET_MS`] in every phase: the job's worker only supervises
//!   the transfer threads, so a cancel never waits for the network. The partial file stays for a resume
//!   ([`DownloadError::keeps_partial`]).
//!
//! - **Proxy**: `HttpTransport` uses the system's proxy by default (environment, then the operating system; [`ProxyMode`],
//!   [`ProxyRules`]), or an explicit one with a login that is never printed.
//! - **Small requests**: [`Downloader::get_json`] and friends ([`Fetch`]) use the same transport, proxy and host caps.
//! - **Threads, not async**: [`Downloader::download`] blocks the calling thread (a job worker) and runs the transfer on its own
//!   threads; there is no async runtime inside and none needed. From async code call it in `spawn_blocking`, or submit it
//!   as a job to the `JobPool`.
//!
//! The network is a port ([`Transport`]); `HttpTransport` (feature `http`, on by default) is ureq with rustls and the
//! operating system's certificate store. The core has no Tauri dependency; the adapter is `paddock_download::tauri` behind the
//! `tauri` feature.

mod config;
mod engine;
mod error;
mod fetch;
mod hosts;
#[cfg(feature = "http")]
mod http;
mod proxy;
mod proxy_os;
mod request;
mod resume;
mod transport;

#[cfg(feature = "tauri")]
pub mod tauri;

pub use config::{Config, MAX_CONNECTIONS};
pub use engine::{Downloaded, Downloader, FreeSpace, phases};
pub use error::{DownloadError, TransportError, TransportErrorKind, human_bytes};
pub use fetch::Fetch;
pub use hosts::{DEFAULT_HOST_CAP, HostCaps, MAX_HOST_CAP};
#[cfg(feature = "http")]
pub use http::{HttpConfig, HttpTransport, ProxyMode, ureq_agent, ureq_config};
pub use proxy::{
    MAX_NO_PROXY_ENTRIES, MAX_PROXY_TEXT, NoProxy, ProxyCredentials, ProxyRules, ProxyScheme,
    ProxyServer,
};
pub use request::{Integrity, MAX_HEADERS, MAX_URL_LEN, Request, Resumable, Sha256Digest, host_of};
pub use transport::{
    ByteRange, ContentRange, Get, Header, Headers, Response, Transport, parse_content_range,
    parse_length, parse_retry_after,
};
