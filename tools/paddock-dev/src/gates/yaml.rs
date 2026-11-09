//! A small YAML tree with line numbers, built from `yaml-rust2`'s marked events (the reader chosen for ADR 0060 WP-1:
//! `yaml-rust2` is MIT/Apache-2.0, pure Rust and gives every event its line, which `plan.rs`'s job splitter does not need
//! and cannot give; a finding must say `file:line`). Every scalar is kept as text: the model reads workflow files, where
//! `true`, `3` and `'true'` all reach the runner as strings in an expression anyway.

use std::collections::HashMap;
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::{Marker, TScalarStyle};

/// One YAML node and the 1-based line it starts on.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// The line of the node (for a block scalar, the line of its first content line).
    pub line: usize,
    /// The value.
    pub value: Value,
}

/// A YAML value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A scalar (also `null`, as empty text).
    Scalar {
        /// The text.
        text: String,
        /// Whether it was written as a block scalar (`|` or `>`).
        block: bool,
    },
    /// A sequence.
    Seq(Vec<Node>),
    /// A mapping, in file order.
    Map(Vec<(String, Node)>),
}

impl Node {
    /// The scalar text, if this is a scalar.
    pub fn text(&self) -> Option<&str> {
        match &self.value {
            Value::Scalar { text, .. } => Some(text),
            _ => None,
        }
    }

    /// The value under `key` of a mapping.
    pub fn get(&self, key: &str) -> Option<&Node> {
        match &self.value {
            Value::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The scalar text under `key`.
    pub fn get_text(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Node::text)
    }

    /// The entries of a mapping (empty for anything else).
    pub fn entries(&self) -> &[(String, Node)] {
        match &self.value {
            Value::Map(m) => m,
            _ => &[],
        }
    }

    /// The items of a sequence (empty for anything else).
    pub fn items(&self) -> &[Node] {
        match &self.value {
            Value::Seq(s) => s,
            _ => &[],
        }
    }
}

enum Frame {
    Seq(Vec<Node>, usize, usize),
    Map(Vec<(String, Node)>, Option<String>, usize, usize),
}

#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
    anchors: HashMap<usize, Node>,
    root: Option<Node>,
    error: Option<String>,
}

impl Builder {
    fn put(&mut self, node: Node, anchor: usize) {
        if anchor != 0 {
            self.anchors.insert(anchor, node.clone());
        }
        match self.stack.last_mut() {
            None => self.root = Some(node),
            Some(Frame::Seq(items, ..)) => items.push(node),
            Some(Frame::Map(entries, pending, ..)) if pending.as_deref() == Some("<<") => {
                *pending = None;
                let _ = entries;
                self.error = Some(format!(
                    "line {}: a YAML merge key (`<<`), which this reader does not follow",
                    node.line
                ));
            }
            Some(Frame::Map(entries, pending, ..)) => match pending.take() {
                Some(key) => entries.push((key, node)),
                None => match node.value {
                    Value::Scalar { text, .. } => *pending = Some(text),
                    _ => self.error = Some(format!("line {}: a mapping key that is not text", node.line)),
                },
            },
        }
    }
}

impl MarkedEventReceiver for Builder {
    fn on_event(&mut self, ev: Event, mark: Marker) {
        let line = mark.line();
        match ev {
            Event::Scalar(text, style, anchor, _) => {
                let block = matches!(style, TScalarStyle::Literal | TScalarStyle::Folded);
                self.put(
                    Node {
                        line,
                        value: Value::Scalar { text, block },
                    },
                    anchor,
                );
            }
            Event::SequenceStart(anchor, _) => self.stack.push(Frame::Seq(Vec::new(), line, anchor)),
            Event::MappingStart(anchor, _) => self.stack.push(Frame::Map(Vec::new(), None, line, anchor)),
            Event::SequenceEnd => {
                if let Some(Frame::Seq(items, line, anchor)) = self.stack.pop() {
                    self.put(
                        Node {
                            line,
                            value: Value::Seq(items),
                        },
                        anchor,
                    );
                }
            }
            Event::MappingEnd => {
                if let Some(Frame::Map(entries, _, line, anchor)) = self.stack.pop() {
                    self.put(
                        Node {
                            line,
                            value: Value::Map(entries),
                        },
                        anchor,
                    );
                }
            }
            Event::Alias(id) => match self.anchors.get(&id).cloned() {
                Some(node) => self.put(node, 0),
                None => self.error = Some(format!("line {line}: an alias of an unknown anchor")),
            },
            _ => {}
        }
    }
}

/// Parses one YAML document. The error names the line when the reader gives one.
pub fn parse(text: &str) -> Result<Node, String> {
    let mut builder = Builder::default();
    Parser::new_from_str(text)
        .load(&mut builder, false)
        .map_err(|e| format!("not valid YAML: {e}"))?;
    if let Some(e) = builder.error {
        return Err(e);
    }
    builder.root.ok_or_else(|| "an empty YAML file".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_lines_of_keys_values_and_block_scalars() {
        let doc = parse("a: 1\nb:\n  - x\n  - y: |\n      one\n      two\n").unwrap();
        assert_eq!(doc.get("a").unwrap().line, 1);
        let b = doc.get("b").unwrap();
        assert_eq!(b.items()[0].line, 3);
        let block = b.items()[1].get("y").unwrap();
        assert_eq!(block.text(), Some("one\ntwo\n"));
        assert_eq!(block.line, 5, "the first content line of a block scalar");
    }

    #[test]
    fn a_merge_key_is_an_error_not_a_silent_miss() {
        let e = parse("base: &b {k: v}\nuse:\n  <<: *b\n  x: 1\n").unwrap_err();
        assert!(e.contains("merge key"), "{e}");
    }

    #[test]
    fn follows_aliases_and_rejects_bad_input() {
        let doc = parse("base: &b {k: v}\nuse: *b\n").unwrap();
        assert_eq!(doc.get("use").unwrap().get_text("k"), Some("v"));
        assert!(parse("a: [1,\n").is_err());
    }
}
