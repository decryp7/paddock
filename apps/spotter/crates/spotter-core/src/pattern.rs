//! Name patterns: which file, folder, tag or asset names a watch looks at. Globs (`*`, `?`) for most people,
//! regular expressions for the rest; both match the whole name, ignoring case.

use crate::user_regex;
use regex::Regex;
use serde::{Deserialize, Serialize};

/// How a pattern is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PatternSyntax {
    /// `*` any characters, `?` one character; everything else literal.
    #[default]
    Glob,
    /// A regular expression (matched anywhere in the name, as the old Release Monitor did).
    Regex,
}

impl PatternSyntax {
    /// From a stored value (`glob` / `regex`).
    pub fn parse(s: &str) -> Result<PatternSyntax, String> {
        match s {
            "glob" => Ok(PatternSyntax::Glob),
            "regex" => Ok(PatternSyntax::Regex),
            other => Err(format!("\"{other}\" is not a pattern syntax (glob or regex).")),
        }
    }

    /// The stored value.
    pub fn as_str(self) -> &'static str {
        match self {
            PatternSyntax::Glob => "glob",
            PatternSyntax::Regex => "regex",
        }
    }
}

/// A compiled name pattern. An empty pattern matches every name.
#[derive(Debug, Clone)]
pub struct NamePattern {
    syntax: PatternSyntax,
    text: String,
    regex: Option<Regex>,
}

impl PartialEq for NamePattern {
    fn eq(&self, other: &Self) -> bool {
        self.syntax == other.syntax && self.text == other.text
    }
}

impl NamePattern {
    /// Matches everything.
    pub fn any() -> NamePattern {
        NamePattern {
            syntax: PatternSyntax::Glob,
            text: String::new(),
            regex: None,
        }
    }

    /// Compiles `text` written in `syntax`. Globs match the whole name; regular expressions anywhere in it.
    pub fn new(syntax: PatternSyntax, text: &str) -> Result<NamePattern, String> {
        let text = text.trim();
        if text.is_empty() || (syntax == PatternSyntax::Glob && text == "*") {
            return Ok(NamePattern {
                syntax,
                text: text.to_string(),
                regex: None,
            });
        }
        let source = match syntax {
            PatternSyntax::Glob => glob_to_regex(text),
            PatternSyntax::Regex => text.to_string(),
        };
        Ok(NamePattern {
            syntax,
            text: text.to_string(),
            regex: Some(user_regex::compile(&source, true)?),
        })
    }

    /// Whether `name` matches.
    pub fn matches(&self, name: &str) -> bool {
        self.regex.as_ref().is_none_or(|r| r.is_match(name))
    }

    /// The pattern as written.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// How it is written.
    pub fn syntax(&self) -> PatternSyntax {
        self.syntax
    }

    /// Whether it matches every name.
    pub fn is_any(&self) -> bool {
        self.regex.is_none()
    }
}

fn glob_to_regex(glob: &str) -> String {
    let mut out = String::from("^");
    for c in glob.chars() {
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            c => out.push_str(&regex::escape(c.encode_utf8(&mut [0; 4]))),
        }
    }
    out.push('$');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_whole_names_ignoring_case() {
        let p = NamePattern::new(PatternSyntax::Glob, "App-*.ZIP").unwrap();
        assert!(p.matches("app-1.2.zip") && !p.matches("app-1.2.zip.sha256") && !p.matches("my-app-1.zip"));
        let q = NamePattern::new(PatternSyntax::Glob, "v?.(x)").unwrap();
        assert!(q.matches("v1.(x)") && !q.matches("v12.(x)"));
        assert!(NamePattern::new(PatternSyntax::Glob, "*").unwrap().is_any());
        assert!(NamePattern::new(PatternSyntax::Glob, " ").unwrap().matches("anything"));
        assert_eq!(q.text(), "v?.(x)");
        assert_eq!(q.syntax(), PatternSyntax::Glob);
    }

    #[test]
    fn regexes_match_anywhere_and_bad_ones_are_refused() {
        let p = NamePattern::new(PatternSyntax::Regex, r".*.txt").unwrap();
        assert!(p.matches("R1.01.00T10.TXT") && !p.is_any());
        assert!(NamePattern::new(PatternSyntax::Regex, "(").is_err());
        assert_eq!(NamePattern::any(), NamePattern::new(PatternSyntax::Glob, "").unwrap());
        assert_ne!(NamePattern::any(), p);
    }

    #[test]
    fn syntaxes_parse() {
        for s in [PatternSyntax::Glob, PatternSyntax::Regex] {
            assert_eq!(PatternSyntax::parse(s.as_str()), Ok(s));
        }
        assert!(PatternSyntax::parse("wild").is_err());
    }
}
