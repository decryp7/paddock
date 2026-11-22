# Amendment to ADR 0049 (2026-11-22): WP-1 (push-check.yml) is dropped; C82 and §7 are reversed by C145, the lessons driver and train-clash

- Status: Accepted (user, 2026-11-22), recorded by the ADR audit of 2026-11-22.
- Amends: WP-1 (kept by the trim of 2026-11-11), §5 (C82, trimmed) and §7 (`./dev drift`, trimmed).

1. **WP-1 is dropped.** `push-check.yml` on every branch push is not built and will not be: the member gate and the pre-push gate replace it.
   Rule 50 (f) in `docs/rules.md`, reserved for it, is free.
2. **§5 and C82.** The trim dropped C82 (a check of `merge=union` files). The user reverses the trim: C145 (the lessons check, `feat/lessons-check-and-job-cleanup`)
   with the lessons merge driver (`.gitattributes` `merge=union`) does that job. C145 cites this ADR §5.
3. **§7 drift.** The trim dropped `./dev drift` (behind, unlanded, overlap). The overlap part is built as `train-clash` (`feat/train-clash-warning`: a warning at
   `worktree add` and in the merge check, over files both sides changed). Behind and unlanded are not built.
