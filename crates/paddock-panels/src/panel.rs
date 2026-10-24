//! Panel ids: what the dock calls a panel and a pop-out window shows. The rules are the workbench core's
//! (`workbench/src/registry.ts`): a panel is a view id (`pn.code`) or one instance of a view, `<view>#<key>`
//! (`pn.code#src/main.rs`), where the key is 1 to [`MAX_KEY`] characters. The view id is a namespaced contribution id
//! (`CONTRIBUTION_ID` of the shell): lower-case segments joined by dots, dashes inside a segment.
//!
//! The webview is never trusted: every id reaching Rust goes through [`Panel::parse`] once, and the rest of the crate
//! works with the typed value.

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use std::fmt;

/// The separator between a view id and an instance key.
pub const SEPARATOR: char = '#';
/// The longest instance key, in characters (a path).
pub const MAX_KEY: usize = 1024;
/// The longest view id, in bytes.
pub const MAX_VIEW: usize = 64;

/// Why a panel id was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelProblem {
    /// Not a namespaced view id (`pn.code`).
    View,
    /// The instance key is empty, longer than [`MAX_KEY`] characters or has control characters.
    Key,
}

impl fmt::Display for PanelProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PanelProblem::View => f.write_str("not a view id (namespaced, like pn.code)"),
            PanelProblem::Key => write!(
                f,
                "an instance key is 1 to {MAX_KEY} characters without control characters"
            ),
        }
    }
}

/// A valid panel: a view and, for a view with instances, the instance's key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Panel {
    view: String,
    key: Option<String>,
}

/// A namespaced view id: `[a-z][a-z0-9]*` then one or more `.[a-z0-9]+(-[a-z0-9]+)*`, at most [`MAX_VIEW`] bytes.
pub fn valid_view_id(id: &str) -> bool {
    if id.len() > MAX_VIEW {
        return false;
    }
    let mut segments = id.split('.');
    let first_ok = segments.next().is_some_and(|s| {
        s.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
            && s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    });
    let mut rest = 0;
    let rest_ok = segments.all(|seg| {
        rest += 1;
        !seg.is_empty()
            && seg.split('-').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            })
    });
    first_ok && rest >= 1 && rest_ok
}

impl Panel {
    /// The panel `id` (`view` or `view#key`). The key is everything after the first `#` (a key may contain `#`).
    pub fn parse(id: &str) -> Result<Panel, PanelProblem> {
        let (view, key) = match id.split_once(SEPARATOR) {
            Some((view, key)) => (view, Some(key)),
            None => (id, None),
        };
        if !valid_view_id(view) {
            return Err(PanelProblem::View);
        }
        if let Some(key) = key {
            let chars = key.chars().count();
            if chars == 0 || chars > MAX_KEY || key.chars().any(char::is_control) {
                return Err(PanelProblem::Key);
            }
        }
        Ok(Panel {
            view: view.to_string(),
            key: key.map(str::to_string),
        })
    }

    /// The view the panel shows.
    pub fn view(&self) -> &str {
        &self.view
    }

    /// The instance key, for a view with instances.
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// The panel id as the dock writes it.
    pub fn id(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Panel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.key {
            Some(key) => write!(f, "{}{SEPARATOR}{key}", self.view),
            None => f.write_str(&self.view),
        }
    }
}

/// The characters kept as they are in a URL query value (everything else is percent-encoded).
const QUERY: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// The URL a pop-out loads: the app's pop-out route with the panel (percent-encoded: a key may be a path), its host and
/// its own label. The UI reads them back with `popoutRoute`.
pub fn popout_url(route: &str, panel: &Panel, host: &str, label: &str) -> String {
    let join = if route.contains('?') { '&' } else { '?' };
    let enc = utf8_percent_encode(&panel.id(), QUERY).to_string();
    format!("{route}{join}panel={enc}&host={host}&label={label}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_is_a_namespaced_id() {
        for ok in [
            "pn.code",
            "pw.models",
            "a.b.c",
            "x1.y-z",
            "dbg.log-view.main",
        ] {
            assert!(valid_view_id(ok), "{ok}");
        }
        for bad in [
            "",
            "code",
            "Pn.code",
            "pn.",
            ".code",
            "pn..code",
            "pn.-code",
            "pn.code-",
            "pn.co--de",
            "1n.code",
            "pn.cö",
            "pn.code#x",
            "pn code",
            "../x",
            &format!("pn.{}", "x".repeat(MAX_VIEW)),
        ] {
            assert!(!valid_view_id(bad), "{bad}");
        }
        assert!(valid_view_id(&format!("pn.{}", "x".repeat(MAX_VIEW - 3))));
    }

    #[test]
    fn a_panel_is_a_view_or_one_instance_of_it() {
        let p = Panel::parse("pn.code").unwrap();
        assert_eq!((p.view(), p.key()), ("pn.code", None));
        let p = Panel::parse("pn.code#src/main.rs#2").unwrap();
        assert_eq!((p.view(), p.key()), ("pn.code", Some("src/main.rs#2")));
        assert_eq!(p.id(), "pn.code#src/main.rs#2");
        assert_eq!(p.to_string(), p.id());
    }

    #[test]
    fn keys_are_bounded_and_views_are_checked() {
        assert_eq!(Panel::parse("pn.code#"), Err(PanelProblem::Key));
        assert_eq!(Panel::parse("pn.code#a\nb"), Err(PanelProblem::Key));
        assert_eq!(
            Panel::parse(&format!("pn.code#{}", "k".repeat(MAX_KEY + 1))),
            Err(PanelProblem::Key)
        );
        assert!(Panel::parse(&format!("pn.code#{}", "k".repeat(MAX_KEY))).is_ok());
        // The limit counts characters, not bytes.
        assert!(Panel::parse(&format!("pn.code#{}", "ä".repeat(MAX_KEY))).is_ok());
        assert_eq!(Panel::parse("#key"), Err(PanelProblem::View));
        assert_eq!(Panel::parse("code#key"), Err(PanelProblem::View));
        assert!(PanelProblem::View.to_string().contains("view id"));
        assert!(PanelProblem::Key.to_string().contains("1024"));
    }

    #[test]
    fn the_url_carries_the_panel_encoded() {
        let p = Panel::parse("pn.code#src/a b&c=d.rs").unwrap();
        assert_eq!(
            popout_url("index.html#/popout", &p, "main", "p-main-1"),
            "index.html#/popout?panel=pn.code%23src%2Fa%20b%26c%3Dd.rs&host=main&label=p-main-1"
        );
        let plain = Panel::parse("pn.code").unwrap();
        assert_eq!(
            popout_url("popout.html?x=1", &plain, "w-2", "p-w-2-3"),
            "popout.html?x=1&panel=pn.code&host=w-2&label=p-w-2-3"
        );
    }
}
