//! Stored benchmark results (`benchmarks.json`): one per model, kept across restarts and used by model selection
//! ("fast" prefers the model that measured faster on this machine). Versioned, bounded, and tolerant of damage like the
//! other state files: the primary file, then its backup, then nothing.

use crate::mgmt::BenchmarkResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The file's schema version.
pub const VERSION: u32 = 1;
/// The most results kept (one per model; the oldest go first).
pub const MAX_RESULTS: usize = 200;
/// The largest file read.
pub const MAX_BYTES: usize = 512 * 1024;

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    results: Vec<BenchmarkResult>,
}

fn sane(r: &BenchmarkResult) -> bool {
    r.gen_tps.is_finite() && r.gen_tps >= 0.0 && r.prompt_tps.is_finite() && r.prompt_tps >= 0.0 && !r.model_id.is_empty()
}

fn parse_one(text: &str) -> Option<Vec<BenchmarkResult>> {
    let f: File = serde_json::from_str(text).ok()?;
    // A file from a newer Pitwall is not guessed at.
    (f.version <= VERSION).then(|| f.results.into_iter().filter(sane).take(MAX_RESULTS).collect())
}

/// The results from the primary file, else the backup, else none.
pub fn load(primary: Option<&str>, backup: Option<&str>) -> Vec<BenchmarkResult> {
    primary
        .and_then(parse_one)
        .or_else(|| backup.and_then(parse_one))
        .unwrap_or_default()
}

/// The file's text.
pub fn to_json(results: &[BenchmarkResult]) -> String {
    let file = File {
        version: VERSION,
        results: results.to_vec(),
    };
    serde_json::to_string(&file).unwrap_or_default()
}

/// Replaces the model's earlier result with `result`, keeping the list bounded (oldest first out).
pub fn upsert(list: &mut Vec<BenchmarkResult>, result: BenchmarkResult) {
    list.retain(|b| b.model_id != result.model_id);
    list.push(result);
    if list.len() > MAX_RESULTS {
        list.sort_by_key(|b| b.at);
        let excess = list.len() - MAX_RESULTS;
        list.drain(..excess);
    }
}

/// Measured generation speed by model id, for selection (results with no speed are left out).
pub fn speeds(list: &[BenchmarkResult]) -> HashMap<String, f64> {
    list.iter()
        .filter(|b| b.gen_tps > 0.0)
        .map(|b| (b.model_id.clone(), b.gen_tps))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(id: &str, tps: f64, at: u64) -> BenchmarkResult {
        BenchmarkResult {
            model_id: id.into(),
            build: "cpu".into(),
            at,
            prompt_tps: 100.0,
            gen_tps: tps,
            ttft_ms: 50,
            context_tokens: 4096,
        }
    }

    #[test]
    fn round_trips_and_falls_back_to_the_backup_then_to_nothing() {
        let list = vec![r("a", 10.0, 1), r("b", 20.0, 2)];
        let text = to_json(&list);
        assert_eq!(load(Some(&text), None), list);
        assert_eq!(load(Some("{ damaged"), Some(&text)), list);
        assert_eq!(load(None, Some(&text)), list);
        assert!(load(Some("garbage"), Some("more garbage")).is_empty());
        assert!(load(None, None).is_empty());
        assert!(
            load(Some(r#"{"version":99,"results":[]}"#), None).is_empty(),
            "newer file is not guessed at"
        );
    }

    #[test]
    fn nonsense_numbers_and_empty_ids_are_dropped() {
        let text = r#"{"version":1,"results":[
            {"modelId":"","build":"x","at":1,"promptTps":1.0,"genTps":1.0,"ttftMs":1,"contextTokens":1},
            {"modelId":"ok","build":"x","at":1,"promptTps":1.0,"genTps":-3.0,"ttftMs":1,"contextTokens":1},
            {"modelId":"good","build":"x","at":1,"promptTps":1.0,"genTps":3.0,"ttftMs":1,"contextTokens":1}]}"#;
        let l = load(Some(text), None);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].model_id, "good");
    }

    #[test]
    fn upsert_keeps_one_per_model_and_is_bounded() {
        let mut l = vec![r("a", 10.0, 1)];
        upsert(&mut l, r("a", 12.0, 5));
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].gen_tps, 12.0);
        for i in 0..(MAX_RESULTS + 10) {
            upsert(&mut l, r(&format!("m{i}"), 1.0, 100 + i as u64));
        }
        assert_eq!(l.len(), MAX_RESULTS);
        assert!(!l.iter().any(|b| b.model_id == "a"), "the oldest went first");
    }

    #[test]
    fn speeds_skip_results_without_a_speed() {
        let s = speeds(&[r("a", 10.0, 1), r("b", 0.0, 1)]);
        assert_eq!(s.get("a"), Some(&10.0));
        assert!(!s.contains_key("b"));
    }
}
