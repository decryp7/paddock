# Amendment to ADR 0092 (2026-11-12): the adopt-tree amendment is withdrawn

- Status: Accepted 2026-11-12 (user, cut review of 2026-11-12). Reverses decision D1 (a) of
  `0092-adopt-tree-2026-11-11` (full adoption, accepted 2026-11-11), which is on `origin/docs/adr-adopt-tree` (`5be022d52`) and
  not on `main`. When that branch lands, its status line takes this amendment's date.
- Related: ER-37 (measure a design against common practice), ER-38, ADR 0092 §4 (S4), Pitcrew plan
  [pitcrew-1-1.md](../../../apps/pitcrew/docs/plans/pitcrew-1-1.md), the cut review (`cut-review-pitwall-pitcrew.md`, 2026-11-12).

## Decision

The shared mechanism that adopts an older version's folder tree (recursive kept folders, Dyno's helper folder, a policy-version
bump that adopts again; T0 to T5, 13.5 days, three security review rounds, never built) is **not built**.

What happens instead, on an upgrade from a version that made a folder with weak rights:

- **Pitcrew:** a runner whose folder is not admin-only (service mode) or owner-only (user engine) is shown as **blocked**, with
  the message "Re-register this runner" (or "Move it to your user engine"); nothing runs from it. The one 1.0.1 install
  re-registers its runners by hand once.
- **Dyno's helper folder:** deleted and recreated by the installer; it holds only Dyno's own files.

## Why (ER-37)

- GitHub's runner tells you to remove and re-configure; UiPath converts a service-mode robot; installers own their Program Files
  and ProgramData folders and recreate them. None walks and re-secures an old tree.
- The family rule "no migrations before v1, reject old data with a clear message" (user, 2026-11-06) says the same.
- 1.0.1 has one installer user, who is the owner.

Risk accepted: one manual re-registration. New facts since D1 (a): the no-migration rule applies, and the install base is one
user. Earlier text is history.
