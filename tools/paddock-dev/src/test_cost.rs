//! Test cost classes (ADR 0121): a test of this tool that does whole-repository work says so in its name, and nextest gives each
//! class its concurrency and its time budget from one place, `tools/paddock-dev/.config/nextest.toml` (the overrides of the default
//! profile, which every profile inherits).
//!
//! | class   | name prefix of the test function | what it does                                                                 |
//! |---------|----------------------------------|-------------------------------------------------------------------------------|
//! | `graph` | `graph_`                         | runs `cargo metadata` (the repository graph, or a fixture's workspaces): the runs queue on cargo's package-cache lock |
//! | `tree`  | `tree_`                          | reads the whole real tree: every tracked file, a parse of every source, the conformance line |
//!
//! A test that does both is `graph` (its budget and group cover the walk too). Everything else is unclassed and keeps the profile's
//! default budget.
//!
//! Two checks keep the declarations true:
//!
//! - **graph, exact, at run time:** every place in this tool that starts `cargo metadata` first calls [`charge`] (or [`charge_argv`]
//!   for a command line it is handed). Under nextest in this workspace it reads `NEXTEST_TEST_GROUP` (nextest sets it for every test, and
//!   a `./dev` the test starts inherits it, ER-47) and finds a test that is not in the class's group. Once blocking, a new reader fails
//!   on its first run, on every OS, with the next step named, instead of flaking under load (runs 8694 and 8830). Outside nextest, or
//!   under another workspace's nextest, it does nothing.
//! - **tree, by measurement:** whole-tree reads have too many entry points to guard each one (git ls-files, syn, node, `./dev check`),
//!   and what makes them a class is their cost, so the cost is what is checked: `./dev ci test-cost` reads nextest's own output (a CI
//!   job log or a local run, through [`crate::nextest_log`]) and reports every unclassed test that used more than a quarter of its stop,
//!   as Bazel's `--test_verbose_timeout_warnings` reports a test whose size does not match its time.
//!
//! **Report-only first (ER-41, ADR 0121 D3), and visible.** Every judged charge is a JSON line of the run's report ([`report_path`]); a
//! misplaced one also goes to the test's stderr, and `./dev ci test-cost --report` turns the report into a `::warning::` per test and a
//! job-summary table (nextest hides a passing test's output, so stderr alone would be seen by nobody). Nothing refuses until the step that
//! records its first green CI run on the real tree by run id fills [`ENFORCED_PROFILES`].
//! `ci test-cost` exits 0 unless `--fail-on-findings`.

use std::path::{Path, PathBuf};

/// A cost class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// Runs `cargo metadata`.
    Graph,
    /// Reads the whole real tree.
    Tree,
}

impl Class {
    /// Every class, in the order a name is matched.
    pub const ALL: [Class; 2] = [Class::Graph, Class::Tree];

    /// The prefix of the test function's name that declares the class.
    pub fn prefix(self) -> &'static str {
        match self {
            Class::Graph => "graph_",
            Class::Tree => "tree_",
        }
    }

    /// The class's nextest test group.
    pub fn group(self) -> &'static str {
        match self {
            Class::Graph => "repo-graph",
            Class::Tree => "whole-tree",
        }
    }

    /// Its name in messages.
    pub fn name(self) -> &'static str {
        match self {
            Class::Graph => "graph",
            Class::Tree => "tree",
        }
    }

    /// The nextest filter that selects the class's tests: the last segment of the test's name starts with the prefix. The class table of
    /// `.config/nextest.toml` uses exactly this string (its test compares them).
    pub fn filter(self) -> String {
        format!("test(/(^|::){}[^:]*$/)", self.prefix())
    }
}

/// The class a test's full name (`module::path::function`) declares, if any.
pub fn declared(test: &str) -> Option<Class> {
    let last = test.rsplit("::").next().unwrap_or(test);
    Class::ALL.into_iter().find(|c| last.starts_with(c.prefix()))
}

/// The prefix of the tests that check the real tree without a cost class (`live_c174_the_real_tree_runs_and_blocks_nothing`).
const LIVE_PREFIX: &str = "live_";

/// Whether a test reads the whole repository, so a pass of it holds only for the tree it ran on (ADR NEW-paddock-dev-test-split §1):
/// both cost classes and the `live_` tests. The test cache keys these on the HEAD tree, never on their package's inputs.
pub fn reads_whole_repository(test: &str) -> bool {
    let last = test.rsplit("::").next().unwrap_or(test);
    declared(test).is_some() || last.starts_with(LIVE_PREFIX)
}

/// The nextest filter that selects exactly the tests [`reads_whole_repository`] names (its test compares them).
pub fn whole_repository_filter() -> String {
    let prefixes: Vec<&str> = Class::ALL.into_iter().map(Class::prefix).chain([LIVE_PREFIX]).collect();
    format!("test(/(^|::)({})[^:]*$/)", prefixes.join("|"))
}

/// The nextest profiles under which a misplaced charge fails the call. Empty: report-only everywhere (ER-41, ADR 0121 D3 as the user
/// decided it, 2026-11-24). The step that records the first green CI run of this check on the real tree, by run id here, adds
/// `default`, `ci` and `member-audits` in the same edit.
pub const ENFORCED_PROFILES: &[&str] = &[];

/// The workspace whose nextest configuration holds the class table; the check does nothing under another workspace's nextest.
const WORKSPACE: &str = "tools/paddock-dev";

/// What the environment says about a charge.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Not a test of this workspace under nextest: nothing to judge.
    Unjudged,
    /// The test is in the class's group.
    Placed,
    /// The test is not in the class's group: the message, and whether its profile enforces it.
    Misplaced {
        /// The finding, with the next step.
        message: String,
        /// The profile is in [`ENFORCED_PROFILES`].
        enforced: bool,
    },
}

/// Judges a charge of `class` from the environment `get` (a pure function of it, so the tests drive it with planted values).
pub fn judge(class: Class, get: &dyn Fn(&str) -> Option<String>) -> Verdict {
    judge_with(class, get, ENFORCED_PROFILES)
}

/// [`judge`] with the enforcing profiles given (the tests plant both modes).
pub fn judge_with(class: Class, get: &dyn Fn(&str) -> Option<String>, enforcing: &[&str]) -> Verdict {
    let Some(root) = get("NEXTEST_WORKSPACE_ROOT") else {
        return Verdict::Unjudged;
    };
    // nextest prints the root with the OS's separators; compare components.
    let normalised = root.replace('\\', "/");
    if !Path::new(normalised.trim_end_matches('/')).ends_with(WORKSPACE) {
        return Verdict::Unjudged;
    }
    let group = get("NEXTEST_TEST_GROUP").unwrap_or_else(|| "@global".into());
    if group == class.group() {
        return Verdict::Placed;
    }
    let test = get("NEXTEST_TEST_NAME").unwrap_or_else(|| "this test".into());
    let profile = get("NEXTEST_PROFILE").unwrap_or_else(|| "default".into());
    let what = match class {
        Class::Graph => "runs `cargo metadata`",
        Class::Tree => "reads the whole real tree",
    };
    let message = format!(
        "test-cost: {test} {what} but is in test group `{group}`, not `{}` (nextest profile `{profile}`). Next step: rename the test \
         function with the prefix `{}` (its class, `{}`), so tools/paddock-dev/.config/nextest.toml gives it the class's group and budget \
         (ADR 0121); or make it cheaper so it no longer {what}.",
        class.group(),
        class.prefix(),
        class.name(),
    );
    Verdict::Misplaced {
        message,
        enforced: enforcing.contains(&profile.as_str()),
    }
}

/// The variable naming the report file a CI step reads (`ci test-cost --report`). Unset, the report goes under the cargo build folder
/// of the nextest run: `<target>/nextest/test-cost/<NEXTEST_RUN_ID>.jsonl`, one file per nextest run, so runs never mix.
pub const REPORT_ENV: &str = "PADDOCK_TEST_COST_REPORT";

/// Where this process's charges are recorded: [`REPORT_ENV`], else the cargo build folder nextest ran from (the nearest ancestor of
/// `NEXTEST_BIN_EXE_paddock-dev` with cargo's `CACHEDIR.TAG`), else nowhere.
pub fn report_path(get: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(p) = get(REPORT_ENV) {
        return Some(PathBuf::from(p));
    }
    // nextest names the workspace's binaries in the build folder (`NEXTEST_BIN_EXE_paddock-dev`): the report goes beside them, never into
    // the checkout.
    let exe = PathBuf::from(get("NEXTEST_BIN_EXE_paddock-dev").or_else(|| get("NEXTEST_BIN_EXE_paddock_dev"))?);
    let target = exe.ancestors().skip(1).find(|d| d.join("CACHEDIR.TAG").is_file())?;
    let run = get("NEXTEST_RUN_ID").unwrap_or_else(|| "run".into());
    Some(target.join("nextest").join("test-cost").join(format!("{run}.jsonl")))
}

/// One charge as a JSON line of the report: every judged charge is recorded, placed or not, so a report that holds no line says the
/// wiring did not reach the tests (not measured), never "clean".
fn record_line(class: Class, call: &str, get: &dyn Fn(&str) -> Option<String>, placed: bool) -> String {
    serde_json::json!({
        "test": get("NEXTEST_TEST_NAME").unwrap_or_default(),
        "binary": get("NEXTEST_BINARY_ID").unwrap_or_default(),
        "class": class.name(),
        "group": get("NEXTEST_TEST_GROUP").unwrap_or_else(|| "@global".into()),
        "profile": get("NEXTEST_PROFILE").unwrap_or_else(|| "default".into()),
        "call": call,
        "placed": placed,
    })
    .to_string()
}

/// Appends one line in a single write (O_APPEND: whole lines from concurrent test processes). Best effort: a report that cannot be
/// written is said on stderr and never fails the test.
fn append(path: &Path, line: &str) {
    use std::io::Write;
    let done = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::OpenOptions::new().create(true).append(true).open(path))
        .and_then(|mut f| f.write_all(format!("{line}\n").as_bytes()));
    if let Err(e) = done {
        eprintln!("test-cost: the report {} could not be written: {e}", path.display());
    }
}

/// Charges `class` to the running test, for the work `call` names (`plan::read_graph`): records the charge in the report, and when the
/// test is misplaced prints the finding to stderr and returns `Err` with the next step under an enforcing profile. Call it before the work.
pub fn charge(class: Class, call: &str) -> Result<(), String> {
    let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    charge_in(class, call, &get, ENFORCED_PROFILES)
}

fn charge_in(class: Class, call: &str, get: &dyn Fn(&str) -> Option<String>, enforcing: &[&str]) -> Result<(), String> {
    let verdict = judge_with(class, get, enforcing);
    if verdict == Verdict::Unjudged {
        return Ok(());
    }
    if let Some(path) = report_path(get) {
        append(&path, &record_line(class, call, get, verdict == Verdict::Placed));
    }
    match verdict {
        Verdict::Misplaced { message, enforced: true } => Err(message),
        Verdict::Misplaced { message, enforced: false } => {
            eprintln!("{message} (report-only until its first green CI run, ER-41)");
            Ok(())
        }
        Verdict::Placed | Verdict::Unjudged => Ok(()),
    }
}

/// A misplaced charge read back from a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Misplaced {
    /// The test's name.
    pub test: String,
    /// Its binary id.
    pub binary: String,
    /// The class it charged.
    pub class: String,
    /// The group it ran in.
    pub group: String,
    /// The nextest profile.
    pub profile: String,
    /// The work that charged it.
    pub call: String,
}

/// What a report holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportRead {
    /// Charges read (placed or not).
    pub charges: usize,
    /// The misplaced ones, each test and call once, in order.
    pub misplaced: Vec<Misplaced>,
    /// Lines that are not a charge record.
    pub malformed: usize,
}

/// Reads a report's JSON lines.
pub fn read_report(text: &str) -> ReportRead {
    let mut out = ReportRead::default();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            out.malformed += 1;
            continue;
        };
        let field = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or_default().to_string();
        let Some(placed) = v.get("placed").and_then(serde_json::Value::as_bool) else {
            out.malformed += 1;
            continue;
        };
        out.charges += 1;
        if !placed {
            let m = Misplaced {
                test: field("test"),
                binary: field("binary"),
                class: field("class"),
                group: field("group"),
                profile: field("profile"),
                call: field("call"),
            };
            if !out.misplaced.contains(&m) {
                out.misplaced.push(m);
            }
        }
    }
    out
}

/// The lines `ci test-cost` prints for a charge report (`::<level>::` per misplaced charge, and "not measured" for a report with no
/// charge) and the Markdown it appends to the job summary.
pub fn render_report(rr: &ReportRead, source: &str, level: &str) -> (String, String) {
    let mut text = format!(
        "charge report {source}: {} charge(s), {} outside their group, {} malformed line(s)",
        rr.charges,
        rr.misplaced.len(),
        rr.malformed
    );
    let mut md = format!(
        "### Test cost classes (ADR 0121)\n\n{} charge(s) of a cost class read, {} outside their group.\n",
        rr.charges,
        rr.misplaced.len()
    );
    if !rr.misplaced.is_empty() {
        md.push_str("\n| test | binary | class | group | profile | call |\n|---|---|---|---|---|---|\n");
    }
    for m in &rr.misplaced {
        let prefix = Class::ALL.into_iter().find(|c| c.name() == m.class).map_or("", Class::prefix);
        text.push_str(&format!(
            "\n::{level}::ci test-cost: {} ({}) charged the {} class through {} in test group `{}` (profile {}). Next step: rename the \
             test function with the class prefix `{prefix}`, so .config/nextest.toml gives it the class's group and budget (ADR 0121).",
            m.test, m.binary, m.class, m.call, m.group, m.profile
        ));
        md.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} |\n",
            m.test, m.binary, m.class, m.group, m.profile, m.call
        ));
    }
    if rr.charges == 0 {
        text.push_str(&format!(
            "\n::{level}::ci test-cost: not measured: the charge report holds no charge (no test of this run reached a cost class, or \
             {REPORT_ENV} did not reach the tests)."
        ));
        md.push_str("\nNot measured: the report holds no charge.\n");
    }
    (text, md)
}

/// Whether a command line runs `cargo metadata` (`cargo [+toolchain] [--flags] metadata ...`).
pub fn runs_cargo_metadata<S: AsRef<str>>(program: &str, args: &[S]) -> bool {
    crate::cargo_target::is_cargo(program)
        && args
            .iter()
            .map(AsRef::as_ref)
            .find(|a| !a.starts_with('+') && !a.starts_with('-'))
            .is_some_and(|sub| sub == "metadata")
}

/// [`charge`] for a command line a runner is handed: charges [`Class::Graph`] when it runs `cargo metadata`.
pub fn charge_argv<S: AsRef<str>>(site: &str, program: &str, args: &[S]) -> Result<(), String> {
    if runs_cargo_metadata(program, args) {
        charge(Class::Graph, site)
    } else {
        Ok(())
    }
}

/// The binary ids of this workspace's tests in nextest's output (`paddock-dev`, `paddock-dev::it`, `paddock-dev::bin/dev`).
fn ours(binary: &str) -> bool {
    binary == "paddock-dev" || binary.starts_with("paddock-dev::")
}

/// One class's longest test in a report.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassStat {
    /// The class's name, or `unclassed`.
    pub class: &'static str,
    /// How many of its tests ran.
    pub tests: usize,
    /// Its longest test and that test's seconds.
    pub longest: Option<(String, f64)>,
}

/// What `ci test-cost` found in the durations of one or more nextest runs.
#[derive(Debug, Clone, PartialEq)]
pub struct CostReport {
    /// Tests of this workspace read.
    pub tests: usize,
    /// Per class (graph, tree, unclassed): count and longest test.
    pub classes: Vec<ClassStat>,
    /// Tests whose time times [`CONTENTION`] passes their stop, longest first: name and seconds.
    pub findings: Vec<(String, f64)>,
}

/// Judges the durations of this workspace's tests against their stops (ADR 0121's time tiers): a test whose time times [`CONTENTION`]
/// passes its stop is a finding, for it has no margin on a loaded host (whole-tree tests ran 5 to 15 times slower in jobs 47411 and
/// 47644). Bazel warns the same way when a test's time does not match its size. Other workspaces' tests in the same log are ignored.
pub fn report(timed: &[crate::nextest_log::Timed], stops: &Stops) -> CostReport {
    let mine: Vec<_> = timed.iter().filter(|t| ours(&t.id.0)).collect();
    let mut classes = Vec::new();
    for c in [Some(Class::Graph), Some(Class::Tree), None] {
        let of: Vec<_> = mine.iter().filter(|t| declared(&t.id.1) == c).collect();
        let longest = of.iter().max_by(|a, b| a.secs.total_cmp(&b.secs)).map(|t| (t.id.1.clone(), t.secs));
        classes.push(ClassStat {
            class: c.map_or("unclassed", Class::name),
            tests: of.len(),
            longest,
        });
    }
    let mut findings: Vec<(String, f64)> = mine
        .iter()
        .filter(|t| t.secs * CONTENTION > stops.of(&t.id.1))
        .map(|t| (t.id.1.clone(), t.secs))
        .collect();
    findings.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    CostReport {
        tests: mine.len(),
        classes,
        findings,
    }
}

/// The worst contention measured on a shared host: whole-tree tests ran 5 to 15 times their local time in jobs 47411 (run 8830 macOS,
/// two heavy jobs on the host) and 47644 (ADR 0121, the time tiers). A test's stop is at least its own time times this.
pub const CONTENTION: f64 = 15.0;

/// The stops of a nextest profile, read from `.config/nextest.toml` with the TOML parser: the profile's own `slow-timeout` (else the
/// default profile's), and each override with a `test(/regex/)` filter that sets one, in order (the first match wins, as in nextest; the
/// profile's own overrides first, then the default profile's). Overrides with other filters are not modelled and are listed.
#[derive(Debug, Clone)]
pub struct Stops {
    /// The stop of a test no override names, in seconds.
    pub base: f64,
    /// (filter regex, stop in seconds), in nextest's order.
    pub overrides: Vec<(regex::Regex, f64)>,
    /// Filters of overrides that set a stop but are not a single `test(/regex/)`.
    pub unmodelled: Vec<String>,
}

fn stop_secs(v: &toml::Value) -> Option<f64> {
    let t = v.as_table()?;
    let after = t.get("terminate-after")?.as_integer()?;
    let period = t.get("period")?.as_str()?;
    let n: f64 = period.trim_end_matches(char::is_alphabetic).parse().ok()?;
    let unit = match period.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.') {
        "ms" => 0.001,
        "s" => 1.0,
        "m" | "min" => 60.0,
        "h" => 3600.0,
        _ => return None,
    };
    Some(n * unit * after as f64)
}

/// Reads [`Stops`] for `profile` from a nextest configuration text.
pub fn stops(config: &str, profile: &str) -> Result<Stops, String> {
    let doc: toml::Table = toml::from_str(config).map_err(|e| format!("the nextest configuration does not parse: {e}"))?;
    let profiles = doc
        .get("profile")
        .and_then(|p| p.as_table())
        .ok_or("the configuration has no [profile] table")?;
    let own = profiles.get(profile).and_then(|p| p.as_table());
    let default = profiles.get("default").and_then(|p| p.as_table());
    let base = own
        .and_then(|p| p.get("slow-timeout"))
        .or_else(|| default.and_then(|p| p.get("slow-timeout")))
        .and_then(stop_secs)
        .ok_or_else(|| format!("profile {profile} has no bounded slow-timeout"))?;
    let mut overrides = Vec::new();
    let mut unmodelled = Vec::new();
    let tables: Vec<_> = if profile == "default" { vec![default] } else { vec![own, default] };
    for t in tables.into_iter().flatten() {
        for o in t.get("overrides").and_then(|o| o.as_array()).into_iter().flatten() {
            let Some(secs) = o.get("slow-timeout").and_then(stop_secs) else {
                continue;
            };
            let filter = o.get("filter").and_then(|f| f.as_str()).unwrap_or_default();
            let inner = filter.strip_prefix("test(/").and_then(|f| f.strip_suffix("/)"));
            match inner.and_then(|r| regex::Regex::new(r).ok()) {
                Some(re) if !inner.unwrap_or_default().contains("/)") => overrides.push((re, secs)),
                _ => unmodelled.push(filter.to_string()),
            }
        }
    }
    Ok(Stops {
        base,
        overrides,
        unmodelled,
    })
}

impl Stops {
    /// The stop of a test, in seconds.
    pub fn of(&self, test: &str) -> f64 {
        self.overrides
            .iter()
            .find(|(re, _)| re.is_match(test))
            .map_or(self.base, |(_, s)| *s)
    }
}

/// One `assert` in a test that compares elapsed wall time: a test a busy host can fail without a bug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WallAssert {
    /// The file, as given.
    pub file: String,
    /// The line of the assert.
    pub line: usize,
    /// The test function.
    pub test: String,
}

/// The asserts on elapsed wall time inside the `#[test]` functions of one Rust source: an assert macro whose tokens read `.elapsed()`,
/// `duration_since(` or `Instant::now()`, or a local bound from one of those. A source that does not parse gives none (a parse error is
/// the compiler's finding). Helpers outside the test function are not followed.
pub fn wall_asserts(file: &str, source: &str) -> Vec<WallAssert> {
    use syn::visit::Visit;
    fn timed(text: &str) -> bool {
        let t: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        t.contains(".elapsed()") || t.contains("duration_since(") || t.contains("Instant::now()")
    }
    /// Whether an expression reads the wall clock: `.elapsed()`, `.duration_since(..)` or `Instant::now()`.
    fn reads_clock(e: &syn::Expr) -> bool {
        struct Clock(bool);
        impl<'ast> Visit<'ast> for Clock {
            fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
                if m.method == "elapsed" || m.method == "duration_since" {
                    self.0 = true;
                }
                syn::visit::visit_expr_method_call(self, m);
            }
            fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
                if let syn::Expr::Path(p) = &*c.func {
                    let segs: Vec<String> = p.path.segments.iter().map(|s| s.ident.to_string()).collect();
                    if segs.ends_with(&["Instant".to_string(), "now".to_string()]) {
                        self.0 = true;
                    }
                }
                syn::visit::visit_expr_call(self, c);
            }
        }
        let mut c = Clock(false);
        c.visit_expr(e);
        c.0
    }
    fn is_test(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|a| a.path().segments.last().is_some_and(|s| s.ident == "test"))
    }
    struct InTest<'a> {
        file: &'a str,
        test: String,
        bound: Vec<String>,
        out: Vec<WallAssert>,
    }
    impl<'ast> Visit<'ast> for InTest<'_> {
        fn visit_local(&mut self, l: &'ast syn::Local) {
            if let (syn::Pat::Ident(id), Some(init)) = (&l.pat, &l.init)
                && reads_clock(&init.expr)
            {
                self.bound.push(id.ident.to_string());
            }
            syn::visit::visit_local(self, l);
        }
        fn visit_macro(&mut self, m: &'ast syn::Macro) {
            let name = m.path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default();
            if name.contains("assert") {
                let text = m.tokens.to_string();
                let words: Vec<&str> = text.split(|c: char| !(c.is_alphanumeric() || c == '_')).collect();
                if timed(&text) || self.bound.iter().any(|b| words.contains(&b.as_str())) {
                    self.out.push(WallAssert {
                        file: self.file.to_string(),
                        line: m.path.segments.first().map_or(0, |s| s.ident.span().start().line),
                        test: self.test.clone(),
                    });
                }
            }
            syn::visit::visit_macro(self, m);
        }
    }
    struct Find<'a> {
        file: &'a str,
        out: Vec<WallAssert>,
    }
    impl<'ast> Visit<'ast> for Find<'_> {
        fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
            if is_test(&f.attrs) {
                let mut v = InTest {
                    file: self.file,
                    test: f.sig.ident.to_string(),
                    bound: Vec::new(),
                    out: Vec::new(),
                };
                v.visit_block(&f.block);
                self.out.extend(v.out);
            }
            syn::visit::visit_item_fn(self, f);
        }
    }
    let Ok(parsed) = syn::parse_file(source) else { return Vec::new() };
    let mut f = Find { file, out: Vec::new() };
    f.visit_file(&parsed);
    f.out
}

/// [`wall_asserts`] over every `.rs` file under `dirs` (sorted, so the list is stable).
pub fn wall_asserts_in(root: &Path, dirs: &[&str]) -> Vec<WallAssert> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    for d in dirs {
        walk(&root.join(d), &mut files);
    }
    files.sort();
    let mut out = Vec::new();
    for f in files {
        if let Ok(text) = std::fs::read_to_string(&f) {
            let rel = f.strip_prefix(root).unwrap_or(&f).to_string_lossy().replace('\\', "/");
            out.extend(wall_asserts(&rel, &text));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let map: BTreeMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k: &str| map.get(k).cloned()
    }

    const ROOT: (&str, &str) = ("NEXTEST_WORKSPACE_ROOT", "/w/paddock/tools/paddock-dev");

    #[test]
    fn a_name_declares_its_class_by_the_prefix_of_its_last_segment() {
        assert_eq!(declared("golden::graph_plans_every_case"), Some(Class::Graph));
        assert_eq!(declared("check::tree_live_tree_is_clean"), Some(Class::Tree));
        assert_eq!(declared("tree_top_level"), Some(Class::Tree));
        // A module named like a prefix declares nothing; nor does a word that only contains it.
        assert_eq!(declared("graph_x::plain_test"), None);
        assert_eq!(declared("tree::of_a_fixture"), None);
        assert_eq!(declared("cli::the_graph_reads"), None);
    }

    #[test]
    fn the_whole_repository_tests_are_both_classes_and_the_live_tests_and_nothing_else() {
        for name in [
            "golden::graph_plans",
            "check::tree_is_clean",
            "check_c174::live_c174_the_real_tree_runs",
            "tree_top",
        ] {
            assert!(reads_whole_repository(name), "{name}");
        }
        for name in [
            "graph_x::plain",
            "tree::of_a_fixture",
            "cli::the_live_reads",
            "a::b_tree_c",
            "disk::state::liveness",
        ] {
            assert!(!reads_whole_repository(name), "{name}");
        }
        // Every class's tests are whole-repository tests: the class table and this target never disagree.
        for c in Class::ALL {
            assert!(reads_whole_repository(&format!("m::{}x", c.prefix())), "{c:?}");
        }
    }

    #[test]
    fn the_whole_repository_filter_selects_exactly_what_reads_whole_repository_selects() {
        let pattern = whole_repository_filter();
        let re = regex::Regex::new(pattern.trim_start_matches("test(/").trim_end_matches("/)")).unwrap();
        for name in [
            "a::graph_b",
            "a::tree_b",
            "a::live_b",
            "live_b",
            "live::b",
            "live_x::y",
            "a::b_live_c",
            "a::plain",
        ] {
            assert_eq!(re.is_match(name), reads_whole_repository(name), "{name}");
        }
    }

    #[test]
    fn the_filter_selects_exactly_what_declared_selects() {
        for c in Class::ALL {
            let pattern = c.filter();
            let re = regex::Regex::new(pattern.trim_start_matches("test(/").trim_end_matches("/)")).unwrap();
            for name in [
                "a::graph_b",
                "graph_b",
                "a::tree_b",
                "tree::b",
                "graph_x::y",
                "a::b_graph_c",
                "a::tree_",
            ] {
                assert_eq!(re.is_match(name), declared(name) == Some(c), "{c:?} {name}");
            }
        }
    }

    // Positive control (ER-33): a charge from a test outside the class's group is a finding, refused under an enforcing profile and
    // report-only under another; inside the group, or outside this workspace's nextest, it is not.
    #[test]
    fn a_charge_outside_the_class_group_is_found_and_refused_only_under_an_enforcing_profile() {
        let enforcing = &["default"];
        let out = judge_with(
            Class::Graph,
            &env(&[
                ROOT,
                ("NEXTEST_TEST_GROUP", "@global"),
                ("NEXTEST_TEST_NAME", "m::reads"),
                ("NEXTEST_PROFILE", "default"),
            ]),
            enforcing,
        );
        let Verdict::Misplaced { message, enforced } = out else {
            panic!("{out:?}")
        };
        assert!(enforced);
        assert!(
            message.contains("m::reads") && message.contains("`graph_`") && message.contains("`repo-graph`"),
            "{message}"
        );
        let out = judge_with(
            Class::Graph,
            &env(&[ROOT, ("NEXTEST_TEST_GROUP", "whole-tree"), ("NEXTEST_PROFILE", "ci")]),
            enforcing,
        );
        assert!(matches!(out, Verdict::Misplaced { enforced: false, .. }), "{out:?}");
        // No group variable at all counts as no group.
        assert!(matches!(
            judge_with(Class::Graph, &env(&[ROOT]), enforcing),
            Verdict::Misplaced { enforced: true, .. }
        ));
        // Today's mode (report-only everywhere until the first green CI run is recorded): found, never enforced.
        let out = judge(
            Class::Graph,
            &env(&[ROOT, ("NEXTEST_TEST_GROUP", "@global"), ("NEXTEST_PROFILE", "default")]),
        );
        assert!(matches!(out, Verdict::Misplaced { enforced: false, .. }), "{out:?}");
    }

    #[test]
    fn a_charge_in_its_group_or_outside_this_workspace_nextest_passes() {
        assert_eq!(
            judge(Class::Graph, &env(&[ROOT, ("NEXTEST_TEST_GROUP", "repo-graph")])),
            Verdict::Placed
        );
        assert_eq!(
            judge(
                Class::Graph,
                &env(&[
                    ("NEXTEST_WORKSPACE_ROOT", "C:\\w\\paddock\\tools\\paddock-dev\\"),
                    ("NEXTEST_TEST_GROUP", "repo-graph")
                ])
            ),
            Verdict::Placed
        );
        assert_eq!(judge(Class::Graph, &env(&[("NEXTEST_TEST_GROUP", "@global")])), Verdict::Unjudged);
        assert_eq!(
            judge(
                Class::Graph,
                &env(&[("NEXTEST_WORKSPACE_ROOT", "/w/paddock/crates"), ("NEXTEST_TEST_GROUP", "@global")])
            ),
            Verdict::Unjudged
        );
        assert_eq!(
            judge(
                Class::Graph,
                &env(&[
                    ("NEXTEST_WORKSPACE_ROOT", "/w/other-paddock-dev"),
                    ("NEXTEST_TEST_GROUP", "@global")
                ])
            ),
            Verdict::Unjudged
        );
    }

    fn timed(binary: &str, name: &str, secs: f64) -> crate::nextest_log::Timed {
        crate::nextest_log::Timed {
            id: (binary.into(), name.into()),
            secs,
            failed: false,
        }
    }

    const CONFIG: &str = r#"
[profile.default]
slow-timeout = { period = "30s", terminate-after = 4 }
[[profile.default.overrides]]
filter = "test(/^net::graph_replay_/)"
slow-timeout = { period = "300s", terminate-after = 12 }
[[profile.default.overrides]]
filter = "test(/(^|::)graph_[^:]*$/)"
slow-timeout = { period = "60s", terminate-after = 15 }
[[profile.default.overrides]]
filter = "binary(it) & test(/x/)"
slow-timeout = { period = "60s", terminate-after = 15 }
[profile.ci]
slow-timeout = { period = "60s", terminate-after = 5 }
"#;

    #[test]
    fn stops_follow_the_profile_and_the_first_matching_override() {
        let st = stops(CONFIG, "ci").unwrap();
        assert_eq!(st.base, 300.0);
        assert_eq!(st.of("net::graph_replay_all"), 3600.0, "the named row wins over the class row");
        assert_eq!(st.of("golden::graph_plans"), 900.0);
        assert_eq!(st.of("queue::tests::fast"), 300.0);
        assert_eq!(st.unmodelled, vec!["binary(it) & test(/x/)".to_string()]);
        assert_eq!(stops(CONFIG, "default").unwrap().base, 120.0);
        assert!(
            stops("[profile.default]\n", "ci").is_err(),
            "an unbounded profile is an error, never a guess"
        );
    }

    // Positive control (ER-33): a test whose time times the contention passes its stop is a finding, in a class or not; the same time
    // under a longer stop, or in another workspace, is not.
    #[test]
    fn a_test_without_margin_for_the_measured_contention_is_a_finding() {
        let st = stops(CONFIG, "ci").unwrap();
        let r = report(
            &[
                timed("paddock-dev::it", "check::live_tree_is_clean", 48.0),
                timed("paddock-dev", "ci_run::tests::the_line_runs", 20.1),
                timed("paddock-dev::it", "golden::graph_plans", 61.0),
                timed("paddock-dev::it", "net::graph_replay_all", 200.0),
                timed("paddock-dev", "queue::tests::fast", 0.2),
                timed("paddock-dev", "queue::tests::just_under", 20.0),
                timed("paddock-chromium::it", "real_browser::slow", 50.0),
            ],
            &st,
        );
        assert_eq!(r.tests, 6);
        assert_eq!(
            r.findings,
            vec![
                ("golden::graph_plans".to_string(), 61.0),
                ("check::live_tree_is_clean".to_string(), 48.0),
                ("ci_run::tests::the_line_runs".to_string(), 20.1),
            ]
        );
        assert_eq!(r.classes[0].class, "graph");
        assert_eq!(r.classes[0].longest, Some(("net::graph_replay_all".to_string(), 200.0)));
        assert_eq!(r.classes[2].tests, 4);
    }

    /// The real configuration parses into stops, and every class row and the named rows are modelled (a filter the reader cannot read
    /// would silently fall back to the base stop).
    #[test]
    fn the_real_configuration_has_a_stop_for_every_class() {
        let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".config/nextest.toml")).unwrap();
        let st = stops(&text, "ci").unwrap();
        assert!(st.unmodelled.is_empty(), "{:?}", st.unmodelled);
        for c in Class::ALL {
            let name = format!("m::{}x", c.prefix());
            assert!(st.of(&name) >= st.base, "{c:?}: {}", st.of(&name));
        }
    }

    // Positive control (ER-33): planted sources with each way of asserting on wall time are found, with the test and the line; a
    // timed helper outside a test, a test without a clock and a clock read that no assert uses are not.
    #[test]
    fn asserts_on_elapsed_wall_time_inside_tests_are_found() {
        let src = r#"
fn helper() { let t = std::time::Instant::now(); assert!(t.elapsed() < D); }
#[test]
fn direct() {
    let t = Instant::now();
    work();
    assert!(t.elapsed() < Duration::from_secs(5), "too slow");
}
#[test]
fn bound_first() {
    let started = Instant::now();
    work();
    let took = started.elapsed();
    assert!(took < LIMIT);
}
#[tokio::test]
async fn since() {
    assert!(Instant::now().duration_since(start) <= Duration::from_millis(10));
}
#[test]
fn no_clock() { assert_eq!(1 + 1, 2); }
#[test]
fn unused_clock() { let t = Instant::now(); work(); println!("{:?}", t.elapsed()); assert!(ok()); }
"#;
        let found: Vec<(usize, String)> = wall_asserts("x.rs", src).into_iter().map(|w| (w.line, w.test)).collect();
        assert_eq!(
            found,
            vec![
                (7, "direct".to_string()),
                (14, "bound_first".to_string()),
                (18, "since".to_string())
            ]
        );
        assert!(wall_asserts("x.rs", "fn broken(").is_empty());
    }

    // Positive control (ER-33): a planted misplaced charge lands in the report file as a JSON line, a placed one is counted, and the
    // reader gives the finding back; outside this workspace's nextest nothing is written.
    #[test]
    fn a_planted_misplaced_charge_appears_in_the_report() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub/cost.jsonl");
        let file_text = file.to_string_lossy().to_string();
        let report = (REPORT_ENV, file_text.as_str());
        let misplaced = env(&[
            ROOT,
            report,
            ("NEXTEST_TEST_GROUP", "@global"),
            ("NEXTEST_TEST_NAME", "m::reads"),
            ("NEXTEST_BINARY_ID", "paddock-dev::it"),
            ("NEXTEST_PROFILE", "ci"),
        ]);
        assert_eq!(charge_in(Class::Graph, "plan::read_graph", &misplaced, &[]), Ok(()));
        assert!(charge_in(Class::Graph, "plan::read_graph", &misplaced, &["ci"]).is_err());
        let placed = env(&[
            ROOT,
            report,
            ("NEXTEST_TEST_GROUP", "repo-graph"),
            ("NEXTEST_TEST_NAME", "m::graph_reads"),
        ]);
        assert_eq!(charge_in(Class::Graph, "locks_guard", &placed, &[]), Ok(()));
        assert_eq!(charge_in(Class::Graph, "x", &env(&[report]), &[]), Ok(()));
        let r = read_report(&std::fs::read_to_string(&file).unwrap());
        assert_eq!(r.charges, 3, "{r:?}");
        assert_eq!(r.malformed, 0);
        assert_eq!(
            r.misplaced,
            vec![Misplaced {
                test: "m::reads".into(),
                binary: "paddock-dev::it".into(),
                class: "graph".into(),
                group: "@global".into(),
                profile: "ci".into(),
                call: "plan::read_graph".into(),
            }]
        );
        assert_eq!(read_report("not json\n{\"x\":1}\n").malformed, 2);
        // ... and in what `ci test-cost` prints and appends to the job summary.
        let (text, md) = render_report(&r, "cost.jsonl", "warning");
        assert!(
            text.contains("::warning::ci test-cost: m::reads (paddock-dev::it) charged the graph class through plan::read_graph"),
            "{text}"
        );
        assert!(text.contains("prefix `graph_`"), "{text}");
        assert!(
            md.contains("| `m::reads` | paddock-dev::it | graph | @global | ci | plan::read_graph |"),
            "{md}"
        );
        let (text, md) = render_report(&ReportRead::default(), "empty.jsonl", "warning");
        assert!(text.contains("not measured") && md.contains("Not measured"), "{text}");
    }

    #[test]
    fn only_cargo_metadata_is_charged() {
        assert!(runs_cargo_metadata("cargo", &["metadata", "--locked"]));
        assert!(runs_cargo_metadata("/x/cargo.exe", &["+1.91", "--quiet", "metadata"]));
        assert!(!runs_cargo_metadata("cargo", &["build", "metadata"]));
        assert!(!runs_cargo_metadata("npm", &["metadata"]));
        assert!(!runs_cargo_metadata::<&str>("cargo", &[]));
        assert_eq!(charge_argv("t", "cargo", &["build"]), Ok(()));
    }

    // The live half of the control: under nextest the class table really places a test by its name. Without nextest (a plain
    // `cargo test`) there is nothing to observe.
    #[test]
    fn graph_a_test_named_with_the_prefix_runs_in_the_class_group() {
        if let Ok(group) = std::env::var("NEXTEST_TEST_GROUP") {
            assert_eq!(
                group,
                Class::Graph.group(),
                "the class table of .config/nextest.toml did not place this test"
            );
            assert_eq!(charge(Class::Graph, "test_cost::tests"), Ok(()));
        }
    }

    #[test]
    fn tree_a_test_named_with_the_prefix_runs_in_the_class_group() {
        if let Ok(group) = std::env::var("NEXTEST_TEST_GROUP") {
            assert_eq!(
                group,
                Class::Tree.group(),
                "the class table of .config/nextest.toml did not place this test"
            );
        }
    }

    #[test]
    fn a_test_without_a_class_runs_in_no_group_and_its_charge_is_found() {
        if let Ok(group) = std::env::var("NEXTEST_TEST_GROUP") {
            assert_eq!(group, "@global");
            let get = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
            assert!(
                matches!(judge(Class::Graph, &get), Verdict::Misplaced { .. }),
                "a charge outside the group was not found"
            );
        }
    }

    /// The class table of `.config/nextest.toml`, read with the TOML parser (ER-42): one override of the default profile per class with
    /// the class's filter, group, threads and budget; the class groups exist; and no other override places a test in a group or gives it
    /// threads, so concurrency and budgets of whole-tree tests come from the classes alone.
    #[test]
    fn the_nextest_class_table_is_the_one_place_for_groups_and_threads() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".config/nextest.toml");
        let text = std::fs::read_to_string(&path).unwrap();
        let doc: toml::Table = toml::from_str(&text).unwrap();
        let groups = doc["test-groups"].as_table().unwrap();
        let mut class_rows = 0;
        for (name, profile) in doc["profile"].as_table().unwrap() {
            let Some(overrides) = profile.get("overrides").and_then(|o| o.as_array()) else {
                continue;
            };
            for o in overrides {
                let filter = o.get("filter").and_then(|f| f.as_str()).unwrap_or_default();
                let class = Class::ALL.into_iter().find(|c| c.filter() == filter);
                match class {
                    Some(c) => {
                        assert_eq!(
                            name, "default",
                            "the class table lives in the default profile (every profile inherits it): {filter}"
                        );
                        assert_eq!(o.get("test-group").and_then(|g| g.as_str()), Some(c.group()), "{filter}");
                        assert!(
                            o.get("threads-required").and_then(|t| t.as_integer()).is_some_and(|t| t >= 1),
                            "{filter}"
                        );
                        assert!(o.get("slow-timeout").is_some(), "{filter}: a class has its own budget");
                        assert!(groups.contains_key(c.group()), "{filter}: group {} is not defined", c.group());
                        class_rows += 1;
                    }
                    None => {
                        assert!(
                            o.get("test-group").is_none(),
                            "profile {name}: only the class table places tests in groups: {filter}"
                        );
                        assert!(
                            o.get("threads-required").is_none(),
                            "profile {name}: only the class table sets threads: {filter}"
                        );
                    }
                }
            }
        }
        assert_eq!(class_rows, Class::ALL.len(), "one class row per class");
        for g in groups.keys() {
            assert!(Class::ALL.iter().any(|c| c.group() == g), "test group {g} belongs to no class");
        }
    }
}
