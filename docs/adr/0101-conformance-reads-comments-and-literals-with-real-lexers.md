# 101. The conformance checks read Rust, shell and YAML with the languages' real lexers and parsers

- Status: Accepted 2026-11-13 (user approved the redesign, ER-21, after the WP-10 batch 2 review). Built in the same change
  (`tools/paddock-dev/src/check/lex/`); the JavaScript host keeps hand lexers until its checks move (WP-10 batches 3 to 5).
- Date: 2026-11-13
- Number: 0101 was free on `origin/main` and on every `origin` branch on 2026-11-13 (`git ls-tree` over each ref; 0098 to 0100 are taken).
- Related: ER-21, ER-22, ER-33, ER-37; ADR 0089 (not-measured never passes); `docs/plans/ci-in-rust.md` WP-10.

## Problem

Waiver comments (`// conformance: <tag> — <reason>`), and the masks the checks scan (C30, C39, C53, C120), were read by hand-written
scanners. Each hole made a marker count where it is not a comment, or hid a real one: a nested `/* /* */ */`, a `cr"..."` raw string
(C30's raw-string gap before it), a here-document body, a multi-line quote, a plain YAML scalar with an apostrophe, `$'...'` (batch 2
review: four P1s; before that, a fixture marker in a string that waived its own file). A scanner fixed for one case missed the next:
the class is "a hand lexer that is not the language's".

## Decision

One tokenizer module per language in the Rust host (ER-22), each on the real thing, read by two views (the mask, the comments only):

- Rust: `ra-ap-rustc_lexer` =0.184.0 (rustc's own lexer as published by rust-analyzer; pure Rust; MIT OR Apache-2.0).
- Shell: `tree-sitter-bash` =0.25.1 (MIT) on `tree-sitter` =0.27.2 (MIT).
- YAML: `tree-sitter-yaml` =0.7.2 (MIT) on the same engine.

A marker counts only in a comment node/token. The Rust host is the oracle: `tests/it/check_lex.rs` holds the JavaScript host's hand lexers
(`mask-rust.mjs`, `shell-lexer.mjs`, `yaml-lexer.mjs`) to it on every tracked `.rs` file, 4000 generated Rust sources, generated shell and
YAML documents and the 36 review probes. Pins are exact. Licences are all in `tools/paddock-dev/deny.toml`'s allow list. Decided on YAML:
the content of a block scalar (`run: |`) is a string, so a `# conformance:` line inside a workflow script is not a YAML comment.

## Comparable practice and alternatives (ER-37)

How the tools whose directives we mimic read them: rustc reads `#[allow]`/`#[expect]` from its own parser; ESLint reads
`/* eslint-disable */` from the comment tokens of its parser (espree); shellcheck reads `# shellcheck disable=` from its own shell parser.
None uses a regular expression over raw text. (Their documentation pages were not re-fetched for this ADR.)

Read 2026-11-13; crates.io API for versions and dates.

| Option | Exactness | Comment fidelity | Error tolerance | Maintenance, licence | Build | Verdict |
|---|---|---|---|---|---|---|
| `ra-ap-rustc_lexer` 0.184.0 (chosen, Rust) | rustc's lexer by construction | yes (comment tokens, doc style) | total: every input lexes; unterminated forms are tokens | rust-analyzer weekly releases; MIT OR Apache-2.0 | pure Rust | chosen |
| `tree-sitter-rust` 0.24.2 (updated 2026-03-27, 25 M downloads, MIT) as one engine for all three | an approximating grammar (raw strings and nested comments by an external scanner) | yes (`comment` extras) | recovers, may mis-nest | active | C (`cc`) | not chosen: less exact than the lexer for what we need, adds a C unit for no gain; the one-engine saving is a dependency we already take for shell and YAML |
| `syn` / `proc-macro2` | a Rust parser | plain comments are not in the token stream (only doc comments, as attributes); recalled, the fetched docs page does not say | `LexError` on invalid code | MIT OR Apache-2.0 | pure Rust | rejected: cannot see the thing we read |
| `tree-sitter-bash` 0.25.1 (chosen, shell) | bash grammar, here-documents, `$'..'`, `${#v}` | yes (`comment` nodes) | recovers from errors | MIT | C (`cc`) | chosen |
| `brush-parser` 0.4.0 (updated 2026-05-03, MIT, 0.9 M downloads), the shell parser of brush-shell | a bash tokenizer and parser | its `Token` is `Operator` or `Word` (read on 0.2.17: no comment variant); snapshot tests named `tokenize_comment` show comments are consumed, not emitted; not confirmed on 0.4.0 | a strict parser: an invalid script is an error | young, one maintainer | pure Rust | not chosen: comments not exposed (unconfirmed); strict |
| `tree-sitter-yaml` 0.7.2 (chosen, YAML) | YAML grammar | yes | recovers | MIT | C | chosen |
| `saphyr` 0.3.4 (2026-11-11, MIT OR Apache-2.0, 3 M downloads) / `yaml-rust2` | YAML 1.2 compliant | loaders drop comments (the `saneyaml` notes say so for semantic loaders; mainline `saphyr-parser` events not confirmed) | errors on invalid YAML | active | pure Rust | not chosen: no comments |
| `granit-parser` (a `saphyr-parser` fork) | YAML 1.2 | emits comment events (`emit_comments`) | errors | a fork, versions 0.0.x to 1.2.0 in a year | pure Rust | the one pure-Rust candidate that keeps comments; revisit if the C build gets in the way (not measured here) |
| libyaml | the reference scanner | its scanner skips comments (recalled, not re-verified) | errors | stable | C | not chosen |
| ANTLR 4 (grammars-v4) | per grammar, as good as the grammar | whatever the grammar keeps on a hidden channel | recovers | the official targets page lists Java, C#, Python, JavaScript, TypeScript, Go, C++, Swift, PHP and Dart: no Rust; the Rust runtime `antlr4rust` 0.5.x is a community project (README: to be merged into ANTLR "eventually"), and the tool is Java (Maven) | pure Rust runtime, Java at generation | rejected: no official Rust target, a Java toolchain in CI |
| `pest`, `nom`, `logos` (combinators and lexer generators) | exactly what we write | what we write | what we write | MIT/Apache-2.0 | pure Rust | rejected: the same hand-written grammar in a new notation, the cause of this ADR |

Cost: about 1.5 to 2 agent-days (the three tokenizer modules, the three JavaScript hand ports, the oracle tests, this ADR) against about 0.5
for one more patch of the regexes, which fixed each case and left the class (four rounds of it already). Common practice says use the
language's parser; the cheaper option, patching the scanners, is the one that failed twice.

## Consequences

- The C build of tree-sitter (`cc`): built here on macOS arm64; `cargo xwin clippy` for Windows and the Linux CI image are run on the
  final commit of this change (see its report), not assumed. If either fails, the YAML and shell readers move to `granit-parser` and a
  pure-Rust shell lexer under the same module boundary.
- JavaScript's hand lexers stay until the JS host retires; they are tested against the oracle, never the other way round.
- A new language (TOML for `deny.toml` markers) is one more file in `lex/`.

Recurring: waiver markers read where they are not comments: C120 waiving `check_engine.rs` through a string literal (WP-10 batch 1),
C39 and C53 fixtures (batch 1), the four lexer P1s (batch 2), C30's raw-string gap.
