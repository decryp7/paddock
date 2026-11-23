//! Content detection, an aid and never a selector (ADR 0117 D14): the kind of a file from its first kilobyte, as bat and ripgrep
//! tell text from binary (`content_inspector`). [`super::reach`] uses it only under a doc owner (a binary file there is a
//! documentation asset without being listed; a text file with an image name is not) and to name the kind of an unowned file in
//! the owner check's error. Inside a project folder every non-prose file stays code whatever it holds.

/// What a file's first kilobyte holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Detected {
    /// Not text (an image, an archive, any binary asset).
    Binary,
    /// A JSON document.
    Json,
    /// A TOML document.
    Toml,
    /// Other text.
    Text,
}

impl Detected {
    /// The word the owner check prints.
    pub fn name(self) -> &'static str {
        match self {
            Detected::Binary => "binary",
            Detected::Json => "JSON",
            Detected::Toml => "TOML",
            Detected::Text => "text",
        }
    }
}

/// The detector [`super::Facts`] carries: a path's kind, `None` when it cannot be read.
pub type DetectOf<'a> = &'a dyn Fn(&str) -> Option<Detected>;

/// How many bytes are inspected (bat and ripgrep look at the first block too).
pub const PEEK: usize = 1024;

/// The kind of `bytes` (only the first [`PEEK`] are read).
#[cfg(feature = "content")]
pub fn detect(bytes: &[u8]) -> Detected {
    let head = &bytes[..bytes.len().min(PEEK)];
    if content_inspector::inspect(head).is_binary() {
        return Detected::Binary;
    }
    let text = String::from_utf8_lossy(head);
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#'))
        .unwrap_or("");
    if first.starts_with('{')
        || first.starts_with('[') && !first.starts_with("[[") && !is_toml_table(first)
    {
        return Detected::Json;
    }
    if is_toml_table(first) || is_toml_pair(first) {
        return Detected::Toml;
    }
    Detected::Text
}

#[cfg(feature = "content")]
fn is_toml_table(line: &str) -> bool {
    let inner = line.trim_start_matches('[').trim_end_matches(']');
    line.starts_with('[')
        && line.ends_with(']')
        && !inner.is_empty()
        && inner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.\" ".contains(c))
}

#[cfg(feature = "content")]
fn is_toml_pair(line: &str) -> bool {
    line.split_once('=').is_some_and(|(k, v)| {
        let k = k.trim();
        !k.is_empty()
            && !v.trim().is_empty()
            && k.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.\"".contains(c))
    })
}
