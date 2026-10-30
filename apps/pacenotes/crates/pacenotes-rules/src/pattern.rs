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
    #[error("the pattern is not valid {language} code; check the pattern or the language")]
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

/// Bounds of one search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchLimits {
    /// Most nodes visited.
    pub max_nodes: usize,
    /// Most matches kept.
    pub max_matches: usize,
}

impl Default for MatchLimits {
    fn default() -> Self {
        MatchLimits {
            max_nodes: 500_000,
            max_matches: 1_000,
        }
    }
}

fn wrapper(language: ParseLanguage) -> Option<(&'static str, &'static str)> {
    match language {
        ParseLanguage::Rust => Some(("fn __pn() {\n", "\n}")),
        ParseLanguage::Go => Some(("package p\nfunc __pn() {\n", "\n}")),
        ParseLanguage::Java => Some(("class P { void m() {\n", "\n} }")),
        ParseLanguage::CSharp => Some(("class P { void M() {\n", "\n} }")),
        ParseLanguage::Kotlin => Some(("fun m() {\n", "\n}")),
        ParseLanguage::C | ParseLanguage::Cpp => Some(("void m(void) {\n", "\n}")),
        ParseLanguage::TypeScript | ParseLanguage::Tsx | ParseLanguage::JavaScript | ParseLanguage::Python => None,
    }
}

/// Swaps `$NAME`, `$$$NAME` and `$_` for identifiers the grammar accepts, and says what each stands for.
fn substitute(pattern: &str) -> Result<(String, BTreeMap<String, Var>), PatternError> {
    let bytes = pattern.as_bytes();
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut vars = BTreeMap::new();
    let mut i = 0;
    let mut n = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
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
        // copy one character
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
    /// Compiles `source` for `language`.
    pub fn compile(language: ParseLanguage, source: &str) -> Result<Pattern, PatternError> {
        let trimmed = source.trim();
        if trimmed.is_empty() {
            return Err(PatternError::Empty);
        }
        if trimmed.len() > MAX_PATTERN_BYTES {
            return Err(PatternError::TooLong(trimmed.len()));
        }
        let (text, vars) = substitute(trimmed)?;
        let grammar = language.grammar().map_err(|e| PatternError::Grammar {
            language: language.id(),
            why: e.to_string(),
        })?;
        let mut parser = Parser::new();
        parser.set_language(&grammar).map_err(|e| PatternError::Grammar {
            language: language.id(),
            why: e.to_string(),
        })?;
        let attempts: Vec<(String, usize)> = {
            let mut v = vec![(text.clone(), 0)];
            if let Some((head, tail)) = wrapper(language) {
                v.push((format!("{head}{text}{tail}"), head.len()));
                // a statement pattern written without its `;`
                v.push((format!("{head}{text};{tail}"), head.len()));
            }
            v
        };
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

    /// Every place the pattern matches in `bytes` (a file of this pattern's language). Nested matches are all reported. Returns what it
    /// found when a bound is reached.
    pub fn find_all(&self, bytes: &[u8], limits: &MatchLimits) -> Vec<Match> {
        let Ok(target_src) = std::str::from_utf8(bytes) else {
            return Vec::new();
        };
        let Ok(grammar) = self.language.grammar() else { return Vec::new() };
        let mut parser = Parser::new();
        if parser.set_language(&grammar).is_err() {
            return Vec::new();
        }
        let Some(tree) = parser.parse(target_src, None) else {
            return Vec::new();
        };
        let Some(root_pattern) = self.root() else { return Vec::new() };
        let kind = root_pattern.kind_id();
        let mut out = Vec::new();
        let mut visited = 0usize;
        let mut cursor = tree.walk();
        loop {
            let node = cursor.node();
            visited += 1;
            if visited > limits.max_nodes || out.len() >= limits.max_matches {
                break;
            }
            if node.kind_id() == kind {
                let mut m = Matcher {
                    pat: &self.source,
                    tgt: target_src,
                    vars: &self.vars,
                    caps: Vec::new(),
                    steps: 0,
                };
                if m.node(root_pattern, node) {
                    let (s, e) = (node.start_position(), node.end_position());
                    if let (Some(a), Some(b)) = (
                        Position::new(s.row as u32 + 1, s.column as u32 + 1),
                        Position::new(e.row as u32 + 1, e.column as u32 + 1),
                    ) && let Ok(span) = Span::new(a, b)
                    {
                        let captures = m
                            .caps
                            .iter()
                            .map(|(n, (a, b))| (n.clone(), target_src[*a..*b].to_owned()))
                            .collect();
                        out.push(Match {
                            span,
                            text: target_src[node.byte_range()].to_owned(),
                            captures,
                        });
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
                    return out;
                }
            }
        }
        out
    }
}

struct Matcher<'a> {
    pat: &'a str,
    tgt: &'a str,
    vars: &'a BTreeMap<String, Var>,
    caps: Vec<(String, (usize, usize))>,
    steps: usize,
}

impl Matcher<'_> {
    fn var(&self, n: Node<'_>) -> Option<&Var> {
        self.vars.get(&self.pat[n.byte_range()])
    }

    fn same_text(&self, a: (usize, usize), b: (usize, usize)) -> bool {
        let words = |r: (usize, usize)| self.tgt[r.0..r.1].split_whitespace().collect::<Vec<_>>();
        words(a) == words(b)
    }

    fn bind(&mut self, name: &str, range: (usize, usize)) -> bool {
        if let Some((_, old)) = self.caps.iter().find(|(n, _)| n == name) {
            return self.same_text(*old, range);
        }
        self.caps.push((name.to_owned(), range));
        true
    }

    fn children<'t>(&self, node: Node<'t>) -> Vec<Node<'t>> {
        let mut cursor = node.walk();
        node.children(&mut cursor).filter(|c| !c.is_extra()).collect()
    }

    fn node(&mut self, p: Node<'_>, t: Node<'_>) -> bool {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            return false;
        }
        match self.var(p).cloned() {
            Some(Var::Any) => return t.is_named(),
            Some(Var::One(name)) => return t.is_named() && self.bind(&name, (t.start_byte(), t.end_byte())),
            Some(Var::Run(_)) => return false,
            None => {}
        }
        if p.kind_id() != t.kind_id() {
            return false;
        }
        let (pc, tc) = (self.children(p), self.children(t));
        if pc.is_empty() || tc.is_empty() {
            return pc.is_empty() && tc.is_empty() && self.pat[p.byte_range()] == self.tgt[t.byte_range()];
        }
        self.seq(&pc, &tc)
    }

    fn seq(&mut self, pc: &[Node<'_>], tc: &[Node<'_>]) -> bool {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            return false;
        }
        let Some((p, rest)) = pc.split_first() else { return tc.is_empty() };
        if let Some(Var::Run(name)) = self.var(*p).cloned() {
            for k in 0..=tc.len() {
                let mark = self.caps.len();
                let range = match (tc.first(), k.checked_sub(1).and_then(|i| tc.get(i))) {
                    (Some(a), Some(b)) => (a.start_byte(), b.end_byte()),
                    _ => (0, 0),
                };
                if self.bind(&name, range) && self.seq(rest, &tc[k..]) {
                    return true;
                }
                self.caps.truncate(mark);
            }
            return false;
        }
        let Some((t, tail)) = tc.split_first() else { return false };
        let mark = self.caps.len();
        if self.node(*p, *t) && self.seq(rest, tail) {
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
    pub fn run(&self, path: &RepoPath, bytes: &[u8], limits: &MatchLimits) -> Vec<Finding> {
        let Some(language) = ParseLanguage::from_path(path) else {
            return Vec::new();
        };
        let Some(pattern) = self.patterns.iter().find(|p| p.language() == language) else {
            return Vec::new();
        };
        if !self.in_scope(path) {
            return Vec::new();
        }
        let matches = pattern.find_all(bytes, limits);
        if matches.is_empty() {
            return Vec::new();
        }
        let symbols = pacenotes_parse::parse(language, bytes, &Limits::default(), &CancelToken::new())
            .map(|o| o.symbols)
            .unwrap_or_default();
        let rule = RuleId::Local(self.spec.id().clone());
        matches
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
            .collect()
    }
}
