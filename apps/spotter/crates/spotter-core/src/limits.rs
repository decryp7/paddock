//! Bounds on everything Spotter reads from outside. Every unbounded input gets a limit here.

/// Releases kept in a watch's history.
pub const MAX_HISTORY: usize = 200;
/// Candidates looked at in one check (directory entries, feed items, tags); the rest are ignored.
pub const MAX_CANDIDATES: usize = 5000;
/// Largest HTTP response body read (8 MiB).
pub const MAX_BODY_BYTES: usize = 8 << 20;
/// Largest checksum file read (64 KiB).
pub const MAX_CHECKSUM_FILE: usize = 64 << 10;
/// Most watches in one configuration.
pub const MAX_WATCHES: usize = 500;
/// Longest watch name, in characters.
pub const MAX_NAME_CHARS: usize = 120;
/// Largest configuration file read (2 MiB).
pub const MAX_CONFIG_BYTES: usize = 2 << 20;
/// Largest state file read (32 MiB).
pub const MAX_STATE_BYTES: usize = 32 << 20;
/// Longest user pattern (regex or glob), in bytes.
pub const MAX_PATTERN_LEN: usize = 1000;
/// Most recent checks remembered per watch.
pub const MAX_CHECK_LOG: usize = 50;
/// Most actions per watch.
pub const MAX_ACTIONS: usize = 8;
/// Most arguments of a "run a command" action.
pub const MAX_COMMAND_ARGS: usize = 32;
/// Shortest check interval (10 seconds).
pub const MIN_INTERVAL_SECS: u32 = 10;
/// Longest check interval (7 days).
pub const MAX_INTERVAL_SECS: u32 = 7 * 24 * 3600;
/// Longest wait after repeated failures (6 hours), unless the interval itself is longer.
pub const MAX_BACKOFF_SECS: u32 = 6 * 3600;
