//! What only the whole plan can know: limits, duplicates, references. Plus the secret-literal lint.

use std::collections::HashSet;

use crate::error::PlanError;
use crate::ids::{ReqRef, clip};
use crate::plan::{
    Cell, MAX_CASES, MAX_COLUMNS, MAX_ENTRIES, MAX_ROWS, MAX_STEPS, MAX_STRING, MAX_WAIT_MS, Plan,
};
use crate::step::{Step, Target, Value};

/// Most levels of `near` inside a target.
pub const MAX_TARGET_DEPTH: usize = 8;

fn limit(what: &str, max: usize) -> PlanError {
    PlanError::Limit {
        what: what.to_owned(),
        max,
    }
}

fn text(field: &str, value: &str) -> Result<(), PlanError> {
    if value.len() > MAX_STRING {
        Err(limit(&format!("`{field}` ({})", clip(value)), MAX_STRING))
    } else {
        Ok(())
    }
}

fn unique<'a>(kind: &'static str, ids: impl Iterator<Item = &'a str>) -> Result<(), PlanError> {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(PlanError::Duplicate {
                kind,
                id: id.to_owned(),
            });
        }
    }
    Ok(())
}

fn target_ok(
    t: &Target,
    depth: usize,
    screens: &HashSet<&str>,
    bad: &dyn Fn(String) -> PlanError,
) -> Result<(), PlanError> {
    if depth > MAX_TARGET_DEPTH {
        return Err(limit(
            "target nesting (`near` inside `near`)",
            MAX_TARGET_DEPTH,
        ));
    }
    if t.is_empty() {
        return Err(bad(
            "a target needs at least one of `text`, `name`, `class` or `near`".to_owned(),
        ));
    }
    text("target text", t.text.as_deref().unwrap_or(""))?;
    text("target name", t.name.as_deref().unwrap_or(""))?;
    if let Some(s) = &t.inside
        && !screens.contains(s.as_str())
    {
        return Err(PlanError::DanglingRef {
            kind: "screen",
            id: s.to_string(),
        });
    }
    match &t.near {
        Some(n) => target_ok(n, depth + 1, screens, bad),
        None => Ok(()),
    }
}

impl Plan {
    /// Checks everything a typed value cannot: limits, duplicate ids, references to requirements, screens, data tables and
    /// columns, timeouts, targets. [`parse_plan`](crate::parse_plan) calls it; call it on a plan built in code before writing.
    pub fn validate(&self) -> Result<(), PlanError> {
        if self.cases.len() > MAX_CASES {
            return Err(limit("cases", MAX_CASES));
        }
        for (what, n) in [
            ("requirements", self.requirements.len()),
            ("screens", self.screens.len()),
            ("data tables", self.data.len()),
        ] {
            if n > MAX_ENTRIES {
                return Err(limit(what, MAX_ENTRIES));
            }
        }
        text("title", &self.title)?;
        text("app name", &self.app.name)?;
        text("app version", self.app.version.as_deref().unwrap_or(""))?;
        if let Some(g) = &self.generated_by {
            text("generated_by tool", &g.tool)?;
            text("generated_by version", g.version.as_deref().unwrap_or(""))?;
        }
        unique(
            "requirement",
            self.requirements.iter().map(|r| r.id.as_str()),
        )?;
        unique("screen", self.screens.iter().map(|s| s.name.as_str()))?;
        unique("case", self.cases.iter().map(|c| c.id.as_str()))?;
        unique("data table", self.data.iter().map(|d| d.name.as_str()))?;
        for r in &self.requirements {
            text("requirement title", &r.title)?;
            if r.acceptance.len() > MAX_ENTRIES {
                return Err(limit("acceptance criteria of a requirement", MAX_ENTRIES));
            }
            r.acceptance
                .iter()
                .try_for_each(|a| text("acceptance criterion", a))?;
        }
        for s in &self.screens {
            text("screen title", s.title.as_deref().unwrap_or(""))?;
            s.contains
                .iter()
                .try_for_each(|c| text("screen contains", c))?;
        }
        for t in &self.data {
            self.validate_table(t)?;
        }
        let screens: HashSet<&str> = self.screens.iter().map(|s| s.name.as_str()).collect();
        for case in &self.cases {
            self.validate_case(case, &screens)?;
        }
        Ok(())
    }

    fn validate_table(&self, t: &crate::plan::DataTable) -> Result<(), PlanError> {
        if t.columns.is_empty() || t.columns.len() > MAX_COLUMNS {
            return Err(PlanError::BadField {
                field: format!("data `{}` columns", t.name),
                msg: format!("a table has 1 to {MAX_COLUMNS} columns"),
            });
        }
        if t.rows.len() > MAX_ROWS {
            return Err(limit(&format!("rows of data table `{}`", t.name), MAX_ROWS));
        }
        unique("column", t.columns.iter().map(|c| c.name.as_str()))?;
        for row in &t.rows {
            if row.len() != t.columns.len() {
                return Err(PlanError::BadField {
                    field: format!("data `{}` rows", t.name),
                    msg: format!("every row needs {} cells", t.columns.len()),
                });
            }
            for (cell, col) in row.iter().zip(&t.columns) {
                match (cell, col.secret) {
                    (Cell::Literal(v), false) => text("data cell", v)?,
                    (Cell::Secret(_), true) => {}
                    (Cell::Literal(v), true) => {
                        return Err(PlanError::BadSecret {
                            value: clip(v),
                            msg: format!(
                                "column `{}` is secret, so its cells are secret names",
                                col.name
                            ),
                        });
                    }
                    (Cell::Secret(s), false) => {
                        return Err(PlanError::BadSecret {
                            value: s.to_string(),
                            msg: format!("column `{}` is not marked `secret = true`", col.name),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_case(
        &self,
        case: &crate::plan::Case,
        screens: &HashSet<&str>,
    ) -> Result<(), PlanError> {
        if case.steps.len() > MAX_STEPS {
            return Err(limit(&format!("steps in case `{}`", case.id), MAX_STEPS));
        }
        text("case title", &case.title)?;
        for r in &case.requirements {
            let req = self
                .requirements
                .iter()
                .find(|q| &q.id == r.requirement())
                .ok_or_else(|| PlanError::DanglingRef {
                    kind: "requirement",
                    id: r.to_string(),
                })?;
            if let ReqRef::Acceptance(a) = r
                && a.number() as usize > req.acceptance.len()
            {
                return Err(PlanError::DanglingRef {
                    kind: "acceptance criterion",
                    id: r.to_string(),
                });
            }
        }
        let table = match &case.data {
            Some(name) => Some(self.data.iter().find(|d| &d.name == name).ok_or_else(|| {
                PlanError::DanglingRef {
                    kind: "data table",
                    id: name.to_string(),
                }
            })?),
            None => None,
        };
        for (index, step) in case.steps.iter().enumerate() {
            let bad = |msg: String| PlanError::BadStep {
                case: case.id.to_string(),
                index,
                msg,
            };
            for t in step.targets() {
                target_ok(t, 0, screens, &bad)?;
            }
            match step {
                Step::WaitFor { timeout_ms: ms, .. }
                | Step::AssertScreen { timeout_ms: ms, .. }
                | Step::Wait { ms }
                    if *ms > MAX_WAIT_MS =>
                {
                    return Err(limit("a wait or timeout in ms", MAX_WAIT_MS as usize));
                }
                Step::AssertScreen { expect_screen, .. }
                    if !screens.contains(expect_screen.as_str()) =>
                {
                    return Err(PlanError::DanglingRef {
                        kind: "screen",
                        id: expect_screen.to_string(),
                    });
                }
                Step::Press { keys } if keys.trim().is_empty() => {
                    return Err(bad("`keys` is empty".to_owned()));
                }
                _ => {}
            }
            if let Step::Press { keys } = step {
                text("keys", keys)?;
            }
            for v in step.values() {
                match v {
                    Value::Literal(t) => text("value", t)?,
                    Value::Data(c) => {
                        let found = table.is_some_and(|t| t.columns.iter().any(|k| &k.name == c));
                        if !found {
                            return Err(PlanError::DanglingRef {
                                kind: "data column of the case's table",
                                id: c.to_string(),
                            });
                        }
                    }
                    Value::Secret(_) => {}
                }
            }
        }
        Ok(())
    }
}

/// A warning that does not stop a plan from loading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lint {
    /// Where: `case ID step N` or `data TABLE column NAME`.
    pub at: String,
    /// What to fix.
    pub message: String,
}

const SENSITIVE: [&str; 7] = [
    "password", "passwd", "token", "card", "secret", "api key", "apikey",
];

fn sensitive(name: &str) -> bool {
    let n = name.to_lowercase().replace(['_', '-'], " ");
    SENSITIVE.iter().any(|w| n.contains(w))
}

/// Warns about literals typed into a field named like a secret (`password`, `token`, `card`...), and about data columns with
/// such names that are not secret columns. A warning, not an error: some such fields hold harmless test values.
pub fn lint(plan: &Plan) -> Vec<Lint> {
    let mut out = Vec::new();
    for case in &plan.cases {
        for (i, step) in case.steps.iter().enumerate() {
            if let Step::Type {
                target,
                value: Value::Literal(v),
            } = step
            {
                let labels = [
                    target.name.as_deref(),
                    target.text.as_deref(),
                    target.near.as_ref().and_then(|n| n.text.as_deref()),
                ];
                if !v.is_empty() && labels.iter().flatten().any(|l| sensitive(l)) {
                    out.push(Lint {
                        at: format!("case {} step {}", case.id, i + 1),
                        message: "a literal is typed into a field that looks like a secret; use {{secret:NAME}} so reports never contain it".to_owned(),
                    });
                }
            }
        }
    }
    for t in &plan.data {
        for c in t
            .columns
            .iter()
            .filter(|c| !c.secret && sensitive(c.name.as_str()))
        {
            out.push(Lint {
                at: format!("data {} column {}", t.name, c.name),
                message: "the column name looks like a secret; mark it `secret = true` and hold secret names".to_owned(),
            });
        }
    }
    out
}
