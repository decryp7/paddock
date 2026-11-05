//! The log line of one check (ER-27): what a user's diagnostic bundle needs to explain a watch that does not find
//! releases, without the user's content. The line names the watch by its id and the kind of its source, never its name,
//! address or the fetched text; a failure says what class of failure it was, the reason Spotter gave, and the next step.
//! The text is built here, pure, so it is tested without a logger; `check.rs` only emits it.

use spotter_core::source::{Source, SourceError};

/// What one finished check looked like.
pub(crate) struct Finished<'a> {
    /// The watch's id.
    pub watch_id: &'a str,
    /// The kind of its source.
    pub source: &'a Source,
    /// How long reading the source took, milliseconds.
    pub took_ms: u64,
    /// How many releases the check found that were new.
    pub new_releases: usize,
    /// Why it failed, when it did.
    pub error: Option<&'a SourceError>,
}

/// The longest reason that goes into the log (characters).
const MAX_REASON: usize = 200;

/// The kind of a source as the log names it.
pub(crate) fn source_kind(source: &Source) -> &'static str {
    match source {
        Source::Folder(_) => "folder",
        Source::WebDirectory(_) => "web-directory",
        Source::Feed { .. } => "feed",
        Source::Forge(_) => "forge",
        Source::Container(_) => "container",
        Source::JsonApi(_) => "json-api",
        Source::WebPage(_) => "web-page",
    }
}

/// The class of a failure and what the user can do about it.
fn failure(error: &SourceError) -> (&'static str, &'static str) {
    match error {
        SourceError::Unreachable(_) => (
            "unreachable",
            "check the network or the share; Spotter retries with a growing pause",
        ),
        SourceError::Unauthorized(_) => ("unauthorized", "update the credentials in the watch"),
        SourceError::NotFound(_) => ("not-found", "check the address of the watch"),
        SourceError::InvalidConfig(_) => ("invalid-config", "open the watch and fix the setting it names"),
        SourceError::InvalidResponse(_) => (
            "invalid-response",
            "the place answered something Spotter cannot read; check the selector",
        ),
        SourceError::NeedsYou(_) => ("needs-you", "open the page once and sign in or solve the check, then check again"),
        SourceError::Failed(_) => ("failed", "check again; if it repeats, report a problem"),
    }
}

/// One line without control characters, cut to [`MAX_REASON`].
fn one_line(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(MAX_REASON).collect()
}

/// The level and text of the line for a finished check: info for a success, warn for a failure.
pub(crate) fn line(done: &Finished<'_>) -> (log::Level, String) {
    let kind = source_kind(done.source);
    match done.error {
        None => (
            log::Level::Info,
            format!(
                "check finished watch={} source={kind} outcome=ok new={} took_ms={}",
                done.watch_id, done.new_releases, done.took_ms
            ),
        ),
        Some(e) => {
            let (class, next) = failure(e);
            (
                log::Level::Warn,
                format!(
                    "check finished watch={} source={kind} outcome=failed class={class} took_ms={} reason=\"{}\" next=\"{next}\"",
                    done.watch_id,
                    done.took_ms,
                    one_line(e.message())
                ),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed() -> Source {
        Source::Feed {
            url: "https://example.org/feeds/secret-token/atom".into(),
        }
    }

    #[test]
    fn a_success_names_the_watch_the_kind_the_count_and_the_time() {
        let source = feed();
        let (level, text) = line(&Finished {
            watch_id: "w1",
            source: &source,
            took_ms: 420,
            new_releases: 2,
            error: None,
        });
        assert_eq!(level, log::Level::Info);
        assert_eq!(text, "check finished watch=w1 source=feed outcome=ok new=2 took_ms=420");
    }

    #[test]
    fn a_failure_names_its_class_its_reason_and_the_next_step() {
        let source = feed();
        let error = SourceError::Unauthorized("the server refused the token (HTTP 401)".into());
        let (level, text) = line(&Finished {
            watch_id: "w1",
            source: &source,
            took_ms: 90,
            new_releases: 0,
            error: Some(&error),
        });
        assert_eq!(level, log::Level::Warn);
        assert!(text.contains("outcome=failed class=unauthorized took_ms=90"), "{text}");
        assert!(text.contains("reason=\"the server refused the token (HTTP 401)\""), "{text}");
        assert!(text.contains("next=\"update the credentials in the watch\""), "{text}");
    }

    #[test]
    fn the_line_never_holds_the_address_or_a_control_character_and_is_bounded() {
        let source = feed();
        let error = SourceError::Failed(format!("line one\nline two {}", "x".repeat(1000)));
        let (_, text) = line(&Finished {
            watch_id: "w1",
            source: &source,
            took_ms: 1,
            new_releases: 0,
            error: Some(&error),
        });
        assert!(!text.contains("secret-token") && !text.contains("example.org"), "{text}");
        assert!(!text.contains('\n'), "{text}");
        assert!(text.len() < 400, "{} bytes", text.len());
    }

    #[test]
    fn every_failure_class_has_its_own_name_and_a_next_step() {
        let all = [
            SourceError::Unreachable(String::new()),
            SourceError::Unauthorized(String::new()),
            SourceError::NotFound(String::new()),
            SourceError::InvalidConfig(String::new()),
            SourceError::InvalidResponse(String::new()),
            SourceError::NeedsYou(String::new()),
            SourceError::Failed(String::new()),
        ];
        let classes: std::collections::BTreeSet<_> = all.iter().map(|e| failure(e).0).collect();
        assert_eq!(classes.len(), all.len());
        assert!(all.iter().all(|e| !failure(e).1.is_empty()));
    }
}
