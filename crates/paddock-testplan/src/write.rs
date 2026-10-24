//! The canonical writers: fixed key order, entries in declared order, so a regenerated file diffs cleanly and
//! `parse(write(x)) == x`.

use std::fmt::Write;

use crate::plan::{Cell, FORMAT_VERSION, Plan};
use crate::result::{CaseResult, EvidenceRef, RunResult};
use crate::secret::Redactor;
use crate::step::{Step, Target};

pub(crate) fn q(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(o, "\\u{:04X}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn list<T>(items: &[T], f: impl Fn(&T) -> String) -> String {
    format!("[{}]", items.iter().map(f).collect::<Vec<_>>().join(", "))
}

fn target(t: &Target) -> String {
    let mut parts = Vec::new();
    if let Some(v) = &t.text {
        parts.push(format!("text = {}", q(v)));
    }
    if let Some(v) = &t.name {
        parts.push(format!("name = {}", q(v)));
    }
    if let Some(c) = t.class {
        parts.push(format!("class = {}", q(c.as_str())));
    }
    if let Some(s) = &t.inside {
        parts.push(format!("inside = {}", q(s.as_str())));
    }
    if let Some(n) = t.nth {
        parts.push(format!("nth = {n}"));
    }
    if let Some(n) = &t.near {
        parts.push(format!("near = {}", target(n)));
    }
    format!("{{ {} }}", parts.join(", "))
}

fn step(out: &mut String, s: &Step) {
    let _ = writeln!(out, "\n[[cases.steps]]\naction = {}", q(s.action()));
    match s {
        Step::Click { target: t } => {
            let _ = writeln!(out, "target = {}", target(t));
        }
        Step::Type { target: t, value } => {
            let _ = writeln!(
                out,
                "target = {}\nvalue = {}",
                target(t),
                q(&value.to_source())
            );
        }
        Step::Press { keys } => {
            let _ = writeln!(out, "keys = {}", q(keys));
        }
        Step::Select { target: t, option } => {
            let _ = writeln!(
                out,
                "target = {}\noption = {}",
                target(t),
                q(&option.to_source())
            );
        }
        Step::WaitFor {
            target: t,
            timeout_ms,
        } => {
            let _ = writeln!(out, "target = {}\ntimeout_ms = {timeout_ms}", target(t));
        }
        Step::AssertScreen {
            expect_screen,
            timeout_ms,
        } => {
            let _ = writeln!(
                out,
                "expect_screen = {}\ntimeout_ms = {timeout_ms}",
                q(expect_screen.as_str())
            );
        }
        Step::AssertText { target: t, text } => {
            if let Some(t) = t {
                let _ = writeln!(out, "target = {}", target(t));
            }
            let _ = writeln!(out, "text = {}", q(&text.to_source()));
        }
        Step::Wait { ms } => {
            let _ = writeln!(out, "ms = {ms}");
        }
    }
}

impl Plan {
    /// The canonical TOML text. Call [`Plan::validate`] first on a plan built in code: only a valid plan reads back equal.
    pub fn to_toml(&self) -> String {
        let mut o = String::new();
        let _ = writeln!(
            o,
            "format = \"sighterplan\"\nversion = {FORMAT_VERSION}\nid = {}\ntitle = {}",
            q(self.id.as_str()),
            q(&self.title)
        );
        let _ = writeln!(o, "\n[app]\nname = {}", q(&self.app.name));
        if let Some(v) = &self.app.version {
            let _ = writeln!(o, "version = {}", q(v));
        }
        if let Some(g) = &self.generated_by {
            let _ = writeln!(o, "\n[generated_by]\ntool = {}", q(&g.tool));
            if let Some(v) = &g.version {
                let _ = writeln!(o, "version = {}", q(v));
            }
        }
        for r in &self.requirements {
            let _ = writeln!(
                o,
                "\n[[requirements]]\nid = {}\ntitle = {}",
                q(r.id.as_str()),
                q(&r.title)
            );
            if !r.acceptance.is_empty() {
                let _ = writeln!(o, "acceptance = {}", list(&r.acceptance, |a| q(a)));
            }
            if let Some(s) = &r.source {
                let _ = writeln!(o, "source = {}", q(s.as_str()));
            }
        }
        for s in &self.screens {
            let _ = writeln!(o, "\n[[screens]]\nname = {}", q(s.name.as_str()));
            if let Some(t) = &s.title {
                let _ = writeln!(o, "title = {}", q(t));
            }
            if !s.contains.is_empty() {
                let _ = writeln!(o, "contains = {}", list(&s.contains, |c| q(c)));
            }
        }
        for t in &self.data {
            let _ = writeln!(o, "\n[[data]]\nname = {}", q(t.name.as_str()));
            let cols = list(&t.columns, |c| {
                if c.secret {
                    format!("{{ name = {}, secret = true }}", q(c.name.as_str()))
                } else {
                    format!("{{ name = {} }}", q(c.name.as_str()))
                }
            });
            let _ = writeln!(o, "columns = {cols}");
            if !t.rows.is_empty() {
                let rows = t
                    .rows
                    .iter()
                    .map(|r| {
                        format!(
                            "  {},\n",
                            list(r, |c| match c {
                                Cell::Literal(v) => q(v),
                                Cell::Secret(s) => q(s.as_str()),
                            })
                        )
                    })
                    .collect::<String>();
                let _ = writeln!(o, "rows = [\n{rows}]");
            }
        }
        for c in &self.cases {
            let _ = writeln!(
                o,
                "\n[[cases]]\nid = {}\ntitle = {}",
                q(c.id.as_str()),
                q(&c.title)
            );
            if !c.requirements.is_empty() {
                let _ = writeln!(
                    o,
                    "requirements = {}",
                    list(&c.requirements, |r| q(&r.to_string()))
                );
            }
            let _ = writeln!(o, "priority = {}", q(c.priority.as_str()));
            if !c.tags.is_empty() {
                let _ = writeln!(o, "tags = {}", list(&c.tags, |t| q(t.as_str())));
            }
            if let Some(d) = &c.data {
                let _ = writeln!(o, "data = {}", q(d.as_str()));
            }
            for s in &c.steps {
                step(&mut o, s);
            }
        }
        o
    }
}

fn evidence(list_: &[EvidenceRef]) -> String {
    list(list_, |e| {
        format!(
            "{{ kind = {}, path = {}, sha256 = {} }}",
            q(e.kind.as_str()),
            q(e.path.as_str()),
            q(&e.sha256)
        )
    })
}

fn case_result(o: &mut String, c: &CaseResult, red: &dyn Redactor) {
    let _ = writeln!(o, "\n[[cases]]\ncase = {}", q(c.case.as_str()));
    if !c.requirements.is_empty() {
        let _ = writeln!(
            o,
            "requirements = {}",
            list(&c.requirements, |r| q(&r.to_string()))
        );
    }
    let _ = writeln!(o, "status = {}", q(c.status.as_str()));
    if let Some(i) = c.iteration {
        let _ = writeln!(o, "iteration = {i}");
    }
    let _ = writeln!(o, "duration_ms = {}", c.duration_ms);
    if let Some(r) = &c.reason {
        let _ = writeln!(o, "reason = {}", q(&red.redact(r)));
    }
    if !c.evidence.is_empty() {
        let _ = writeln!(o, "evidence = {}", evidence(&c.evidence));
    }
    for s in &c.steps {
        let _ = writeln!(
            o,
            "\n[[cases.steps]]\nindex = {}\nstatus = {}",
            s.index,
            q(s.status.as_str())
        );
        if let Some(n) = &s.screen_seen {
            let _ = writeln!(o, "screen_seen = {}", q(n.as_str()));
        }
        if let Some(v) = s.score {
            let _ = writeln!(o, "score = {v:?}");
        }
        if !s.evidence.is_empty() {
            let _ = writeln!(o, "evidence = {}", evidence(&s.evidence));
        }
    }
}

impl RunResult {
    /// The canonical TOML text. Free text (the reason of a case, the environment) goes through `redactor` first, so a
    /// resolved secret becomes `{{secret:NAME}}`. Call [`validate_result`](crate::validate_result) first on a result built in code.
    pub fn to_toml(&self, redactor: &dyn Redactor) -> String {
        let mut o = String::new();
        let _ = writeln!(
            o,
            "format = \"sighterresult\"\nversion = {FORMAT_VERSION}\nplan = {}\nplan_hash = {}\nrun_id = {}\nstarted = {}",
            q(self.plan.as_str()),
            q(&self.plan_hash),
            q(&self.run_id),
            q(self.started.as_str())
        );
        let e = &self.env;
        let _ = writeln!(o, "\n[env]\nos = {}", q(&redactor.redact(&e.os)));
        if let Some(v) = e.dpi {
            let _ = writeln!(o, "dpi = {v}");
        }
        for (k, v) in [
            ("theme", &e.theme),
            ("language", &e.language),
            ("resolution", &e.resolution),
            ("app_build", &e.app_build),
            ("sighter_version", &e.sighter_version),
        ] {
            if let Some(v) = v {
                let _ = writeln!(o, "{k} = {}", q(&redactor.redact(v)));
            }
        }
        for c in &self.cases {
            case_result(&mut o, c, redactor);
        }
        o
    }
}
