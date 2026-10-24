//! Golden files for what a plugin describes and renders: the text is pretty JSON, compared (or, with
//! `PADDOCK_UPDATE_GOLDEN=1`, written) by `paddock-testkit`'s golden mechanism, so a change of a plugin's surface (a tool
//! operation, a settings field, a view) shows up in review.

use crate::fake::FakeHost;
use paddock_plugin_api::Plugin;
use paddock_view::ViewDoc;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

/// `value` as pretty JSON with a trailing newline.
pub fn pretty<T: Serialize>(value: &T) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_else(|e| format!("cannot serialise: {e}"));
    text.push('\n');
    text
}

/// What the plugin describes, as golden text.
///
/// # Panics
///
/// When the plugin does not connect or describe itself.
pub fn described_text(plugin: Arc<dyn Plugin>) -> String {
    let host = FakeHost::start(plugin).expect("the plugin connects to the fake host");
    pretty(&host.describe().expect("the plugin describes itself"))
}

/// Compares what the plugin describes with the golden file at `path`.
///
/// # Panics
///
/// When they differ (see `paddock_testkit::assert_golden`).
pub fn assert_described(plugin: Arc<dyn Plugin>, path: &Path) {
    paddock_testkit::assert_golden(path, &described_text(plugin));
}

/// Compares a view document with the golden file at `path`.
///
/// # Panics
///
/// When they differ.
pub fn assert_view(doc: &ViewDoc, path: &Path) {
    paddock_testkit::assert_golden(path, &pretty(doc));
}
