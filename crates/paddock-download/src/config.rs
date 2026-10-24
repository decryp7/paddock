//! The tunables of a [`Downloader`](crate::Downloader). Apps expose the ones users care about (connections, size limit) as Settings.

use std::time::Duration;

/// Most connections one download may use, whatever the setting says.
pub const MAX_CONNECTIONS: usize = 16;

/// How downloads are run. `Config::default()` suits large files from CDNs; tests shorten the waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Connections one download uses when the host serves byte ranges (1..=[`MAX_CONNECTIONS`]; the host cap still applies).
    pub connections: usize,
    /// Files smaller than this use one connection.
    pub segment_threshold: u64,
    /// Smallest range one request is given, and the smallest worth splitting off a slow connection.
    pub min_block: u64,
    /// Largest range one request is given.
    pub max_block: u64,
    /// Bytes downloaded ahead of the in-order hash that may wait in memory (the hash never re-reads the file, so ranges ahead of
    /// it are held until it catches up). Bounds memory: this plus one 64 KiB buffer per connection.
    pub max_buffer: u64,
    /// Default size limit of a file (a [`Request::max_bytes`](crate::Request::max_bytes) overrides it).
    pub max_bytes: u64,
    /// Failed attempts in a row (without new bytes in between) before a range or the stream gives up.
    pub attempts: u32,
    /// First wait after a failure; doubles per failure up to `max_wait`. Also the shortest wait after a 429/503.
    pub backoff: Duration,
    /// Longest wait after a failure or for a `Retry-After`.
    pub max_wait: Duration,
    /// Throttled answers (429/503) on the last remaining connection before the download gives up.
    pub max_throttles: u32,
    /// No byte for this long (outside a deliberate wait) ends the download as stalled; the partial file is kept.
    pub stall_timeout: Duration,
    /// Longest time one range request may take to deliver its body.
    pub range_timeout: Duration,
    /// How often the resume state is saved while downloading.
    pub save_every: Duration,
    /// Free space that must be left on the destination's disk after the download; a download that would leave less is
    /// refused before anything is written ([`DownloadError::NotEnoughSpace`](crate::DownloadError::NotEnoughSpace)).
    pub disk_reserve: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            connections: 4,
            segment_threshold: 8 * 1024 * 1024,
            min_block: 256 * 1024,
            max_block: 8 * 1024 * 1024,
            max_buffer: 64 * 1024 * 1024,
            max_bytes: 64 * 1024 * 1024 * 1024,
            attempts: 5,
            backoff: Duration::from_millis(500),
            max_wait: Duration::from_secs(60),
            max_throttles: 24,
            stall_timeout: Duration::from_secs(120),
            range_timeout: Duration::from_secs(600),
            save_every: Duration::from_secs(1),
            disk_reserve: 64 * 1024 * 1024,
        }
    }
}

impl Config {
    /// The wait after `failures` failures in a row (1 = the first).
    pub(crate) fn backoff_for(&self, failures: u32) -> Duration {
        self.backoff
            .saturating_mul(2u32.saturating_pow(failures.saturating_sub(1)))
            .min(self.max_wait)
    }

    /// The wait after a 429/503: the server's `Retry-After`, at least `backoff`, at most `max_wait`.
    pub(crate) fn throttle_wait(&self, retry_after: Option<Duration>) -> Duration {
        retry_after
            .unwrap_or(self.backoff)
            .max(self.backoff)
            .min(self.max_wait)
    }

    /// Range size for `remaining` bytes over `connections`: about eight ranges per connection, within the block limits and so that
    /// every connection fits in the buffer.
    pub(crate) fn block_size(&self, remaining: u64, connections: usize) -> u64 {
        let n = connections.max(1) as u64;
        let fit = (self.max_buffer / n).max(1);
        (remaining / (n * 8))
            .clamp(
                self.min_block.max(1),
                self.max_block.max(self.min_block.max(1)),
            )
            .min(fit)
    }
}
