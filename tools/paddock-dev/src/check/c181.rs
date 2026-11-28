//! C181: a test reads a lock's state only through `paddock_testkit::lock::await_free` or `await_held`, never with one direct read in an
//! assertion (user, 2026-11-28, in chat; ER-12, ER-22, ER-33; rule 5). ER-21 class, seen twice: on Unix a `flock` belongs to the open
//! file description, and a spawn on any thread gives its child a copy of the descriptor table until it execs, so a just-released
//! lock can read held for a moment. Dyno's `InstanceLock` flaked on it (2026-10-31, lessons.md "A just-released lock can look held")
//! and the throttle `filelock` test again (2026-11-28, `a_child_started_while_the_lock_is_held_does_not_inherit_it`); each fix
//! repaired one test. The one fix is the shared wait: the test polls its own probe until the state settles, bounded by the hang detector.
//!
//! Read: every `.rs` file outside `target` and `node_modules` that holds one of the read names, parsed with `syn` (ER-42: never a
//! home-made regex over the text). Only test code counts: a file under a `tests` folder or named `tests.rs`, `*_tests.rs` or
//! `testutil*.rs` ([`paddock_repo_graph::model::os::is_test_file`]), and an item marked `#[test]` or `#[cfg(test)]` in any other file.
//! In that code, an `assert!`, `assert_eq!`, `assert_ne!` or `debug_assert*!` whose checked expression holds a **lock-state read** is one
//! finding at the read:
//! - a call named `is_held`, `is_held_now`, `is_held_settled`, `is_locked` or `is_free` (a method or a path call), or
//! - an `is_err`, `is_ok`, `is_some`, `is_none`, `unwrap_err` or `expect_err` on a chain that started with `try_lock`, `try_lock_shared`
//!   or `try_lock_exclusive` (a take that is used as a probe).
//!
//! A call given to `await_free` or `await_held` is not read (that is the helper). A read names what it probes, so one with no argument
//! (`evidence.is_held()`, a method on a value that reports a precondition) is not a probe of a lock. Not read: a read bound to a name first and asserted
//! afterwards, a read inside a macro other than the asserts, and a probe under another name; a lexical bound by design, which only
//! ever costs a missed site. A probe that a test *means* to read once (a control that a lock really is held by a live holder) goes
//! through `await_held` too: a held reading needs no settling, but the one form keeps the check and the reader simple.
//!
//! **Report-only (warnings, mode report-only; ER-41) until its first green run on the real tree**, recorded here and in
//! docs/gates.toml (`[[report_only]]` C181). Lists only shrink (ER-46): no baseline file. Rust host only: born in Rust.

use super::finding::Finding;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use paddock_repo_graph::model::os::is_test_file;
use syn::punctuated::Punctuated;
use syn::visit::Visit;

/// The rule field: rule 5, a test-only failure is fixed properly (deterministic, no sleeps).
const RULE: u32 = 5;

/// Calls that read a lock's state in one look.
const READS: [&str; 5] = ["is_held", "is_held_now", "is_held_settled", "is_locked", "is_free"];

/// Methods that turn a take into a probe when they end a chain that started with `try_lock*`.
const VERDICTS: [&str; 6] = ["is_err", "is_ok", "is_some", "is_none", "unwrap_err", "expect_err"];

/// Words a file must hold to be parsed at all.
const WORDS: [&str; 4] = ["is_held", "is_locked", "is_free", "try_lock"];

const FIX: &str = "Read the lock through the shared wait: `paddock_testkit::lock::await_free(\"<what>\", || <probe reads held>)` after a release, drop or exit, `await_held` for a holder; a just-released lock can read held for a moment (lessons.md, \"A just-released lock can look held\").";

/// One direct read: its 1-based line, the test `fn` around it and the read's name.
#[derive(Debug, PartialEq, Eq)]
pub struct Site {
    /// The line of the read.
    pub line: usize,
    /// The innermost named `fn` around it (empty outside any).
    pub function: String,
    /// What was read: `is_held`, `try_lock().is_err()`.
    pub read: String,
}

/// Whether an attribute list marks test code: `#[test]` or a `#[cfg(..)]` that names `test` and no `not`.
fn is_test_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        if a.path().is_ident("test") {
            return true;
        }
        if !a.path().is_ident("cfg") {
            return false;
        }
        let s = match &a.meta {
            syn::Meta::List(l) => l.tokens.to_string(),
            _ => String::new(),
        };
        s.split(|c: char| !c.is_alphanumeric() && c != '_').any(|w| w == "test") && !s.contains("not")
    })
}

/// The attributes of the items that can carry `#[cfg(test)]`.
fn item_attrs(i: &syn::Item) -> &[syn::Attribute] {
    use syn::Item::*;
    match i {
        Mod(x) => &x.attrs,
        Fn(x) => &x.attrs,
        Impl(x) => &x.attrs,
        Use(x) => &x.attrs,
        Const(x) => &x.attrs,
        Static(x) => &x.attrs,
        Struct(x) => &x.attrs,
        Enum(x) => &x.attrs,
        Trait(x) => &x.attrs,
        Type(x) => &x.attrs,
        _ => &[],
    }
}

/// The lock-state reads inside one asserted expression.
struct Reads<'a> {
    function: &'a str,
    out: &'a mut Vec<Site>,
}

/// The name of the last segment of a call's path.
fn last_name(f: &syn::Expr) -> Option<String> {
    match f {
        syn::Expr::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

/// Whether the chain `e` (through methods, `?` and parentheses) started with a `try_lock*` call.
fn starts_with_try_lock(e: &syn::Expr) -> bool {
    match e {
        syn::Expr::MethodCall(m) => m.method.to_string().starts_with("try_lock") || starts_with_try_lock(&m.receiver),
        syn::Expr::Try(t) => starts_with_try_lock(&t.expr),
        syn::Expr::Paren(p) => starts_with_try_lock(&p.expr),
        _ => false,
    }
}

impl<'ast> Visit<'ast> for Reads<'_> {
    fn visit_expr_call(&mut self, c: &'ast syn::ExprCall) {
        let name = last_name(&c.func);
        if name.as_deref().is_some_and(|n| n.starts_with("await_")) {
            return;
        }
        if let (Some(n), syn::Expr::Path(p)) = (&name, &*c.func)
            && READS.contains(&n.as_str())
            && !c.args.is_empty()
            && let Some(seg) = p.path.segments.last()
        {
            self.out.push(Site {
                line: seg.ident.span().start().line,
                function: self.function.to_string(),
                read: n.clone(),
            });
        }
        syn::visit::visit_expr_call(self, c);
    }

    fn visit_expr_method_call(&mut self, m: &'ast syn::ExprMethodCall) {
        let name = m.method.to_string();
        if name.starts_with("await_") {
            return;
        }
        if READS.contains(&name.as_str()) && !m.args.is_empty() {
            self.out.push(Site {
                line: m.method.span().start().line,
                function: self.function.to_string(),
                read: name.clone(),
            });
        } else if VERDICTS.contains(&name.as_str()) && starts_with_try_lock(&m.receiver) {
            self.out.push(Site {
                line: m.method.span().start().line,
                function: self.function.to_string(),
                read: format!("try_lock(..).{name}()"),
            });
        }
        syn::visit::visit_expr_method_call(self, m);
    }
}

/// The walk over one file: it reads the asserts of test code.
struct Walk {
    in_test: bool,
    functions: Vec<String>,
    sites: Vec<Site>,
}

impl Walk {
    /// How many leading arguments of the assert macro `name` are the checked expression (the rest is the message), or `None`.
    fn checked_args(name: &str) -> Option<usize> {
        match name {
            "assert" | "debug_assert" => Some(1),
            "assert_eq" | "assert_ne" | "debug_assert_eq" | "debug_assert_ne" => Some(2),
            _ => None,
        }
    }

    fn with_test<R>(&mut self, test: bool, f: impl FnOnce(&mut Self) -> R) -> R {
        let before = self.in_test;
        self.in_test |= test;
        let r = f(self);
        self.in_test = before;
        r
    }
}

impl<'ast> Visit<'ast> for Walk {
    fn visit_item(&mut self, i: &'ast syn::Item) {
        let test = is_test_attr(item_attrs(i));
        self.with_test(test, |w| syn::visit::visit_item(w, i));
    }

    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        self.functions.push(f.sig.ident.to_string());
        syn::visit::visit_item_fn(self, f);
        self.functions.pop();
    }

    fn visit_impl_item_fn(&mut self, f: &'ast syn::ImplItemFn) {
        let test = is_test_attr(&f.attrs);
        self.functions.push(f.sig.ident.to_string());
        self.with_test(test, |w| syn::visit::visit_impl_item_fn(w, f));
        self.functions.pop();
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let checked = m
            .path
            .segments
            .last()
            .and_then(|s| Walk::checked_args(&s.ident.to_string()))
            .filter(|_| self.in_test);
        if let Some(n) = checked {
            if let Ok(args) = m.parse_body_with(Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated) {
                let function = self.functions.last().cloned().unwrap_or_default();
                for e in args.iter().take(n) {
                    Reads {
                        function: &function,
                        out: &mut self.sites,
                    }
                    .visit_expr(e);
                }
            }
            return;
        }
        syn::visit::visit_macro(self, m);
    }
}

/// The direct lock-state reads in the assertions of one file's text. `test_file`: the whole file is test code. A file that does not
/// parse has none.
pub fn scan(text: &str, test_file: bool) -> Vec<Site> {
    if !WORDS.iter().any(|w| text.contains(w)) {
        return Vec::new();
    }
    let Ok(file) = syn::parse_file(text) else { return Vec::new() };
    let mut w = Walk {
        in_test: test_file,
        functions: Vec::new(),
        sites: Vec::new(),
    };
    w.visit_file(&file);
    w.sites.sort_by_key(|s| s.line);
    w.sites
}

/// The files whose text can hold test code.
fn read(rel: &str) -> bool {
    rel.ends_with(".rs") && !rel.split('/').any(|s| s == "target" || s == "node_modules")
}

/// Whether the whole file is test code.
fn test_file(rel: &str) -> bool {
    is_test_file(rel) || rel.rsplit('/').next().is_some_and(|n| n.starts_with("testutil"))
}

/// The check.
pub struct C181;

impl Check for C181 {
    fn id(&self) -> &'static str {
        "C181"
    }

    fn rule(&self) -> u32 {
        RULE
    }

    fn title(&self) -> &'static str {
        "a test reads a lock's state through paddock_testkit::lock::await_free or await_held, never once in an assertion"
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
                // Red: a path call (3), a try_lock verdict (4), a try_lock chain with unwrap (5), a method read with a message (6).
                (
                    "crates/a/tests/it/locks.rs",
                    "#[test]\nfn a() {\n    assert!(!is_held(&p));\n    assert_eq!(l.try_lock().is_err(), false);\n    assert!(locks.try_lock(id).unwrap().is_none());\n    assert!(!x.is_held_settled(&p), \"free\");\n}\n",
                ),
                // Red: a test item of a production file.
                (
                    "crates/b/src/lib.rs",
                    "pub fn real() {\n    assert!(is_held(&p));\n}\n#[cfg(test)]\nmod t {\n    #[test]\n    fn b() {\n        assert!(is_held(&p));\n    }\n}\n",
                ),
                // Green: the helper, a read bound to a name, production code and a message that names a read.
                (
                    "crates/c/tests/it/ok.rs",
                    "#[test]\nfn c() {\n    paddock_testkit::lock::await_free(\"free\", || is_held(&p));\n    await_held(\"held\", || l.try_lock().is_err());\n    let held = is_held(&p);\n    assert!(done, \"{}\", is_held(&p));\n    assert!(evidence.is_held());\n}\n",
                ),
            ],
            expect: vec![
                ("crates/a/tests/it/locks.rs", 3),
                ("crates/a/tests/it/locks.rs", 4),
                ("crates/a/tests/it/locks.rs", 5),
                ("crates/a/tests/it/locks.rs", 6),
                ("crates/b/src/lib.rs", 8),
            ],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let mut findings = Vec::new();
        let mut probes = 0;
        for f in ctx.files().iter().filter(|f| read(f)) {
            let Some(text) = ctx.read(f) else { continue };
            probes += 1;
            for s in scan(&text, test_file(f)) {
                findings.push(Finding::warn(
                    "C181",
                    RULE,
                    f,
                    s.line,
                    format!(
                        "`{}` asserts on one direct lock-state read (`{}`): a just-released lock can read held for a moment",
                        s.function, s.read
                    ),
                    FIX,
                ));
            }
        }
        CheckOutput {
            findings,
            probes,
            completed: true,
            precondition: Precondition {
                met: probes > 0 || ctx.is_in_memory(),
                detail: format!("{probes} Rust files read"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str, test_file: bool) -> Vec<(usize, String)> {
        scan(text, test_file).into_iter().map(|s| (s.line, s.read)).collect()
    }

    #[test]
    fn a_direct_read_in_an_assert_of_a_test_file_is_a_site_at_the_read() {
        let t = "fn t() {\n    assert!(!slotfile::is_held(&p).unwrap());\n}\n";
        assert_eq!(lines(t, true), vec![(2, "is_held".to_string())]);
    }

    #[test]
    fn every_assert_macro_and_both_sides_of_an_equality_count() {
        let t = "fn t() {\n    assert_eq!(is_held_now(&p), false);\n    assert_ne!(true, is_locked(&p));\n    debug_assert!(is_held(&p));\n}\n";
        assert_eq!(lines(t, true).len(), 3);
    }

    #[test]
    fn a_try_lock_used_as_a_probe_is_a_site_but_a_bare_take_is_not() {
        let t = "fn t() {\n    assert!(f.try_lock().is_err());\n    assert!(f.try_lock().is_ok());\n    f.try_lock().unwrap();\n    assert!(f.metadata().is_ok());\n}\n";
        let got = lines(t, true);
        assert_eq!(got.iter().map(|(l, _)| *l).collect::<Vec<_>>(), vec![2, 3], "{got:?}");
    }

    #[test]
    fn a_read_given_to_the_helper_is_not_a_site() {
        let t = "fn t() {\n    await_free(\"x\", || is_held(&p));\n    paddock_testkit::lock::await_held(\"y\", || f.try_lock().is_err());\n}\n";
        assert!(lines(t, true).is_empty());
    }

    #[test]
    fn production_code_is_not_read_but_a_cfg_test_item_and_a_test_fn_are() {
        let t = "fn real() {\n    assert!(is_held(&p));\n}\n#[cfg(test)]\nmod m {\n    fn b() {\n        assert!(is_held(&p));\n    }\n}\n#[test]\nfn c() {\n    assert!(is_held(&p));\n}\n";
        assert_eq!(lines(t, false).iter().map(|(l, _)| *l).collect::<Vec<_>>(), vec![7, 12]);
        assert!(lines(t, false).iter().all(|(l, _)| *l != 2));
    }

    #[test]
    fn a_message_argument_and_a_read_bound_to_a_name_are_not_sites() {
        let t = "fn t() {\n    let held = is_held(&p);\n    assert!(done, \"{}\", is_held(&p));\n    assert!(held);\n}\n";
        assert!(lines(t, true).is_empty());
    }

    #[test]
    fn a_read_with_no_argument_is_a_value_method_not_a_probe() {
        let t = "fn t() {\n    assert!(evidence.is_held(), \"{evidence}\");\n    assert!(lock.is_held(&p));\n}\n";
        assert_eq!(lines(t, true), vec![(3, "is_held".to_string())]);
    }

    #[test]
    fn a_file_that_does_not_parse_has_no_sites() {
        assert!(lines("fn t( { assert!(is_held(", true).is_empty());
    }
}
