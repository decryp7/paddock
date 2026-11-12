//! `dev brief check <file>`: does a brief to an agent have the fields and the statements that every brief needs, and none of the
//! patterns that have gone wrong before? The template is `docs/agents/brief-template.md`.
//!
//! The check is structural. It never judges the task. A field is a line `Label: value` (a leading `- ` or `**` is ignored); its
//! block is that line and the lines after it up to the next field or heading. The known-bad patterns skip a line that says
//! "never", "not" or "do not" (a rule telling agents what to avoid is not an instruction to do it).
//!
//! Findings print as `<file>:<line>: <code>: <message>`; any finding ends with exit 5, so a script can refuse a brief.

use clap::{Args, Subcommand};
use paddock_cli::{CliError, Exit, Report};
use regex::Regex;
use serde_json::json;
use std::path::PathBuf;

/// `brief` subcommands.
#[derive(Subcommand)]
pub enum BriefCommand {
    /// Check a brief for the required fields and the known-bad patterns (docs/agents/brief-template.md).
    Check(CheckArgs),
}

/// Arguments of `brief check`.
#[derive(Args)]
pub struct CheckArgs {
    /// The brief, a Markdown file.
    file: PathBuf,
}

/// The scratch and worktree prefix an agent's files must be under.
pub const SCRATCH_ROOT: &str = "/Volumes/d8sst7/dev-targets/";
/// The longest commit subject the commit-msg hook accepts.
pub const MAX_SUBJECT: usize = 100;

/// The allowed scratch locations: a subfolder of one of these is the agent's own.
pub const SCRATCH_ROOTS: [&str; 2] = ["/Volumes/d8sst7/dev-targets/", "/Volumes/d7sst7/dev-targets/"];

fn first_word(v: &str) -> &str {
    v.split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c| c == '`' || c == ',' || c == '.')
}

/// Whether `path` is a strict subfolder (one level or more) of an allowed scratch location. A root itself, a trailing-slash root
/// and any path with `..` are refused: the agent's own subfolder is the only place it may write and clean.
pub fn scratch_ok(path: &str) -> Result<(), String> {
    let trimmed = path.trim_end_matches('/');
    if path.split('/').any(|c| c == "..") {
        return Err(format!("the scratch folder `{path}` has `..`: name the agent's own subfolder"));
    }
    for root in SCRATCH_ROOTS {
        if let Some(rest) = trimmed.strip_prefix(root) {
            return if rest.is_empty() {
                Err(format!(
                    "the scratch folder `{path}` is the root {root}, not a subfolder: name your own subfolder, for example {root}scratch/<agent id>"
                ))
            } else {
                Ok(())
            };
        }
    }
    if trimmed.starts_with("/private/tmp/claude-") {
        let parts: Vec<&str> = trimmed.trim_start_matches("/private/tmp/claude-").split('/').collect();
        return match parts.as_slice() {
            [_, _, _, "scratchpad"] => Err(format!(
                "the scratch folder `{path}` is the session scratchpad root, not a subfolder: name your own subfolder of it"
            )),
            [_, _, _, "scratchpad", sub, ..] if !sub.is_empty() => Ok(()),
            _ => Err(format!("the scratch folder `{path}` is not a subfolder of the session scratchpad")),
        };
    }
    Err(format!(
        "the scratch folder `{path}` is not a subfolder of an allowed location ({} or the session scratchpad): never /tmp, /var/folders or the internal disk",
        SCRATCH_ROOTS.join(", ")
    ))
}

/// One problem in a brief.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// 1-based line, 0 for a problem of the whole file.
    pub line: usize,
    /// A stable code (`missing-field`, `bad-scratch`, ...).
    pub code: &'static str,
    /// What is wrong and what to write instead.
    pub message: String,
}

/// A field of the brief.
struct Field {
    label: String,
    line: usize,
    value: String,
    /// The value and the lines after it.
    block: String,
}

/// The fields every brief has.
const REQUIRED: [&str; 9] = [
    "role",
    "agent id",
    "worktree",
    "branch",
    "base",
    "scratch",
    "plan line",
    "gates",
    "report",
];

/// Statements every brief makes, as (code, pattern, what to say).
const STATEMENTS: [(&str, &str, &str); 7] = [
    (
        "no-agent-id-export",
        r"PADDOCK_AGENT_ID",
        "tell the agent to export PADDOCK_AGENT_ID=<role>:<task> before `./dev worktree`",
    ),
    (
        "no-claim-drop-order",
        r"(?is)claim.*drop",
        "give the worktree order: add (locks), claim, work, then drop at the end",
    ),
    (
        "no-subject-limit",
        r"100 char",
        "say commit subjects are 100 characters or fewer, with the detail in the body",
    ),
    (
        "no-attribution-rule",
        r"(?i)attribution|co-authored",
        "say there is no attribution in commits, PRs, code or docs",
    ),
    (
        "no-main-checkout-rule",
        r"(?i)main checkout",
        "say there are no git writes in the main checkout",
    ),
    (
        "no-recurring-line",
        r"Recurring:",
        "ask for the report to end with a `Recurring:` line",
    ),
    (
        "no-hand-off",
        r"(?i)tip sha|hand-off|hand off",
        "say what the hand-off reports (the tip SHA, red output, gates run and not run)",
    ),
];

/// A pattern of this file (literals, checked by the tests).
fn re(pattern: &str) -> Regex {
    #[allow(clippy::expect_used)]
    Regex::new(pattern).expect("a valid pattern")
}

fn field_re() -> Regex {
    // `Label: value`, optionally a list item and bold. Labels are short words, so prose with a colon is not a field.
    re(r"^\s*(?:[-*]\s+)?\*{0,2}([A-Za-z][A-Za-z ]{1,24}?)\*{0,2}:\*{0,2}\s*(.*)$")
}

fn fields(text: &str) -> Vec<Field> {
    let re = field_re();
    let mut out: Vec<Field> = Vec::new();
    let mut in_code = false;
    for (i, l) in text.lines().enumerate() {
        if l.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if !in_code && l.starts_with('#') {
            out.push(Field {
                label: String::new(),
                line: i + 1,
                value: String::new(),
                block: String::new(),
            });
            continue;
        }
        let caps = if in_code { None } else { re.captures(l) };
        match caps {
            Some(c) if !l.trim_start().starts_with('>') => out.push(Field {
                label: c[1].trim().to_lowercase(),
                line: i + 1,
                value: c[2].trim().to_string(),
                block: c[2].trim().to_string(),
            }),
            _ => {
                if let Some(f) = out.last_mut() {
                    f.block.push('\n');
                    f.block.push_str(l);
                }
            }
        }
    }
    out.retain(|f| !f.label.is_empty());
    out
}

fn negated(line: &str) -> bool {
    let l = line.to_lowercase();
    ["never", "not ", "no ", "don't", "do not", "instead of", "forbidden", "refuse"]
        .iter()
        .any(|w| l.contains(w))
}

fn placeholder() -> Regex {
    re(r"<[^<>\n]+>")
}

/// Checks a brief's text.
pub fn check(text: &str) -> Vec<Finding> {
    let mut found = Vec::new();
    let fs = fields(text);
    let ph = placeholder();
    for label in REQUIRED {
        match fs.iter().find(|f| f.label == label) {
            None => found.push(Finding {
                line: 0,
                code: "missing-field",
                message: format!(
                    "the field `{label}` is missing; add a line `{}: ...` (see docs/agents/brief-template.md)",
                    capitalise(label)
                ),
            }),
            Some(f) if f.value.is_empty() && f.block.trim().is_empty() => found.push(Finding {
                line: f.line,
                code: "empty-field",
                message: format!("the field `{label}` is empty"),
            }),
            Some(f) if ph.is_match(&f.value) => found.push(Finding {
                line: f.line,
                code: "unfilled-placeholder",
                message: format!("the field `{label}` still has a <placeholder>: fill it in"),
            }),
            Some(_) => {}
        }
    }
    let get = |label: &str| fs.iter().find(|f| f.label == label);
    if let Some(f) = get("agent id") {
        let first = f.value.split(|c: char| c.is_whitespace() || c == ',').next().unwrap_or("");
        let ok = re(r"^[a-z]+:[A-Za-z0-9._-]+$");
        if !ph.is_match(&f.value) && !ok.is_match(first) {
            found.push(Finding {
                line: f.line,
                code: "bad-agent-id",
                message: format!("the agent id `{first}` is not `<role>:<task>` (for example implementer:dev-load)"),
            });
        }
    }
    if let Some(f) = get("worktree")
        && !ph.is_match(&f.value)
        && !f.value.to_lowercase().starts_with("none")
        && !f.value.starts_with(SCRATCH_ROOT)
    {
        found.push(Finding {
            line: f.line,
            code: "bad-scratch",
            message: format!("the worktree `{}` is not under {SCRATCH_ROOT}", f.value),
        });
    }
    if let Some(f) = get("scratch")
        && !ph.is_match(&f.value)
        && let Err(why) = scratch_ok(first_word(&f.value))
    {
        found.push(Finding {
            line: f.line,
            code: "bad-scratch",
            message: why,
        });
    }
    if let Some(f) = get("plan line")
        && !ph.is_match(&f.value)
    {
        let none = f.value.to_lowercase().starts_with("none");
        let quoted = f
            .block
            .lines()
            .any(|l| l.trim_start().starts_with('>') && l.trim_start().trim_start_matches('>').trim().len() > 3);
        let source = re(r"[\w./-]+\.(?:md|toml|rs|ts|yml|json):\d+").is_match(&f.block);
        if !none && !(quoted && source) {
            found.push(Finding {
                line: f.line,
                code: "plan-line-not-quoted",
                message: "quote the plan or ADR line this task implements (a `>` line) with its source as path:line, or write `none: <why>`; never from memory".into(),
            });
        }
    }
    for (code, pat, say) in STATEMENTS {
        if !re(pat).is_match(text) {
            found.push(Finding {
                line: 0,
                code,
                message: say.to_string(),
            });
        }
    }
    found.extend(patterns(text));
    found.sort_by_key(|f| f.line);
    found
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// The known-bad patterns, line by line.
fn patterns(text: &str) -> Vec<Finding> {
    let mut found = Vec::new();
    let scratch = re(r"(/private)?/var/folders|(^|[\s`'\x22=(])(/private)?/tmp(/|\b)|\$TMPDIR|\$\{TMPDIR\}|mktemp");
    let unguarded_cd = re(r"\bcd\s+[^;&|`\n]+;");
    let attribution = re(r"(?i)co-authored-by|generated with|claude-session");
    let main_git = re(
        r"git\s+-C\s+/Volumes/d8sst7/dev\.decryptology\.net/paddock/?\s+(commit|merge|checkout|switch|reset|push|rebase|add|pull|fetch|worktree|branch|tag|cherry-pick|am|apply|stash|clean|restore)\b",
    );
    let subject = re(r#"git\s+commit\b[^\n]*?\s-m\s+["']([^"'\n]*)["']"#);
    let mut release_line = None;
    let mut drop_line = None;
    for (i, l) in text.lines().enumerate() {
        let n = i + 1;
        let skip = negated(l);
        if scratch.is_match(&l.replace("/private/tmp/claude-", "")) && !skip {
            found.push(Finding {
                line: n,
                code: "bad-scratch",
                message: format!("scratch under /tmp, /var/folders or $TMPDIR: use a folder under {SCRATCH_ROOT}"),
            });
        }
        if unguarded_cd.is_match(l) && !skip {
            found.push(Finding {
                line: n,
                code: "unguarded-cd",
                message: "`cd <dir>;` runs the next command in the old folder when the cd fails: write `cd <dir> && ...` (ER-2)".into(),
            });
        }
        if attribution.is_match(l) && !skip {
            found.push(Finding {
                line: n,
                code: "attribution",
                message: "an attribution line (Co-Authored-By, Generated with, a session trailer) is forbidden (ER-5)".into(),
            });
        }
        if main_git.is_match(l) && !skip {
            found.push(Finding {
                line: n,
                code: "main-checkout-write",
                message: "a git write in the main checkout: work in your own worktree (ER-2)".into(),
            });
        }
        if l.contains("git stash") && !skip {
            found.push(Finding {
                line: n,
                code: "git-stash",
                message: "`git stash` is forbidden (ER-3): commit instead".into(),
            });
        }
        if l.contains("--no-verify") && !skip {
            found.push(Finding {
                line: n,
                code: "no-verify",
                message: "`--no-verify` skips the hooks: fix what the hook says".into(),
            });
        }
        for c in subject.captures_iter(l) {
            let len = c[1].chars().count();
            if len > MAX_SUBJECT {
                found.push(Finding {
                    line: n,
                    code: "long-subject",
                    message: format!(
                        "the commit subject is {len} characters, over {MAX_SUBJECT}: shorten it and put the detail in the body"
                    ),
                });
            }
        }
        if let Some(col) = l.find("worktree release").filter(|_| !skip && release_line.is_none()) {
            release_line = Some((n, col));
        }
        if let Some(col) = l.find("worktree drop").filter(|_| !skip && drop_line.is_none()) {
            drop_line = Some((n, col));
        }
    }
    match (release_line, drop_line) {
        (Some((r, _)), None) => found.push(Finding {
            line: r,
            code: "release-without-drop",
            message: "`worktree release` at the end leaves the worktree behind: the agent ends with `worktree drop`".into(),
        }),
        (Some(r), Some(d)) if r < d => found.push(Finding {
            line: r.0,
            code: "release-before-drop",
            message: format!("`worktree release` (line {}) comes before `worktree drop` (line {}): release unlocks the worktree and forgets its owner but removes nothing; the agent ends with drop only", r.0, d.0),
        }),
        _ => {}
    }
    found
}

/// The command: reads the file, prints the findings, ends with `Exit::Findings` when there are any.
pub fn command(c: BriefCommand) -> Result<Report, CliError> {
    let BriefCommand::Check(a) = c;
    let text = std::fs::read_to_string(&a.file).map_err(|e| {
        CliError::failure(format!("cannot read {}: {e}.", a.file.display())).with_hint("Pass the path of the brief, a Markdown file.")
    })?;
    Ok(report(&a.file.display().to_string(), &check(&text)))
}

/// The report of a check.
pub fn report(file: &str, found: &[Finding]) -> Report {
    let data = json!({
        "file": file,
        "findings": found.iter().map(|f| json!({"line": f.line, "code": f.code, "message": f.message})).collect::<Vec<_>>(),
    });
    if found.is_empty() {
        return Report::new(
            data,
            format!("{file}: the brief has every required field and no known-bad pattern."),
        );
    }
    let mut text = String::new();
    for f in found {
        text.push_str(&format!("{file}:{}: {}: {}\n", f.line, f.code, f.message));
    }
    text.push_str(&format!(
        "{} finding(s). Fix them, then run `./dev brief check {file}` again.",
        found.len()
    ));
    Report::new(data, text).with_exit(Exit::Findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str = include_str!("../../../docs/agents/brief-template.md");

    /// The template with every field filled in plausibly.
    fn filled() -> String {
        let ph = placeholder();
        let mut out = Vec::new();
        for l in TEMPLATE.lines() {
            let l = if l.starts_with("Role:") {
                "Role: implementer".to_string()
            } else if l.starts_with("Agent id:") {
                "Agent id: implementer:dev-load. Run `export PADDOCK_AGENT_ID=implementer:dev-load` first.".to_string()
            } else if l.starts_with("Worktree:") {
                "Worktree: /Volumes/d8sst7/dev-targets/wt/feat-x".to_string()
            } else if l.starts_with("Scratch:") {
                "Scratch: /Volumes/d8sst7/dev-targets/scratch/implementer-dev-load".to_string()
            } else if l.starts_with("> Source:") {
                "> Source: docs/adr/0001-x.md:12 at abc1234".to_string()
            } else if l.starts_with("Worktree order:") || l.starts_with("Main checkout:") {
                l.to_string()
            } else {
                ph.replace_all(l, "x").into_owned()
            };
            out.push(l);
        }
        out.join("\n")
    }

    fn codes(text: &str) -> Vec<&'static str> {
        check(text).iter().map(|f| f.code).collect()
    }

    #[test]
    fn the_unfilled_template_fails_on_every_placeholder_field() {
        let f = check(TEMPLATE);
        for label in ["role", "agent id", "worktree", "branch", "base", "scratch", "gates"] {
            assert!(
                f.iter()
                    .any(|f| f.code == "unfilled-placeholder" && f.message.contains(&format!("`{label}`"))),
                "{label}: {f:?}"
            );
        }
    }

    #[test]
    fn the_filled_template_passes() {
        let f = check(&filled());
        assert!(f.is_empty(), "{f:?}");
        let r = report("b.md", &f);
        assert_eq!(r.exit, Exit::Ok);
    }

    #[test]
    fn a_missing_field_is_named() {
        let text = filled().replace("Gates:", "Checks of the work:");
        let f = check(&text);
        assert!(
            f.iter().any(|f| f.code == "missing-field" && f.message.contains("`gates`")),
            "{f:?}"
        );
    }

    #[test]
    fn release_before_drop_and_release_alone_are_refused() {
        let text = format!(
            "{}\nAt the end run `./dev worktree release <path>` and then `./dev worktree drop <path>`.\n",
            filled()
        );
        assert!(codes(&text).contains(&"release-before-drop"));
        let text = format!("{}\nAt the end run `./dev worktree release <path>`.\n", filled());
        assert!(codes(&text).contains(&"release-without-drop"));
        // Drop alone, and a rule that says never release, are fine.
        let text = format!("{}\nUse drop, never `./dev worktree release`.\n", filled());
        assert!(check(&text).is_empty());
    }

    #[test]
    fn scratch_outside_the_agent_disk_is_refused() {
        for bad in ["/tmp/x", "/var/folders/ab/T/x", "$TMPDIR/x", "/private/tmp/x"] {
            let text = filled().replace("/Volumes/d8sst7/dev-targets/scratch/implementer-dev-load", bad);
            assert!(codes(&text).contains(&"bad-scratch"), "{bad}");
        }
        let text = format!("{}\nWrite the log to /tmp/out.log.\n", filled());
        assert!(codes(&text).contains(&"bad-scratch"));
        // The rule itself names /tmp and is not a finding.
        assert!(check(&filled()).is_empty());
    }

    #[test]
    fn scratch_must_be_the_agents_own_subfolder_never_a_root() {
        let with = |v: &str| filled().replace("/Volumes/d8sst7/dev-targets/scratch/implementer-dev-load", v);
        let bad_scratch = |v: &str| {
            check(&with(v))
                .iter()
                .any(|f| f.code == "bad-scratch" && f.message.contains("subfolder"))
        };
        // The roots themselves are refused.
        for root in [
            "/Volumes/d8sst7/dev-targets",
            "/Volumes/d8sst7/dev-targets/",
            "/Volumes/d7sst7/dev-targets/",
            "/private/tmp/claude-501/-Volumes-x/ab12/scratchpad",
            "/private/tmp/claude-501/-Volumes-x/ab12/scratchpad/",
        ] {
            assert!(bad_scratch(root), "{root}");
        }
        // A parent-dir escape is refused too.
        assert!(bad_scratch("/Volumes/d8sst7/dev-targets/scratch/../.."));
        // Subfolders of an allowed location pass.
        for ok in [
            "/Volumes/d8sst7/dev-targets/scratch/a",
            "/Volumes/d7sst7/dev-targets/scratch/a",
            "/private/tmp/claude-501/-Volumes-x/ab12/scratchpad/agent-a",
        ] {
            let f = check(&with(ok));
            assert!(f.is_empty(), "{ok}: {f:?}");
        }
        // Another place under /private/tmp is not the session scratchpad.
        assert!(check(&with("/private/tmp/other")).iter().any(|f| f.code == "bad-scratch"));
    }

    #[test]
    fn a_missing_scratch_field_is_refused() {
        let text: String = filled()
            .lines()
            .filter(|l| !l.starts_with("Scratch:"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            check(&text)
                .iter()
                .any(|f| f.code == "missing-field" && f.message.contains("`scratch`"))
        );
    }

    #[test]
    fn the_plan_line_must_be_quoted_with_a_source() {
        let stripped: String = filled().lines().filter(|l| !l.starts_with('>')).collect::<Vec<_>>().join("\n");
        assert!(codes(&stripped).contains(&"plan-line-not-quoted"));
        let ok = stripped.replace("Plan line: ", "Plan line: none: no plan covers this. ");
        assert!(!codes(&ok).contains(&"plan-line-not-quoted"));
    }

    #[test]
    fn a_quote_without_a_source_line_is_refused() {
        let text: String = filled()
            .lines()
            .filter(|l| !l.starts_with("> Source:"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(codes(&text).contains(&"plan-line-not-quoted"));
    }

    #[test]
    fn long_subjects_unguarded_cd_attribution_and_main_checkout_git_are_refused() {
        let long = "x".repeat(101);
        let text = format!(
            "{}\ngit commit -m \"{long}\"\ncd /Volumes/d8sst7/dev-targets/wt/a; git status\nCo-Authored-By: someone\ngit -C /Volumes/d8sst7/dev.decryptology.net/paddock merge feat/x\ngit stash\n",
            filled()
        );
        let c = codes(&text);
        for want in ["long-subject", "unguarded-cd", "attribution", "main-checkout-write", "git-stash"] {
            assert!(c.contains(&want), "{want}: {c:?}");
        }
        let ok = format!(
            "{}\ngit commit -m \"{}\"\ncd /Volumes/d8sst7/dev-targets/wt/a && git status\n",
            filled(),
            "x".repeat(100)
        );
        assert!(check(&ok).is_empty(), "{:?}", check(&ok));
    }

    #[test]
    fn a_bad_agent_id_is_refused() {
        let text = filled().replace("Agent id: implementer:dev-load.", "Agent id: dev-load.");
        assert!(codes(&text).contains(&"bad-agent-id"));
    }

    #[test]
    fn missing_statements_are_named() {
        let text: String = filled()
            .lines()
            .filter(|l| !l.contains("100 char") && !l.contains("Recurring"))
            .collect::<Vec<_>>()
            .join("\n");
        let c = codes(&text);
        assert!(c.contains(&"no-subject-limit") && c.contains(&"no-recurring-line"), "{c:?}");
    }

    #[test]
    fn the_report_lists_file_line_and_code_and_ends_with_findings() {
        let r = report(
            "b.md",
            &[Finding {
                line: 3,
                code: "bad-scratch",
                message: "m".into(),
            }],
        );
        assert_eq!(r.exit, Exit::Findings);
        assert!(r.text.starts_with("b.md:3: bad-scratch: m"), "{}", r.text);
        assert_eq!(r.data["findings"][0]["code"], "bad-scratch");
    }
}
