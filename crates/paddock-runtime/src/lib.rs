//! Pinned runtimes: programs an app downloads on demand instead of bundling them (Chromium for the browser panel and page
//! rendering, FFmpeg for video, llama.cpp for local models). Nothing that is not in the app's catalogue is ever downloaded.
//!
//! * [`Catalogue`]: one TOML file per runtime, built into the app: the pinned version and, per platform, the archive's URL,
//!   size and SHA-256. Parsed once into typed values; anything wrong refuses the whole catalogue with a reason.
//! * [`RuntimeStore`]: the runtime's folder in the app data folder. [`RuntimeStore::install`] runs as one job with
//!   phases on their own scales: the download through `paddock-download` (hashed while written, resumable), `unpack`
//!   through `paddock-archive` (zip, tar, tar.gz, tar.xz; bytes of the expanded size), `self-test` and `install`. A release is used only after its self-test passed; the
//!   one in use stays until then and is kept afterwards for [`RuntimeStore::rollback`]. A release that fails its
//!   self-test, or that the app later finds broken ([`RuntimeStore::quarantine`]), is moved aside and never used again.
//! * [`RuntimeKind`]: what differs per runtime: where the executable is inside the archive and how to test it.
//!
//! ```text
//! <root>/                      e.g. AppDirs::data()/runtimes/chromium
//!   state.json                 the release in use, the one before it, the quarantine list (versioned)
//!   .lock                      held while an install runs (one per store, across processes)
//!   downloads/<archive>        the archive while it downloads (`.part`, resumable); removed after the install
//!   .staging-<version>/        a release being unpacked and tested; renamed to <version> only when it passed
//!   <version>/                 an installed release, with its `runtime.json` marker
//!   .quarantine/<version>-<n>/ releases that failed, newest few kept for a diagnostics report
//! ```

mod catalogue;
mod error;
mod platform;
mod store;

pub use catalogue::{
    ArchiveKind, Catalogue, FORMAT, MAX_ARCHIVE_BYTES, MAX_CATALOGUE_BYTES, MAX_PLATFORMS, Pinned,
    is_folder_name,
};
pub use error::RuntimeError;
pub use paddock_archive::Options as ArchiveOptions;
pub use platform::{Arch, OsKind, Platform};
pub use store::{
    DEFAULT_KEEP_QUARANTINED, Installed, MARKER, QuarantineRecord, RuntimeKind, RuntimeStore,
    STATE_FILE, Status,
};

/// Result of this crate.
pub type Result<T> = std::result::Result<T, RuntimeError>;
