# Plan: Pitcrew 1.1

- Status: plan of record, written 2026-11-12 after the cut review of the same day (until now Pitcrew 1.1 had no plan file: the
  scope was the roadmap's Phase 3, its "Engineering follow-ups" and the ADRs below). Docs only: nothing here is built by this
  change. Scope is **full**, as the user decided; this file records what stays and the two cuts the user approved on
  2026-11-12.
- App: Pitcrew (`apps/pitcrew`), now at 1.0.2 (`tauri.conf.json`; the LocalSystem service of 1.0.1 is the shipped hole below).
- Order: Pitcrew 1.1 is parked until Dyno 1.1.1, Pitwall 1.1 and Debrief 1.2 ship (the release order in force on 2026-11-12). The
  security fix (first row) may land and be released earlier if the user asks; this plan does not change the order.
- Estimates: raw agent-days. **A figure marked "guess" is the review's own estimate** (`cut-review-pitwall-pitcrew.md`, 2026-11-12,
  which found no plan to read); firm numbers follow when each row has a design. No end date is given.
- Sources: [roadmap.md](../roadmap.md) (Phase 3 and the follow-ups), Pitcrew ADRs [0004](../adr/0004-ipc-protocol-and-acl.md),
  [0009](../adr/0009-linux-service-and-runner-environment.md) and [0022](../adr/0022-host-environment-and-shared-folders.md),
  family ADR 0092 (owner-only files; its adopt-tree amendment is withdrawn, see the cuts), ADR 0052 (its amendment of the same
  date), ER-38 (privileged services check the caller), `origin/fix/pitcrew-service-caller-check`.

## What the user decided on 2026-11-12 (cut review)

1. **Adopt-tree is cut** (family ADR 0092's amendment `0092-adopt-tree-2026-11-11`, decision D1 (a) full adoption, is reversed; 13.5
   days). An upgrade from 1.0.1 does not walk and re-secure the old tree. A runner whose folder is not owner-only or admin-only
   is shown **blocked** with the message "Re-register this runner" (or "Move it to your user engine"), and nothing runs from it.
   Reasons: no comparable tool adopts an old tree (GitHub's runner says remove and re-configure; installers recreate their own
   folders), the family rule is "no migrations before v1, reject old data with a clear message", and 1.0.1 has one installer
   user. Risk accepted: the one 1.0.1 install re-registers its runners by hand once. Dyno's helper folder is recreated by its
   installer and needs no adoption.
2. **The ADR 0052 items that gated this release are trimmed** (14-16 days per the review): the gate is Pitcrew's
   `features.toml` rows, report-only (0.5), and C85 never blocks a tag. WP-A1 to WP-A3, WP-B1 to WP-B3, WP-B5 and WP-C3 of
   ADR 0052 leave the Pitcrew 1.1 gate; they stay in that ADR as later work with no tag attached.

## What stays (scope is full)

| Item | What | State | Days |
|---|---|---|---|
| **Caller check (ER-38)** | The service authorises every request that changes something by the caller's identity read from the OS (pipe client token); a request that changes what runs elevated needs an administrator caller. "Service only in admin-only folders" (ADR 0004 amendment) plus a per-user engine for the rest, and the pipe owner check. Branch `fix/pitcrew-service-caller-check` (`a52ef707d`): review round 8 is done and 7 VM rows are written, not run | partly built, not landed; VM rows open | 1.5-2.5 (guess) |
| **Blocked and re-register path** | The cut above: a runner folder that is not admin-only or owner-only is blocked on upgrade with a clear message and a next step; test with a fixture of an old-layout folder | open | 0.5-1 (guess) |
| **Features rows, report-only** | `apps/pitcrew/features.toml` as Pitwall has it (ADR 0052 §1.4, C85); owed entries allowed, never a tag block | open | 0.5 |
| **Move onto the shell** | Pitcrew's UI on the shared shell and its long jobs on `paddock-jobs` (ADR 0052 §"Before the Pitcrew 1.1 tag" named this as the reason for its gate) | open; not sized here | not sized |
| **Host environment** | Environment page, shared folders, variables, ports, secrets, PATH additions, per-runner effective environment, the Actions cache server (ADR 0022; trimmed by 12-14 days on 2026-11-11) | accepted, planned | about 13-15 (guess) |
| **Auto-scale** | Pool, queue watching, quiet hours (roadmap; no ADR text on main) | planned | 8-12 (guess) |
| **Headless modes** | LaunchDaemon, systemd system unit, one SCM service per runner under a virtual account | planned | 5-8 (guess) |
| **Runners** | Forgejo's runner, GitHub's self-hosted runner, per-runner `env` for interactive Windows runners (roadmap Phase 3) | planned | not sized |
| **Safer updates** | Roll-back on a first-job crash; compressed `.xz` downloads | planned | 2-3 (guess) |
| **Windows job object** | Orphaned job processes counted in a runner's usage (ADR 0007) | planned | 1-2 (guess) |
| **Tray popup widget (P7)** | The popup of the 2026-11-05 readiness plan | planned | about 8 |
| **Launch at login, log-tail drain fix** | Shared switch, on by default; the ER-27 gap of 2026-11-03 and the missing tray of 2026-11-07 | open | 1-2 |
| **ADR 0056 `Manager` adoption** | Only if a real bug needs it (the copy register no longer blocks tags, 2026-11-11) | not planned | - |
| **Release work** | Notes, rule 25 checks, rule 54 dispatches with the bundle, the half-day VM session (inside the caller-check rows), one independent UI review (ER-23), the tag from CI | open | about 4-6 (guess) |

**Total, full scope:** about 45-70 raw days (guess). That is the review's current-plan figure of about 75-100, less the adopt-tree
(13.5) and the ADR 0052 items (about 18), plus the blocked path (0.5-1) and the report-only rows (0.5). The vision proof of
concept (stage 1, 5-10 days, approved 2026-11-11 for after the release work) is not 1.1 scope.

## Rules for the fix, not new gates

- Security review of the caller check: **two rounds on a fixed design at most** (the review's proposal, offered to the user; if
  it is not adopted the family rule of one round plus one for security code stays). A finding after the last round is fixed with its
  failing test and a VM row, not a further round.
- Every code fix comes with a test that fails without it (ER-11); the VM rows follow ER-33 (a row that runs shows its positive
  control).

## Landing

The caller check lands after the VM rows. This plan lands with `docs/cut-review-plans-2026-11-12`, after train 6.
