//! Bytes to typed values, once, at the edge (parse, don't validate).

use crate::error::PlanError;
use crate::ids::{
    CaseId, Column, DataName, PlanId, RelPath, ReqRef, RequirementId, ScreenName, SecretRef, Tag,
    Timestamp, clip,
};
use crate::plan::{
    AppRef, Case, Cell, DataColumn, DataTable, FORMAT_VERSION, Generator, MAX_FILE_BYTES,
    MIN_READABLE_VERSION, Plan, Priority, Requirement, ScreenExpect, SourceLink,
};
use crate::raw::{Header, RawCase, RawData, RawEvidence, RawPlan, RawResult, RawStep, RawTarget};
use crate::result::{
    CaseResult, Environment, EvidenceKind, EvidenceRef, RunResult, Status, StepResult,
};
use crate::step::{ControlClass, Step, Target, Value};

/// Parses a `.sighterplan`. Refuses a file over 4 MiB, a newer major, unknown keys, bad ids and dangling references.
pub fn parse_plan(bytes: &[u8]) -> Result<Plan, PlanError> {
    let text = decode(bytes)?;
    check_header(text, "sighterplan")?;
    let raw: RawPlan = toml::from_str(text).map_err(|e| from_toml(text, &e))?;
    let plan = build_plan(raw)?;
    plan.validate()?;
    Ok(plan)
}

/// Parses a `.sighterresult`. The same limits and rules; evidence paths must stay inside the result file's folder.
pub fn parse_result(bytes: &[u8]) -> Result<RunResult, PlanError> {
    let text = decode(bytes)?;
    check_header(text, "sighterresult")?;
    let raw: RawResult = toml::from_str(text).map_err(|e| from_toml(text, &e))?;
    let result = build_result(raw)?;
    crate::result_check::validate_result(&result)?;
    Ok(result)
}

fn decode(bytes: &[u8]) -> Result<&str, PlanError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(PlanError::Limit {
            what: "file size in bytes".to_owned(),
            max: MAX_FILE_BYTES,
        });
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    std::str::from_utf8(bytes).map_err(|e| {
        let upto = &bytes[..e.valid_up_to()];
        let line = upto.iter().filter(|b| **b == b'\n').count() + 1;
        let col = upto.iter().rev().take_while(|b| **b != b'\n').count() + 1;
        PlanError::Syntax {
            line,
            col,
            msg: "the file is not valid UTF-8".to_owned(),
        }
    })
}

fn position(text: &str, offset: usize) -> (usize, usize) {
    let offset = (0..=offset.min(text.len()))
        .rev()
        .find(|o| text.is_char_boundary(*o))
        .unwrap_or(0);
    let before = &text[..offset];
    (
        before.matches('\n').count() + 1,
        before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1,
    )
}

fn from_toml(text: &str, e: &toml::de::Error) -> PlanError {
    let (line, col) = e.span().map_or((1, 1), |s| position(text, s.start));
    let msg = e.message();
    if let Some(name) = msg
        .strip_prefix("unknown field `")
        .and_then(|rest| rest.split('`').next())
    {
        return PlanError::UnknownKey {
            path: name.to_owned(),
            line,
        };
    }
    PlanError::Syntax {
        line,
        col,
        msg: msg.to_owned(),
    }
}

fn check_header(text: &str, expected: &'static str) -> Result<(), PlanError> {
    let h: Header = toml::from_str(text).map_err(|e| from_toml(text, &e))?;
    match h.format.as_deref() {
        Some(f) if f == expected => {}
        other => {
            return Err(PlanError::WrongFormat {
                expected,
                found: clip(other.unwrap_or("")),
            });
        }
    }
    let found = h.version.ok_or_else(|| PlanError::BadField {
        field: "version".to_owned(),
        msg: "missing; add `version = 1` at the top of the file".to_owned(),
    })?;
    let found = u32::try_from(found).unwrap_or(if found < 0 { 0 } else { u32::MAX });
    if !(MIN_READABLE_VERSION..=FORMAT_VERSION).contains(&found) {
        return Err(PlanError::Version {
            found,
            min: MIN_READABLE_VERSION,
            max: FORMAT_VERSION,
        });
    }
    Ok(())
}

fn opt<T>(
    v: Option<&String>,
    f: impl Fn(&str) -> Result<T, PlanError>,
) -> Result<Option<T>, PlanError> {
    v.map(|s| f(s)).transpose()
}

fn build_plan(raw: RawPlan) -> Result<Plan, PlanError> {
    let requirements = raw
        .requirements
        .into_iter()
        .map(|r| {
            Ok(Requirement {
                id: RequirementId::new(&r.id)?,
                title: r.title,
                acceptance: r.acceptance,
                source: opt(r.source.as_ref(), SourceLink::new)?,
            })
        })
        .collect::<Result<_, PlanError>>()?;
    let screens = raw
        .screens
        .into_iter()
        .map(|s| {
            Ok(ScreenExpect {
                name: ScreenName::new(&s.name)?,
                title: s.title,
                contains: s.contains,
            })
        })
        .collect::<Result<_, PlanError>>()?;
    Ok(Plan {
        id: PlanId::new(&raw.id)?,
        title: raw.title,
        app: AppRef {
            name: raw.app.name,
            version: raw.app.version,
        },
        generated_by: raw.generated_by.map(|g| Generator {
            tool: g.tool,
            version: g.version,
        }),
        requirements,
        screens,
        cases: raw
            .cases
            .into_iter()
            .map(build_case)
            .collect::<Result<_, _>>()?,
        data: raw
            .data
            .into_iter()
            .map(build_table)
            .collect::<Result<_, _>>()?,
    })
}

fn build_case(raw: RawCase) -> Result<Case, PlanError> {
    let priority = match raw.priority.as_deref() {
        None | Some("normal") => Priority::Normal,
        Some("low") => Priority::Low,
        Some("high") => Priority::High,
        Some("critical") => Priority::Critical,
        Some(other) => {
            return Err(PlanError::BadField {
                field: format!("priority `{}`", clip(other)),
                msg: "expected low, normal, high or critical".to_owned(),
            });
        }
    };
    let steps = raw
        .steps
        .into_iter()
        .enumerate()
        .map(|(i, s)| build_step(&raw.id, i, s))
        .collect::<Result<_, _>>()?;
    Ok(Case {
        id: CaseId::new(&raw.id)?,
        title: raw.title,
        requirements: raw
            .requirements
            .iter()
            .map(|r| ReqRef::new(r))
            .collect::<Result<_, _>>()?,
        priority,
        tags: raw
            .tags
            .iter()
            .map(|t| Tag::new(t))
            .collect::<Result<_, _>>()?,
        data: opt(raw.data.as_ref(), DataName::new)?,
        steps,
    })
}

fn build_target(raw: RawTarget) -> Result<Target, PlanError> {
    Ok(Target {
        text: raw.text,
        name: raw.name,
        class: opt(raw.class.as_ref(), ControlClass::parse)?,
        inside: opt(raw.inside.as_ref(), ScreenName::new)?,
        near: raw
            .near
            .map(|n| build_target(*n).map(Box::new))
            .transpose()?,
        nth: raw.nth,
    })
}

fn build_step(case: &str, index: usize, raw: RawStep) -> Result<Step, PlanError> {
    let bad = |msg: String| PlanError::BadStep {
        case: clip(case),
        index,
        msg,
    };
    let present: Vec<&str> = [
        ("target", raw.target.is_some()),
        ("value", raw.value.is_some()),
        ("keys", raw.keys.is_some()),
        ("option", raw.option.is_some()),
        ("timeout_ms", raw.timeout_ms.is_some()),
        ("expect_screen", raw.expect_screen.is_some()),
        ("text", raw.text.is_some()),
        ("ms", raw.ms.is_some()),
    ]
    .into_iter()
    .filter_map(|(n, p)| p.then_some(n))
    .collect();
    let (required, optional): (&[&str], &[&str]) = match raw.action.as_str() {
        "click" => (&["target"], &[]),
        "type" => (&["target", "value"], &[]),
        "press" => (&["keys"], &[]),
        "select" => (&["target", "option"], &[]),
        "wait_for" => (&["target", "timeout_ms"], &[]),
        "assert_screen" => (&["expect_screen", "timeout_ms"], &[]),
        "assert_text" => (&["text"], &["target"]),
        "wait" => (&["ms"], &[]),
        other => {
            return Err(bad(format!(
                "unknown action `{}`; use click, type, press, select, wait_for, assert_screen, assert_text or wait",
                clip(other)
            )));
        }
    };
    if let Some(missing) = required.iter().find(|r| !present.contains(r)) {
        return Err(bad(format!("`{}` needs `{missing}`", raw.action)));
    }
    if let Some(extra) = present
        .iter()
        .find(|p| !required.contains(p) && !optional.contains(p))
    {
        return Err(bad(format!("`{}` does not take `{extra}`", raw.action)));
    }
    let target = |t: Option<RawTarget>| t.map(build_target).transpose();
    let need = |t: Option<RawTarget>| target(t).map(|t| t.unwrap_or_default());
    Ok(match raw.action.as_str() {
        "click" => Step::Click {
            target: need(raw.target)?,
        },
        "type" => Step::Type {
            target: need(raw.target)?,
            value: Value::parse(raw.value.as_deref().unwrap_or_default())?,
        },
        "press" => Step::Press {
            keys: raw.keys.unwrap_or_default(),
        },
        "select" => Step::Select {
            target: need(raw.target)?,
            option: Value::parse(raw.option.as_deref().unwrap_or_default())?,
        },
        "wait_for" => Step::WaitFor {
            target: need(raw.target)?,
            timeout_ms: raw.timeout_ms.unwrap_or_default(),
        },
        "assert_screen" => Step::AssertScreen {
            expect_screen: ScreenName::new(raw.expect_screen.as_deref().unwrap_or_default())?,
            timeout_ms: raw.timeout_ms.unwrap_or_default(),
        },
        "assert_text" => Step::AssertText {
            target: target(raw.target)?,
            text: Value::parse(raw.text.as_deref().unwrap_or_default())?,
        },
        _ => Step::Wait {
            ms: raw.ms.unwrap_or_default(),
        },
    })
}

fn build_table(raw: RawData) -> Result<DataTable, PlanError> {
    let columns: Vec<DataColumn> = raw
        .columns
        .into_iter()
        .map(|c| {
            Ok(DataColumn {
                name: Column::new(&c.name)?,
                secret: c.secret,
            })
        })
        .collect::<Result<_, PlanError>>()?;
    let rows = raw
        .rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .enumerate()
                .map(|(i, cell)| match columns.get(i) {
                    Some(c) if c.secret => {
                        SecretRef::new(&cell)
                            .map(Cell::Secret)
                            .map_err(|_| PlanError::BadSecret {
                                value: clip(&cell),
                                msg: format!(
                                    "column `{}` is secret, so its cells are secret names",
                                    c.name
                                ),
                            })
                    }
                    _ => Ok(Cell::Literal(cell)),
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<_, _>>()?;
    Ok(DataTable {
        name: DataName::new(&raw.name)?,
        columns,
        rows,
    })
}

fn build_evidence(raw: RawEvidence) -> Result<EvidenceRef, PlanError> {
    let kind = EvidenceKind::parse(&raw.kind).ok_or_else(|| PlanError::BadField {
        field: format!("evidence kind `{}`", clip(&raw.kind)),
        msg: "expected screenshot, recording or ocr_text".to_owned(),
    })?;
    Ok(EvidenceRef {
        kind,
        path: RelPath::new(&raw.path)?,
        sha256: raw.sha256,
    })
}

fn status(value: &str) -> Result<Status, PlanError> {
    Status::parse(value).ok_or_else(|| PlanError::BadField {
        field: format!("status `{}`", clip(value)),
        msg: "expected pass, fail, blocked, needs_recording or skipped".to_owned(),
    })
}

fn build_result(raw: RawResult) -> Result<RunResult, PlanError> {
    let cases = raw
        .cases
        .into_iter()
        .map(|c| {
            let steps = c
                .steps
                .into_iter()
                .map(|s| {
                    Ok(StepResult {
                        index: s.index,
                        status: status(&s.status)?,
                        screen_seen: opt(s.screen_seen.as_ref(), ScreenName::new)?,
                        score: s.score.map(|v| v as f32),
                        evidence: s
                            .evidence
                            .into_iter()
                            .map(build_evidence)
                            .collect::<Result<_, _>>()?,
                    })
                })
                .collect::<Result<_, PlanError>>()?;
            Ok(CaseResult {
                case: CaseId::new(&c.case)?,
                requirements: c
                    .requirements
                    .iter()
                    .map(|r| ReqRef::new(r))
                    .collect::<Result<_, _>>()?,
                status: status(&c.status)?,
                iteration: c.iteration,
                duration_ms: c.duration_ms,
                steps,
                evidence: c
                    .evidence
                    .into_iter()
                    .map(build_evidence)
                    .collect::<Result<_, _>>()?,
                reason: c.reason,
            })
        })
        .collect::<Result<_, PlanError>>()?;
    Ok(RunResult {
        plan: PlanId::new(&raw.plan)?,
        plan_hash: raw.plan_hash,
        run_id: raw.run_id,
        started: Timestamp::new(&raw.started)?,
        env: Environment {
            os: raw.env.os,
            dpi: raw.env.dpi,
            theme: raw.env.theme,
            language: raw.env.language,
            resolution: raw.env.resolution,
            app_build: raw.env.app_build,
            sighter_version: raw.env.sighter_version,
        },
        cases,
    })
}
