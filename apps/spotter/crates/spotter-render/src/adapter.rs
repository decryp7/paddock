//! The Spotter-specific part: [`ChromiumRenderer`] implements [`PageRenderer`] on top of the driver modules.

use crate::discover::{self, Browser};
use crate::launch::{self, Chromium, EndedBy, LaunchOptions, Mode};
use crate::page::Outcome;
use crate::ready::{Readiness, Rule};
use crate::Error;
use spotter_core::render::{PageRenderer, RenderError, RenderedPage};
use spotter_core::source::BoxFuture;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Renders pages in a supervised Chromium: one browser and one throwaway profile per render.
#[derive(Debug, Clone)]
pub struct ChromiumRenderer {
    data_dir: PathBuf,
    profile_base: PathBuf,
    mode: Mode,
    memory_cap: Option<u64>,
    browser: Option<Browser>,
    extra_args: Vec<String>,
}

impl ChromiumRenderer {
    /// `data_dir` is the app data folder (a downloaded Chromium lives there); throwaway profiles go under
    /// `profile_base` (a temp or cache folder owned by the app).
    pub fn new(data_dir: impl Into<PathBuf>, profile_base: impl Into<PathBuf>) -> ChromiumRenderer {
        ChromiumRenderer {
            data_dir: data_dir.into(),
            profile_base: profile_base.into(),
            mode: Mode::OffScreen,
            memory_cap: Some(2 << 30),
            browser: None,
            extra_args: Vec::new(),
        }
    }

    /// Uses this executable instead of looking one up.
    pub fn with_browser(mut self, browser: Browser) -> Self {
        self.browser = Some(browser);
        self
    }

    /// Extra browser flags (none by default; tests use this for a root container, see `discover::test_browser_flags`).
    pub fn with_extra_args(mut self, args: Vec<String>) -> Self {
        self.extra_args = args;
        self
    }

    /// Window behaviour (default off screen).
    pub fn with_mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    /// The memory cap for the browser tree (default 2 GiB; `None` for no cap).
    pub fn with_memory_cap(mut self, cap: Option<u64>) -> Self {
        self.memory_cap = cap;
        self
    }

    fn resolve(&self) -> Option<Browser> {
        self.browser
            .clone()
            .or_else(discover::find_installed)
            .or_else(|| discover::find_downloaded(&self.data_dir))
    }
}

/// Only web addresses are rendered.
fn check_url(url: &str) -> Result<(), RenderError> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(())
    } else {
        Err(RenderError::Failed("only http and https addresses can be rendered".into()))
    }
}

fn blocking_render(
    exe: Browser,
    opts: LaunchOptions,
    url: String,
    selector: Option<String>,
    timeout: Duration,
) -> Result<RenderedPage, RenderError> {
    let failed = |e: Error| RenderError::Failed(e.to_string());
    let browser = Chromium::launch(&exe, &opts).map_err(failed)?;
    // The page's budget starts when the browser is up: a cold start (first run, antivirus scan, a loaded runner) has
    // its own limit and must not eat the time the page was promised. The watchdog still caps the whole render.
    let started = Instant::now();
    let left = |reason: &'static str| {
        timeout
            .checked_sub(started.elapsed())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| RenderError::Failed(format!("the page did not finish in time ({reason})")))
    };
    let result = (|| {
        let page = browser.new_page().map_err(failed)?;
        match page.navigate(&url, left("loading")?) {
            Ok(()) => {}
            Err(Error::Timeout(_)) => return Err(RenderError::Failed("the page did not finish loading in time".into())),
            Err(e) => return Err(failed(e)),
        }
        match page
            .wait_ready(selector.as_deref(), &Rule::default(), left("waiting")?)
            .map_err(failed)?
        {
            Outcome::Ready => {}
            Outcome::Challenge(c) => return Err(RenderError::NeedsYou(c.reason().into())),
            Outcome::TimedOut(Readiness::WaitingForElement) => {
                return Err(RenderError::Failed(format!(
                    "the element {} did not appear in time",
                    selector.as_deref().unwrap_or("")
                )))
            }
            Outcome::TimedOut(_) => return Err(RenderError::Failed("the page kept changing and never settled in time".into())),
        }
        let (html, final_url) = page.capture().map_err(failed)?;
        Ok(RenderedPage { html, final_url })
    })();
    match (&result, browser.ended_by()) {
        (Err(_), EndedBy::MemoryCap) => Err(RenderError::Failed("the page used too much memory and was stopped".into())),
        (Err(_), EndedBy::TimeLimit) => Err(RenderError::Failed("the page took too long and was stopped".into())),
        _ => result,
    }
}

impl ChromiumRenderer {
    async fn render_page(&self, url: &str, wait_selector: Option<&str>, timeout: Duration) -> Result<RenderedPage, RenderError> {
        check_url(url)?;
        let Some(exe) = self.resolve() else {
            return Err(RenderError::Unavailable(Error::NotFound.to_string()));
        };
        let mut opts = LaunchOptions::new(&self.profile_base);
        opts.mode = self.mode;
        opts.extra_args = self.extra_args.clone();
        opts.memory_cap = self.memory_cap;
        // The watchdog is the backstop behind the page's own deadline.
        opts.max_lifetime = launch::lifetime_for(timeout);
        let (url, sel) = (url.to_string(), wait_selector.map(str::to_string));
        tokio::task::spawn_blocking(move || blocking_render(exe, opts, url, sel, timeout))
            .await
            .map_err(|e| RenderError::Failed(format!("the renderer stopped unexpectedly: {e}")))?
    }
}

impl PageRenderer for ChromiumRenderer {
    fn render<'a>(
        &'a self,
        url: &'a str,
        wait_selector: Option<&'a str>,
        timeout: Duration,
    ) -> BoxFuture<'a, Result<RenderedPage, RenderError>> {
        Box::pin(self.render_page(url, wait_selector, timeout))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_addresses() {
        assert!(check_url("https://a.test/x").is_ok());
        assert!(check_url("http://a.test").is_ok());
        assert!(matches!(check_url("file:///etc/passwd"), Err(RenderError::Failed(_))));
        assert!(matches!(check_url("javascript:alert(1)"), Err(RenderError::Failed(_))));
    }

    #[tokio::test]
    async fn no_browser_is_unavailable_with_a_hint() {
        let d = tempfile::tempdir().unwrap();
        // An explicit executable that is missing still launches (and fails); with none anywhere, Unavailable.
        let r = ChromiumRenderer::new(d.path(), d.path());
        if r.resolve().is_none() {
            let e = r.render("https://a.test/", None, Duration::from_secs(1)).await.unwrap_err();
            assert!(matches!(e, RenderError::Unavailable(h) if h.contains("install")));
        }
    }

    #[tokio::test]
    async fn a_missing_executable_fails_cleanly() {
        let d = tempfile::tempdir().unwrap();
        let r = ChromiumRenderer::new(d.path(), d.path()).with_browser(Browser {
            flavor: discover::Flavor::Chromium,
            path: "/nonexistent/chrome".into(),
        });
        let e = r.render("http://127.0.0.1:9/", None, Duration::from_secs(2)).await.unwrap_err();
        assert!(matches!(e, RenderError::Failed(_)));
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 0);
    }
}
