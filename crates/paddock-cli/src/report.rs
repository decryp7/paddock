use crate::{CliError, Exit, Format};
use serde_json::{Map, Value, json};
use std::io::Write;

/// A successful result: the data for JSON mode, the text for people, and the exit code (`Ok`, or `Findings` when a
/// check found something).
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    /// Merged into the JSON object next to `"ok": true` (an object's fields) or stored as `"result"` (anything else).
    pub data: Value,
    /// What people see.
    pub text: String,
    /// `Exit::Ok` or `Exit::Findings` (a check found something; still `"ok": true`), or any other code for a command that did
    /// its work and printed its report but ended unsuccessfully: JSON then says `"ok": false` and adds `exit` and `kind`.
    pub exit: Exit,
}

impl Report {
    /// A result that ends with exit 0.
    pub fn new(data: Value, text: impl Into<String>) -> Report {
        Report {
            data,
            text: text.into(),
            exit: Exit::Ok,
        }
    }

    /// Ends with `exit` instead (`Exit::Findings`, or a failure code that still prints this report).
    pub fn with_exit(mut self, exit: Exit) -> Report {
        self.exit = exit;
        self
    }

    /// The JSON object printed in JSON mode.
    pub fn json(&self) -> Value {
        let mut m = Map::new();
        let succeeded = matches!(self.exit, Exit::Ok | Exit::Findings);
        m.insert("ok".into(), Value::Bool(succeeded));
        match &self.data {
            Value::Object(o) => m.extend(
                o.iter()
                    .filter(|(k, _)| k.as_str() != "ok")
                    .map(|(k, v)| (k.clone(), v.clone())),
            ),
            Value::Null => {}
            other => {
                m.insert("result".into(), other.clone());
            }
        }
        if !succeeded {
            m.insert("exit".into(), json!(self.exit.code()));
            m.insert("kind".into(), json!(self.exit.name()));
        }
        Value::Object(m)
    }
}

/// The JSON object printed when a command fails in JSON mode.
pub fn error_json(e: &CliError) -> Value {
    json!({"ok": false, "error": e.message, "hint": e.hint, "exit": e.exit.code(), "kind": e.exit.name()})
}

/// Writes the outcome of a command and returns the exit code: the result to `out` (text or one JSON object), a failure
/// to `err` as text, or to `out` as the JSON error object (so a script reads one stream) and its text to `err` as well.
/// A closed pipe does not change the exit code.
pub fn finish(
    format: Format,
    result: Result<Report, CliError>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Exit {
    match result {
        Ok(r) => {
            let _ = match format {
                Format::Json => writeln!(out, "{}", r.json()),
                Format::Text if r.text.is_empty() => Ok(()),
                Format::Text => writeln!(out, "{}", r.text.trim_end_matches('\n')),
            };
            r.exit
        }
        Err(e) => {
            if format == Format::Json {
                let _ = writeln!(out, "{}", error_json(&e));
            }
            let _ = writeln!(err, "error: {}", e.text());
            e.exit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::FailAfter;

    fn run(format: Format, r: Result<Report, CliError>) -> (Exit, String, String) {
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let x = finish(format, r, &mut o, &mut e);
        (
            x,
            String::from_utf8(o).unwrap(),
            String::from_utf8(e).unwrap(),
        )
    }

    #[test]
    fn json_is_one_object_and_text_is_for_people() {
        let (x, o, e) = run(
            Format::Json,
            Ok(Report::new(json!({"count": 3}), "3 items")),
        );
        assert_eq!(x, Exit::Ok);
        assert_eq!(
            serde_json::from_str::<Value>(o.trim()).unwrap(),
            json!({"ok": true, "count": 3})
        );
        assert!(e.is_empty());
        let (_, o, _) = run(
            Format::Text,
            Ok(Report::new(json!({"count": 3}), "3 items\n")),
        );
        assert_eq!(o, "3 items\n");
        let (_, o, _) = run(Format::Text, Ok(Report::new(Value::Null, "")));
        assert!(o.is_empty());
    }

    #[test]
    fn non_object_data_goes_under_result_and_ok_cannot_be_overridden() {
        assert_eq!(
            Report::new(json!([1, 2]), "").json(),
            json!({"ok": true, "result": [1, 2]})
        );
        assert_eq!(
            Report::new(json!({"ok": false, "a": 1}), "").json(),
            json!({"ok": true, "a": 1})
        );
        assert_eq!(Report::new(Value::Null, "").json(), json!({"ok": true}));
    }

    #[test]
    fn a_report_that_ends_in_failure_says_ok_false_with_its_exit_and_kind() {
        let r = Report::new(json!({"removed": 2}), "partial").with_exit(Exit::Failure);
        assert_eq!(
            r.json(),
            json!({"ok": false, "removed": 2, "exit": 1, "kind": "failure"})
        );
        let (x, o, _) = run(Format::Json, Ok(r));
        assert_eq!(x, Exit::Failure);
        assert_eq!(
            serde_json::from_str::<Value>(o.trim()).unwrap()["ok"],
            false
        );
        assert_eq!(
            Report::new(json!({"a": 1}), "")
                .with_exit(Exit::Findings)
                .json(),
            json!({"ok": true, "a": 1})
        );
    }

    #[test]
    fn findings_end_with_their_own_code() {
        let (x, _, _) = run(
            Format::Text,
            Ok(Report::new(Value::Null, "2 findings").with_exit(Exit::Findings)),
        );
        assert_eq!(x, Exit::Findings);
    }

    #[test]
    fn failures_name_the_next_step_in_both_modes() {
        let err = || CliError::not_running("Pitwall", "Start the app, then try again.");
        let (x, o, e) = run(Format::Text, Err(err()));
        assert_eq!((x, o.as_str()), (Exit::NotRunning, ""));
        assert_eq!(
            e,
            "error: Pitwall is not running. Start the app, then try again.\n"
        );
        let (x, o, e) = run(Format::Json, Err(err()));
        assert_eq!(x.code(), 3);
        let v: Value = serde_json::from_str(o.trim()).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["exit"], 3);
        assert_eq!(v["kind"], "not-running");
        assert_eq!(v["hint"], "Start the app, then try again.");
        assert!(e.starts_with("error: "));
    }

    #[test]
    fn a_closed_pipe_keeps_the_exit_code() {
        assert_eq!(
            finish(
                Format::Json,
                Ok(Report::new(Value::Null, "")),
                &mut FailAfter(0),
                &mut FailAfter(0)
            ),
            Exit::Ok
        );
        assert_eq!(
            finish(
                Format::Json,
                Err(CliError::usage("x")),
                &mut FailAfter(0),
                &mut FailAfter(0)
            ),
            Exit::Usage
        );
    }
}
