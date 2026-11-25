//! C154: a spec does not use a fixed time as its synchronisation point, and a spec that fakes timers puts the real ones back in a
//! hook (ER-21 class "a fixed wait or a leaked fake clock fails a spec on a loaded runner": Dyno `extras-page` leaked fake timers after
//! a timeout, c249b271c; Spotter `flush(TEST_DEBOUNCE_MS + 20)`, ad0221ac1; Pitcrew `flush(PREVIEW_DEBOUNCE_MS + 50)`; ER-12, rule 49).
//!
//! Read: `*.spec.ts` and `*.e2e.ts` outside `node_modules` and snapshot folders, with comments and string contents masked (a comment
//! or a message that names `vi.useFakeTimers` is not a use). Three rules, one finding per site:
//!
//! 1. **fake-timers**: a file calls `vi.useFakeTimers(` but no `afterEach`, `afterAll` or `onTestFinished` call of the file, and no
//!    `test-setup` file of its app (`apps/<app>/` or `ui/projects/<project>/`) holding such a hook or calling `installTestKit`
//!    (the shared kit restores the real clock after every test), calls `vi.useRealTimers(`. A restore only in a `finally` block does
//!    not count either (a test that times out never reaches it) and is told apart in the message.
//! 2. **fixed-wait**: `flush(<CONSTANT> + <n>)`, a constant debounce plus a guess, used to wait for async work. Wait on the condition
//!    instead (`until` of `@decryp7/paddock-ui/testing`, `settleUntil`, `vi.waitFor`). The other fixed-delay waits (`waitForTimeout`,
//!    a `setTimeout` promise, `sleep(n)`) are conformance C71's and are not repeated here.
//! 3. **lazy-wait**: `vi.waitFor(` or `expect.poll(` whose callback only imports a module (`import(`) and holds no `expect` and no
//!    `throw`: it passes on the first call, so it waits for nothing. Also `advanceTimersByTime(` (the sync form, which flushes no
//!    promise) followed directly by an `expect(` that reads the DOM (`querySelector`, `nativeElement`, `textContent`, `getBy`,
//!    `locator`).
//!
//! **Blocking (errors, mode gate) since its first green run on the real tree, ci.yml run 9065 (11cd82593, 2026-11-25; ER-41).**
//! Lists only shrink (ER-46): no baseline file; a baseline needs the user's approval.

use super::finding::Finding;
use super::jsre::rx;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use regex::Regex;
use std::sync::LazyLock;

static SPEC: LazyLock<Regex> = LazyLock::new(|| rx(r"\.(?:spec|e2e)\.ts$"));
static NOT_READ: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|/)(?:node_modules|[^/]*-snapshots)/"));
static SETUP: LazyLock<Regex> = LazyLock::new(|| rx(r"(?:^|/)test-setup\.[a-z]+$"));
static FAKE: LazyLock<Regex> = LazyLock::new(|| rx(r"\bvi\.useFakeTimers\s*\("));
static HOOK: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:afterEach|afterAll|onTestFinished)\s*\("));
static FINALLY: LazyLock<Regex> = LazyLock::new(|| rx(r"\bfinally\s*\{"));
static REAL: LazyLock<Regex> = LazyLock::new(|| rx(r"\buseRealTimers\b"));
static FIXED_FLUSH: LazyLock<Regex> = LazyLock::new(|| rx(r"\bflush\(\s*[A-Z][A-Z0-9_]*\s*\+\s*[0-9]+\s*\)"));
static LAZY_WAIT: LazyLock<Regex> = LazyLock::new(|| rx(r"\b(?:vi\.waitFor|expect\.poll)\s*\("));
static IMPORT: LazyLock<Regex> = LazyLock::new(|| rx(r"\bimport\s*\("));
static CONDITION: LazyLock<Regex> = LazyLock::new(|| rx(r"\bexpect\b|\bthrow\b"));
static SYNC_ADVANCE: LazyLock<Regex> = LazyLock::new(|| rx(r"\badvanceTimersByTime\s*\("));
static DOM_EXPECT: LazyLock<Regex> = LazyLock::new(|| rx(r"^\s*expect\([^;]*(?:querySelector|nativeElement|textContent|getBy|locator)"));

const FIX_FAKE: &str = "Put the real clock back in `afterEach(() => vi.useRealTimers())` (or call installTestKit() from the app's test-setup, which does it after every test).";
const FIX_FLUSH: &str = "Wait on the condition: `until(() => ..., { what })` from @decryp7/paddock-ui/testing, `settleUntil` or `vi.waitFor` with an `expect` inside; a constant plus a guess is too short on a loaded runner.";
const FIX_LAZY: &str = "Give the wait a condition that fails until the thing is there (an `expect` or a `throw` on the view), or use `until` from @decryp7/paddock-ui/testing.";

/// One site: its 1-based line, the rule and the message.
#[derive(Debug, PartialEq, Eq)]
pub struct Site {
    /// The 1-based line.
    pub line: usize,
    /// `fake-timers`, `fixed-wait` or `lazy-wait`.
    pub rule: &'static str,
    /// What is wrong.
    pub message: String,
}

/// `text` with comments and the insides of string and template literals replaced by spaces (newlines kept), so patterns and bracket
/// matching never see them. A regex literal is not told from a division: one holding a quote can unbalance the rest, which only
/// ever costs a missed site, never a false one.
pub fn mask(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        let next = c.get(i + 1).copied();
        if ch == '/' && next == Some('/') {
            while i < c.len() && c[i] != '\n' {
                out.push(' ');
                i += 1;
            }
        } else if ch == '/' && next == Some('*') {
            out.push_str("  ");
            i += 2;
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                out.push(if c[i] == '\n' { '\n' } else { ' ' });
                i += 1;
            }
            if i < c.len() {
                out.push_str("  ");
                i += 2;
            }
        } else if ch == '\'' || ch == '"' || ch == '`' {
            out.push(ch);
            i += 1;
            while i < c.len() && c[i] != ch {
                if c[i] == '\\' && i + 1 < c.len() {
                    out.push(' ');
                    i += 1;
                }
                // A plain quote ends at the line's end; a template runs on.
                if c[i] == '\n' && ch != '`' {
                    break;
                }
                out.push(if c[i] == '\n' { '\n' } else { ' ' });
                i += 1;
            }
            if i < c.len() && c[i] == ch {
                out.push(ch);
                i += 1;
            }
        } else {
            out.push(ch);
            i += 1;
        }
    }
    out
}

/// The text from `at` (a call's name) to its matching `)`, or to the end; `open`/`close` pick `(`/`)` or `{`/`}`.
fn span(masked: &str, at: usize, open: char, close: char) -> &str {
    let mut depth = 0usize;
    let mut started = false;
    for (off, ch) in masked[at..].char_indices() {
        if ch == open {
            depth += 1;
            started = true;
        } else if ch == close && started {
            depth -= 1;
            if depth == 0 {
                return &masked[at..at + off + 1];
            }
        }
    }
    &masked[at..]
}

fn line_of(masked: &str, at: usize) -> usize {
    masked[..at].bytes().filter(|b| *b == b'\n').count() + 1
}

/// Whether a hook (or a `finally` when `finally_ok`) in `masked` puts the real timers back.
fn restores_in_hook(masked: &str) -> bool {
    HOOK.find_iter(masked).any(|m| REAL.is_match(span(masked, m.start(), '(', ')')))
}

fn restores_in_finally(masked: &str) -> bool {
    FINALLY.find_iter(masked).any(|m| REAL.is_match(span(masked, m.start(), '{', '}')))
}

/// The sites of one spec's text. `setup_restores`: the app's test-setup puts the real timers back after every test.
pub fn scan(text: &str, setup_restores: bool) -> Vec<Site> {
    let m = mask(text);
    let mut sites = Vec::new();
    if let Some(first) = FAKE.find(&m)
        && !setup_restores
        && !restores_in_hook(&m)
    {
        let how = if restores_in_finally(&m) {
            "it puts the real timers back only in a `finally` block, which a test that times out never reaches"
        } else {
            "no afterEach, afterAll or onTestFinished of the file puts the real timers back"
        };
        sites.push(Site {
            line: line_of(&m, first.start()),
            rule: "fake-timers",
            message: format!("`vi.useFakeTimers` is called and {how}: the fake clock leaks into the next test of the worker"),
        });
    }
    for hit in FIXED_FLUSH.find_iter(&m) {
        sites.push(Site {
            line: line_of(&m, hit.start()),
            rule: "fixed-wait",
            message: format!(
                "`{}` waits a constant plus a guess: too short when the runner is loaded",
                hit.as_str()
            ),
        });
    }
    for hit in LAZY_WAIT.find_iter(&m) {
        let body = span(&m, hit.start(), '(', ')');
        let inner = &body[hit.end() - hit.start()..];
        if IMPORT.is_match(inner) && !CONDITION.is_match(inner) {
            sites.push(Site {
                line: line_of(&m, hit.start()),
                rule: "lazy-wait",
                message: format!(
                    "`{}` only imports a module and holds no condition (no `expect`, no `throw`): it waits for nothing",
                    hit.as_str().trim_end_matches('(')
                ),
            });
        }
    }
    for hit in SYNC_ADVANCE.find_iter(&m) {
        if hit.as_str().contains("Async") {
            continue;
        }
        let rest = &m[hit.start()..];
        let Some(end) = rest.find(';') else { continue };
        let after = rest[end + 1..].trim_start_matches([' ', '\t', '\r']);
        let Some(line) = after.strip_prefix('\n').map(|a| a.lines().next().unwrap_or("")) else {
            continue;
        };
        if DOM_EXPECT.is_match(line) {
            sites.push(Site {
                line: line_of(&m, hit.start()),
                rule: "lazy-wait",
                message: "`advanceTimersByTime` is followed directly by an assertion on the view: it runs no promise, so async UI is not there yet".to_string(),
            });
        }
    }
    sites.sort_by_key(|s| s.line);
    sites
}

/// The root of the app a file belongs to (`apps/<app>/`, `ui/projects/<project>/`), if any.
fn app_root(file: &str) -> Option<String> {
    let parts: Vec<&str> = file.split('/').collect();
    match parts.as_slice() {
        ["apps", app, ..] => Some(format!("apps/{app}/")),
        ["ui", "projects", p, ..] => Some(format!("ui/projects/{p}/")),
        _ => None,
    }
}

/// The check.
pub struct C154;

impl Check for C154 {
    fn id(&self) -> &'static str {
        "C154"
    }

    fn rule(&self) -> u32 {
        49
    }

    fn title(&self) -> &'static str {
        "specs wait on a condition, not a fixed time, and put the real timers back after faking them"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::Gate
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                ("apps/a/app/src/leak.spec.ts", "it('x', () => {\n  vi.useFakeTimers();\n});\n"),
                (
                    "apps/a/app/src/fine.spec.ts",
                    "afterEach(() => vi.useRealTimers());\nit('x', () => { vi.useFakeTimers(); });\n",
                ),
                ("apps/b/app/src/fixed.spec.ts", "await flush(DEBOUNCE_MS + 20);\n"),
                ("apps/b/app/src/lazy.spec.ts", "await vi.waitFor(() => import('./x'));\n"),
                (
                    "apps/b/app/src/advance.spec.ts",
                    "vi.advanceTimersByTime(300);\nexpect(el.querySelector('a')).toBeNull();\n",
                ),
                (
                    "apps/b/app/src/commented.spec.ts",
                    "// vi.useFakeTimers() and flush(DEBOUNCE_MS + 20)\nconst s = 'flush(A_MS + 1)';\n",
                ),
                ("apps/b/app/src/not-a-spec.ts", "await flush(DEBOUNCE_MS + 20);\n"),
            ],
            expect: vec![
                ("apps/a/app/src/leak.spec.ts", 2),
                ("apps/b/app/src/advance.spec.ts", 1),
                ("apps/b/app/src/fixed.spec.ts", 1),
                ("apps/b/app/src/lazy.spec.ts", 1),
            ],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let all = ctx.tree_files();
        let setups_restoring: Vec<String> = all
            .iter()
            .filter(|f| SETUP.is_match(f) && !NOT_READ.is_match(f))
            .filter(|f| {
                ctx.read(f).is_some_and(|t| {
                    let m = mask(&t);
                    restores_in_hook(&m) || m.contains("installTestKit(")
                })
            })
            .filter_map(|f| app_root(f))
            .collect();
        let mut findings = Vec::new();
        let mut probes = 0;
        for f in ctx.files().iter().filter(|f| SPEC.is_match(f) && !NOT_READ.is_match(f)) {
            let Some(text) = ctx.read(f) else { continue };
            probes += 1;
            let setup = app_root(f).is_some_and(|r| setups_restoring.contains(&r));
            for s in scan(&text, setup) {
                let fix = match s.rule {
                    "fake-timers" => FIX_FAKE,
                    "fixed-wait" => FIX_FLUSH,
                    _ => FIX_LAZY,
                };
                findings.push(Finding::error("C154", 49, f, s.line, format!("[{}] {}", s.rule, s.message), fix));
            }
        }
        CheckOutput {
            findings,
            probes,
            completed: true,
            precondition: Precondition {
                met: probes > 0 || ctx.is_in_memory(),
                detail: format!("{probes} spec files read"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(text: &str) -> Vec<&'static str> {
        scan(text, false).into_iter().map(|s| s.rule).collect()
    }

    #[test]
    fn fake_timers_without_a_restoring_hook_is_flagged_at_the_first_use() {
        let s = scan(
            "describe('a', () => {\n  it('x', () => {\n    vi.useFakeTimers();\n  });\n});\n",
            false,
        );
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].rule, s[0].line), ("fake-timers", 3));
    }

    #[test]
    fn a_restore_in_after_each_after_all_or_on_test_finished_passes() {
        for hook in [
            "afterEach(() => { vi.useRealTimers(); });",
            "afterAll(() => vi.useRealTimers());",
            "onTestFinished(() => vi.useRealTimers());",
        ] {
            assert!(rules(&format!("{hook}\nvi.useFakeTimers();\n")).is_empty(), "{hook}");
        }
    }

    #[test]
    fn a_restore_in_the_test_body_or_only_in_a_finally_still_leaks_and_the_message_says_which() {
        assert_eq!(rules("vi.useFakeTimers();\nvi.useRealTimers();\n"), vec!["fake-timers"]);
        let s = scan("try { vi.useFakeTimers(); } finally { vi.useRealTimers(); }", false);
        assert!(s[0].message.contains("finally"), "{}", s[0].message);
    }

    #[test]
    fn the_apps_setup_restoring_the_clock_clears_the_rule() {
        assert!(scan("vi.useFakeTimers();", true).is_empty());
    }

    #[test]
    fn a_comment_or_a_string_naming_the_call_is_not_a_use() {
        assert!(rules("// vi.useFakeTimers()\n/* flush(A_MS + 1) */\nconst s = \"vi.useFakeTimers(); flush(A_MS + 1)\";\n").is_empty());
    }

    #[test]
    fn a_constant_plus_a_number_in_flush_is_a_fixed_wait_but_a_bare_number_or_a_condition_is_not() {
        assert_eq!(rules("await flush(TEST_DEBOUNCE_MS + 20);"), vec!["fixed-wait"]);
        assert!(rules("await flush(10);\nawait flush();\nawait until(f, () => ok);").is_empty());
    }

    #[test]
    fn a_wait_that_only_imports_has_no_condition() {
        assert_eq!(rules("await vi.waitFor(() => import('./x'));"), vec!["lazy-wait"]);
        assert_eq!(
            rules("await expect.poll(async () => { await import('./x'); }).toBe(1);"),
            vec!["lazy-wait"]
        );
        assert!(rules("await vi.waitFor(async () => { const m = await import('./x'); expect(m.ok).toBe(true); });").is_empty());
    }

    #[test]
    fn a_sync_advance_followed_by_a_dom_assertion_is_flagged_but_not_an_async_one_or_a_plain_value() {
        assert_eq!(
            rules("vi.advanceTimersByTime(300);\nexpect(el.querySelector('a')).toBeNull();"),
            vec!["lazy-wait"]
        );
        assert!(rules("await vi.advanceTimersByTimeAsync(300);\nexpect(el.querySelector('a')).toBeNull();").is_empty());
        assert!(rules("vi.advanceTimersByTime(300);\nexpect(calls).toBe(1);").is_empty());
    }
}
