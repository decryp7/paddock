//! The condition and string evaluator of ADR 0060 §2.4: the small subset of GitHub/Gitea expression syntax the workflows
//! use. Anything outside it evaluates to [`Val::Unknown`], which never counts as "runs" (fail closed, ER-33).

use std::cell::RefCell;
use std::collections::BTreeSet;

/// A value of the evaluator.
#[derive(Debug, Clone, PartialEq)]
pub enum Val {
    /// Text.
    Str(String),
    /// A boolean.
    Bool(bool),
    /// A number.
    Num(f64),
    /// `null` (an unset input or env variable).
    Null,
    /// An output of the plan job: "runs when the planner marks `<name>`" (§2.4). Truthy; records the name.
    Planned(String),
    /// A value only the runner knows (a secret, the commit, a step output): it can stand in an argument, where it changes no
    /// decision, but it decides nothing in a condition and is unresolved as a command or a path.
    Opaque(String),
    /// Something the model cannot know (a syntax outside the subset).
    Unknown,
}

impl Val {
    fn text(&self) -> Option<String> {
        match self {
            Val::Str(s) => Some(s.clone()),
            Val::Bool(b) => Some(b.to_string()),
            Val::Num(n) => Some(if n.fract() == 0.0 {
                format!("{}", *n as i64)
            } else {
                n.to_string()
            }),
            Val::Null => Some(String::new()),
            Val::Planned(_) | Val::Opaque(_) | Val::Unknown => None,
        }
    }
}

/// Where the names of an expression (`github.ref`, `inputs.x`, `matrix.os`) get their values.
pub trait Lookup {
    /// The value of a dotted name; [`Val::Unknown`] when the model has none.
    fn get(&self, name: &str) -> Val;
}

impl<F: Fn(&str) -> Val> Lookup for F {
    fn get(&self, name: &str) -> Val {
        self(name)
    }
}

/// The result of evaluating a text with `${{ }}` parts.
#[derive(Debug, Clone, PartialEq)]
pub struct Interpolated {
    /// The text with every resolvable part replaced; an unresolvable part stays as written.
    pub text: String,
    /// Whether a part could not be resolved.
    pub unresolved: bool,
    /// The plan outputs the evaluation relied on.
    pub planned: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Name(String),
    Str(String),
    Num(f64),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
}

fn tokenize(src: &str) -> Option<Vec<Tok>> {
    let c: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < c.len() {
        let ch = c[i];
        match ch {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '\'' => {
                let mut s = String::new();
                i += 1;
                loop {
                    match c.get(i) {
                        None => return None,
                        Some('\'') if c.get(i + 1) == Some(&'\'') => {
                            s.push('\'');
                            i += 2;
                        }
                        Some('\'') => {
                            i += 1;
                            break;
                        }
                        Some(x) => {
                            s.push(*x);
                            i += 1;
                        }
                    }
                }
                out.push(Tok::Str(s));
            }
            '=' if c.get(i + 1) == Some(&'=') => {
                out.push(Tok::Op("=="));
                i += 2;
            }
            '!' if c.get(i + 1) == Some(&'=') => {
                out.push(Tok::Op("!="));
                i += 2;
            }
            '!' => {
                out.push(Tok::Op("!"));
                i += 1;
            }
            '&' if c.get(i + 1) == Some(&'&') => {
                out.push(Tok::Op("&&"));
                i += 2;
            }
            '|' if c.get(i + 1) == Some(&'|') => {
                out.push(Tok::Op("||"));
                i += 2;
            }
            d if d.is_ascii_digit() => {
                let start = i;
                while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                    i += 1;
                }
                out.push(Tok::Num(c[start..i].iter().collect::<String>().parse().ok()?));
            }
            a if a.is_ascii_alphabetic() || a == '_' => {
                let start = i;
                while i < c.len() && (c[i].is_ascii_alphanumeric() || matches!(c[i], '_' | '.' | '-' | '*')) {
                    i += 1;
                }
                out.push(Tok::Name(c[start..i].iter().collect()));
            }
            _ => return None,
        }
    }
    Some(out)
}

struct Eval<'a> {
    toks: Vec<Tok>,
    pos: usize,
    lookup: &'a dyn Lookup,
    planned: RefCell<BTreeSet<String>>,
}

impl Eval<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn or(&mut self) -> Option<Val> {
        let mut left = self.and()?;
        while self.peek() == Some(&Tok::Op("||")) {
            self.pos += 1;
            let right = self.and()?;
            left = match self.truth(&left) {
                Some(true) => left,
                Some(false) => right,
                None => {
                    if self.truth(&right) == Some(true) {
                        right
                    } else {
                        Val::Unknown
                    }
                }
            };
        }
        Some(left)
    }

    fn and(&mut self) -> Option<Val> {
        let mut left = self.eq()?;
        while self.peek() == Some(&Tok::Op("&&")) {
            self.pos += 1;
            let right = self.eq()?;
            left = match self.truth(&left) {
                Some(false) => left,
                Some(true) => right,
                None => {
                    if self.truth(&right) == Some(false) {
                        right
                    } else {
                        Val::Unknown
                    }
                }
            };
        }
        Some(left)
    }

    fn eq(&mut self) -> Option<Val> {
        let left = self.unary()?;
        let negate = match self.peek() {
            Some(Tok::Op("==")) => false,
            Some(Tok::Op("!=")) => true,
            _ => return Some(left),
        };
        self.pos += 1;
        let right = self.unary()?;
        Some(self.compare(&left, &right, negate))
    }

    fn compare(&self, a: &Val, b: &Val, negate: bool) -> Val {
        // A plan output compared with 'true', or found not 'false' (a gate whose `run` is empty when it did not run): "runs when
        // the planner marks it". Other comparisons are not modelled.
        if let (Val::Planned(name), other) | (other, Val::Planned(name)) = (a, b) {
            return match (other.text().as_deref(), negate) {
                (Some("true"), false) | (Some("false"), true) => {
                    self.planned.borrow_mut().insert(name.clone());
                    Val::Bool(true)
                }
                _ => Val::Unknown,
            };
        }
        match (a.text(), b.text()) {
            (Some(x), Some(y)) => Val::Bool((x.eq_ignore_ascii_case(&y)) != negate),
            _ => Val::Unknown,
        }
    }

    fn unary(&mut self) -> Option<Val> {
        if self.peek() == Some(&Tok::Op("!")) {
            self.pos += 1;
            let v = self.unary()?;
            return Some(match v {
                Val::Planned(_) => Val::Unknown,
                other => match self.truth(&other) {
                    Some(t) => Val::Bool(!t),
                    None => Val::Unknown,
                },
            });
        }
        self.atom()
    }

    fn atom(&mut self) -> Option<Val> {
        let tok = self.toks.get(self.pos)?.clone();
        self.pos += 1;
        match tok {
            Tok::LParen => {
                let v = self.or()?;
                if self.peek() != Some(&Tok::RParen) {
                    return None;
                }
                self.pos += 1;
                Some(v)
            }
            Tok::Str(s) => Some(Val::Str(s)),
            Tok::Num(n) => Some(Val::Num(n)),
            Tok::Name(n) if self.peek() == Some(&Tok::LParen) => {
                self.pos += 1;
                let mut args = Vec::new();
                if self.peek() == Some(&Tok::RParen) {
                    self.pos += 1;
                } else {
                    loop {
                        args.push(self.or()?);
                        match self.peek() {
                            Some(Tok::Comma) => self.pos += 1,
                            Some(Tok::RParen) => {
                                self.pos += 1;
                                break;
                            }
                            _ => return None,
                        }
                    }
                }
                Some(self.call(&n, &args))
            }
            Tok::Name(n) => Some(match n.as_str() {
                "true" => Val::Bool(true),
                "false" => Val::Bool(false),
                "null" => Val::Null,
                _ => self.lookup.get(&n),
            }),
            _ => None,
        }
    }

    fn call(&self, name: &str, args: &[Val]) -> Val {
        let text = |i: usize| args.get(i).and_then(Val::text);
        match name.to_ascii_lowercase().as_str() {
            "success" | "always" => Val::Bool(true),
            "cancelled" => Val::Bool(false),
            "startswith" => match (text(0), text(1)) {
                (Some(a), Some(b)) => Val::Bool(a.to_lowercase().starts_with(&b.to_lowercase())),
                _ => Val::Unknown,
            },
            "endswith" => match (text(0), text(1)) {
                (Some(a), Some(b)) => Val::Bool(a.to_lowercase().ends_with(&b.to_lowercase())),
                _ => Val::Unknown,
            },
            // A JSON array literal stays its text: `contains(fromJSON('["job"]'), 'job')` is then a substring test, which is exact for
            // the rerun gating of ci.yml (a one-name list).
            "fromjson" => match args.first() {
                Some(Val::Str(s)) if s.trim_start().starts_with('[') => Val::Str(s.clone()),
                _ => Val::Unknown,
            },
            "contains" => match (text(0), text(1)) {
                (Some(a), Some(b)) => Val::Bool(a.to_lowercase().contains(&b.to_lowercase())),
                _ => Val::Unknown,
            },
            "format" => {
                let Some(mut out) = text(0) else { return Val::Unknown };
                for (i, a) in args.iter().enumerate().skip(1) {
                    match a.text() {
                        Some(t) => out = out.replace(&format!("{{{}}}", i - 1), &t),
                        None => return Val::Unknown,
                    }
                }
                Val::Str(out)
            }
            // failure(), toJSON, hashFiles, join (and fromJSON of anything but an array literal): outside the model.
            _ => Val::Unknown,
        }
    }

    /// `Some(truthiness)`, or `None` for an unknown value. A plan output is truthy and recorded.
    fn truth(&self, v: &Val) -> Option<bool> {
        match v {
            Val::Bool(b) => Some(*b),
            Val::Str(s) => Some(!s.is_empty()),
            Val::Num(n) => Some(*n != 0.0),
            Val::Null => Some(false),
            Val::Planned(name) => {
                self.planned.borrow_mut().insert(name.clone());
                Some(true)
            }
            Val::Opaque(_) | Val::Unknown => None,
        }
    }
}

/// Evaluates one expression (the inside of `${{ }}`, or an `if:` without them). A syntax error gives [`Val::Unknown`].
pub fn eval(src: &str, lookup: &dyn Lookup) -> (Val, BTreeSet<String>) {
    let Some(toks) = tokenize(src) else {
        return (Val::Unknown, BTreeSet::new());
    };
    let mut e = Eval {
        toks,
        pos: 0,
        lookup,
        planned: RefCell::new(BTreeSet::new()),
    };
    match e.or() {
        Some(v) if e.pos == e.toks.len() => {
            let planned = e.planned.borrow().clone();
            (v, planned)
        }
        _ => (Val::Unknown, BTreeSet::new()),
    }
}

/// The three-valued answer to an `if:`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tri {
    /// Runs.
    Yes,
    /// Does not run.
    No,
    /// The model cannot tell.
    Unknown,
}

/// Evaluates an `if:` condition (with or without `${{ }}`); returns the answer and the plan outputs it relied on.
pub fn condition(src: &str, lookup: &dyn Lookup) -> (Tri, BTreeSet<String>) {
    let t = src.trim();
    let inner = t.strip_prefix("${{").and_then(|r| r.strip_suffix("}}")).unwrap_or(t);
    let (v, planned) = eval(inner, lookup);
    let probe = Eval {
        toks: Vec::new(),
        pos: 0,
        lookup,
        planned: RefCell::new(BTreeSet::new()),
    };
    match probe.truth(&v) {
        Some(true) => {
            let mut planned = planned;
            planned.extend(probe.planned.borrow().iter().cloned());
            (Tri::Yes, planned)
        }
        Some(false) => (Tri::No, BTreeSet::new()),
        None => (Tri::Unknown, BTreeSet::new()),
    }
}

/// Replaces every `${{ expr }}` in `text`.
pub fn interpolate(text: &str, lookup: &dyn Lookup) -> Interpolated {
    let mut out = String::new();
    let mut rest = text;
    let mut unresolved = false;
    let mut planned = BTreeSet::new();
    while let Some(start) = rest.find("${{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 3..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            unresolved = true;
            rest = "";
            break;
        };
        let (v, p) = eval(&after[..end], lookup);
        match (&v, v.text()) {
            (Val::Opaque(name), _) => {
                // Marked with guillemets so that a command or path made of it can be told from an argument.
                out.push_str(&format!("\u{2039}{name}\u{203a}"));
            }
            (_, Some(t)) => {
                out.push_str(&t);
                planned.extend(p);
            }
            (_, None) => {
                out.push_str(&rest[start..start + 3 + end + 2]);
                unresolved = true;
            }
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Interpolated {
        text: out,
        unresolved,
        planned,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(name: &str) -> Val {
        match name {
            "github.event_name" => Val::Str("push".into()),
            "github.ref" => Val::Str("refs/heads/main".into()),
            "inputs.webkit" => Val::Null,
            "needs.plan.outputs.ui" => Val::Planned("ui".into()),
            "secrets.K" => Val::Unknown,
            _ => Val::Unknown,
        }
    }

    fn yes(src: &str) -> Tri {
        condition(src, &ctx).0
    }

    #[test]
    fn ci_yml_webkit_condition_from_the_real_file_is_true_on_a_push_to_main() {
        // .gitea/workflows/ci.yml e2e-webkit
        let src = "${{ needs.plan.outputs.ui == 'true' && (inputs.webkit == true || (github.event_name == 'push' && (github.ref == 'refs/heads/main' || startsWith(github.ref, 'refs/tags/v')))) }}";
        let (t, planned) = condition(src, &ctx);
        assert_eq!(t, Tri::Yes);
        assert!(planned.contains("ui"));
    }

    #[test]
    fn a_plan_output_found_not_false_is_planned_like_one_equal_to_true_and_other_negations_are_unknown() {
        let (t, planned) = condition("always() && needs.plan.outputs.ui != 'false'", &ctx);
        assert_eq!(t, Tri::Yes);
        assert!(planned.contains("ui"));
        assert_ne!(yes("needs.plan.outputs.ui != 'true'"), Tri::Yes);
    }

    #[test]
    fn the_same_condition_is_false_on_a_pull_request() {
        let pr = |n: &str| match n {
            "github.event_name" => Val::Str("pull_request".into()),
            "github.ref" => Val::Str("refs/pull/1/merge".into()),
            "inputs.webkit" => Val::Null,
            "needs.plan.outputs.ui" => Val::Planned("ui".into()),
            _ => Val::Unknown,
        };
        let src = "needs.plan.outputs.ui == 'true' && (inputs.webkit == true || (github.event_name == 'push' && github.ref == 'refs/heads/main'))";
        assert_eq!(condition(src, &pr).0, Tri::No);
    }

    #[test]
    fn the_rerun_gating_of_ci_yml_is_true_when_no_rerun_names_jobs() {
        let ctx = |n: &str| match n {
            "needs.plan.outputs.rerun_jobs" => Val::Null,
            "needs.plan.outputs.ui" => Val::Planned("ui".into()),
            _ => Val::Unknown,
        };
        let src = "needs.plan.outputs.ui == 'true' && contains(fromJSON(needs.plan.outputs.rerun_jobs || '[\"ui\"]'), 'ui')";
        let (t, planned) = condition(src, &ctx);
        assert_eq!(t, Tri::Yes);
        assert!(planned.contains("ui"));
    }

    #[test]
    fn status_functions_and_negation() {
        assert_eq!(yes("${{ !cancelled() }}"), Tri::Yes);
        assert_eq!(yes("always()"), Tri::Yes);
        assert_eq!(yes("failure()"), Tri::Unknown);
        assert_eq!(yes("!(github.event_name == 'push')"), Tri::No);
    }

    #[test]
    fn unknown_poisons_unless_the_other_side_decides() {
        assert_eq!(yes("secrets.K != ''"), Tri::Unknown);
        assert_eq!(yes("secrets.K != '' && false"), Tri::No);
        assert_eq!(yes("secrets.K != '' || true"), Tri::Yes);
        assert_eq!(yes("nonsense ((("), Tri::Unknown);
    }

    #[test]
    fn plan_outputs_only_count_in_the_positive_form() {
        assert_eq!(yes("needs.plan.outputs.ui == 'true'"), Tri::Yes);
        assert_eq!(yes("needs.plan.outputs.ui != 'true'"), Tri::Unknown);
        assert_eq!(yes("!needs.plan.outputs.ui"), Tri::Unknown);
    }

    #[test]
    fn interpolation_keeps_what_it_cannot_resolve() {
        let r = interpolate("a ${{ github.event_name }} b ${{ secrets.K }} c", &ctx);
        assert!(r.unresolved);
        assert_eq!(r.text, "a push b ${{ secrets.K }} c");
        let ok = interpolate("${{ format('{0}-{1}', 'x', github.event_name) }}", &ctx);
        assert_eq!((ok.text.as_str(), ok.unresolved), ("x-push", false));
    }

    #[test]
    fn opaque_values_stand_in_arguments_but_decide_nothing() {
        let lk = |n: &str| match n {
            "secrets.TOKEN" => Val::Opaque("secrets.TOKEN".into()),
            _ => Val::Unknown,
        };
        let r = interpolate("node x.mjs --token ${{ secrets.TOKEN }}", &lk);
        assert!(!r.unresolved);
        assert_eq!(r.text, "node x.mjs --token \u{2039}secrets.TOKEN\u{203a}");
        assert_eq!(condition("secrets.TOKEN != ''", &lk).0, Tri::Unknown);
        assert_eq!(condition("secrets.TOKEN", &lk).0, Tri::Unknown);
    }

    #[test]
    fn env_style_fallback_chains() {
        // working-directory: ${{ env.PADDOCK_WORKSPACE || github.workspace }}/tools/x
        let lk = |n: &str| match n {
            "env.PADDOCK_WORKSPACE" => Val::Null,
            "github.workspace" => Val::Str("<ws>".into()),
            _ => Val::Unknown,
        };
        assert_eq!(
            interpolate("${{ env.PADDOCK_WORKSPACE || github.workspace }}/tools/x", &lk).text,
            "<ws>/tools/x"
        );
    }
}
