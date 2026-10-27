//! The port for pages that need JavaScript (ADR 0002). The engine calls [`PageRenderer`] for "rendered" web-page
//! watches; `spotter-render` implements it with a supervised Chromium, tests use a fake.
use crate::source::BoxFuture;
use std::time::Duration;

/// A rendered page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPage {
    /// The DOM serialised after the page settled.
    pub html: String,
    /// The address after redirects.
    pub final_url: String,
}

/// Why a page could not be rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
    /// The page wants a login, a captcha or a consent dialog; the user has to deal with it (never solved here).
    NeedsYou(String),
    /// No browser is available; the hint says what to install or enable.
    Unavailable(String),
    /// Anything else (timeout, crash, navigation error).
    Failed(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::NeedsYou(m) | RenderError::Unavailable(m) | RenderError::Failed(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for RenderError {}

/// Renders a page in a browser.
///
/// Contract: never panics; stops after `timeout` and answers [`RenderError::Failed`]; reports a challenge as
/// [`RenderError::NeedsYou`] and a missing browser as [`RenderError::Unavailable`].
pub trait PageRenderer: Send + Sync {
    /// Loads `url`, waits until `wait_selector` (when given) is present and the page is quiet, and returns the DOM.
    fn render<'a>(
        &'a self,
        url: &'a str,
        wait_selector: Option<&'a str>,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<RenderedPage, RenderError>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_display_their_message() {
        for e in [
            RenderError::NeedsYou("n".into()),
            RenderError::Unavailable("u".into()),
            RenderError::Failed("f".into()),
        ] {
            assert_eq!(e.to_string().len(), 1);
        }
    }
}
