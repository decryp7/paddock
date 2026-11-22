# Amendment to ADR 0042 §5 (2026-11-22): a complete verdict is reused on main and on schedules, with one key

- Status: Accepted (user, 2026-11-21 and 2026-11-22): ER-49 ("test a commit once") and ADR 0113 (on `fix/run-scope-reuse-p2`, copied to main with its code), with the check-result reuse of ADR 0038 X-2 (approved 2026-11-21). Recorded by the ADR audit of 2026-11-22.
- Amends: §5, "tree-hash reuse for branch runs only, never on main or a schedule".

1. **Scope.** A commit with a complete green run is not run again: the push to main, a re-dispatch, a tag build and a schedule reuse the verdict
   through the one reader (ER-15). A run happens for a new commit, or once to retry a suspected flake (ER-10, ER-12).
2. **One key.** The local check-result reuse in the pre-push gate (ADR 0038 §5 / WP-6, "G26", row X-2 of `docs/plans/ci-in-rust.md`) uses the
   same tree-hash key function as CI's reuse; there is no second reuse mechanism (rule 55). This also amends 0022's "has no result cache".
