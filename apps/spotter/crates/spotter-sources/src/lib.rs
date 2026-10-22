//! Spotter's source adapters: the I/O behind the core's [`spotter_core::source::ReleaseSource`] port.
//!
//! - [`folder`]: folders, mounted drives and UNC shares (with a deadline, and statuses for unreachable shares).
//! - [`http`]: HTTP with bounded bodies and the registry token dance.
//! - [`download`]: downloads and copies with resume and checksum verification.
//!
//! [`checker`] builds a port implementation for every plan.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod download;
pub mod folder;
pub mod http;

/// Port implementations for the core's plans.
pub mod checker {
    use crate::http::{Http, MAX_BODY};
    use spotter_core::release::Candidate;
    use spotter_core::source::{BoxFuture, FolderSource, HttpRequest, Parser, Plan, ReleaseSource, SourceError};

    /// Lists a folder source.
    pub struct FolderChecker(pub FolderSource);

    impl ReleaseSource for FolderChecker {
        fn list(&self, limit: usize) -> BoxFuture<'_, Result<Vec<Candidate>, SourceError>> {
            Box::pin(async move {
                let entries = crate::folder::list(&self.0.path, crate::folder::LIST_TIMEOUT).await?;
                let mut c = self.0.candidates(entries);
                c.truncate(limit);
                Ok(c)
            })
        }
    }

    /// Performs a request and parses its answer.
    pub struct HttpChecker {
        /// The client.
        pub http: Http,
        /// The request.
        pub request: HttpRequest,
        /// How to read it.
        pub parser: Parser,
        /// Registry credentials (user, token).
        pub credentials: Option<(String, String)>,
    }

    impl ReleaseSource for HttpChecker {
        fn list(&self, limit: usize) -> BoxFuture<'_, Result<Vec<Candidate>, SourceError>> {
            Box::pin(async move {
                let body = self.http.get(&self.request, self.credentials.as_ref(), MAX_BODY).await?;
                let mut c = self.parser.parse(&body)?;
                c.truncate(limit);
                Ok(c)
            })
        }
    }

    /// Stands in for plans that need the page renderer, which only the engine has: it explains instead of guessing.
    pub struct NeedsRenderer;

    impl ReleaseSource for NeedsRenderer {
        fn list(&self, _limit: usize) -> BoxFuture<'_, Result<Vec<Candidate>, SourceError>> {
            Box::pin(async {
                Err(SourceError::Failed(
                    "This source shows its versions with JavaScript and needs a browser; the engine renders it.".into(),
                ))
            })
        }
    }

    /// The checker for a plan.
    pub fn for_plan(plan: Plan, http: &Http, credentials: Option<(String, String)>) -> Box<dyn ReleaseSource> {
        match plan {
            Plan::Render { .. } => Box::new(NeedsRenderer),
            Plan::Folder(f) => Box::new(FolderChecker(f)),
            Plan::Http { request, parser } => Box::new(HttpChecker {
                http: http.clone(),
                request,
                parser,
                credentials,
            }),
        }
    }
}
