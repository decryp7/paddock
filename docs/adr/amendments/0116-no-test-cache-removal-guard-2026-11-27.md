# Amendment to ADR 0116 (2026-11-27): no check guards a removal from the `noTestCache` list

- Status: Accepted 2026-11-27 by the coordinator's decision on train/2026-11-27-y (wire the unused guard where the ADR says, or
  delete it; ER-33: a guard that is never called is not held). Amends ADR 0116 §3, the sentence "a test compares the list against
  the merge base for removals".
- Related: ER-33, ER-44, ER-46; docs/test-cache.toml (`no_test_cache`).

## What changed

ADR 0116 §3 says a removal from the `noTestCache` list (`no_test_cache` in docs/test-cache.toml) needs the user's approval in the
change, and that a test compares the list with the merge base. The comparison was built as `test_cache::config::removed`, but no
check, gate, workflow or test of the real tree ever called it: only its own unit test did. An independent review of train y's
member fix (ci/paddock-dev-os-scope) found the gap when decision 113 put the first entry on the list (`crate:tools/paddock-dev`).

The function and its unit test are deleted. The approval rule stands and is a review rule: a change that removes an entry says so
and carries the user's approval. What still holds automatically:

- the shipped file parses, and each `crate:<dir>` entry names a crate of the tree (`the_shipped_file_parses_and_holds_its_invariants`
  in `tools/paddock-dev/src/test_cache/tests.rs`), so a mistyped entry cannot silently cache a target;
- a listed target is never offered for reuse (`a_never_cached_paddock_dev_is_not_offered_for_reuse_but_other_targets_still_are`).

## If a check is wanted later

The shared allow-list reader (`tools/paddock-dev/src/check/allow_list.rs`) already compares registers with the merge base for
growth (ER-46). A removal check for this list is that reader in the other direction, as a new conformance check that starts
report-only (ER-41). It is not built.
