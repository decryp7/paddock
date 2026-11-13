//! A small, dependency-free reader for the parts of Gitea and GitHub workflow files the checks need: jobs (with `needs`) and steps
//! (with `env`, `run`, `uses`). It is line based on purpose; it reads the indentation, not the YAML grammar. The port of
//! `util/workflows.mjs`, with the same quirks (the parity test compares both on every workflow of the tree).

use crate::check::jsre::{DOT, is_js_space, rx, trim, units};
use regex::Regex;
use std::sync::LazyLock;

/// One job: `id`, the 1-based `line` of its header, its `needs`, and its `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    /// The job id.
    pub id: String,
    /// The 1-based line of the header.
    pub line: usize,
    /// The jobs it needs.
    pub needs: Vec<String>,
    /// The job's lines, joined by `\n`.
    pub text: String,
}

/// One step. `run`, `uses` and `name` are `None` when the step has no such key; `env` keeps the order of first appearance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// The `name:` value.
    pub name: Option<String>,
    /// The 1-based line of the `- ` marker.
    pub line: usize,
    /// The step's lines, the `- ` marker turned into indentation, joined by `\n`.
    pub text: String,
    /// The `run:` value (a block scalar comes back as its trimmed lines joined by `\n`).
    pub run: Option<String>,
    /// The `uses:` value.
    pub uses: Option<String>,
    /// The step's own `env:` entries.
    pub env: Vec<(String, String)>,
}

/// A workflow file: path, text and what was read from it.
#[derive(Clone, Debug)]
pub struct Workflow {
    /// The repository path.
    pub file: String,
    /// The text (empty when it cannot be read).
    pub text: String,
    /// Its jobs.
    pub jobs: Vec<Job>,
    /// Its steps (every `steps:` list of the file).
    pub steps: Vec<Step>,
}

static JOBS_HEAD: LazyLock<Regex> = LazyLock::new(|| rx(r"^jobs:\s*$"));
static JOB_HEAD: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*[\w.-]+:\s*$"));
static STEPS_HEAD: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*steps:\s*$"));
static STEP_MARK: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*-\s"));
static NEEDS: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"^(\s*)needs:\s*({DOT}*)$")));
static COMMENT: LazyLock<Regex> = LazyLock::new(|| rx(&format!("#{DOT}*$")));
static LEAD_DASH: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*-\s*"));
static MARKER: LazyLock<Regex> = LazyLock::new(|| rx(r"^(\s*)-\s"));
static AFTER_KEY: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"^\s*({DOT}*)$")));
static BLOCK_SCALAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[|>][+-]?$").unwrap_or_else(|e| panic!("{e}")));
static ENV_PAIR: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"^\s*([A-Za-z_][\w]*):\s*({DOT}*)$")));

/// JavaScript's `l.length - l.trimStart().length`: the leading white space, in UTF-16 units.
fn indent(l: &str) -> usize {
    units(&l[..l.len() - l.trim_start_matches(is_js_space).len()])
}

fn is_blank(l: &str) -> bool {
    let t = trim(l);
    t.is_empty() || t.starts_with('#')
}

/// The indexes of the lines after `i` that are indented deeper than `base` (blank and comment lines kept).
fn block_after(lines: &[&str], i: usize, base: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for (j, l) in lines.iter().enumerate().skip(i + 1) {
        if !is_blank(l) && indent(l) <= base {
            break;
        }
        out.push(j);
    }
    out
}

fn strip_comment(s: &str) -> String {
    COMMENT.replace(s, "").into_owned()
}

/// The jobs of a workflow.
pub fn parse_jobs(text: &str) -> Vec<Job> {
    let lines: Vec<&str> = text.split('\n').collect();
    let Some(at) = lines.iter().position(|l| JOBS_HEAD.is_match(l)) else {
        return Vec::new();
    };
    let body = block_after(&lines, at, 0);
    let Some(&first) = body.iter().find(|&&j| !is_blank(lines[j])) else {
        return Vec::new();
    };
    let base = indent(lines[first]);
    let heads: Vec<usize> = body
        .iter()
        .copied()
        .filter(|&j| !is_blank(lines[j]) && indent(lines[j]) == base && JOB_HEAD.is_match(lines[j]))
        .collect();
    let last = body.last().map_or(0, |b| b + 1);
    heads
        .iter()
        .enumerate()
        .map(|(n, &h)| {
            let end = heads.get(n + 1).copied().unwrap_or(last);
            let job_lines = &lines[h..end];
            let id = trim(lines[h]);
            let id = id.strip_suffix(':').unwrap_or(id).to_string();
            Job {
                id,
                line: h + 1,
                needs: parse_needs(job_lines, base),
                text: job_lines.join("\n"),
            }
        })
        .collect()
}

fn parse_needs(job_lines: &[&str], base: usize) -> Vec<String> {
    for (i, l) in job_lines.iter().enumerate() {
        let Some(m) = NEEDS.captures(l) else { continue };
        if units(&m[1]) != base + 2 {
            continue;
        }
        let rest_owned = strip_comment(&m[2]);
        let rest = trim(&rest_owned);
        if rest.starts_with('[') {
            let inner = rest.strip_prefix('[').unwrap_or(rest);
            let inner = inner.strip_suffix(']').unwrap_or(inner);
            return inner.split(',').map(|s| trim(s).to_string()).filter(|s| !s.is_empty()).collect();
        }
        if !rest.is_empty() {
            return vec![rest.to_string()];
        }
        return job_lines
            .iter()
            .skip(i + 1)
            .take_while(|l| STEP_MARK.is_match(l))
            .map(|l| trim(&strip_comment(&LEAD_DASH.replace(l, ""))).to_string())
            .collect();
    }
    Vec::new()
}

/// The steps of a workflow.
pub fn parse_steps(text: &str) -> Vec<Step> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if !STEPS_HEAD.is_match(l) {
            continue;
        }
        let body = block_after(&lines, i, indent(l));
        let Some(&first) = body.iter().find(|&&j| !is_blank(lines[j])) else {
            continue;
        };
        let base = indent(lines[first]);
        let heads: Vec<usize> = body
            .iter()
            .copied()
            .filter(|&j| indent(lines[j]) == base && STEP_MARK.is_match(lines[j]))
            .collect();
        let last = body.last().map_or(0, |b| b + 1);
        for (n, &h) in heads.iter().enumerate() {
            let end = heads.get(n + 1).copied().unwrap_or(last);
            out.push(describe_step(&lines[h..end], h + 1));
        }
    }
    out
}

/// The value of the step's own `key:` at column `base` (exactly `base` white-space units before it).
fn key_value(norm: &[String], base: usize, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    for (i, l) in norm.iter().enumerate() {
        if indent(l) != base {
            continue;
        }
        let Some(rest) = l.trim_start_matches(is_js_space).strip_prefix(&prefix) else {
            continue;
        };
        let Some(m) = AFTER_KEY.captures(rest) else { continue };
        let v = trim(&m[1]);
        if BLOCK_SCALAR.is_match(v) {
            let block: Vec<&str> = norm
                .iter()
                .skip(i + 1)
                .take_while(|l| is_blank(l) || indent(l) > base)
                .map(|l| trim(l))
                .collect();
            return Some(block.join("\n"));
        }
        return Some(v.to_string());
    }
    None
}

fn describe_step(step_lines: &[&str], line: usize) -> Step {
    // The "- " marker becomes indentation, so every key of the step sits at the same column.
    let norm: Vec<String> = step_lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if i == 0 {
                MARKER.replace(l, "${1}  ").into_owned()
            } else {
                l.to_string()
            }
        })
        .collect();
    let base = indent(&norm[0]);
    let mut env: Vec<(String, String)> = Vec::new();
    let env_at = norm.iter().position(|l| {
        indent(l) == base
            && l.trim_start_matches(is_js_space)
                .strip_prefix("env:")
                .is_some_and(|r| r.chars().all(is_js_space))
    });
    if let Some(e) = env_at {
        for l in norm.iter().skip(e + 1).take_while(|l| is_blank(l) || indent(l) > base) {
            if let Some(m) = ENV_PAIR.captures(l) {
                let (k, v) = (m[1].to_string(), m[2].to_string());
                match env.iter_mut().find(|(ek, _)| *ek == k) {
                    Some(slot) => slot.1 = v,
                    None => env.push((k, v)),
                }
            }
        }
    }
    Step {
        name: key_value(&norm, base, "name"),
        line,
        text: norm.join("\n"),
        run: key_value(&norm, base, "run"),
        uses: key_value(&norm, base, "uses"),
        env,
    }
}

/// Reads one workflow file's jobs and steps.
pub fn read(file: &str, text: &str) -> Workflow {
    Workflow {
        file: file.to_string(),
        text: text.to_string(),
        jobs: parse_jobs(text),
        steps: parse_steps(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WF: &str = "name: x\njobs:\n  a:\n    needs: [b, c]\n    steps:\n      - name: one\n        run: |\n          echo hi\n          echo there\n        env:\n          K: v\n          L: w\n      - uses: ./.gitea/actions/x\n  b:\n    needs:\n      - a # why\n    runs-on: x\n";

    #[test]
    fn jobs_have_ids_lines_needs_and_text() {
        let jobs = parse_jobs(WF);
        assert_eq!(jobs.iter().map(|j| j.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(jobs[0].line, 3);
        assert_eq!(jobs[0].needs, ["b", "c"]);
        assert_eq!(jobs[1].needs, ["a"]);
        assert!(jobs[0].text.contains("echo there"));
    }

    #[test]
    fn steps_have_name_run_uses_and_env() {
        let steps = parse_steps(WF);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].name.as_deref(), Some("one"));
        assert_eq!(steps[0].run.as_deref(), Some("echo hi\necho there"));
        assert_eq!(steps[0].env, [("K".into(), "v".into()), ("L".into(), "w".into())]);
        assert_eq!(steps[1].uses.as_deref(), Some("./.gitea/actions/x"));
        assert_eq!(steps[1].line, 13);
        assert_eq!(steps[1].name, None);
    }

    #[test]
    fn a_file_without_jobs_has_none() {
        assert!(parse_jobs("name: x\n").is_empty());
        assert!(parse_steps("name: x\n").is_empty());
    }
}
