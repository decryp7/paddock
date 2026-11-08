//! The two writing operations: append a log line, create a page. Both take the date as a parameter (the command line passes
//! the system clock's), refuse input that would produce a line the lint rejects, and never overwrite.

use std::io::Write;
use std::path::Path;

use crate::date::{self, Day};

/// The kinds a log line may have.
pub const KINDS: [&str; 3] = ["ingest", "lint", "fix"];
/// Longest subject, title, summary or source, in characters.
pub const MAX_TEXT: usize = 500;
/// Longest slug.
pub const MAX_SLUG: usize = 64;

/// Why an operation did not happen: `Refused` is a rule of the wiki, `Io` a file problem, `Input` a bad argument.
#[derive(Debug, PartialEq, Eq)]
pub enum OpError {
    /// A bad argument; says what to pass.
    Input(String),
    /// The wiki's state forbids it (the page exists).
    Refused(String),
    /// A file could not be read or written.
    Io(String),
}

fn one_line(what: &str, s: &str) -> Result<String, OpError> {
    let t = s.trim();
    if t.is_empty() {
        return Err(OpError::Input(format!("{what} must not be empty")));
    }
    if t.chars().count() > MAX_TEXT {
        return Err(OpError::Input(format!("{what} is longer than {MAX_TEXT} characters")));
    }
    if t.chars().any(|c| c.is_control()) {
        return Err(OpError::Input(format!("{what} must be one line without control characters")));
    }
    Ok(t.to_string())
}

fn append(path: &Path, text: &str) -> Result<(), OpError> {
    let io = |e: std::io::Error| OpError::Io(format!("cannot write {}: {e}", path.display()));
    let existing = std::fs::read_to_string(path).map_err(|e| OpError::Io(format!("cannot read {}: {e}", path.display())))?;
    let mut f = std::fs::OpenOptions::new().append(true).open(path).map_err(io)?;
    let mut chunk = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        chunk.push('\n');
    }
    chunk.push_str(text);
    f.write_all(chunk.as_bytes()).map_err(io)
}

/// Appends `## [today] kind | subject` to `<wiki>/log.md` and returns the line.
pub fn log(wiki: &Path, kind: &str, subject: &str, today: Day) -> Result<String, OpError> {
    if !KINDS.contains(&kind) {
        return Err(OpError::Input(format!(
            "the log kind must be one of {}, not `{kind}`",
            KINDS.join(", ")
        )));
    }
    let subject = one_line("the subject", subject)?;
    let line = format!("## [{}] {kind} | {subject}", date::format_date(today));
    append(&wiki.join("log.md"), &format!("{line}\n"))?;
    Ok(line)
}

/// What a new page is made of.
pub struct NewPage<'a> {
    /// File name without `.md`: lower case letters, digits and hyphens.
    pub slug: &'a str,
    /// The page title.
    pub title: &'a str,
    /// One line for the index; defaults to the title.
    pub summary: Option<&'a str>,
    /// The `sources:` entries (may be empty; the lint then asks for some).
    pub sources: &'a [String],
}

/// Is `slug` a safe page name: `[a-z0-9][a-z0-9-]*`, at most [`MAX_SLUG`] characters, not a reserved page.
pub fn check_slug(slug: &str) -> Result<(), OpError> {
    let ok = !slug.is_empty()
        && slug.len() <= MAX_SLUG
        && slug.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && slug.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !ok {
        return Err(OpError::Input(format!(
            "the slug `{slug}` must be lower-case letters, digits and hyphens (at most {MAX_SLUG}), starting with a letter or digit"
        )));
    }
    if matches!(slug, "readme" | "index" | "log") {
        return Err(OpError::Input(format!("`{slug}` is a reserved page name")));
    }
    Ok(())
}

/// Creates `<wiki>/<slug>.md` from the template and adds its line to `index.md`. Returns the file name.
pub fn new_page(wiki: &Path, page: &NewPage<'_>, today: Day) -> Result<String, OpError> {
    check_slug(page.slug)?;
    let title = one_line("the title", page.title)?;
    let summary = match page.summary {
        Some(s) => one_line("the summary", s)?,
        None => title.clone(),
    };
    let mut sources = Vec::new();
    for s in page.sources {
        let s = one_line("a source", s)?;
        if s.contains([',', '[', ']']) {
            return Err(OpError::Input(format!("the source `{s}` must not contain `,`, `[` or `]`")));
        }
        sources.push(s);
    }
    let name = format!("{}.md", page.slug);
    let path = wiki.join(&name);
    let index = wiki.join("index.md");
    if !index.is_file() {
        return Err(OpError::Io(format!(
            "{} does not exist; create the wiki's index.md first",
            index.display()
        )));
    }
    let day = date::format_date(today);
    let body = format!(
        "---\ntitle: {title}\nverified: {day}\nsources: [{}]\n---\nSummary in three to five lines.\n\n## Current state\n- TODO: one claim, with its source in the same bullet, src: <path or id>\n\n## Open questions\n\n## See also\n",
        sources.join(", ")
    );
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            OpError::Refused(format!("{} already exists; edit it instead", path.display()))
        } else {
            OpError::Io(format!("cannot create {}: {e}", path.display()))
        }
    })?;
    file.write_all(body.as_bytes())
        .map_err(|e| OpError::Io(format!("cannot write {}: {e}", path.display())))?;
    append(&index, &format!("- [{title}]({name}): {summary}. Verified {day}.\n"))?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert!(check_slug("ci-and-runners").is_ok());
        for bad in ["", "-x", "Up", "a b", "a/b", "../x", "log", "index", "readme", "é"] {
            assert!(check_slug(bad).is_err(), "{bad}");
        }
        assert!(check_slug(&"a".repeat(65)).is_err());
    }

    #[test]
    fn one_line_rules() {
        assert!(one_line("x", "a\nb").is_err());
        assert!(one_line("x", "  ").is_err());
        assert_eq!(one_line("x", " a | b ").as_deref(), Ok("a | b"));
    }
}
