//! Structural patterns over tree-sitter, in the style of ast-grep (deterministic, no model).
//!
//! A pattern is code of the rule's language with placeholders: `$NAME` stands for one syntax node (the same name twice must match the
//! same text), `$$$NAME` for any run of sibling nodes (zero or more; commas between arguments are part of the run), `$_` for any one
//! node. Layout and comments do not matter; every other token, operator included, must match. A pattern is parsed once, and matched
//! against a file's tree with explicit bounds (pattern size and depth, nodes visited, matches kept, steps per match attempt).

use pacenotes_core::rule::{Check, RuleSpec};
use pacenotes_core::{Language, Location, Position, RepoPath, Span};
use pacenotes_parse::{Limits, ParseLanguage};
use paddock_jobs::CancelToken;
use std::collections::BTreeMap;
use std::rc::Rc;
use tree_sitter::{Node, Parser, Tree};

use crate::shared_copy::{Finding, FindingFingerprint, FixSentence, FixSource, RuleId, path_matches};

/// The longest pattern in bytes.
pub const MAX_PATTERN_BYTES: usize = 4096;
/// The most placeholders in one pattern.
pub const MAX_PLACEHOLDERS: usize = 32;
/// The deepest pattern tree.
pub const MAX_PATTERN_DEPTH: usize = 64;
/// The most steps one match attempt may take before it counts as no match.
const MAX_STEPS: usize = 100_000;

/// Why a pattern or a rule was refused. Every message says what to fix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    /// Nothing to match.
    #[error("the pattern is empty; write the code to look for")]
    Empty,
    /// Too long.
    #[error("the pattern is {0} bytes; at most {MAX_PATTERN_BYTES} are accepted")]
    TooLong(usize),
    /// Too many placeholders.
    #[error("the pattern has more than {MAX_PLACEHOLDERS} placeholders")]
    TooManyPlaceholders,
    /// The pattern tree is too deep.
    #[error("the pattern nests deeper than {MAX_PATTERN_DEPTH} levels")]
    TooDeep,
    /// The pattern is only a placeholder and would match everything.
    #[error("the pattern is only a placeholder and matches everything; add the code around it")]
    TooGeneral,
    /// The pattern does not parse in the language.
    #[error("the pattern is not valid {language} code, or is more than one statement or expression; write one statement or one expression")]
    Syntax {
        /// The language.
        language: &'static str,
    },
    /// The grammar could not be loaded.
    #[error("the {language} grammar cannot be used: {why}")]
    Grammar {
        /// The language.
        language: &'static str,
        /// What the runtime said.
        why: String,
    },
    /// The rule is not a pattern rule.
    #[error("rule {0} is a prose rule, not a pattern rule")]
    NotAPattern(String),
    /// A pattern rule names no language.
    #[error("rule {0} has a pattern and no `languages`; name the languages it applies to")]
    NoLanguage(String),
    /// A language without a grammar.
    #[error(
        "rule {rule} names the language {language}, which has no grammar; use one of go, java, kotlin, c, cpp, csharp, typescript, javascript, rust, python"
    )]
    UnsupportedLanguage {
        /// The rule.
        rule: String,
        /// The language as written.
        language: String,
    },
    /// The fix sentence is not valid.
    #[error("rule {rule}: {problem}")]
    BadFix {
        /// The rule.
        rule: String,
        /// What is wrong.
        problem: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Var {
    One(String),
    Run(String),
    Any,
}

/// A compiled pattern for one language.
pub struct Pattern {
    language: ParseLanguage,
    tree: Tree,
    source: String,
    range: (usize, usize),
    vars: BTreeMap<String, Var>,
}

impl std::fmt::Debug for Pattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pattern")
            .field("language", &self.language.id())
            .field("source", &&self.source[self.range.0..self.range.1])
            .finish()
    }
}

/// One place a pattern matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Where, in bytes.
    pub span: Span,
    /// The matched text.
    pub text: String,
    /// What each named placeholder matched.
    pub captures: BTreeMap<String, String>,
}

/// Why a search stopped early; the caller reports a partial result (exit code 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Truncated {
    /// The file is larger than [`MatchLimits::max_bytes`] and was not searched.
    TooLarge {
        /// Its size.
        len: usize,
        /// The limit.
        max: usize,
    },
    /// More than [`MatchLimits::max_nodes`] nodes.
    Nodes,
    /// More than [`MatchLimits::max_matches`] matches.
    Matches,
    /// The file used up [`MatchLimits::max_steps`].
    Steps,
    /// The run was cancelled.
    Cancelled,
}

/// What a search found, and why it stopped early if it did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Found {
    /// The matches, in file order.
    pub matches: Vec<Match>,
    /// Set when the result is partial.
    pub truncated: Option<Truncated>,
    /// Match steps used on the whole file (a test of a bound counts these, not seconds).
    pub steps: usize,
}

/// Bounds of one search of one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchLimits {
    /// Largest file searched (the parser's own default, 2 MiB). A bigger file is not searched, so it has no findings and no
    /// fingerprints; a baseline entry for it reads as no longer found, and the run is partial.
    pub max_bytes: usize,
    /// Most nodes visited.
    pub max_nodes: usize,
    /// Most matches kept.
    pub max_matches: usize,
    /// Most match steps for the whole file (all attempts together).
    pub max_steps: usize,
}

impl Default for MatchLimits {
    fn default() -> Self {
        MatchLimits {
            max_bytes: pacenotes_parse::DEFAULT_MAX_BYTES,
            max_nodes: 500_000,
            max_matches: 1_000,
            max_steps: 5_000_000,
        }
    }
}

/// Statement and expression wrappers (head, tail) for languages whose grammar has no free-standing statements. Tried in order after the
/// pattern as written: the statement as written, the statement with a `;`, then an expression in an initialiser (so `$X.length` works).
fn wrappers(language: ParseLanguage) -> Vec<(&'static str, &'static str, bool)> {
    let stmt = |head, tail| [(head, tail, false), (head, tail, true)];
    match language {
        ParseLanguage::Rust => stmt("fn __pn() {\n", "\n}")
            .into_iter()
            .chain([("fn __pn() { let __e = ", "; }", false)])
            .collect(),
        ParseLanguage::Go => stmt("package p\nfunc __pn() {\n", "\n}")
            .into_iter()
            .chain([("package p\nfunc __pn() { __e := ", "\n}", false)])
            .collect(),
        ParseLanguage::Java => stmt("class P { void m() {\n", "\n} }")
            .into_iter()
            .chain([("class P { void m() { Object __e = ", "; } }", false)])
            .collect(),
        ParseLanguage::CSharp => stmt("class P { void M() {\n", "\n} }")
            .into_iter()
            .chain([("class P { void M() { var __e = ", "; } }", false)])
            .collect(),
        ParseLanguage::Kotlin => stmt("fun m() {\n", "\n}")
            .into_iter()
            .chain([("fun m() { val __e = ", "\n}", false)])
            .collect(),
        ParseLanguage::C | ParseLanguage::Cpp => stmt("void m(void) {\n", "\n}")
            .into_iter()
            .chain([("void m(void) { int __e = ", "; }", false)])
            .collect(),
        ParseLanguage::TypeScript | ParseLanguage::Tsx | ParseLanguage::JavaScript | ParseLanguage::Python => Vec::new(),
    }
}

/// Swaps `$NAME`, `$$$NAME` and `$_` for identifiers the grammar accepts, and says what each stands for. A `$` inside a string literal is
/// part of the string and is matched literally.
fn substitute(language: ParseLanguage, pattern: &str) -> Result<(String, BTreeMap<String, Var>), PatternError> {
    let single_quoted_strings = matches!(
        language,
        ParseLanguage::Python | ParseLanguage::JavaScript | ParseLanguage::TypeScript | ParseLanguage::Tsx
    );
    let bytes = pattern.as_bytes();
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut vars = BTreeMap::new();
    let mut i = 0;
    let mut n = 0;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == b'\\' && i + 1 < bytes.len() {
                let len = pattern[i + 1..].chars().next().map_or(1, char::len_utf8);
                out.push_str(&pattern[i..i + 1 + len]);
                i += 1 + len;
                continue;
            }
            if c == q {
                quote = None;
            }
        } else if c == b'"' || (c == b'\'' && single_quoted_strings) {
            quote = Some(c);
        } else if c == b'$' {
            let run = pattern[i..].starts_with("$$$");
            let start = i + if run { 3 } else { 1 };
            let mut end = start;
            while end < bytes.len()
                && (bytes[end].is_ascii_uppercase() || bytes[end] == b'_' || (end > start && bytes[end].is_ascii_digit()))
            {
                end += 1;
            }
            if end > start {
                let name = &pattern[start..end];
                let ident = format!("__pnv{n}x");
                n += 1;
                if n > MAX_PLACEHOLDERS {
                    return Err(PatternError::TooManyPlaceholders);
                }
                let var = if name == "_" && !run {
                    Var::Any
                } else if run {
                    Var::Run(name.to_owned())
                } else {
                    Var::One(name.to_owned())
                };
                vars.insert(ident.clone(), var);
                out.push_str(&ident);
                i = end;
                continue;
            }
        }
        let ch_len = pattern[i..].chars().next().map_or(1, char::len_utf8);
        out.push_str(&pattern[i..i + ch_len]);
        i += ch_len;
    }
    Ok((out, vars))
}

fn depth(node: Node<'_>) -> usize {
    let mut cursor = node.walk();
    let mut max = 0usize;
    let mut level = 0usize;
    loop {
        max = max.max(level);
        if cursor.goto_first_child() {
            level += 1;
        } else {
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() || cursor.node() == node {
                    return max;
                }
                level -= 1;
            }
        }
    }
}

impl Pattern {
    /// Compiles `source` for `language`. A pattern is **one statement or one expression**: several statements (`a(); b();`) are
    /// refused, because they would have to match a run of siblings in a particular block, which a pattern cannot say.
    pub fn compile(language: ParseLanguage, source: &str) -> Result<Pattern, PatternError> {
        let trimmed = source.trim();
        if trimmed.is_empty() {
            return Err(PatternError::Empty);
        }
        if trimmed.len() > MAX_PATTERN_BYTES {
            return Err(PatternError::TooLong(trimmed.len()));
        }
        let (text, vars) = substitute(language, trimmed)?;
        let grammar = language.grammar().map_err(|e| PatternError::Grammar {
            language: language.id(),
            why: e.to_string(),
        })?;
        let mut parser = Parser::new();
        parser.set_language(&grammar).map_err(|e| PatternError::Grammar {
            language: language.id(),
            why: e.to_string(),
        })?;
        let mut attempts: Vec<(String, usize)> = vec![(text.clone(), 0)];
        for (head, tail, semicolon) in wrappers(language) {
            let semi = if semicolon { ";" } else { "" };
            attempts.push((format!("{head}{text}{semi}{tail}"), head.len()));
        }
        for (full, offset) in attempts {
            let Some(tree) = parser.parse(&full, None) else { continue };
            if tree.root_node().has_error() {
                continue;
            }
            let range = (offset, offset + text.len());
            let Some(node) = tree.root_node().descendant_for_byte_range(range.0, range.1) else {
                continue;
            };
            // the deepest node that spans the whole pattern
            if node.start_byte() != range.0 || node.end_byte() != range.1 {
                continue;
            }
            // the whole file as the root means several statements side by side, which a pattern cannot express
            if node.parent().is_none() {
                continue;
            }
            if depth(node) > MAX_PATTERN_DEPTH {
                return Err(PatternError::TooDeep);
            }
            if vars.contains_key(&full[range.0..range.1]) {
                return Err(PatternError::TooGeneral);
            }
            return Ok(Pattern {
                language,
                tree,
                source: full,
                range,
                vars,
            });
        }
        Err(PatternError::Syntax { language: language.id() })
    }

    /// The language.
    pub fn language(&self) -> ParseLanguage {
        self.language
    }

    fn root(&self) -> Option<Node<'_>> {
        self.tree.root_node().descendant_for_byte_range(self.range.0, self.range.1)
    }

    /// Every place the pattern matches in `bytes` (a file of this pattern's language). Nested matches are all reported. Stops, and says
    /// why in [`Found::truncated`], when a bound is reached or `cancel` fires.
    pub fn find_all(&self, bytes: &[u8], limits: &MatchLimits, cancel: &CancelToken) -> Found {
        let mut found = Found::default();
        if bytes.len() > limits.max_bytes {
            found.truncated = Some(Truncated::TooLarge {
                len: bytes.len(),
                max: limits.max_bytes,
            });
            return found;
        }
        let Ok(target_src) = std::str::from_utf8(bytes) else { return found };
        let Ok(grammar) = self.language.grammar() else { return found };
        let mut parser = Parser::new();
        if parser.set_language(&grammar).is_err() {
            return found;
        }
        let Some(tree) = parser.parse(target_src, None) else { return found };
        let Some(root_pattern) = self.root() else { return found };
        let kind = root_pattern.kind_id();
        let mut visited = 0usize;
        let mut total_steps = 0usize;
        let mut cursor = tree.walk();
        'walk: loop {
            let node = cursor.node();
            visited += 1;
            if visited > limits.max_nodes {
                found.truncated = Some(Truncated::Nodes);
                break;
            }
            if visited.is_multiple_of(256) && cancel.is_cancelled() {
                found.truncated = Some(Truncated::Cancelled);
                break;
            }
            if node.kind_id() == kind {
                if found.matches.len() >= limits.max_matches {
                    found.truncated = Some(Truncated::Matches);
                    break;
                }
                let mut m = Matcher {
                    pat: &self.source,
                    tgt: target_src,
                    vars: &self.vars,
                    caps: Vec::new(),
                    steps: 0,
                    total: &mut total_steps,
                    total_max: limits.max_steps,
                    cancel,
                    exhausted: false,
                    cancelled: false,
                };
                let matched = m.node(root_pattern, node);
                let (exhausted, cancelled) = (m.exhausted, m.cancelled);
                if matched {
                    let (s, e) = (node.start_position(), node.end_position());
                    if let (Some(a), Some(b)) = (
                        Position::new(s.row as u32 + 1, s.column as u32 + 1),
                        Position::new(e.row as u32 + 1, e.column as u32 + 1),
                    ) && let Ok(span) = Span::new(a, b)
                    {
                        let captures = m
                            .caps
                            .iter()
                            .map(|(n, cap)| (n.clone(), range_text(target_src, cap.nodes())))
                            .collect();
                        found.matches.push(Match {
                            span,
                            text: target_src[node.byte_range()].to_owned(),
                            captures,
                        });
                    }
                }
                if cancelled {
                    found.truncated = Some(Truncated::Cancelled);
                    break;
                }
                if exhausted {
                    // an attempt that ran out is reported, never silently "no match"; the file ends when its whole budget is gone
                    found.truncated.get_or_insert(Truncated::Steps);
                    if total_steps > limits.max_steps {
                        break;
                    }
                }
            }
            if cursor.goto_first_child() {
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    break 'walk;
                }
            }
        }
        found.steps = total_steps;
        found
    }
}

fn range_text(src: &str, nodes: &[Node<'_>]) -> String {
    match (nodes.first(), nodes.last()) {
        (Some(a), Some(b)) => src[a.start_byte()..b.end_byte()].to_owned(),
        _ => String::new(),
    }
}

/// The significant leaf tokens under some nodes (comments and layout left out), produced one at a time, so `a.b` and `a . b` are the
/// same and a comparison can stop at the first difference.
struct Leaves<'t> {
    stack: Vec<Node<'t>>,
}

impl<'t> Leaves<'t> {
    fn new(nodes: &[Node<'t>]) -> Leaves<'t> {
        Leaves {
            stack: nodes.iter().rev().copied().collect(),
        }
    }

    fn next(&mut self, src: &'t str) -> Option<&'t str> {
        while let Some(n) = self.stack.pop() {
            if n.is_extra() {
                continue;
            }
            if n.child_count() == 0 {
                return Some(&src[n.byte_range()]);
            }
            let mut cursor = n.walk();
            let kids: Vec<Node<'t>> = n.children(&mut cursor).collect();
            self.stack.extend(kids.into_iter().rev());
        }
        None
    }
}

/// What a placeholder matched: one node, or a range of a sibling list that is shared, never copied, so binding a run costs nothing
/// that is not counted.
#[derive(Clone)]
enum Cap<'t> {
    One(Node<'t>),
    Run(Rc<Vec<Node<'t>>>, usize, usize),
}

impl<'t> Cap<'t> {
    fn nodes(&self) -> &[Node<'t>] {
        match self {
            Cap::One(n) => std::slice::from_ref(n),
            Cap::Run(all, from, to) => &all[*from..*to],
        }
    }
}

struct Matcher<'a, 't> {
    pat: &'a str,
    tgt: &'t str,
    vars: &'a BTreeMap<String, Var>,
    caps: Vec<(String, Cap<'t>)>,
    /// Steps of this attempt.
    steps: usize,
    /// Steps of the whole file.
    total: &'a mut usize,
    total_max: usize,
    cancel: &'a CancelToken,
    /// Set when this attempt or the file ran out of steps.
    exhausted: bool,
    /// Set when the cancel token fired.
    cancelled: bool,
}

impl<'t> Matcher<'_, 't> {
    fn var(&self, n: Node<'_>) -> Option<&Var> {
        self.vars.get(&self.pat[n.byte_range()])
    }

    /// Counts a step; false once this attempt or the whole file is out of budget.
    fn step(&mut self) -> bool {
        self.steps += 1;
        *self.total += 1;
        if self.steps.is_multiple_of(4096) && self.cancel.is_cancelled() {
            self.cancelled = true;
            return false;
        }
        if self.cancelled || self.steps > MAX_STEPS || *self.total > self.total_max {
            self.exhausted = true;
            return false;
        }
        true
    }

    /// Whether two captures are the same tokens, compared lazily with an early exit; every token compared is a step.
    fn same_tokens(&mut self, a: &[Node<'t>], b: &[Node<'t>]) -> bool {
        let (mut ia, mut ib) = (Leaves::new(a), Leaves::new(b));
        loop {
            match (ia.next(self.tgt), ib.next(self.tgt)) {
                (None, None) => return true,
                (Some(x), Some(y)) => {
                    if !self.step() || x != y {
                        return false;
                    }
                }
                _ => return false,
            }
        }
    }

    fn bind(&mut self, name: &str, cap: Cap<'t>) -> bool {
        if let Some(i) = self.caps.iter().position(|(n, _)| n == name) {
            let old = self.caps[i].1.clone();
            return self.same_tokens(old.nodes(), cap.nodes());
        }
        self.caps.push((name.to_owned(), cap));
        true
    }

    fn children(&self, node: Node<'t>) -> Vec<Node<'t>> {
        let mut cursor = node.walk();
        node.children(&mut cursor).filter(|c| !c.is_extra()).collect()
    }

    fn node(&mut self, p: Node<'_>, t: Node<'t>) -> bool {
        if !self.step() {
            return false;
        }
        match self.var(p).cloned() {
            Some(Var::Any) => return t.is_named(),
            Some(Var::One(name)) => return t.is_named() && self.bind(&name, Cap::One(t)),
            Some(Var::Run(_)) => return false,
            None => {}
        }
        if p.kind_id() != t.kind_id() {
            return false;
        }
        let (pc, tc) = (self.children(p), Rc::new(self.children(t)));
        if pc.is_empty() || tc.is_empty() {
            return pc.is_empty() && tc.is_empty() && self.pat[p.byte_range()] == self.tgt[t.byte_range()];
        }
        self.seq(&pc, &tc, 0)
    }

    /// Matches the pattern siblings `pc` against the target siblings `tc[at..]`.
    fn seq(&mut self, pc: &[Node<'_>], tc: &Rc<Vec<Node<'t>>>, at: usize) -> bool {
        if !self.step() {
            return false;
        }
        let Some((p, rest)) = pc.split_first() else { return at >= tc.len() };
        if let Some(Var::Run(name)) = self.var(*p).cloned() {
            for k in at..=tc.len() {
                let mark = self.caps.len();
                if self.bind(&name, Cap::Run(tc.clone(), at, k)) && self.seq(rest, tc, k) {
                    return true;
                }
                self.caps.truncate(mark);
                if *self.total > self.total_max || self.steps > MAX_STEPS || self.cancelled {
                    return false;
                }
            }
            return false;
        }
        let Some(t) = tc.get(at).copied() else { return false };
        let mark = self.caps.len();
        if self.node(*p, t) && self.seq(rest, tc, at + 1) {
            return true;
        }
        self.caps.truncate(mark);
        false
    }
}

/// The file extension a language's examples are given, so a rule test can name a path.
pub fn example_extension(language: ParseLanguage) -> &'static str {
    match language {
        ParseLanguage::Go => "go",
        ParseLanguage::Java => "java",
        ParseLanguage::Kotlin => "kt",
        ParseLanguage::C => "c",
        ParseLanguage::Cpp => "cpp",
        ParseLanguage::CSharp => "cs",
        ParseLanguage::TypeScript => "ts",
        ParseLanguage::Tsx => "tsx",
        ParseLanguage::JavaScript => "js",
        ParseLanguage::Rust => "rs",
        ParseLanguage::Python => "py",
    }
}

/// A structural rule ready to run: its spec, its fix sentence and one compiled pattern per language.
pub struct CompiledRule {
    spec: RuleSpec,
    fix: FixSentence,
    patterns: Vec<Pattern>,
}

impl CompiledRule {
    /// Compiles a pattern rule. Refuses a prose rule, a rule without `languages`, one with a language that has no grammar, one without
    /// a fix sentence, and a pattern that does not compile in each of its languages.
    pub fn compile(spec: &RuleSpec) -> Result<CompiledRule, PatternError> {
        let id = spec.id().as_str().to_owned();
        let Check::Pattern(pattern) = spec.check() else {
            return Err(PatternError::NotAPattern(id));
        };
        if spec.languages().is_empty() {
            return Err(PatternError::NoLanguage(id));
        }
        let fix = FixSentence::parse(spec.fix()).map_err(|e| PatternError::BadFix {
            rule: id.clone(),
            problem: e.to_string(),
        })?;
        let mut patterns = Vec::new();
        for language in spec.languages() {
            let parse_language = match language {
                Language::Known(k) => ParseLanguage::from_known(*k),
                Language::Other(_) => None,
            }
            .ok_or_else(|| PatternError::UnsupportedLanguage {
                rule: id.clone(),
                language: language.to_string(),
            })?;
            patterns.push(Pattern::compile(parse_language, pattern)?);
        }
        Ok(CompiledRule {
            spec: spec.clone(),
            fix,
            patterns,
        })
    }

    /// The rule.
    pub fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    /// The languages it runs on.
    pub fn languages(&self) -> impl Iterator<Item = ParseLanguage> + '_ {
        self.patterns.iter().map(Pattern::language)
    }

    fn in_scope(&self, path: &RepoPath) -> bool {
        let p = path.as_str();
        (self.spec.include().is_empty() || self.spec.include().iter().any(|g| path_matches(g, p)))
            && !self.spec.exclude().iter().any(|g| path_matches(g, p))
    }

    /// The findings of this rule in one file. A file outside the rule's scope or languages has none.
    pub fn run(&self, path: &RepoPath, bytes: &[u8], limits: &MatchLimits, cancel: &CancelToken) -> RuleRun {
        let none = RuleRun::default();
        let Some(language) = ParseLanguage::from_path(path) else {
            return none;
        };
        let Some(pattern) = self.patterns.iter().find(|p| p.language() == language) else {
            return none;
        };
        if !self.in_scope(path) {
            return none;
        }
        let found = pattern.find_all(bytes, limits, cancel);
        if found.matches.is_empty() {
            return RuleRun {
                findings: Vec::new(),
                truncated: found.truncated,
            };
        }
        let symbols = pacenotes_parse::parse(language, bytes, &Limits::default(), cancel)
            .map(|o| o.symbols)
            .unwrap_or_default();
        let rule = RuleId::Local(self.spec.id().clone());
        let findings = found
            .matches
            .into_iter()
            .map(|m| {
                let enclosing = symbols
                    .iter()
                    .filter(|s| s.span.contains(&m.span))
                    .max_by_key(|s| s.depth)
                    .map(|s| s.qualified_name.as_str());
                Finding {
                    fingerprint: Some(FindingFingerprint::compute(&rule, &m.text, enclosing)),
                    rule: rule.clone(),
                    severity: self.spec.severity(),
                    location: Some(Location {
                        path: path.clone(),
                        span: m.span,
                    }),
                    message: self.spec.title().to_owned(),
                    fix: self.fix.clone(),
                    fix_source: FixSource::Rule,
                    explanation: None,
                    evidence: vec![m.text],
                    fix_diff: None,
                    confidence: None,
                }
            })
            .collect();
        RuleRun {
            findings,
            truncated: found.truncated,
        }
    }
}

/// The findings of one rule in one file, and why the search stopped early if it did (the caller then reports a partial result).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuleRun {
    /// The findings found.
    pub findings: Vec<Finding>,
    /// Set when the result is partial.
    pub truncated: Option<Truncated>,
}
