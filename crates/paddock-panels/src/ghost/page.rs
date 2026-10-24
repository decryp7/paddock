//! The drag ghost's static page: three files the app serves from its public folder (next to `index.html`) under the
//! names [`HTML_NAME`], [`CSS_NAME`] and [`JS_NAME`]. Copy them in the app's build (or write them at start-up); the page
//! has no commands and no capabilities, takes its content through `window.paddockGhost({ title, icon, snapshot, dark })`
//! and only ever sets text, an SVG path's `d` and a PNG data URL, never markup.

/// The page.
pub const HTML: &str = include_str!("../../assets/drag-ghost.html");
/// Its style.
pub const CSS: &str = include_str!("../../assets/drag-ghost.css");
/// Its script.
pub const JS: &str = include_str!("../../assets/drag-ghost.js");
/// The file names the page expects.
pub const HTML_NAME: &str = "drag-ghost.html";
pub const CSS_NAME: &str = "drag-ghost.css";
pub const JS_NAME: &str = "drag-ghost.js";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_links_its_files_and_runs_no_inline_code() {
        assert!(
            HTML.contains(&format!("href=\"{CSS_NAME}\""))
                && HTML.contains(&format!("src=\"{JS_NAME}\""))
        );
        assert!(!HTML.contains("<script>") && !HTML.contains("style=\"") && !HTML.contains("http"));
        assert_eq!(super::super::GHOST_PAGE, HTML_NAME);
    }

    #[test]
    fn the_script_offers_the_function_the_shell_calls_and_never_parses_markup() {
        assert!(JS.contains("window.paddockGhost = function"));
        assert!(!JS.contains("innerHTML") && !JS.contains("eval(") && !JS.contains("debrief"));
        assert!(!HTML.contains("debrief") && !HTML.to_lowercase().contains("debrief"));
        assert!(!CSS.contains("http"));
    }
}
