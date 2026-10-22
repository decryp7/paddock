//! The renderer port. It has exactly the shape of `spotter_core::render::PageRenderer` so the core's definition can
//! replace this one: only [`crate::adapter`] implements it and nothing else in this crate refers to it.

use std::time::Duration;

/// A page after JavaScript ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPage {
    /// The document's `outerHTML`.
    pub html: String,
    /// The address after redirects.
    pub final_url: String,
}

/// Why a page could not be rendered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// The page wants a person (login, captcha, consent). Never solved automatically.
    #[error("needs you: {0}")]
    NeedsYou(String),
    /// No renderer can run here; the text says what to install or enable.
    #[error("renderer unavailable: {0}")]
    Unavailable(String),
    /// Anything else (timeout, crash, protocol error).
    #[error("{0}")]
    Failed(String),
}

/// Renders a URL with JavaScript.
#[allow(async_fn_in_trait)]
pub trait PageRenderer {
    /// Loads `url`, waits until it is ready (load finished, `wait_selector` present when given, DOM quiet) or
    /// `timeout` has passed, and returns the document.
    async fn render(&self, url: &str, wait_selector: Option<&str>, timeout: Duration) -> Result<RenderedPage, RenderError>;
}
