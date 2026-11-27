//! C180: under `pipefail`, a pipeline that ends in a reader that stops early is not a test (ER-21 class, ER-33, ER-45; rule 82; user
//! decision 130a, 2026-11-27). `producer | grep -q pat` exits 141 when grep leaves at the first match and the producer is still
//! writing (SIGPIPE), so the pipeline reports failure for a pattern that matched. Seen 3+ times: the shell demo's macOS verify said "DMG is not
//! signed" for a signed DMG (docs/lessons.md), and run 9247 said "check broken" for ci.yml's throttle graph probe. The same holds for
//! `grep -m N` and for `head`, whose capture of the first lines fails the same way.
//!
//! What is read, and how:
//! - a workflow's or composite action's `run:` steps, through `paddock_ci_config::yaml` (ER-42), and every `.sh` and `.bash` file;
//! - the shell text is parsed with tree-sitter-bash (`pipeline` and `command` nodes), never matched as text: a comment, a quoted string
//!   and a here-document body are no pipelines, and a here-string (`grep -q pat <<<"$out"`) is no pipeline either;
//! - a stage is flagged when it is not the first of its pipeline and is `grep`, `egrep`, `fgrep` or `rg` with `-q`, `--quiet`, `--silent`
//!   or `-m N`, or is `head` (a negative count reads everything and is fine).
//!
//! Where the shell has `pipefail` (the check reads it, it does not guess): a `.sh` file from the `set -o pipefail` (or `set -eo pipefail`)
//! command that enables it until a `set +o pipefail`; a step whose shell is `bash` or absent, from its first line (the project's own record:
//! act runs `bash -e -o pipefail {0}`, see ci.yml's "heavy tool, no GTK" step and run 9247), a step with `shell: sh`, `cmd`, `pwsh`,
//! `powershell` or `python` never. A script without the option is not judged: its pipelines cannot fail this way. A `.ps1` file has no
//! `grep` and no `pipefail`: not read. A script that is only sourced reads the caller's options: not followed, so a missed site is the
//! cost, never a false one.
//!
//! Each finding names the safe form: match a here-string (`grep -q pat <<<"$out"`) or capture the output first (`out=$(producer)`) and
//! match that; the first line of a capture is `${out%%$'\n'*}`.
//!
//! Report-only (warnings, mode report-only; ER-41) until its first green run on the real tree, recorded here and in docs/gates.toml
//! (`[[report_only]]` C180). No baseline file (ER-46): the report is the list. Rust host only: born in Rust.

use super::finding::Finding;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use paddock_ci_config::yaml::{self, Node};
use tree_sitter::{Node as TsNode, Parser};

/// The rule field: rule 82, checks and probes have no side effects and never exit silently under `set -e` or `pipefail` (ER-45).
const RULE: u32 = 82;

/// How a stage leaves its input early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `grep -q`, `--quiet` or `--silent`: exits at the first match.
    Quiet,
    /// `grep -m N`: exits after N matches.
    MaxCount,
    /// `head` with a count: exits after the lines it wants.
    Head,
}

/// One flagged stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// The 1-based line inside the scanned text.
    pub line: usize,
    /// How the stage stops early.
    pub shape: Shape,
    /// The stage as written, on one line and cut at 60 characters.
    pub stage: String,
}

/// The text of a node, empty when it is not UTF-8.
fn text<'a>(node: TsNode, src: &'a str) -> &'a str {
    node.utf8_text(src.as_bytes()).unwrap_or("")
}

/// The `command` a pipeline stage runs: through a redirection and a `!`, never into a group or a subshell.
fn stage_command(node: TsNode) -> Option<TsNode> {
    match node.kind() {
        "command" => Some(node),
        "redirected_statement" => node.child_by_field_name("body").and_then(stage_command),
        "negated_command" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor).find_map(stage_command)
        }
        _ => None,
    }
}

/// The program name (its last path segment) and the arguments of a `command`.
fn name_and_args(cmd: TsNode, src: &str) -> (String, Vec<String>) {
    let mut name = String::new();
    let mut args = Vec::new();
    let mut cursor = cmd.walk();
    for child in cmd.named_children(&mut cursor) {
        match child.kind() {
            "command_name" => name = text(child, src).rsplit('/').next().unwrap_or("").to_string(),
            "variable_assignment" | "file_redirect" | "heredoc_redirect" | "herestring_redirect" | "comment" => {}
            _ => args.push(text(child, src).to_string()),
        }
    }
    (name, args)
}

/// What `set` does to `pipefail`: `Some(true)` for `-o pipefail` (also `-eo pipefail`), `Some(false)` for `+o pipefail`.
fn pipefail_event(name: &str, args: &[String]) -> Option<bool> {
    if name != "set" {
        return None;
    }
    let mut prev = "";
    let mut event = None;
    for a in args {
        if a == "pipefail" && prev.contains('o') {
            if prev.starts_with('-') {
                event = Some(true);
            } else if prev.starts_with('+') {
                event = Some(false);
            }
        }
        prev = a;
    }
    event
}

/// Whether a negative count (`-n -1`, `-n-1`, `-c -5`) makes `head` read its whole input.
fn head_reads_all(args: &[String]) -> bool {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a.starts_with("-n-") || a.starts_with("-c-") {
            return true;
        }
        if (a == "-n" || a == "-c") && it.next().is_some_and(|v| v.starts_with('-')) {
            return true;
        }
    }
    false
}

/// Whether the program stops reading early, and how.
fn early_exit(name: &str, args: &[String]) -> Option<Shape> {
    match name {
        "grep" | "egrep" | "fgrep" | "rg" => {
            for a in args {
                if a == "--" {
                    break;
                }
                if let Some(long) = a.strip_prefix("--") {
                    match long {
                        "quiet" | "silent" => return Some(Shape::Quiet),
                        l if l == "max-count" || l.starts_with("max-count=") => return Some(Shape::MaxCount),
                        _ => {}
                    }
                } else if let Some(cluster) = a.strip_prefix('-') {
                    for c in cluster.chars() {
                        match c {
                            'q' => return Some(Shape::Quiet),
                            'm' => return Some(Shape::MaxCount),
                            // These take the rest of the cluster (or the next word) as their value.
                            'e' | 'f' | 'A' | 'B' | 'C' | 'd' | 'D' => break,
                            _ => {}
                        }
                    }
                }
            }
            None
        }
        "head" => (!head_reads_all(args)).then_some(Shape::Head),
        _ => None,
    }
}

/// The flagged stages of shell text. `pipefail_at_start`: the shell starts with `pipefail` on (a bash step); a `set -o pipefail` later in
/// the text switches it on, a `set +o pipefail` off. The second value is whether the parse had errors (the sites found are still real).
pub fn scan(src: &str, pipefail_at_start: bool) -> (Vec<Site>, bool) {
    let mut parser = Parser::new();
    if parser.set_language(&tree_sitter_bash::LANGUAGE.into()).is_err() {
        return (Vec::new(), true);
    }
    let Some(tree) = parser.parse(src, None) else {
        return (Vec::new(), true);
    };
    let mut events: Vec<(usize, bool)> = Vec::new();
    let mut pipes: Vec<TsNode> = Vec::new();
    collect(tree.root_node(), src, &mut events, &mut pipes);
    let mut sites = Vec::new();
    for pipe in pipes {
        let on = events
            .iter()
            .rev()
            .find(|(at, _)| *at < pipe.start_byte())
            .map_or(pipefail_at_start, |e| e.1);
        if !on {
            continue;
        }
        let mut cursor = pipe.walk();
        let stages: Vec<TsNode> = pipe.named_children(&mut cursor).filter(|n| n.kind() != "comment").collect();
        for stage in stages.into_iter().skip(1) {
            let Some(cmd) = stage_command(stage) else { continue };
            let (name, args) = name_and_args(cmd, src);
            if let Some(shape) = early_exit(&name, &args) {
                let one_line: String = text(cmd, src).split_whitespace().collect::<Vec<_>>().join(" ");
                sites.push(Site {
                    line: cmd.start_position().row + 1,
                    shape,
                    stage: one_line.chars().take(60).collect(),
                });
            }
        }
    }
    sites.sort_by_key(|s| s.line);
    (sites, tree.root_node().has_error())
}

fn collect<'t>(node: TsNode<'t>, src: &str, events: &mut Vec<(usize, bool)>, pipes: &mut Vec<TsNode<'t>>) {
    match node.kind() {
        "pipeline" => pipes.push(node),
        "command" => {
            let (name, args) = name_and_args(node, src);
            if let Some(on) = pipefail_event(&name, &args) {
                events.push((node.start_byte(), on));
            }
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, src, events, pipes);
    }
}

/// `${{ ... }}` expressions replaced by letters of the same length (newlines kept), so the shell parser sees a word.
pub fn mask_expressions(run: &str) -> String {
    let mut out = String::with_capacity(run.len());
    let mut rest = run;
    while let Some(open) = rest.find("${{") {
        out.push_str(&rest[..open]);
        let after = &rest[open..];
        let Some(close) = after.find("}}") else {
            out.push_str(after);
            return out;
        };
        let span = &after[..close + 2];
        out.extend(span.chars().map(|c| if c == '\n' { '\n' } else { 'X' }));
        rest = &after[close + 2..];
    }
    out.push_str(rest);
    out
}

/// Whether a step's shell has `pipefail` on from its first line: absent or `bash` (act runs `bash -e -o pipefail {0}`), or a custom
/// bash line that names it. `None`: not a bash shell at all (cmd, pwsh, sh, python): nothing to judge.
pub fn step_pipefail(shell: Option<&str>) -> Option<bool> {
    match shell.map(str::trim) {
        None | Some("") | Some("bash") => Some(true),
        Some(s) if s.starts_with("bash ") => Some(s.contains("pipefail")),
        Some(_) => None,
    }
}

/// One `run:` text to judge: where it starts, the text and whether `pipefail` is on at its first line.
pub struct RunText {
    /// The 1-based file line of the text's first line.
    pub line: usize,
    /// The step's script.
    pub script: String,
    /// `pipefail` is on from the first line.
    pub pipefail: bool,
}

/// The `run:` texts of a workflow (`jobs.<id>.steps`) or a composite action (`runs.steps`), with the shell each runs in.
pub fn run_texts(doc: &Node) -> Vec<RunText> {
    let default_shell = |n: Option<&Node>| n.and_then(|d| d.get("run")).and_then(|r| r.get_text("shell")).map(str::to_string);
    let workflow_shell = default_shell(doc.get("defaults"));
    let mut lists: Vec<(Option<String>, &Node)> = Vec::new();
    if let Some(jobs) = doc.get("jobs") {
        for (_, job) in jobs.entries() {
            let shell = default_shell(job.get("defaults")).or_else(|| workflow_shell.clone());
            if let Some(steps) = job.get("steps") {
                lists.push((shell, steps));
            }
        }
    }
    if let Some(steps) = doc.get("runs").and_then(|r| r.get("steps")) {
        lists.push((None, steps));
    }
    let mut out = Vec::new();
    for (shell, steps) in lists {
        for step in steps.items() {
            let Some(run) = step.get("run") else { continue };
            let Some(script) = run.text() else { continue };
            let own = step.get_text("shell").map(str::to_string).or_else(|| shell.clone());
            let Some(pipefail) = step_pipefail(own.as_deref()) else { continue };
            out.push(RunText {
                line: run.line,
                script: mask_expressions(script),
                pipefail,
            });
        }
    }
    out
}

fn is_shell_file(rel: &str) -> bool {
    (rel.ends_with(".sh") || rel.ends_with(".bash")) && !rel.split('/').any(|s| s == "target" || s == "node_modules")
}

fn is_yaml_file(rel: &str) -> bool {
    (rel.ends_with(".yml") || rel.ends_with(".yaml")) && !rel.split('/').any(|s| s == "target" || s == "node_modules")
}

fn message(site: &Site, in_step: bool) -> String {
    let what = match site.shape {
        Shape::Quiet => "grep exits at the first match",
        Shape::MaxCount => "grep -m exits after its count",
        Shape::Head => "head exits after its lines",
    };
    format!(
        "`... | {}` under pipefail ({}): {what}, the writer gets SIGPIPE (141) and the pipeline fails although the text matched",
        site.stage,
        if in_step { "a bash step" } else { "this script" }
    )
}

const FIX: &str = "Do not pipe into an early reader: match a here-string (`grep -q pat <<<\"$out\"`) or capture the output first (`out=$(producer)`) and match that; the first line of a capture is `${out%%$'\\n'*}`.";

/// The check.
pub struct C180;

impl Check for C180 {
    fn id(&self) -> &'static str {
        "C180"
    }

    fn rule(&self) -> u32 {
        RULE
    }

    fn title(&self) -> &'static str {
        "under pipefail, no `| grep -q`, `| grep -m` or `| head`: match a here-string or the captured output"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::ReportOnly
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                // Red: a bash step with no shell key, a composite step with `shell: bash`, and a script that sets pipefail.
                (
                    ".gitea/workflows/w.yml",
                    "jobs:\n  a:\n    runs-on: x\n    steps:\n      - run: |\n          echo start\n          cargo tree | grep -q '^paddock-proc '\n          echo \"${{ matrix.os }}\" | head -n1\n",
                ),
                (
                    ".gitea/actions/b/action.yml",
                    "runs:\n  using: composite\n  steps:\n    - shell: bash\n      run: |\n        ps aux | grep -qi 'x'\n",
                ),
                (
                    "scripts/bad.sh",
                    "#!/usr/bin/env bash\nset -euo pipefail\nfoo | grep -Eq 'a'\nfoo | grep -m1 a\nif ! foo |& grep --quiet a; then :; fi\nx=$(foo | head -1)\n",
                ),
                // Green: here-string and capture, first-stage grep, other greps, a negative head count, a text and a here-document that only name
                // the shape, a shell that is not bash, a script without pipefail, pipefail switched off.
                (
                    "scripts/good.sh",
                    "#!/usr/bin/env bash\nset -euo pipefail\nout=$(foo)\ngrep -q a <<<\"$out\"\ngrep -q a file\nfoo | grep -c a\nfoo | grep -E 'q'\nfoo | head -n -1\n# foo | grep -q a\ncat <<'EOF'\nfoo | grep -q a\nEOF\necho 'foo | grep -q a'\n",
                ),
                ("scripts/nopf.sh", "#!/bin/sh\nset -e\nfoo | grep -q a\nfoo | head -1\n"),
                (
                    "scripts/off.sh",
                    "#!/usr/bin/env bash\nset -euo pipefail\nset +o pipefail\nfoo | grep -q a\n",
                ),
                (
                    ".gitea/workflows/s.yml",
                    "jobs:\n  a:\n    runs-on: x\n    defaults:\n      run:\n        shell: sh\n    steps:\n      - run: foo | grep -q a\n      - shell: cmd\n        run: foo | grep -q a\n      - shell: bash\n        run: |\n          out=$(foo)\n          grep -q a <<<\"$out\"\n",
                ),
            ],
            expect: vec![
                (".gitea/actions/b/action.yml", 6),
                (".gitea/workflows/w.yml", 7),
                (".gitea/workflows/w.yml", 8),
                ("scripts/bad.sh", 3),
                ("scripts/bad.sh", 4),
                ("scripts/bad.sh", 5),
                ("scripts/bad.sh", 6),
            ],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut findings = Vec::new();
        let (mut probes, mut scripts, mut runs, mut parse_errors) = (0, 0, 0, 0);
        let mut completed = true;
        let mut unread: Vec<String> = Vec::new();
        for f in ctx.files().iter() {
            if is_shell_file(f) {
                let Some(src) = ctx.read(f) else { continue };
                probes += 1;
                scripts += 1;
                let (sites, bad) = scan(&src, false);
                parse_errors += usize::from(bad);
                for s in sites {
                    findings.push(Finding::warn("C180", RULE, f, s.line, message(&s, false), FIX));
                }
            } else if is_yaml_file(f) {
                let Some(src) = ctx.read(f) else { continue };
                if !src.contains("steps:") {
                    continue;
                }
                let doc = match yaml::parse(&src) {
                    Ok(d) => d,
                    Err(e) => {
                        // A workflow or action the reader cannot read was not looked at (ER-33); other YAML is not ours.
                        if f.starts_with(".gitea/") {
                            completed = false;
                            unread.push(format!("{f}: {e}"));
                        }
                        continue;
                    }
                };
                for run in run_texts(&doc) {
                    probes += 1;
                    runs += 1;
                    let (sites, bad) = scan(&run.script, run.pipefail);
                    parse_errors += usize::from(bad);
                    for s in sites {
                        findings.push(Finding::warn("C180", RULE, f, run.line + s.line - 1, message(&s, true), FIX));
                    }
                }
            }
        }
        CheckOutput {
            findings,
            probes,
            completed,
            precondition: Precondition {
                met: probes > 0 || ctx.is_in_memory(),
                detail: format!(
                    "{scripts} shell scripts and {runs} run steps read ({parse_errors} with parse errors, scanned as far as they parsed){}",
                    if unread.is_empty() {
                        String::new()
                    } else {
                        format!("; not read: {}", unread.join("; "))
                    }
                ),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str, pipefail: bool) -> Vec<usize> {
        scan(text, pipefail).0.into_iter().map(|s| s.line).collect()
    }

    #[test]
    fn an_early_reader_after_a_pipe_is_a_site_only_with_pipefail_on() {
        let t = "foo | grep -q a\nfoo | grep -qi a\nfoo | grep -Eq a\nfoo | grep -qE a\nfoo | grep -m1 a\nfoo | grep -m 2 a\nfoo | grep --quiet a\nfoo | head -n1\nfoo | head -1\nfoo | head\n";
        assert_eq!(lines(t, true), (1..=10).collect::<Vec<_>>());
        assert!(lines(t, false).is_empty(), "no pipefail, no SIGPIPE failure");
        assert_eq!(lines(&format!("set -euo pipefail\n{t}"), false).len(), 10);
        assert!(lines(&format!("set -o pipefail\nset +o pipefail\n{t}"), false).is_empty());
    }

    #[test]
    fn readers_that_read_everything_and_the_first_stage_are_no_sites() {
        for t in [
            "grep -q a file\n",
            "foo | grep a\n",
            "foo | grep -c a\n",
            "foo | grep -e q\n",
            "foo | grep -E '^(a|q)'\n",
            "foo | head -n -1\n",
            "foo | head -n-2\n",
            "grep -q a <<<\"$out\"\n",
            "foo | sed -n 1p\n",
            "# foo | grep -q a\n",
            "echo 'foo | grep -q a'\n",
            "cat <<EOF\nfoo | grep -q a\nEOF\n",
        ] {
            assert!(lines(t, true).is_empty(), "{t:?}: {:?}", scan(t, true).0);
        }
    }

    #[test]
    fn a_stage_in_a_substitution_a_condition_or_with_a_redirect_is_seen() {
        assert_eq!(lines("x=$(foo | head -1)\n", true), vec![1]);
        assert_eq!(lines("if foo | grep -q a; then :; fi\n", true), vec![1]);
        assert_eq!(lines("! foo | grep -q a\n", true), vec![1]);
        assert_eq!(lines("foo | grep -q a 2>/dev/null\n", true), vec![1]);
        assert_eq!(lines("foo | /usr/bin/grep -q a\n", true), vec![1]);
        assert_eq!(lines("foo |\n  grep -q a\n", true), vec![2]);
    }

    #[test]
    fn the_site_names_its_stage() {
        let (s, _) = scan("foo | grep -q '^paddock-proc '\n", true);
        assert_eq!(s[0].stage, "grep -q '^paddock-proc '");
        assert_eq!(s[0].shape, Shape::Quiet);
    }

    #[test]
    fn expressions_are_masked_to_words_of_the_same_length() {
        assert_eq!(mask_expressions("a ${{ x }} b\nc ${{ y\n}} d"), "a XXXXXXXX b\nc XXXXXX\nXX d");
        assert_eq!(mask_expressions("open ${{ never"), "open ${{ never");
    }

    #[test]
    fn the_shell_of_a_step_decides_whether_pipefail_is_on() {
        assert_eq!(step_pipefail(None), Some(true));
        assert_eq!(step_pipefail(Some("bash")), Some(true));
        assert_eq!(step_pipefail(Some("bash -e {0}")), Some(false));
        assert_eq!(step_pipefail(Some("bash -eo pipefail {0}")), Some(true));
        for other in ["sh", "cmd", "pwsh", "powershell", "python"] {
            assert_eq!(step_pipefail(Some(other)), None, "{other}");
        }
    }

    #[test]
    fn run_steps_are_read_with_their_lines_and_shell_defaults() {
        let doc = yaml::parse(
            "defaults:\n  run:\n    shell: cmd\njobs:\n  a:\n    defaults:\n      run:\n        shell: bash\n    steps:\n      - run: |\n          foo | grep -q a\n  b:\n    steps:\n      - run: foo | grep -q a\n",
        )
        .unwrap();
        let runs = run_texts(&doc);
        assert_eq!(runs.len(), 1, "job b inherits the workflow's cmd and is skipped");
        assert_eq!(runs[0].line, 11);
        assert!(runs[0].pipefail);
    }
}
