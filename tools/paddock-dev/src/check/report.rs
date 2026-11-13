//! The report of a conformance run in the JavaScript host's exact shape (`conformance/src/report.mjs`), so CI annotations, the job
//! summary and the tests that read them do not change when the host does: the log lines, the Markdown summary and the result JSON
//! (`app`, `checks`, `findings`, `waived`, `errors`, `warnings`; none of the Rust host's extras).

use super::RunResult;
use super::finding::{Finding, Severity};
use serde_json::{Value, json};

fn where_(f: &Finding) -> String {
    if f.file.is_empty() {
        "-".into()
    } else if f.line > 0 {
        format!("{}:{}", f.file, f.line)
    } else {
        f.file.clone()
    }
}

/// The plain log lines; the `::error` annotations make Gitea show them on the run.
pub fn format_text(r: &RunResult) -> String {
    let mut out = Vec::new();
    for f in &r.findings {
        let level = if f.severity == Severity::Error { "::error" } else { "::warning" };
        out.push(format!(
            "{level} file={},line={}::{} (rule {}) {} Fix: {}",
            f.file, f.line, f.check, f.rule, f.message, f.fix
        ));
        out.push(format!(
            "{:<5} {} {}  {}\n        fix: {}",
            f.severity.name(),
            f.check,
            where_(f),
            f.message,
            f.fix
        ));
    }
    for w in &r.waived {
        out.push(format!(
            "waived {} {}  {} (until {}: {})",
            w.finding.check,
            where_(&w.finding),
            w.finding.message,
            w.expires,
            w.reason
        ));
    }
    out.push(format!(
        "{}: {} checks, {} errors, {} warnings, {} waived",
        r.app,
        r.checks.len(),
        r.errors(),
        r.warnings(),
        r.waived.len()
    ));
    out.join("\n")
}

fn esc(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// The Markdown of the Gitea job summary.
pub fn format_markdown(r: &RunResult) -> String {
    let head = format!(
        "## Conformance: {}\n\n{} errors, {} warnings, {} waived (checks: {})\n",
        r.app,
        r.errors(),
        r.warnings(),
        r.waived.len(),
        r.checks.join(", ")
    );
    if r.findings.is_empty() && r.waived.is_empty() {
        return format!("{head}\nAll checks passed.\n");
    }
    let rows: Vec<String> = r
        .findings
        .iter()
        .map(|f| {
            format!(
                "| {} | {} | {} | `{}` | {} | {} |",
                f.severity.name(),
                f.check,
                f.rule,
                esc(&where_(f)),
                esc(&f.message),
                esc(&f.fix)
            )
        })
        .collect();
    let mut md = format!(
        "{head}\n| Severity | Check | Rule | Where | Finding | Fix |\n|---|---|---|---|---|---|\n{}\n",
        rows.join("\n")
    );
    if !r.waived.is_empty() {
        md.push_str("\n### Waived\n\n| Check | Where | Reason | Expires |\n|---|---|---|---|\n");
        let rows: Vec<String> = r
            .waived
            .iter()
            .map(|w| {
                format!(
                    "| {} | `{}` | {} | {} |",
                    w.finding.check,
                    esc(&where_(&w.finding)),
                    esc(&w.reason),
                    w.expires
                )
            })
            .collect();
        md.push_str(&format!("{}\n", rows.join("\n")));
    }
    md
}

/// The result JSON of the JavaScript host: no run records.
pub fn result_json(r: &RunResult) -> Value {
    json!({
        "app": r.app,
        "checks": r.checks,
        "findings": r.findings.iter().map(Finding::json).collect::<Vec<_>>(),
        "waived": r.waived.iter().map(super::finding::Waived::json).collect::<Vec<_>>(),
        "errors": r.errors(),
        "warnings": r.warnings(),
    })
}
