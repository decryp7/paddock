//! Shell on tree-sitter-bash: a comment is a `comment` node of the parse tree, so a `#` inside a quote, `$'...'`, `${#var}`, `$#`, a URL
//! fragment or the body of a here-document is no comment. The parser recovers from errors, so a script that does not parse still gives
//! the comments it found.

use tree_sitter::{Language, Node, Parser};

/// Parses `text` with `language` and collects the byte ranges of the nodes of kind `comment`.
pub(super) fn comment_nodes(language: &Language, text: &str) -> Vec<(usize, usize)> {
    let mut parser = Parser::new();
    if parser.set_language(language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect(tree.root_node(), &mut out);
    out
}

fn collect(node: Node, out: &mut Vec<(usize, usize)>) {
    if node.kind() == "comment" {
        out.push((node.start_byte(), node.end_byte()));
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect(child, out);
    }
}

/// The byte ranges of the comments of a shell script.
pub fn comment_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut v = comment_nodes(&tree_sitter_bash::LANGUAGE.into(), text);
    v.sort_unstable();
    v
}

#[cfg(test)]
mod tests {
    use crate::check::lex::{Kind, comments_only};

    fn found(text: &str) -> Vec<String> {
        comments_only(text, Kind::Shell)
            .split('\n')
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }

    #[test]
    fn a_hash_in_a_quote_a_here_document_or_an_expansion_is_no_comment() {
        assert_eq!(found("echo 'a # b' \"c # d\" # real\n"), ["# real"]);
        assert_eq!(found("cat <<EOF\n# body\nEOF\n# after\n"), ["# after"]);
        assert_eq!(found("echo ${#var} $# a#b # real\n"), ["# real"]);
        assert_eq!(found("echo $'a\\'b' # real\n"), ["# real"]);
        assert_eq!(found("echo 'a\n # inside\n'\n# real\n"), ["# real"]);
        assert_eq!(found("curl http://x/#frag # real\n"), ["# real"]);
        assert_eq!(found("echo a\t# tab\n"), ["# tab"]);
    }
}
