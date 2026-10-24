//! JUnit XML export, with requirement ids in `<properties>` so CI dashboards and Allure can group by requirement.

use std::fmt::Write;

use crate::plan::Plan;
use crate::result::{RunResult, Status};
use crate::secret::Redactor;

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            '\n' | '\t' => o.push(c),
            c if c.is_control() || matches!(c, '\u{FFFE}' | '\u{FFFF}') => {}
            c => o.push(c),
        }
    }
    o
}

fn secs(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

/// Writes one `<testsuite>` for the run. A `fail` is a `<failure>`, a `blocked` an `<error>`, `needs_recording` and
/// `skipped` are `<skipped>`. Free text goes through `redactor`; the plan supplies case titles.
pub fn write(plan: &Plan, result: &RunResult, redactor: &dyn Redactor) -> String {
    let count = |s: Status| result.cases.iter().filter(|c| c.status == s).count();
    let skipped = count(Status::Skipped) + count(Status::NeedsRecording);
    let time: u64 = result.cases.iter().map(|c| c.duration_ms).sum();
    let mut o = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites>\n");
    let _ = writeln!(
        o,
        "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{skipped}\" time=\"{}\" timestamp=\"{}\">",
        esc(&redactor.redact(&plan.title)),
        result.cases.len(),
        count(Status::Fail),
        count(Status::Blocked),
        secs(time),
        esc(result.started.as_str())
    );
    let _ = writeln!(o, "    <properties>");
    for (k, v) in [
        ("plan", result.plan.to_string()),
        ("plan_hash", result.plan_hash.clone()),
        ("run_id", result.run_id.clone()),
        ("os", result.env.os.clone()),
    ] {
        let _ = writeln!(
            o,
            "      <property name=\"{k}\" value=\"{}\"/>",
            esc(&redactor.redact(&v))
        );
    }
    let _ = writeln!(o, "    </properties>");
    for c in &result.cases {
        let title = plan
            .case(&c.case)
            .map_or_else(|| c.case.to_string(), |p| p.title.clone());
        let name = match c.iteration {
            Some(i) => format!("{title} [row {}]", i + 1),
            None => title,
        };
        let _ = writeln!(
            o,
            "    <testcase classname=\"{}\" name=\"{}\" time=\"{}\">",
            esc(result.plan.as_str()),
            esc(&redactor.redact(&name)),
            secs(c.duration_ms)
        );
        let _ = writeln!(o, "      <properties>");
        let _ = writeln!(
            o,
            "        <property name=\"case\" value=\"{}\"/>",
            esc(c.case.as_str())
        );
        for r in &c.requirements {
            let _ = writeln!(
                o,
                "        <property name=\"requirement\" value=\"{}\"/>",
                esc(&r.to_string())
            );
        }
        let _ = writeln!(o, "      </properties>");
        let reason = esc(&redactor.redact(c.reason.as_deref().unwrap_or("")));
        match c.status {
            Status::Pass => {}
            Status::Fail => {
                let _ = writeln!(o, "      <failure message=\"{reason}\"/>");
            }
            Status::Blocked => {
                let _ = writeln!(o, "      <error message=\"{reason}\"/>");
            }
            Status::NeedsRecording | Status::Skipped => {
                let _ = writeln!(
                    o,
                    "      <skipped message=\"{}{reason}\"/>",
                    if c.status == Status::NeedsRecording {
                        "needs recording. "
                    } else {
                        ""
                    }
                );
            }
        }
        if !c.steps.is_empty() || !c.evidence.is_empty() {
            let _ = writeln!(o, "      <system-out>");
            for s in &c.steps {
                let _ = writeln!(o, "step {}: {}", s.index + 1, s.status.as_str());
            }
            for e in c
                .evidence
                .iter()
                .chain(c.steps.iter().flat_map(|s| &s.evidence))
            {
                let _ = writeln!(o, "evidence {}: {}", e.kind.as_str(), esc(e.path.as_str()));
            }
            let _ = writeln!(o, "      </system-out>");
        }
        let _ = writeln!(o, "    </testcase>");
    }
    o.push_str("  </testsuite>\n</testsuites>\n");
    o
}
