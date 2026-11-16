# Amendment to ADR 0098 (2026-11-16): growth checks measure against the branch's target, not `origin/main`

- Status: Accepted (user, 2026-11-16): growth for C53, C71, C104 and C115 is measured against the branch's target, and an
  unresolvable target is a not-measured error in CI (ADR 0089). Implemented on `feat/testkit-stand-in`.

## What changes, and for which gates

Every baseline and allow list that "may only shrink" is compared with the list at the merge base. Until now the ref it was compared
with was `PADDOCK_BASE_REF`, else `origin/main`, read in five places (C53, C71, C104, C115, and each new reader). On a member branch of
a train `origin/main` is the wrong target: the train's own earlier entries count as the member's growth, and nothing says so.

From this change:

- One function reads the variable: `check::baseline::target_ref` (Rust) and `targetRef` in `conformance/src/util/baseline.mjs`
  (JavaScript). The run's own ref, else `PADDOCK_BASE_REF`, else `origin/main`; an empty value is unset. A source audit test per host
  pins that no other code reads it.
- **These gates stay blocking and their meaning changes:** C53, C71, C104 and C115 (errors on growth) now measure growth against the
  target branch instead of `origin/main`. The shared allow-list reader (C121, C124, C125, C130) measures the same way; those checks are
  report-only (ER-41), so its growth finding is a warning.
- Where the target is set: `.gitea/workflows/ci.yml` sets `PADDOCK_BASE_REF: ${{ inputs.base || 'origin/main' }}` in the conformance job
  and `${{ inputs.base }}` in the train-gates job (the dispatch's `base`: the commit of main the queue builds the train on). This
  workflow has no pull_request trigger. `./dev check --pre-push` sets it to the branch's target: the nearest of `<remote>/main` and
  `<remote>/train/*` that the pushed commit contains, never the remote ref's own old tip (a second push would then hide the first push's
  growth: reproduced on C53) and never another feature branch; an explicit `--base` (the merge check) is kept. It prints the target
  and why (ER-27).
- **Not measured never passes (ADR 0089):** in CI a target that cannot be resolved is a not-measured error no waiver covers, for the
  blocking baselines too; locally it is a warning with the next step. A base list in an older format is a warning with its own fix, not
  a missing target.

## Why this is a behaviour change worth the user's attention

A blocking gate now answers a different question on a train member (growth since the target, not since main). On the first runs it can
report growth that the old gate hid (the train's entries seen from main are hidden today only because main has them too) or stop
reporting growth the old gate blamed on the member. The first CI runs with the variable set are the measurement; until one is green on
the real tree this stays recorded as a proposal.

## Common practice

A pull request is judged against its target: Danger's `git` object names the two commits it compares as `base` ("The git commit Danger
is comparing from") and `head`, and gives file contents "at the PR merge base" as the before state
(<https://danger.systems/js/reference.html>, read 2026-11-16). That page does not spell out how the modified-file list is computed, so
it supports the principle (compare with the target's merge base) but is not a measurement of it. Measuring a stacked branch against the
main line is the failure this amendment removes: the stack's earlier work shows as the top branch's. Cost: about 0.5 agent-day, done.
