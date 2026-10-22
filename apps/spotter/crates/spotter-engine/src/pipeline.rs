//! Getting candidates for one watch: plan -> fetch (a folder, a polite HTTP request, or the page renderer) -> parse.
//! Selecting, soaking and comparing with the watch's state is the engine's job; this module only reads the source.
use crate::fetch::{FetchOptions, Fetched, PoliteFetcher, Validators};
use serde::{Deserialize, Serialize};
use spotter_core::limits::MAX_CANDIDATES;
use spotter_core::release::{Candidate, LatestBy};
use spotter_core::render::{PageRenderer, RenderError};
use spotter_core::source::folder::{ScanStats, Snapshot};
use spotter_core::source::web_page::{change_summary, PageMode, WebPageSource};
use spotter_core::source::{Plan, Secrets, Source, SourceError};
use spotter_core::time::Timestamp;
use spotter_core::watch::Watch;
use spotter_sources::folder::{Readable, ScanRequest};
use std::sync::Arc;
use std::time::Duration;

/// What a page watched for changes looked like when it last changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageBaseline {
    /// Hex SHA-256 of the noise-filtered text.
    pub hash: String,
    /// When this content was first seen.
    pub since: Timestamp,
    /// The noise-filtered text lines (bounded).
    pub lines: Vec<String>,
}

/// What reading a source produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Gathered {
    /// The server said nothing changed (HTTP 304).
    Unchanged,
    /// Candidates, with what to remember for the next check.
    Fresh {
        /// What the source has now.
        candidates: Vec<Candidate>,
        /// Validators for a conditional request next time.
        validators: Validators,
        /// The page baseline for change watching, new or updated.
        baseline: Option<PageBaseline>,
        /// What a folder scan remembers for the next check, and how the scan went.
        folder: Option<(Snapshot, ScanStats)>,
    },
}

/// Reads sources.
pub struct Checker {
    /// The HTTP fetcher.
    pub fetcher: Arc<PoliteFetcher>,
    /// The renderer for JavaScript pages, when one exists.
    pub renderer: Option<Arc<dyn PageRenderer>>,
    /// How long a page may take to render.
    pub render_timeout: Duration,
}

fn render_error(e: RenderError) -> SourceError {
    match e {
        RenderError::NeedsYou(m) => SourceError::NeedsYou(m),
        RenderError::Unavailable(m) | RenderError::Failed(m) => SourceError::Failed(m),
    }
}

impl Checker {
    /// Reads the watch's source. `conditional` are the validators of the last fetch (for HTTP sources), `baseline`
    /// the remembered page for change watching.
    pub async fn gather(
        &self,
        watch: &Watch,
        secrets: &Secrets,
        conditional: Option<Validators>,
        baseline: Option<&PageBaseline>,
        folder: Option<&Snapshot>,
        now: Timestamp,
    ) -> Result<Gathered, SourceError> {
        let page = match &watch.source {
            Source::WebPage(p) => Some(p),
            _ => None,
        };
        let (body, validators) = match watch.source.plan(secrets) {
            Plan::Folder(f) => {
                let scan = spotter_sources::folder::scan(ScanRequest {
                    source: f,
                    readable: readable_for(watch),
                    previous: folder.cloned(),
                    sample_ignored: 0,
                    settle: true,
                })
                .await?;
                return Ok(Gathered::Fresh {
                    candidates: scan.candidates,
                    validators: Validators::default(),
                    baseline: None,
                    folder: Some((scan.snapshot, scan.stats)),
                });
            }
            Plan::Http { request, parser } => {
                let opts = FetchOptions {
                    allow_local: watch.allow_local,
                    conditional: conditional.filter(|c| !c.is_empty()),
                    credentials: watch.source.registry_credentials(secrets),
                };
                match self.fetcher.get(&request, &opts).await? {
                    Fetched::NotModified => return Ok(Gathered::Unchanged),
                    Fetched::Body { bytes, validators, .. } => (bytes, (validators, parser)),
                }
            }
            Plan::Render {
                url,
                wait_selector,
                parser,
            } => {
                let Some(renderer) = &self.renderer else {
                    return Err(SourceError::Failed(
                        "This watch needs a browser to show the page, and none is available. Open Settings to install or enable one."
                            .into(),
                    ));
                };
                let rendered = renderer
                    .render(&url, wait_selector.as_deref(), self.render_timeout)
                    .await
                    .map_err(render_error)?;
                (rendered.html.into_bytes(), (Validators::default(), parser))
            }
        };
        let (validators, parser) = validators;
        let (candidates, baseline) = match page {
            Some(p) if p.mode == PageMode::Change => {
                let (c, b) = change_candidate(p, &body, baseline, now)?;
                (vec![c], Some(b))
            }
            _ => (parser.parse(&body)?, None),
        };
        Ok(Gathered::Fresh {
            candidates: candidates.into_iter().take(MAX_CANDIDATES).collect(),
            validators,
            baseline,
            folder: None,
        })
    }
}

/// A pure check of whether the watch's scheme can read a name, so names that could never become a release are
/// dropped before their size and times are asked for. Only when names must hold a version (the default "latest by"),
/// or when the pattern itself says what to read.
pub fn readable_for(watch: &Watch) -> Option<Readable> {
    let scheme = watch.scheme.clone();
    (watch.filter.latest_by == LatestBy::Version || scheme.custom_regex().is_some())
        .then(|| Arc::new(move |text: &str| scheme.extract_full(text).is_ok()) as Readable)
}

/// Identifies the settings a folder snapshot is valid for.
pub fn snapshot_fingerprint(watch: &Watch) -> String {
    format!(
        "{:?}|{}|{:?}|{:?}",
        watch.spec.source, watch.spec.versions, watch.spec.version_pattern, watch.filter.latest_by
    )
}

/// The candidate of a page watched for changes: dated by when its current (noise-filtered) content first appeared.
fn change_candidate(
    page: &WebPageSource,
    body: &[u8],
    previous: Option<&PageBaseline>,
    now: Timestamp,
) -> Result<(Candidate, PageBaseline), SourceError> {
    let ex = page.extract(&String::from_utf8_lossy(body))?;
    let hash = ex.hash.to_hex();
    let (baseline, changed_from) = match previous {
        Some(p) if p.hash == hash => (p.clone(), None),
        Some(p) => (
            PageBaseline {
                hash,
                since: now,
                lines: ex.lines.clone(),
            },
            Some(p.lines.clone()),
        ),
        None => (
            PageBaseline {
                hash,
                since: now,
                lines: ex.lines.clone(),
            },
            None,
        ),
    };
    let mut c = page.candidate(&ex, baseline.since);
    if let Some(old) = changed_from {
        c.name = change_summary(&old, &baseline.lines).chars().take(300).collect();
    } else if let Some(p) = previous {
        // Unchanged content keeps its original name through the history; the name is not part of the identity.
        c.name = format!("Page changed {}", &spotter_core::source::web_page::change_version(p.since)[..10]);
    }
    let mut baseline = baseline;
    baseline.lines.truncate(spotter_core::source::web_page::MAX_LINES);
    Ok((c, baseline))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> WebPageSource {
        WebPageSource::from_config(
            &spotter_core::schema::Config::new()
                .with("url", "https://a.b/")
                .with("mode", "change"),
        )
        .unwrap()
    }

    #[test]
    fn change_candidates_track_when_the_content_appeared() {
        let t1 = Timestamp(1_000_000_000_000);
        let t2 = Timestamp(t1.0 + 86_400_000);
        let (c1, b1) = change_candidate(&page(), b"<p>one</p><p>two</p>", None, t1).unwrap();
        assert_eq!(b1.since, t1);
        let (c2, b2) = change_candidate(&page(), b"<p>one</p><p>two</p><p>1,000 views</p>", Some(&b1), t2).unwrap();
        assert_eq!(
            (b2.since, c2.version_text.clone()),
            (t1, c1.version_text.clone()),
            "noise is not a change"
        );
        let (c3, b3) = change_candidate(&page(), b"<p>one</p><p>three</p>", Some(&b2), t2).unwrap();
        assert_eq!(b3.since, t2);
        assert_ne!(c3.version_text, c1.version_text);
        assert!(
            c3.name.contains("added 1 line(s)") && c3.name.contains("removed 1 line(s)"),
            "{}",
            c3.name
        );
        assert!(change_candidate(&page(), b"<p>x</p>", None, t1).is_ok());
    }

    #[test]
    fn render_errors_map_to_source_errors() {
        assert!(matches!(render_error(RenderError::NeedsYou("n".into())), SourceError::NeedsYou(_)));
        assert!(matches!(render_error(RenderError::Unavailable("u".into())), SourceError::Failed(_)));
        assert!(matches!(render_error(RenderError::Failed("f".into())), SourceError::Failed(_)));
    }
}
