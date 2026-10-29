//! The walk: a `TreeCursor` iteration with an explicit stack of frames, bounded by [`Limits`].

use std::ops::ControlFlow;

use pacenotes_core::{Position, Span, SymbolKind};
use paddock_jobs::CancelToken;
use tree_sitter::Node;

use crate::lang::ParseLanguage;
use crate::model::{CallCandidate, Import, Outline, ParsedSymbol};
use crate::spec::{Names, SymbolRule, spec};
use crate::{Limits, ParseError, chunk};

/// The longest name, target or callee kept, in bytes.
const MAX_NAME_BYTES: usize = 256;
const MAX_TARGET_BYTES: usize = 512;
/// How often the walk looks at the cancel token, in nodes.
const CANCEL_EVERY: usize = 1024;

/// What the walk remembers about an ancestor.
#[derive(Default)]
struct Frame {
    container: Option<String>,
    container_kind: Option<SymbolKind>,
    callable: Option<String>,
}

fn text(node: Node<'_>, bytes: &[u8]) -> String {
    let range = node.byte_range();
    let slice = bytes.get(range).unwrap_or_default();
    let mut s = String::from_utf8_lossy(slice).into_owned();
    cut(&mut s, MAX_TARGET_BYTES);
    s
}

fn cut(s: &mut String, max: usize) {
    if s.len() > max {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
}

fn name_text(node: Node<'_>, bytes: &[u8]) -> Option<String> {
    let mut s = text(node, bytes);
    cut(&mut s, MAX_NAME_BYTES);
    let s = s.trim().to_owned();
    (!s.is_empty()).then_some(s)
}

fn span(node: Node<'_>) -> Option<Span> {
    let (a, b) = (node.start_position(), node.end_position());
    let start = Position::new(u32::try_from(a.row + 1).ok()?, u32::try_from(a.column + 1).ok()?)?;
    let end = Position::new(u32::try_from(b.row + 1).ok()?, u32::try_from(b.column + 1).ok()?)?;
    Span::new(start, end).ok()
}

/// The last identifier-like name inside `node`: `a.b.parse` gives `parse`.
fn last_name(node: Node<'_>, bytes: &[u8], depth: usize) -> Option<String> {
    if depth > 8 {
        return None;
    }
    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "property_identifier" | "type_identifier" | "simple_identifier" | "name"
    ) {
        return name_text(node, bytes);
    }
    for field in ["name", "field", "property", "attribute", "function"] {
        if let Some(child) = node.child_by_field_name(field) {
            return last_name(child, bytes, depth + 1);
        }
    }
    let mut cursor = node.walk();
    let last = node.named_children(&mut cursor).last();
    last.and_then(|c| last_name(c, bytes, depth + 1))
}

/// The identifier under a C or C++ declarator chain (`*name(args)` gives `name`).
fn c_declarator(node: Node<'_>, bytes: &[u8]) -> Option<String> {
    let mut current = node.child_by_field_name("declarator")?;
    for _ in 0..16 {
        match current.kind() {
            "identifier" | "field_identifier" | "type_identifier" | "qualified_identifier" | "destructor_name" | "operator_name" => {
                return name_text(current, bytes);
            }
            _ => current = current.child_by_field_name("declarator").or_else(|| current.named_child(0))?,
        }
    }
    None
}

fn declarator_names(node: Node<'_>, bytes: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    let mut queue = vec![(node, 0usize)];
    while let Some((n, depth)) = queue.pop() {
        let mut walker = n.walk();
        let first_identifier = n.named_children(&mut walker).find(|c| c.kind().ends_with("identifier"));
        let is_declarator = n.kind() == "variable_declarator" || (n.kind() == "variable_declaration" && first_identifier.is_some());
        if is_declarator {
            let name = n.child_by_field_name("name").or(first_identifier);
            if let Some(name) = name.filter(|c| c.kind().ends_with("identifier")).and_then(|c| name_text(c, bytes)) {
                found.push(name);
            }
            continue;
        }
        if depth < 3 {
            let mut cursor = n.walk();
            let mut children: Vec<Node<'_>> = n.named_children(&mut cursor).collect();
            children.reverse();
            queue.extend(children.into_iter().map(|c| (c, depth + 1)));
        }
    }
    found
}

fn go_receiver(node: Node<'_>, bytes: &[u8]) -> Option<String> {
    let receiver = node.child_by_field_name("receiver")?;
    let mut cursor = receiver.walk();
    let param = receiver.named_children(&mut cursor).next()?;
    let mut ty = param.child_by_field_name("type")?;
    for _ in 0..4 {
        match ty.kind() {
            "type_identifier" => return name_text(ty, bytes),
            _ => ty = ty.named_child(0)?,
        }
    }
    None
}

fn impl_type(node: Node<'_>, bytes: &[u8]) -> Option<String> {
    let ty = node.child_by_field_name("type")?;
    let inner = if ty.kind() == "generic_type" {
        ty.child_by_field_name("type").unwrap_or(ty)
    } else {
        ty
    };
    name_text(inner, bytes)
}

/// `(qualifier, name)` of each symbol a node declares.
fn names(rule: &SymbolRule, node: Node<'_>, bytes: &[u8]) -> Vec<(Option<String>, String)> {
    let one = |n: Option<String>| n.map(|n| vec![(None, n)]).unwrap_or_default();
    match rule.names {
        Names::Literal(l) => one(Some((*l).to_owned())),
        Names::Field(f) => one(node.child_by_field_name(f).and_then(|n| name_text(n, bytes))),
        Names::AllFields(f) => {
            let mut cursor = node.walk();
            node.children_by_field_name(f, &mut cursor)
                .filter_map(|n| name_text(n, bytes))
                .map(|n| (None, n))
                .collect()
        }
        Names::CDeclarator => one(c_declarator(node, bytes)),
        Names::TypedefDeclarator => one(c_declarator(node, bytes)),
        Names::GoMethod => {
            let Some(name) = node.child_by_field_name("name").and_then(|n| name_text(n, bytes)) else {
                return Vec::new();
            };
            vec![(go_receiver(node, bytes), name)]
        }
        Names::ImplType => one(impl_type(node, bytes)),
        Names::Declarator => declarator_names(node, bytes).into_iter().map(|n| (None, n)).collect(),
    }
}

fn is_callable(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor)
}

fn is_type_like(kind: SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::Interface | SymbolKind::Enum | SymbolKind::Trait
    )
}

fn function_value(node: Node<'_>) -> bool {
    node.kind() == "variable_declarator"
        && node.child_by_field_name("value").is_some_and(|v| {
            matches!(
                v.kind(),
                "arrow_function" | "function_expression" | "function" | "generator_function"
            )
        })
}

fn import_target(rule_field: Option<&str>, node: Node<'_>, bytes: &[u8]) -> Option<String> {
    let raw = match rule_field {
        Some(f) => text(node.child_by_field_name(f)?, bytes),
        None => text(node, bytes),
    };
    let mut t = raw.trim().trim_end_matches(';').trim();
    for keyword in ["import ", "using ", "use "] {
        if let Some(rest) = t.strip_prefix(keyword) {
            t = rest.trim();
            break;
        }
    }
    let t = t.trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>' || c == '`');
    (!t.is_empty()).then(|| t.to_owned())
}

struct State<'a> {
    language: ParseLanguage,
    bytes: &'a [u8],
    out: Outline,
}

impl State<'_> {
    fn qualified(&self, frames: &[Frame], qualifier: Option<&str>, name: &str) -> String {
        let sep = self.language.separator();
        let mut parts: Vec<&str> = frames.iter().filter_map(|f| f.container.as_deref()).collect();
        if let Some(q) = qualifier {
            parts.push(q);
        }
        parts.push(name);
        parts.join(sep)
    }

    fn visit(&mut self, node: Node<'_>, frames: &[Frame]) -> Frame {
        let table = spec(self.language);
        let kind = node.kind();
        if !node.is_named() {
            return Frame::default();
        }
        if let Some(rule) = table.symbols.iter().find(|r| r.kind == kind) {
            return self.symbol(rule, node, frames);
        }
        if let Some(rule) = table.imports.iter().find(|r| r.kind == kind) {
            if let (Some(target), Some(span)) = (import_target(rule.field, node, self.bytes), span(node)) {
                self.out.imports.push(Import { target, span });
            }
            return Frame::default();
        }
        if let Some(rule) = table.calls.iter().find(|r| r.kind == kind) {
            let callee = if rule.callee.is_empty() {
                node.named_child(0)
            } else {
                node.child_by_field_name(rule.callee)
            };
            if let (Some(name), Some(span)) = (callee.and_then(|c| last_name(c, self.bytes, 0)), span(node)) {
                let caller = frames.iter().rev().find_map(|f| f.callable.clone());
                self.out.calls.push(CallCandidate {
                    callee: name,
                    caller,
                    span,
                });
            }
        }
        Frame::default()
    }

    fn symbol(&mut self, rule: &SymbolRule, node: Node<'_>, frames: &[Frame]) -> Frame {
        let inside_callable = frames.iter().any(|f| f.callable.is_some());
        if (rule.skip_in_callable && inside_callable) || rule.needs_field.is_some_and(|f| node.child_by_field_name(f).is_none()) {
            return Frame::default();
        }
        let found = names(rule, node, self.bytes);
        let mut frame = Frame::default();
        for (i, (qualifier, name)) in found.iter().enumerate() {
            let mut kind = match (rule.refine, rule.symbol) {
                (Some(f), _) => f(node),
                (None, Some(k)) => k,
                (None, None) => SymbolKind::Struct,
            };
            if function_value(node) {
                kind = SymbolKind::Function;
            }
            let in_type = frames
                .iter()
                .rev()
                .find(|f| f.container_kind.is_some() || f.callable.is_some())
                .and_then(|f| f.container_kind)
                .is_some_and(is_type_like);
            let qualified_name_member = self.language == ParseLanguage::Cpp && name.contains("::");
            if kind == SymbolKind::Function && (in_type || qualified_name_member) {
                kind = SymbolKind::Method;
            }
            let qualified = self.qualified(frames, qualifier.as_deref(), name);
            if let (Some(_), Some(span)) = (rule.symbol, span(node)) {
                let depth = frames.iter().filter(|f| f.container.is_some()).count();
                self.out.symbols.push(ParsedSymbol {
                    kind,
                    name: name.clone(),
                    qualified_name: qualified.clone(),
                    span,
                    depth: u32::try_from(depth).unwrap_or(u32::MAX),
                });
            }
            if i == 0 {
                if rule.container {
                    frame.container = Some(name.clone());
                    frame.container_kind = Some(kind);
                }
                if is_callable(kind) {
                    frame.callable = Some(qualified);
                }
            }
        }
        frame
    }
}

pub(crate) fn parse(language: ParseLanguage, bytes: &[u8], limits: &Limits, cancel: &CancelToken) -> Result<Outline, ParseError> {
    if bytes.len() > limits.max_bytes {
        return Err(ParseError::TooLarge {
            len: bytes.len(),
            max: limits.max_bytes,
        });
    }
    if cancel.is_cancelled() {
        return Err(ParseError::Cancelled);
    }
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language.grammar()?).map_err(|e| ParseError::Grammar {
        language: language.id(),
        why: e.to_string(),
    })?;
    let mut cancelled = false;
    let tree = {
        let mut progress = |_: &tree_sitter::ParseState| {
            if cancel.is_cancelled() {
                cancelled = true;
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        let options = tree_sitter::ParseOptions::new().progress_callback(&mut progress);
        parser.parse_with_options(&mut |offset, _| bytes.get(offset..).unwrap_or_default(), None, Some(options))
    };
    if cancelled {
        return Err(ParseError::Cancelled);
    }
    let tree = tree.ok_or(ParseError::NoTree)?;

    let mut state = State {
        language,
        bytes,
        out: Outline {
            language,
            symbols: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            chunks: Vec::new(),
            partial: false,
            has_errors: tree.root_node().has_error(),
        },
    };
    let mut cursor = tree.walk();
    let mut frames: Vec<Frame> = Vec::new();
    let mut visited = 0usize;
    'walk: loop {
        visited += 1;
        if visited > limits.max_nodes {
            state.out.partial = true;
            break;
        }
        if visited.is_multiple_of(CANCEL_EVERY) && cancel.is_cancelled() {
            return Err(ParseError::Cancelled);
        }
        let frame = state.visit(cursor.node(), &frames);
        if cursor.goto_first_child() {
            frames.push(frame);
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                break 'walk;
            }
            frames.pop();
        }
    }
    let total_lines =
        u32::try_from(bytes.iter().filter(|b| **b == b'\n').count() + usize::from(!bytes.ends_with(b"\n") && !bytes.is_empty()))
            .unwrap_or(u32::MAX);
    state.out.chunks = chunk::build(&state.out.symbols, total_lines);
    Ok(state.out)
}
