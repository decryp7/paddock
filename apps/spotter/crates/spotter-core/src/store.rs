//! The state file `state.json`: per-watch state, the dashboard's journal and plugin health. Versioned and tolerant:
//! an unreadable file or entry falls back to empty state with a warning (the watches re-check and re-baseline).

use crate::journal::Journal;
use crate::limits::MAX_STATE_BYTES;
use crate::plugin_health::PluginHealth;
use crate::state::WatchState;
use crate::watch::WatchId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The schema this build writes.
pub const STATE_SCHEMA: u32 = 1;

/// Everything in `state.json`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StateDoc {
    /// Schema version.
    pub schema: u32,
    /// Per watch.
    pub watches: BTreeMap<WatchId, WatchState>,
    /// The dashboard's history.
    pub journal: Journal,
    /// Per plugin id.
    pub plugins: BTreeMap<String, PluginHealth>,
}

/// Parses `state.json`, keeping what can be read.
pub fn parse_state(text: &str) -> (StateDoc, Vec<String>) {
    let mut warnings = Vec::new();
    if text.len() > MAX_STATE_BYTES {
        return (
            StateDoc::default(),
            vec!["The state file is too large; Spotter starts with fresh state.".into()],
        );
    }
    let raw: serde_json::Map<String, serde_json::Value> = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            return (
                StateDoc::default(),
                vec![format!("The state file could not be read ({e}); Spotter starts with fresh state.")],
            )
        }
    };
    let schema = raw.get("schema").and_then(|v| v.as_u64()).unwrap_or(0);
    if schema > u64::from(STATE_SCHEMA) {
        return (
            StateDoc::default(),
            vec!["The state file was written by a newer Spotter; Spotter starts with fresh state.".into()],
        );
    }
    let mut doc = StateDoc {
        schema: STATE_SCHEMA,
        ..StateDoc::default()
    };
    if let Some(serde_json::Value::Object(ws)) = raw.get("watches") {
        for (id, v) in ws {
            match (WatchId::parse(id), serde_json::from_value::<WatchState>(v.clone())) {
                (Ok(id), Ok(s)) => {
                    doc.watches.insert(id, s);
                }
                _ => warnings.push(format!("The state of watch \"{id}\" could not be read; it starts fresh.")),
            }
        }
    }
    if let Some(j) = raw.get("journal") {
        doc.journal = serde_json::from_value(j.clone()).unwrap_or_else(|_| {
            warnings.push("The dashboard history could not be read; it starts fresh.".into());
            Journal::default()
        });
    }
    if let Some(p) = raw.get("plugins") {
        doc.plugins = serde_json::from_value(p.clone()).unwrap_or_default();
    }
    (doc, warnings)
}

/// Writes `state.json`.
pub fn render_state(doc: &StateDoc) -> String {
    let mut doc = doc.clone();
    doc.schema = STATE_SCHEMA;
    serde_json::to_string(&doc).unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::EventKind;
    use crate::time::Timestamp;

    #[test]
    fn state_round_trips_and_tolerates_damage() {
        let mut doc = StateDoc::default();
        let id = WatchId::parse("a").unwrap();
        doc.watches.insert(id.clone(), WatchState::default());
        doc.journal.record(Timestamp(1), Some(id), EventKind::Recovered);
        doc.plugins.insert("p".into(), PluginHealth::default());
        let text = render_state(&doc);
        let (back, warnings) = parse_state(&text);
        assert!(warnings.is_empty());
        assert_eq!(back.watches, doc.watches);
        assert_eq!(back.journal, doc.journal);
        assert_eq!(back.schema, STATE_SCHEMA);

        let (_, w) = parse_state(r#"{"schema":1,"watches":{"BAD":{},"b":{"failures":"x"},"c":{}},"journal":5}"#);
        assert_eq!(w.len(), 3, "{w:?}");
        let (d, _) = parse_state(r#"{"watches":{"c":{}}}"#);
        assert_eq!(d.watches.len(), 1);
        assert!(parse_state("garbage").1[0].contains("could not be read"));
        assert!(parse_state(r#"{"schema":9}"#).1[0].contains("newer"));
        assert!(parse_state(&" ".repeat(MAX_STATE_BYTES + 1)).1[0].contains("too large"));
    }
}
