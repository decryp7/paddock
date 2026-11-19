//! The verdict of a finished, red CI run, from `ci triage`'s JSON (the port of the integrator's `triage-verdict.py`).
//!
//! PASS only when the triage itself worked and every failing row is a FLAKY test (not a whole job) whose location is on the
//! accepted list. Anything else is STOP: a row it cannot read, a skipped job, counts that do not add up to the rows, a red run
//! with no rows. A pass is a value only [`verdict`] makes: [`Pass`] has no public constructor (PRINCIPLES: a gate's result
//! comes only from the check).

use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

/// A location of a test the integrator accepts as flaky, tied to its app: `apps/<app>/app/<spec>:<line>[:<col>]`, for example
/// `apps/debrief/app/e2e/a11y.e2e.ts:6:7`. Without a column any column of that line is covered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    app: String,
    rel: String,
    line: u32,
    col: Option<u32>,
}

impl Accepted {
    /// Parses the location; anything else is refused (a typo would accept nothing, silently, and a location with no app could be
    /// any app's test).
    #[allow(clippy::expect_used)] // a constant pattern: a bug of this file, caught by the first test
    pub fn parse(text: &str) -> Result<Accepted, String> {
        static SHAPE: OnceLock<Regex> = OnceLock::new();
        let shape = SHAPE.get_or_init(|| {
            Regex::new(r"^apps/([\w-]+)/app/((?:[\w.-]+/)*[\w.-]+\.(?:e2e|spec)\.ts):(\d+)(?::(\d+))?$").expect("static pattern")
        });
        let c = shape.captures(text).ok_or_else(|| {
            format!("`{text}` is not a flaky location: write apps/<app>/app/<spec>:<line>[:<col>] of an .e2e.ts or .spec.ts file, for example apps/debrief/app/e2e/a11y.e2e.ts:6:7")
        })?;
        let num = |i: usize| {
            c.get(i)
                .map(|m| m.as_str().parse::<u32>())
                .transpose()
                .map_err(|_| format!("`{text}`: the line or column is too large"))
        };
        Ok(Accepted {
            app: c[1].to_string(),
            rel: c[2].to_string(),
            line: num(3)?.unwrap_or(0),
            col: num(4)?,
        })
    }

    /// The app the test belongs to.
    pub fn app(&self) -> &str {
        &self.app
    }

    /// The package folder of the app: `apps/<app>/app`.
    pub fn package(&self) -> String {
        format!("apps/{}/app", self.app)
    }

    /// The spec's path inside the package (`e2e/a11y.e2e.ts`).
    pub fn rel(&self) -> &str {
        &self.rel
    }

    /// The spec's path in the repository.
    pub fn file(&self) -> String {
        format!("{}/{}", self.package(), self.rel)
    }

    /// Whether the location a CI log names (`e2e/a11y.e2e.ts:6:7`, relative to the package) is this one.
    pub(super) fn covers(&self, location: &str) -> bool {
        let mut it = location.rsplitn(3, ':');
        let (col, line, rel) = (it.next(), it.next(), it.next());
        match (
            rel,
            line.and_then(|l| l.parse::<u32>().ok()),
            col.and_then(|c| c.parse::<u32>().ok()),
        ) {
            (Some(rel), Some(line), Some(col)) => rel == self.rel && line == self.line && self.col.is_none_or(|c| c == col),
            _ => false,
        }
    }
}

/// The proof that a run's failures are all accepted flaky tests. Only [`verdict`] makes one.
#[derive(Debug, PartialEq, Eq)]
pub struct Pass {
    /// How many failing rows there were, each an accepted FLAKY test.
    pub rows: usize,
    _seal: Seal,
}

/// Private: nothing outside this module can name it, so nothing outside can build a [`Pass`].
#[derive(Debug, PartialEq, Eq)]
struct Seal;

/// The first `file:line:col` of an e2e or spec file in a test's name.
#[allow(clippy::expect_used)] // a constant pattern: a bug of this file, caught by the first test
pub fn location_of(test: &str) -> Option<String> {
    static LOCATION: OnceLock<Regex> = OnceLock::new();
    let re =
        LOCATION.get_or_init(|| Regex::new(r"(?:^|\s|›\s)((?:[\w.-]+/)*[\w.-]+\.(?:e2e|spec)\.ts:\d+:\d+)\b").expect("static pattern"));
    re.captures(test).map(|c| c[1].to_string())
}

fn cut(text: &str) -> String {
    text.chars().take(120).collect()
}

/// Judges `doc` (`ci triage --json`: `ok`, `rows`, `counts`, `skipped_jobs`). `Ok(Pass)` only when nothing stops it; otherwise
/// every reason, each one line.
pub fn verdict(doc: &Value, accepted: &[Accepted]) -> Result<Pass, Vec<String>> {
    if doc.get("ok") != Some(&Value::Bool(true)) {
        return Err(vec!["the triage did not succeed (ok is not true)".into()]);
    }
    let (Some(rows), Some(counts)) = (
        doc.get("rows").and_then(Value::as_array),
        doc.get("counts").and_then(Value::as_object),
    ) else {
        return Err(vec!["the triage has no rows or counts".into()]);
    };
    let mut reasons = Vec::new();
    if rows.is_empty() {
        reasons.push("a red run with no failing rows: the triage could not see the failure".to_string());
    }
    // `skipped_jobs` must be there and be a count: a triage that does not say how many jobs never ran has not looked.
    match doc.get("skipped_jobs").and_then(Value::as_u64) {
        Some(0) => {}
        Some(n) => reasons.push(format!("{n} job(s) were skipped")),
        None => reasons.push("the triage has no skipped_jobs count (missing or not a number)".to_string()),
    }
    // Every count must be a whole number: one that is not (null, text, a negative) cannot be added up to the rows.
    let mut counted: u64 = 0;
    for (name, v) in counts {
        match v.as_u64() {
            Some(n) => counted += n,
            None => reasons.push(format!("count {name} is not a whole number ({v})")),
        }
    }
    if counted != rows.len() as u64 {
        reasons.push(format!(
            "counts {} do not add up to the {} row(s)",
            Value::Object(counts.clone()),
            rows.len()
        ));
    }
    for row in rows {
        let status = row.get("status").and_then(Value::as_str).unwrap_or("");
        let test = row.get("test").and_then(Value::as_str).unwrap_or("");
        if status != "flaky" {
            let word = if status.is_empty() { "unreadable" } else { status };
            reasons.push(format!("{word}: {}", cut(test)));
            continue;
        }
        if row.get("whole_job").is_some_and(|w| !matches!(w, Value::Null | Value::Bool(false))) {
            reasons.push(format!("flaky whole job, not a test: {}", cut(test)));
            continue;
        }
        match location_of(test) {
            Some(loc) if accepted.iter().any(|a| a.covers(&loc)) => {}
            _ => reasons.push(format!("flaky and not accepted: {}", cut(test))),
        }
    }
    if reasons.is_empty() {
        Ok(Pass {
            rows: rows.len(),
            _seal: Seal,
        })
    } else {
        Err(reasons)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const A11Y: &str = "[webkit] › e2e/a11y.e2e.ts:6:7 › main screen has no serious accessibility violations (dark)";
    const HOME: &str = "[webkit] › e2e/ux-home.e2e.ts:73:7 › home (dashboard and health check) › can be turned off for later imports";
    const CHAT: &str = "[webkit] › e2e/ai-chat.e2e.ts:130:7 › AI chat › a long conversation scrolls in its list only";
    const WINDOWING: &str = "[webkit] › e2e/windowing.e2e.ts:93:5 › background work lists running tasks and stops one";

    fn flaky(test: &str) -> Value {
        json!({"job": "end-to-end (shard 3/4)", "test": test, "whole_job": false, "status": "flaky",
               "flaky": {"count": 1, "runs": 144, "also_on_baseline": false}})
    }

    /// A run in the shape of `ci triage --json` (run 8300's), with the given rows and counts.
    fn doc(rows: Vec<Value>, counts: Value) -> Value {
        json!({"ok": true, "run": 8300, "counts": counts, "complete": false, "skipped_jobs": 0, "gaps": ["x"], "rows": rows})
    }

    fn two_flaky() -> Value {
        doc(
            vec![flaky(A11Y), flaky(HOME)],
            json!({"new": 0, "already_failing": 0, "flaky": 2, "failed": 0, "unknown": 0}),
        )
    }

    fn accepted(list: &[&str]) -> Vec<Accepted> {
        list.iter().map(|a| Accepted::parse(a).unwrap()).collect()
    }

    fn stops(d: &Value, list: &[&str]) -> Vec<String> {
        verdict(d, &accepted(list)).expect_err("must stop")
    }

    /// The positive control: the check does produce a pass, for exactly the accepted flaky tests (a column-less entry covers
    /// any column of its line), so the STOP controls below are not stops that nothing could ever turn into a pass.
    #[test]
    fn only_accepted_flaky_tests_pass() {
        let pass = verdict(
            &two_flaky(),
            &accepted(&["apps/debrief/app/e2e/a11y.e2e.ts:6:7", "apps/debrief/app/e2e/ux-home.e2e.ts:73"]),
        )
        .unwrap();
        assert_eq!(pass.rows, 2);
    }

    #[test]
    fn a_flaky_test_that_is_not_accepted_stops() {
        let why = stops(&two_flaky(), &["apps/debrief/app/e2e/a11y.e2e.ts:6:7"]);
        assert_eq!(why.len(), 1, "{why:?}");
        assert!(
            why[0].starts_with("flaky and not accepted: ") && why[0].contains("ux-home"),
            "{why:?}"
        );
        assert!(!stops(&two_flaky(), &[]).is_empty());
    }

    #[test]
    fn a_line_only_entry_does_not_cover_another_line_and_an_exact_one_not_another_column() {
        let d = doc(vec![flaky(WINDOWING)], json!({"flaky": 1}));
        assert!(!stops(&d, &["apps/debrief/app/e2e/windowing.e2e.ts:94"]).is_empty());
        assert!(!stops(&d, &["apps/debrief/app/e2e/windowing.e2e.ts:93:6"]).is_empty());
        assert!(verdict(&d, &accepted(&["apps/debrief/app/e2e/windowing.e2e.ts:93"])).is_ok());
    }

    /// Run 8300: a NEW failure beside two accepted flakes is a STOP.
    #[test]
    fn run_8300_with_a_new_failure_stops() {
        let new = json!({"job": "end-to-end (shard 3/4)", "test": CHAT, "whole_job": false, "status": "new"});
        let d = doc(
            vec![flaky(A11Y), new, flaky(HOME)],
            json!({"new": 1, "already_failing": 0, "flaky": 2, "failed": 0, "unknown": 0}),
        );
        let why = stops(
            &d,
            &["apps/debrief/app/e2e/a11y.e2e.ts:6:7", "apps/debrief/app/e2e/ux-home.e2e.ts:73:7"],
        );
        assert_eq!(why.len(), 1, "{why:?}");
        assert!(why[0].starts_with("new: ") && why[0].contains("ai-chat"), "{why:?}");
    }

    /// Run 8301: a test that failed on every attempt (its flaky history shown beside it) is not FLAKY: STOP.
    #[test]
    fn run_8301_with_a_test_that_failed_every_attempt_stops() {
        let failed = json!({"job": "end-to-end (shard 4/4)", "test": WINDOWING, "whole_job": false, "status": "failed",
                            "flaky_history": {"count": 2, "runs": 144, "also_on_baseline": false}});
        let d = doc(
            vec![failed],
            json!({"new": 0, "already_failing": 0, "flaky": 0, "failed": 1, "unknown": 0}),
        );
        let why = stops(&d, &["apps/debrief/app/e2e/windowing.e2e.ts:93:5"]);
        assert!(why[0].starts_with("failed: "), "{why:?}");
    }

    #[test]
    fn already_failing_and_unknown_rows_stop_too() {
        for status in ["already-failing", "unknown", ""] {
            let row = json!({"job": "j", "test": A11Y, "whole_job": false, "status": status});
            let why = stops(&doc(vec![row], json!({"x": 1})), &["apps/debrief/app/e2e/a11y.e2e.ts:6:7"]);
            assert_eq!(why.len(), 1, "{status}: {why:?}");
        }
    }

    #[test]
    fn a_flaky_whole_job_is_not_a_test_and_stops() {
        let mut row = flaky(A11Y);
        row["whole_job"] = json!(true);
        row["test"] = Value::Null;
        let d = doc(vec![row], json!({"flaky": 1}));
        assert!(stops(&d, &["apps/debrief/app/e2e/a11y.e2e.ts:6:7"])[0].starts_with("flaky whole job, not a test"));
        // with the name still there (the script's control): the same
        let mut row = flaky(A11Y);
        row["whole_job"] = json!(true);
        assert!(stops(&doc(vec![row], json!({"flaky": 1})), &["apps/debrief/app/e2e/a11y.e2e.ts:6:7"])[0].starts_with("flaky whole job"));
    }

    #[test]
    fn a_skipped_job_stops() {
        let mut d = two_flaky();
        d["skipped_jobs"] = json!(1);
        let why = stops(
            &d,
            &["apps/debrief/app/e2e/a11y.e2e.ts:6:7", "apps/debrief/app/e2e/ux-home.e2e.ts:73:7"],
        );
        assert_eq!(why, vec!["1 job(s) were skipped".to_string()]);
    }

    #[test]
    fn counts_that_do_not_add_up_to_the_rows_stop() {
        let mut d = two_flaky();
        d["counts"] = json!({"new": 1, "already_failing": 0, "flaky": 2, "failed": 0, "unknown": 0});
        let why = stops(
            &d,
            &["apps/debrief/app/e2e/a11y.e2e.ts:6:7", "apps/debrief/app/e2e/ux-home.e2e.ts:73:7"],
        );
        assert!(why[0].contains("do not add up to the 2 row(s)"), "{why:?}");
    }

    #[test]
    fn a_red_run_with_no_rows_stops() {
        let d = doc(
            vec![],
            json!({"new": 0, "already_failing": 0, "flaky": 0, "failed": 0, "unknown": 0}),
        );
        assert!(stops(&d, &[])[0].contains("no failing rows"));
    }

    #[test]
    fn a_triage_that_did_not_succeed_stops() {
        let mut d = two_flaky();
        d["ok"] = json!(false);
        assert_eq!(
            stops(&d, &["apps/debrief/app/e2e/a11y.e2e.ts:6:7"]),
            vec!["the triage did not succeed (ok is not true)".to_string()]
        );
        d.as_object_mut().unwrap().remove("ok");
        assert!(!stops(&d, &[]).is_empty());
    }

    #[test]
    fn a_document_without_rows_or_counts_stops() {
        assert!(stops(&json!({"ok": true}), &[])[0].contains("no rows or counts"));
        assert!(stops(&json!({"ok": true, "rows": {}, "counts": []}), &[])[0].contains("no rows or counts"));
        assert!(stops(&json!("text"), &[])[0].contains("did not succeed"));
    }

    #[test]
    fn a_flaky_row_with_no_readable_location_stops() {
        let d = doc(vec![flaky("some test with no location")], json!({"flaky": 1}));
        assert!(stops(&d, &["apps/debrief/app/e2e/a11y.e2e.ts:6:7"])[0].starts_with("flaky and not accepted"));
    }

    #[test]
    fn accepted_entries_are_tied_to_their_app_and_have_a_checked_shape() {
        let a = Accepted::parse("apps/debrief/app/e2e/live-tail-follow.e2e.ts:240").unwrap();
        assert_eq!(
            (a.app(), a.rel(), a.package().as_str()),
            ("debrief", "e2e/live-tail-follow.e2e.ts", "apps/debrief/app")
        );
        assert_eq!(a.file(), "apps/debrief/app/e2e/live-tail-follow.e2e.ts");
        for bad in [
            "",
            "a11y",
            "e2e/a11y.e2e.ts:6:7",
            "apps/debrief/app/e2e/a11y.e2e.ts",
            "apps/debrief/app/e2e/a11y.e2e.ts:x",
            "apps/debrief/e2e/a11y.e2e.ts:6:7",
            "apps/debrief/app/a11y.ts:6:7",
            "apps/debrief/app/e2e/a11y.e2e.ts:6:7|apps/x/app/e2e/b.e2e.ts:1",
        ] {
            assert!(Accepted::parse(bad).is_err(), "{bad}");
        }
    }
}
