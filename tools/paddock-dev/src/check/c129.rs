//! C129: no new shell (the shell-to-Rust plan, docs/plans/shell-to-rust.md §6 and WP-0; ER-21 class "a shell script behaves differently
//! on macOS, Linux and Git Bash": run 8138, the prune and du/stat fixes of train 12). Shell and Python scripts are being ported to
//! Rust; this check keeps new ones out while the port runs.
//!
//! Flagged, one finding per file at line 1: a file named `*.sh`, `*.bash`, `*.zsh`, `*.ps1`, `*.psm1`, `*.py`, `*.cmd` or `*.bat`, or an
//! extensionless file whose first line is a `#!` naming `sh`, `bash`, `zsh`, `python` or `pwsh` (also through `env`), unless it is an
//! exception (a path or pattern in `conformance/shell-exceptions.json` with its plan id E1 to E7, a reason and an owner) or is in the
//! baseline `conformance/shell-baseline.json`: today's files with their line counts, one per line, sorted by path. A file with more lines
//! than its entry has grown (a finding); fewer is fine. The baseline may only shrink: an entry whose file is gone is stale (a warning), and
//! an entry the merge base's baseline lacks, or one with more lines than there, is a finding. `node conformance/bin/paddock-shell-baseline.mjs
//! --shrink` regenerates it (removes stale entries and lowers counts, never adds or raises; `--init` writes it from scratch).
//!
//! Also reported, never failing the run: a `.sh` or `.ps1` path that `.gitea` workflows and actions, `paddock.ci.json` files and
//! `lefthook.yml` name but the tree lacks, and one summary line with the count of inline `run:` lines in `.gitea` (what D7 of the plan is
//! decided on). A file that cannot be read is not measured (an error at that file); an unreadable or malformed baseline or exception
//! list makes the check broken (one finding). **Report-only (warnings) until its first green run on the real tree is recorded here
//! (ER-41).** Recorded: not yet.
//!
//! The JavaScript check `conformance/src/checks/C129-no-new-shell.mjs` is the original; `tests/it/check_c129.rs` compares the two finding
//! for finding.

use super::baseline::{Growth, growth};
use super::finding::Finding;
use super::jsre::{DOT, is_js_space, rx, trim, units};
use super::outcome::Outcome;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use regex::Regex;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

/// The baseline both hosts read.
pub const BASELINE: &str = "conformance/shell-baseline.json";
/// The exceptions both hosts read.
pub const EXCEPTIONS: &str = "conformance/shell-exceptions.json";
const REGENERATE: &str = "node conformance/bin/paddock-shell-baseline.mjs --shrink";
const EXCEPTION_IDS: [&str; 7] = ["E1", "E2", "E3", "E4", "E5", "E6", "E7"];
const FIX_NEW: &str = "Port it to Rust (`./dev`, an xtask or a check), or name it in conformance/shell-exceptions.json with the plan's exception id and a reason (docs/plans/shell-to-rust.md §2).";

static KINDS: LazyLock<Regex> = LazyLock::new(|| rx(r"(?i)\.(?:sh|bash|zsh|ps1|psm1|py|cmd|bat)$"));
static SHEBANG: LazyLock<Regex> =
    LazyLock::new(|| rx(r"^#![ \t]*(?:\S*/)?(?:env[ \t]+(?:-\S+[ \t]+)*)?(?:sh|bash|zsh|python[0-9.]*|pwsh)(?:[ \t]|$)"));
static SCRIPT_WORD: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:\./)?(?:[A-Za-z0-9_.@-]+/)+[A-Za-z0-9_.@-]+\.(?:sh|ps1)$"));
static NOT_A_WORD: LazyLock<Regex> = LazyLock::new(|| rx(r"[^A-Za-z0-9_.@/$*{}-]+"));
static RUN_LINE: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"^([ \t]*)(-[ \t]+)?run:[ \t]*({DOT}*)$")));
static RUN_BLOCK: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"^[|>][+-]?[0-9]*[ \t]*(?:#{DOT}*)?$")));

/// A path pattern as a regular expression: `**/` is any number of folders, `*` one name, a trailing `/` the folder and all below it.
pub fn glob_regex(pattern: &str) -> Regex {
    let mut src = String::from("^");
    let mut rest = pattern;
    while let Some(c) = rest.chars().next() {
        if let Some(r) = rest.strip_prefix("**/") {
            src.push_str("(?:[^/]+/)*");
            rest = r;
            continue;
        }
        if c == '*' {
            src.push_str("[^/]*");
        } else {
            src.push_str(&regex::escape(&c.to_string()));
        }
        rest = &rest[c.len_utf8()..];
    }
    if pattern.ends_with('/') {
        src.push_str(".*");
    }
    src.push('$');
    rx(&src)
}

/// One baseline entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The file.
    pub path: String,
    /// Its line count when listed.
    pub lines: usize,
}

/// One exception.
#[derive(Debug)]
pub struct Exception {
    /// The path or pattern.
    pub path: String,
    /// E1 to E7.
    pub id: String,
    re: Regex,
}

fn parse_json(text: &str, file: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(text).map_err(|e| format!("{file} is not JSON: {e}"))
}

/// A whole, non-negative JSON number (`3` and `3.0`, as JavaScript's `Number.isInteger` has them).
fn whole(v: &serde_json::Value) -> Option<usize> {
    v.as_u64().map(|n| n as usize).or_else(|| {
        v.as_f64()
            .filter(|f| *f >= 0.0 && f.fract() == 0.0 && *f < 9e15)
            .map(|f| f as usize)
    })
}

/// The baseline's entries, or why it cannot be read.
pub fn parse_baseline(text: &str) -> Result<Vec<Entry>, String> {
    let v = parse_json(text, BASELINE)?;
    let Some(files) = v["files"].as_array() else {
        return Err(format!("{BASELINE} has no `files` array"));
    };
    files
        .iter()
        .map(|e| {
            let path = e["path"].as_str().unwrap_or("");
            match whole(&e["lines"]) {
                Some(lines) if !path.is_empty() => Ok(Entry { path: path.into(), lines }),
                _ => Err(format!("{BASELINE}: every entry needs a path and a whole number of lines")),
            }
        })
        .collect()
}

/// The exceptions, or why the list cannot be read.
pub fn parse_exceptions(text: &str) -> Result<Vec<Exception>, String> {
    let v = parse_json(text, EXCEPTIONS)?;
    let Some(list) = v["exceptions"].as_array() else {
        return Err(format!("{EXCEPTIONS} has no `exceptions` array"));
    };
    list.iter()
        .map(|e| {
            let path = e["path"].as_str().unwrap_or("");
            let id = e["id"].as_str().unwrap_or("");
            let reason = trim(e["reason"].as_str().unwrap_or(""));
            let owner = trim(e["owner"].as_str().unwrap_or(""));
            if path.is_empty() || !EXCEPTION_IDS.contains(&id) || reason.chars().count() < 8 || owner.is_empty() {
                return Err(format!(
                    "{EXCEPTIONS}: every exception needs a path, an id from E1 to E7, a reason of at least 8 characters and an owner"
                ));
            }
            Ok(Exception {
                path: path.into(),
                id: id.into(),
                re: glob_regex(path),
            })
        })
        .collect()
}

/// The number of lines of `text` (a last line without a newline counts).
pub fn line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.split('\n').count() - usize::from(text.ends_with('\n'))
    }
}

fn base_name(f: &str) -> &str {
    f.rsplit('/').next().unwrap_or(f)
}

fn dir_name(f: &str) -> &str {
    f.rfind('/').map_or("", |i| &f[..i])
}

/// Whether the first line of `text` is a shell or Python shebang.
pub fn shebang(text: &str) -> bool {
    let first = text.split('\n').next().unwrap_or("");
    SHEBANG.is_match(first.strip_suffix('\r').unwrap_or(first))
}

/// What a run found among the files it read.
pub struct Found {
    /// Shell files no exception covers: `(file, lines)`.
    pub shell: Vec<(String, usize)>,
    /// How many files each exception covered, by index.
    pub hits: HashMap<usize, usize>,
    /// Files that could not be read.
    pub unread: Vec<String>,
}

/// The shell files of `files` (sorted) that no exception covers.
pub fn shell_files(ctx: &Ctx, files: &[String], exceptions: &[Exception]) -> Found {
    let mut found = Found {
        shell: Vec::new(),
        hits: HashMap::new(),
        unread: Vec::new(),
    };
    for f in files {
        // `None`: not read yet; `Some(None)`: could not be read.
        let mut text: Option<Option<std::rc::Rc<String>>> = None;
        let shell = if KINDS.is_match(f) {
            true
        } else if base_name(f).rfind('.').is_some_and(|i| i > 0) {
            false
        } else {
            let t = ctx.read(f);
            text = Some(t.clone());
            t.is_some_and(|t| !t.contains('\0') && shebang(&t))
        };
        if !shell {
            if matches!(text, Some(None)) {
                found.unread.push(f.clone());
            }
            continue;
        }
        if let Some(at) = exceptions.iter().position(|e| e.re.is_match(f)) {
            *found.hits.entry(at).or_default() += 1;
            continue;
        }
        let text = text.unwrap_or_else(|| ctx.read(f));
        match text {
            Some(t) => found.shell.push((f.clone(), line_count(&t))),
            None => found.unread.push(f.clone()),
        }
    }
    found
}

fn trim_start(s: &str) -> &str {
    s.trim_start_matches(is_js_space)
}

fn script_words(line: &str) -> Vec<String> {
    if trim_start(line).starts_with('#') {
        return Vec::new();
    }
    NOT_A_WORD
        .split(line)
        .filter(|w| SCRIPT_WORD.is_match(w))
        .map(|w| w.strip_prefix("./").unwrap_or(w).to_string())
        .collect()
}

fn is_caller(f: &str) -> bool {
    (f.starts_with(".gitea/") && (f.ends_with(".yml") || f.ends_with(".yaml"))) || base_name(f) == "paddock.ci.json" || f == "lefthook.yml"
}

/// Inline `run:` lines of one workflow or action file: `(blocks, lines)`: non-blank, non-comment lines of each `run:` value.
pub fn inline_run(text: &str) -> (usize, usize) {
    let rows: Vec<&str> = text.split('\n').collect();
    let (mut blocks, mut lines) = (0, 0);
    let mut i = 0;
    while i < rows.len() {
        let row = rows[i].strip_suffix('\r').unwrap_or(rows[i]);
        i += 1;
        let Some(m) = RUN_LINE.captures(row) else { continue };
        if trim_start(rows[i - 1]).starts_with('#') {
            continue;
        }
        let rest = trim(m.get(3).map_or("", |g| g.as_str()));
        if RUN_BLOCK.is_match(rest) {
            let key = units(&m[1]) + m.get(2).map_or(0, |g| units(g.as_str()));
            let mut n = 0;
            let mut j = i;
            while j < rows.len() {
                let r = rows[j].strip_suffix('\r').unwrap_or(rows[j]);
                if trim(r).is_empty() {
                    j += 1;
                    continue;
                }
                if units(r) - units(trim_start(r)) <= key {
                    break;
                }
                if !trim_start(r).starts_with('#') {
                    n += 1;
                }
                j += 1;
            }
            blocks += 1;
            lines += n;
            i = j;
        } else if !rest.is_empty() && !rest.starts_with('#') {
            blocks += 1;
            lines += 1;
        }
    }
    (blocks, lines)
}

fn broken(file: &str, why: &str, fix: &str) -> CheckOutput {
    CheckOutput {
        findings: vec![
            Finding::warn("C129", 55, file, 1, format!("the check cannot look: {why}"), fix)
                .unwaivable()
                .with_outcome(Outcome::CheckBroken),
        ],
        probes: 1,
        completed: true,
        precondition: Precondition {
            met: true,
            detail: format!("{file} is read"),
        },
    }
}

fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

/// The check.
pub struct C129;

impl Check for C129 {
    fn id(&self) -> &'static str {
        "C129"
    }

    fn rule(&self) -> u32 {
        55
    }

    fn title(&self) -> &'static str {
        "No new shell"
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
                (BASELINE, "{\"files\":[{\"path\":\"old.sh\",\"lines\":1}]}"),
                (
                    EXCEPTIONS,
                    "{\"exceptions\":[{\"path\":\"ok.sh\",\"id\":\"E1\",\"reason\":\"it must start first\",\"owner\":\"plan\"}]}",
                ),
                ("bad.sh", "echo hi\n"),
                ("old.sh", "echo hi\n"),
                ("ok.sh", "echo hi\n"),
                ("notes.txt", "#!/bin/sh\n"),
            ],
            expect: vec![("bad.sh", 1)],
        }
    }

    #[allow(clippy::too_many_lines)]
    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let restore_baseline =
            "Regenerate it: `node conformance/bin/paddock-shell-baseline.mjs --init` (the first run only), or restore it from git.";
        let restore_exceptions = format!(
            "Restore {EXCEPTIONS}: {{\"exceptions\": [{{\"path\": \"...\", \"id\": \"E1\", \"reason\": \"...\", \"owner\": \"...\"}}]}}."
        );
        let baseline = match ctx
            .read(BASELINE)
            .map(|t| parse_baseline(&t))
            .unwrap_or_else(|| Err(format!("{BASELINE} cannot be read")))
        {
            Ok(b) => b,
            Err(why) => return broken(BASELINE, &why, restore_baseline),
        };
        let exceptions = match ctx
            .read(EXCEPTIONS)
            .map(|t| parse_exceptions(&t))
            .unwrap_or_else(|| Err(format!("{EXCEPTIONS} cannot be read")))
        {
            Ok(e) => e,
            Err(why) => return broken(EXCEPTIONS, &why, &restore_exceptions),
        };
        let files = ctx.files();
        let tree = ctx.tree_files();
        let whole_run = files.len() == tree.len();
        let mut out = Vec::new();
        let found = shell_files(ctx, &files, &exceptions);
        for f in &found.unread {
            out.push(
                Finding::error(
                    "C129",
                    55,
                    f,
                    1,
                    format!("not measured: {f} could not be read, so shell in it cannot be seen"),
                    "Make the file readable (permissions) and run the check again.",
                )
                .unwaivable()
                .with_outcome(Outcome::NotMeasured),
            );
        }
        let known: HashMap<&str, usize> = baseline.iter().map(|e| (e.path.as_str(), e.lines)).collect();
        for (file, lines) in &found.shell {
            match known.get(file.as_str()) {
                None => out.push(Finding::warn(
                    "C129",
                    55,
                    file,
                    1,
                    format!("new shell file ({lines} lines): shell is being ported to Rust"),
                    FIX_NEW,
                )),
                Some(old) if lines > old => out.push(Finding::warn(
                    "C129",
                    55,
                    file,
                    1,
                    format!("shell file grew from {old} to {lines} lines: the baseline may only shrink"),
                    format!("Port the change to Rust instead of growing the script. {FIX_NEW}"),
                )),
                Some(_) => {}
            }
        }
        let current: HashSet<&str> = found.shell.iter().map(|(f, _)| f.as_str()).collect();
        let scoped: HashSet<&str> = files.iter().map(String::as_str).collect();
        let in_tree: HashSet<&str> = tree.iter().map(String::as_str).collect();
        for e in &baseline {
            // a file the run did not look at (a `--files` run) or could not read is not judged
            let judged = !in_tree.contains(e.path.as_str()) || scoped.contains(e.path.as_str());
            if judged && !current.contains(e.path.as_str()) && !found.unread.contains(&e.path) {
                out.push(Finding::warn(
                    "C129",
                    55,
                    BASELINE,
                    1,
                    format!("stale baseline entry ({}): the file is gone or is no longer shell", e.path),
                    format!("Remove it: `{REGENERATE}`."),
                ));
            }
        }
        for (i, e) in exceptions.iter().enumerate() {
            if !found.hits.contains_key(&i) && !tree.iter().any(|f| e.re.is_match(f)) {
                out.push(Finding::warn(
                    "C129",
                    55,
                    EXCEPTIONS,
                    1,
                    format!("stale exception ({}, {}) names no file", e.path, e.id),
                    format!("Remove it from {EXCEPTIONS}."),
                ));
            }
        }
        if !baseline.is_empty() {
            let entries_of = |text: &str| parse_baseline(text).ok();
            let exact = |e: &Entry| format!("{}\t{}", e.path, e.lines);
            let loose = |e: &Entry| e.path.clone();
            match growth(ctx.root(), &ctx.base_ref(), BASELINE, &baseline, &entries_of, &exact, Some(&loose)) {
                Growth::Reason(reason) => out.push(Finding::warn(
                    "C129",
                    55,
                    BASELINE,
                    1,
                    format!("the baseline's shrink-only rule is not enforced here: {reason}"),
                    "Fetch the target branch (`git fetch origin main`, full history), or set PADDOCK_BASE_REF to the ref to compare with.",
                )),
                Growth::Added { base, added, moved } => {
                    for m in &moved {
                        let now = &baseline[m.current];
                        if now.lines > m.from.lines {
                            out.push(Finding::warn(
                                "C129",
                                55,
                                BASELINE,
                                1,
                                format!(
                                    "the baseline raised {} from {} to {} lines; it may only shrink",
                                    now.path, m.from.lines, now.lines
                                ),
                                "Lower the entry again and port the change to Rust instead.",
                            ));
                        }
                    }
                    if added > 0 {
                        out.push(Finding::warn(
                            "C129",
                            55,
                            BASELINE,
                            1,
                            format!(
                                "the baseline grew: {added} {} not in the baseline at the merge base {}; it may only shrink",
                                plural(added, "entry is", "entries are"),
                                &base[..base.len().min(9)]
                            ),
                            "Remove the entries and port the scripts. Growing the baseline needs the user's approval.",
                        ));
                    }
                }
            }
        }
        let tops: BTreeSet<&str> = files
            .iter()
            .filter(|f| f.contains('/'))
            .map(|f| f.split('/').next().unwrap_or(""))
            .collect();
        let apps: BTreeSet<&str> = files
            .iter()
            .filter(|f| f.starts_with("apps/") && f.split('/').count() > 2)
            .map(|f| f.split('/').nth(1).unwrap_or(""))
            .collect();
        for f in files.iter().filter(|f| is_caller(f)) {
            let Some(text) = ctx.read(f) else { continue };
            let here = dir_name(f);
            for (i, row) in text.split('\n').enumerate() {
                for w in script_words(row) {
                    if !tops.contains(w.split('/').next().unwrap_or("")) {
                        continue;
                    }
                    let tracked = |p: &str| in_tree.contains(p);
                    if tracked(&w)
                        || (!here.is_empty() && tracked(&format!("{here}/{w}")))
                        || apps.iter().any(|a| tracked(&format!("apps/{a}/{w}")))
                    {
                        continue;
                    }
                    out.push(Finding::warn(
                        "C129",
                        55,
                        f,
                        i + 1,
                        format!("names the script `{w}`, which the tree does not have"),
                        "Switch the caller to the command that replaced the script, or restore the script.",
                    ));
                }
            }
        }
        let (mut blocks, mut lines) = (0, 0);
        for f in files
            .iter()
            .filter(|f| f.starts_with(".gitea/") && (f.ends_with(".yml") || f.ends_with(".yaml")))
        {
            let Some(text) = ctx.read(f) else { continue };
            let (b, l) = inline_run(&text);
            blocks += b;
            lines += l;
        }
        if blocks > 0 && whole_run {
            out.push(Finding::warn(
                "C129",
                55,
                ".gitea",
                1,
                format!(
                    "inline shell in .gitea: {lines} lines in {blocks} `run:` {} (reported, not a finding of a file)",
                    plural(blocks, "block", "blocks")
                ),
                "Decision D7 of docs/plans/shell-to-rust.md is taken on this number.",
            ));
        }
        CheckOutput {
            findings: out,
            probes: files.len(),
            completed: true,
            precondition: Precondition {
                met: true,
                detail: format!("{BASELINE} and {EXCEPTIONS} are read"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_counts_and_shebangs() {
        assert_eq!(["", "a", "a\n", "a\nb", "a\n\n"].map(line_count), [0, 1, 1, 2, 2]);
        assert!(shebang("#!/bin/sh\n") && shebang("#!/usr/bin/env python3\n") && shebang("#!/usr/bin/env -S bash -e\n"));
        assert!(!shebang("#!/usr/bin/env node\n") && !shebang("# !/bin/sh\n") && !shebang("#!/bin/shx\n"));
    }

    #[test]
    fn script_words_and_inline_run() {
        assert_eq!(
            script_words("run: bash ./scripts/a.sh scripts/$X.sh scripts/*.sh a.sh"),
            ["scripts/a.sh"]
        );
        assert!(script_words("  # scripts/a.sh").is_empty());
        let yaml = "jobs:\n  a:\n    steps:\n      - run: |\n          echo a\n\n          # note\n          echo b\n      - run: echo c\n        run-extra: no\n      - name: x\n        run: >-\n          one\n          two\n      # run: echo skipped\n      - uses: y";
        assert_eq!(inline_run(yaml), (3, 5));
    }

    #[test]
    fn the_lists_need_their_fields() {
        assert_eq!(
            parse_baseline("{\"files\":[{\"path\":\"a.sh\",\"lines\":2.0}]}").unwrap(),
            [Entry {
                path: "a.sh".into(),
                lines: 2
            }]
        );
        assert!(parse_baseline("{\"files\":[{\"path\":\"a.sh\",\"lines\":-1}]}").is_err());
        assert!(parse_baseline("{}").unwrap_err().contains("files"));
        assert!(parse_exceptions("{\"exceptions\":[{\"path\":\"a\",\"id\":\"E9\",\"reason\":\"long enough\",\"owner\":\"x\"}]}").is_err());
        assert!(parse_exceptions("{\"exceptions\":[{\"path\":\"a\",\"id\":\"E1\",\"reason\":\"long enough\",\"owner\":\"x\"}]}").is_ok());
        assert!(glob_regex("scripts/vm-win/*.ps1").is_match("scripts/vm-win/a.ps1"));
        assert!(!glob_regex("scripts/vm-win/*.ps1").is_match("scripts/vm-win/x/a.ps1"));
    }
}
