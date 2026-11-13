//! YAML on tree-sitter-yaml: a comment is a `comment` node, so a `#` inside a quoted scalar, a block scalar (`run: |` scripts included:
//! their content is a string; the shell comments of such a script are not YAML comments) or glued to a plain scalar is no comment, and an
//! apostrophe inside a plain scalar (`name: don't build # why`) opens no quote.

/// The byte ranges of the comments of a YAML document.
pub fn comment_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut v = super::shell::comment_nodes(&tree_sitter_yaml::LANGUAGE.into(), text);
    v.sort_unstable();
    v
}

#[cfg(test)]
mod tests {
    use crate::check::lex::{Kind, comments_only};

    fn found(text: &str) -> Vec<String> {
        comments_only(text, Kind::Yaml)
            .split('\n')
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect()
    }

    #[test]
    fn a_plain_scalar_may_hold_an_apostrophe_and_a_block_scalar_holds_no_comment() {
        assert_eq!(found("- name: don't build # why\n"), ["# why"]);
        assert_eq!(found("run: echo '# no'\n"), Vec::<String>::new());
        assert_eq!(found("run: |\n  # in the script\n  ./dev x\n# real\n"), ["# real"]);
        assert_eq!(found("a: \"x # y\" # real\nb: 'it''s' # two\n"), ["# real", "# two"]);
    }
}
