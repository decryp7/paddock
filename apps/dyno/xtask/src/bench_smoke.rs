//! `bench-smoke`: mechanics check of the benchmark runners (ADR 0030 section 5), for a VM or a clean machine: tiny sizes, one
//! repetition, no speed asserted. Every kind must end with exactly one `result` line that is Done or Unavailable (with a reason),
//! every number finite and positive, and the disk test must leave nothing in its folder.

use std::path::{Path, PathBuf};
use std::time::Duration;

const RUN_LIMIT: Duration = Duration::from_secs(300);

/// The problems with one runner's output (empty: it passed), and the `ok:` line when it did.
pub fn check(name: &str, output: &str) -> (Vec<String>, Option<String>) {
    let lines: Vec<&str> = output.lines().filter(|l| !l.contains(r#""t":"heartbeat""#)).collect();
    let all: Vec<&str> = output.lines().collect();
    let last = lines.last().copied().unwrap_or("");
    let mut problems = Vec::new();
    if all.iter().filter(|l| l.contains(r#""t":"result""#)).count() != 1 {
        problems.push(format!("{name}: not exactly one result line"));
    }
    if !last.contains(r#""t":"result""#) {
        problems.push(format!("{name}: the result is not the last line"));
    }
    let ok = if last.contains(r#""state":"done""#) {
        if has_value(last, |v| {
            v.starts_with('-') || v.starts_with("0.0,") || v.starts_with("0.0}") || v.starts_with("0,") || v.starts_with("0}")
        }) {
            problems.push(format!("{name}: a number is zero or negative"));
        }
        if has_value(last, |v| v.starts_with("NaN") || v.starts_with("null") || v.starts_with("inf")) {
            problems.push(format!("{name}: a number is not finite"));
        }
        Some(format!("ok: {name} done"))
    } else if last.contains(r#""state":"unavailable""#) && reason_nonempty(last) {
        Some(format!("ok: {name} unavailable with a reason"))
    } else {
        let cut: String = last.chars().take(200).collect();
        problems.push(format!("{name}: ended neither Done nor Unavailable with a reason: {cut}"));
        None
    };
    let passed = problems.is_empty();
    (problems, if passed { ok } else { None })
}

/// True when some `"value":<rest>` in `line` satisfies `bad` on `<rest>`.
fn has_value(line: &str, bad: impl Fn(&str) -> bool) -> bool {
    line.match_indices(r#""value":"#).any(|(i, m)| bad(&line[i + m.len()..]))
}

/// `"reason":"` followed by at least one character that is not the closing quote (the script's glob also took an empty reason; the
/// intent, a reason, is what is checked).
fn reason_nonempty(line: &str) -> bool {
    line.find(r#""reason":""#)
        .is_some_and(|i| line[i + 10..].chars().next().is_some_and(|c| c != '"'))
}

fn runner(dir: &Path, name: &str) -> PathBuf {
    dir.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() })
}

/// `cargo xtask bench-smoke <folder>`.
pub fn run_task(args: &[String]) -> Result<(), String> {
    let dir = match args {
        [d] => PathBuf::from(d),
        _ => return Err("usage: cargo xtask bench-smoke <folder holding dyno-bench and dyno-bench-gpu>".to_string()),
    };
    let bench = runner(&dir, "dyno-bench");
    let gpu = runner(&dir, "dyno-bench-gpu");
    let (bench, gpu) = (bench.to_string_lossy().into_owned(), gpu.to_string_lossy().into_owned());
    let work = tempdir()?;
    let mut failures = Vec::new();
    let go = |failures: &mut Vec<String>, name: &str, program: &str, args: &[&str]| -> Result<(), String> {
        let ran = crate::run(program, args, &[], RUN_LIMIT)?;
        let (problems, ok) = check(name, &format!("{}{}", ran.stdout, ran.stderr));
        if let Some(ok) = ok {
            println!("{ok}");
        }
        failures.extend(problems);
        Ok(())
    };
    let fast = ["--reps", "1", "--warmup-ms", "0", "--gap-ms", "0", "--millis", "50"];
    go(&mut failures, "cpu", &bench, &[&["cpu"][..], &fast].concat())?;
    go(&mut failures, "memory", &bench, &[&["memory"][..], &fast].concat())?;
    let w = work.to_string_lossy().into_owned();
    go(
        &mut failures,
        "disk",
        &bench,
        &["disk", "--disk-mib", "16", "--reps", "1", "--dir", &w],
    )?;
    let left: Vec<String> = std::fs::read_dir(&work).map_or_else(
        |_| Vec::new(),
        |rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        },
    );
    if !left.is_empty() {
        failures.push(format!("disk: the scratch folder is not empty afterwards: {}", left.join(" ")));
    }
    go(&mut failures, "gpu", &gpu, &["--smoke"])?;
    let _ = std::fs::remove_dir_all(&work);
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures
            .iter()
            .map(|f| format!("::error::bench-smoke: {f}"))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// A fresh private scratch folder in the temp folder (`TMPDIR`, `TEMP` or `TMP`).
fn tempdir() -> Result<PathBuf, String> {
    let base = ["TMPDIR", "TEMP", "TMP"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .ok_or("::error::no temp folder is set (TMPDIR, TEMP or TMP); set TMPDIR to a scratch folder")?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = Path::new(&base).join(format!("dyno-bench-smoke-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("::error::cannot create {}: {e}", dir.display()))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HB: &str = r#"{"t":"heartbeat"}"#;

    #[test]
    fn a_done_result_with_positive_finite_numbers_passes() {
        let out = format!("{{\"t\":\"progress\"}}\n{HB}\n{{\"t\":\"result\",\"state\":\"done\",\"value\":12.5}}\n{HB}\n");
        assert_eq!(check("cpu", &out), (vec![], Some("ok: cpu done".to_string())));
    }

    #[test]
    fn zero_negative_and_non_finite_numbers_are_each_named() {
        for (v, want) in [
            ("0.0", "zero or negative"),
            ("0", "zero or negative"),
            ("-3.1", "zero or negative"),
            ("NaN", "not finite"),
            ("null", "not finite"),
        ] {
            let out = format!("{{\"t\":\"result\",\"state\":\"done\",\"value\":{v}}}\n");
            let (p, ok) = check("memory", &out);
            assert!(p.iter().any(|m| m.contains(want)), "{v}: {p:?}");
            assert_eq!(ok, None);
        }
    }

    #[test]
    fn unavailable_needs_a_reason_and_anything_else_fails() {
        let with = "{\"t\":\"result\",\"state\":\"unavailable\",\"reason\":\"no gpu\"}\n";
        assert_eq!(check("gpu", with).1, Some("ok: gpu unavailable with a reason".to_string()));
        let without = "{\"t\":\"result\",\"state\":\"unavailable\",\"reason\":\"\"}\n";
        assert!(check("gpu", without).0.iter().any(|m| m.contains("neither Done nor Unavailable")));
        assert!(check("gpu", "{\"t\":\"result\",\"state\":\"failed\"}\n")
            .0
            .iter()
            .any(|m| m.contains("neither")));
    }

    #[test]
    fn exactly_one_result_and_it_is_last() {
        let two = "{\"t\":\"result\",\"state\":\"done\",\"value\":1.0}\n{\"t\":\"result\",\"state\":\"done\",\"value\":2.0}\n";
        assert!(check("cpu", two).0.iter().any(|m| m.contains("not exactly one result line")));
        let early = "{\"t\":\"result\",\"state\":\"done\",\"value\":1.0}\n{\"t\":\"progress\"}\n";
        assert!(check("cpu", early).0.iter().any(|m| m.contains("not the last line")));
        assert!(check("cpu", "").0.iter().any(|m| m.contains("not exactly one result line")));
    }
}
