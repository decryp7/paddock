# 7. `pacenotes-rules`: structural patterns, SARIF and the joint rules model

Status: proposed (2026-10-30), design note of package C1 (plan BN7). Numbered at merge. It implements the joint rules model of
`docs/research/2026-10-joint-rules-model.md` (approved decisions of 2026-10-30) in Pacenotes, with temporary copies of the shared half.

## Context

Review needs deterministic checks that need no model: structural patterns over the tree-sitter parse, results imported from other
tools (clippy, ESLint) as SARIF 2.1.0 or rdjson, waivers and suppressions with a reason and an expiry, baselines for legacy code, and
rule tests. Conformance has a second, JavaScript model of the same things; the note fixes one model, `paddock-rules`, to be created in
C3a. C1 comes first and so keeps the shared half in `pacenotes-rules/src/shared_copy`.

## Decision

- **Crate** `pacenotes-rules`, pure (no process, no I/O besides what the caller passes): depends on `pacenotes-core`, `pacenotes-parse`
  (grammars), `tree-sitter` (the same pinned version), `serde_json`, `toml`, `sha2`. No `ast-grep` crate: the matcher is small, ours
  and bounded, and avoids a second tree-sitter in the dependency graph.
- **Patterns** (`pattern`): code of the rule's language with `$NAME` (one node, the same name twice means the same text), `$$$NAME` (a
  run of siblings, commas included) and `$_`. The pattern is parsed with the target's grammar (wrapped in a function or class for
  languages that need a statement context, with and without a trailing `;`) and matched node by node: same kind, same children, tokens
  and operators equal, comments and layout ignored. Bounds: pattern 4 KiB, 32 placeholders, depth 64, 500,000 nodes, 1,000 matches, and
  100,000 steps per match attempt (past that an attempt is no match). Matching is deterministic and runs per file; it takes bytes and
  returns findings, so the index's parse pool can drive it.
- **Findings** carry the rule's id, severity and **required** fix sentence (`RuleSpec.fix` in `pacenotes-core` is now required, 512
  bytes, one line), and a `FindingFingerprint` (rule id, algorithm version 1, normalised snippet, enclosing qualified name from the
  file's outline; no path) so a baseline survives a rename.
- **Rule tests** (`rule_test`): every `bad` example must be flagged and no `good` one, in each of the rule's languages. The core already
  refuses a structural rule without both.
- **SARIF and rdjson**: as in the note, section 4. Our results go in our `run`, each imported tool's in its own with its raw id;
  suppressed results stay in `results`; columns are converted between our UTF-8 bytes and the file's code points or UTF-16 units, which
  needs the file's text (without it a result keeps its lines and loses its columns, counted). Input is bounded and never panics.
- **Waivers, suppressions, baselines**: `apply_waivers` implements expiry (inclusive, UTC, today passed in), counted waivers (above the
  ceiling a waiver hides nothing, below it the report suggests lowering it), `W001` expired, `W002` incomplete, `W003` unused (only for
  rules that ran), `W004` over its ceiling, `W005` lower the ceiling. Files: `[[waiver]]`/`[[suppression]]` TOML with `version = 1` and
  a baseline TOML with the algorithm version; both bounded.
- **Exit code**: 2 error, 1 a finding at or above the threshold, 3 partial or a rule skipped, else 0.

## Parallelism and cost (rule 51)

The cost is parsing the file once for the match and once more for the outline (to name the enclosing symbol), per file that has a
match; files without a match cost one parse. Both can come from the index's single parse when the engine wires it (A3 follow-up), at
which point `find_all` takes the tree. Matching itself is linear in the tree for a fixed pattern, bounded by the step limit. SARIF
reading is a single pass over the input with the caps above. Nothing here is measured yet; the first measurement belongs with the
engine wiring, on the Paddock monorepo, in `docs/performance.md`.

## Consequences

- C3a moves `shared_copy` into `paddock-rules` and deletes it; `tests/it/shared_copy.rs` fails while both exist.
- Until `pacenotes-core` re-exports the shared types, `pacenotes-rules` uses the core's `Severity`, `Location`, `Span`, `RepoPath` and
  `Confidence` directly, so nothing is copied twice.
- A rule that only matches by pattern misses what the grammar cannot express (types resolved across files); that is tier 3 and not this
  crate's job.
