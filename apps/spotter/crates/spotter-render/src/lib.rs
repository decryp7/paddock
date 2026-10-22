//! Spotter's renderer: a small, self-contained Chromium driver (ADR 0002) behind `spotter_core::render::PageRenderer`.
//!
//! The driver modules ([`cdp`], [`launch`], [`page`], [`process`], [`discover`], [`ready`], [`challenge`],
//! [`install`], [`manifest`]) know nothing of Spotter and will move to the shared paddock repo; only [`adapter`]
//! (and [`status`], which mirrors the UI's `RendererStatus`) are Spotter specific.

// The browser is a GUI program started by the supervisor; `taskkill`/`kill` are short helpers. The driver is meant to
// be copied between apps, so it uses std's `Command` directly instead of the app's quiet-command helper.
#![allow(clippy::disallowed_methods)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod adapter;
pub mod cdp;
pub mod challenge;
pub mod discover;
pub mod install;
pub mod launch;
pub mod manifest;
pub mod page;
pub mod process;
pub mod ready;
pub mod status;

pub use adapter::ChromiumRenderer;
pub use spotter_core::render::{PageRenderer, RenderError, RenderedPage};
pub use status::{renderer_status, RendererKind, RendererStatus};

/// Everything that can go wrong in the driver; messages tell the user what to do.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No browser installed.
    #[error("no Chromium-based browser was found; install Edge, Chrome or Chromium, or download one in Settings")]
    NotFound,
    /// The process could not start.
    #[error("the browser could not be started: {0}")]
    Launch(String),
    /// A step took too long.
    #[error("the browser did not answer in time ({0})")]
    Timeout(&'static str),
    /// The connection broke.
    #[error("the browser connection failed: {0}")]
    Connection(String),
    /// The browser rejected a command.
    #[error("the browser reported an error: {0}")]
    Protocol(String),
    /// The browser ended (crash, timeout or memory cap).
    #[error("the browser was closed")]
    Closed,
    /// Download or install problem.
    #[error("{0}")]
    Install(String),
}

/// Driver result.
pub type Result<T> = std::result::Result<T, Error>;
