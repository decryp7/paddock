# 42. One owner per shared harness, and smaller merge batches: a home branch per harness, size and age caps, CI once per tree

- Status: Proposed (2026-11-03). The user approved drafting it on 2026-11-03 (ER-21 redesigns, item 6). No work starts
  before it is accepted.
- Date: 2026-11-03
- Related: ER-1 / rule 1 (one release at a time), ER-2 / rule 2 (worktrees), ER-4 (complete Conventional Commits), ER-8 /
  rule 3 (the full suite once on the combined result), ER-21 / rule 40, ER-22 / rules 42-43 (share first), rule 50 (CI
  economy), rule 55 and ADR 0034 (one implementation per mechanism, the copies register), ADR 0014 §9 (CODEOWNERS, owned
  paths), ADR 0038 (CI parity before push; its gate stamp and dispatch guard), ADR 0040 (the Windows GUI session, which
  merges `vm-win` into `vm-smoke`). The `integrate` skill.
- Evidence (read at `origin/main` `9da2ccf59`, on the named branches, and in the coordinator log
  `/Volumes/d7sst7/dev-targets/handoff/2026-10-29-overnight.md`, cited as "log" with its line, 2026-11-03):
  - `CODEOWNERS`: one human owner (`@decryp7`) for every path; it says it is "not a gate" but the written form of owned
    paths in briefs. It cannot say which agent or branch holds a harness now.
  - `docs/known-copies.toml` (ADR 0034, 46 mechanisms): copies of a mechanism **on `main`**. It has no notion of branches.
  - `conformance/src/commit-lint.mjs`: `MAX_HEADER = 100`. `lefthook.yml` has `pre-commit` and `pre-push` hooks but no
    `commit-msg` hook, so the limit is first checked by CI's `commits` job.
  - Measured with `git log --no-merges <all 218 remote branches> --not origin/main -- <path>` (2026-11-03): commits not on
    `main` that touch `scripts/vm-smoke` 68, with 36 distinct subjects (32 are the same change again, by cherry-pick or
    landing copy); `conformance/src` 61 (58 distinct); `tools/paddock-dev` 33 (30); `.gitea/actions` 27 (26);
    `scripts/affected.mjs` 17; `ui/projects/paddock-e2e` 16; `crates/paddock-testkit` 5; `scripts/vm-win.sh` 4, on two
    branches only (`tools/win-test-vm`, `tools/win-update-test`), neither merged.
  - Landings on `main` (`git rev-list --count --no-merges`, files from `git diff --name-only`): `925bf02a4..c3404cdc7` 47
    commits, 122 files; `c3404cdc7..09293bcde` 3, 14; `09293bcde..cd4aa0afa` 5, 8; `cd4aa0afa..233c1339b` 30, 78;
    `233c1339b..72edd2eaa` (Dyno 1.0) **146 commits, 403 files**; `72edd2eaa..9da2ccf59` 3, 2. Waiting:
    `steward/land-1` (Spotter 1.0) **222 commits, 524 files**, oldest commit 2026-10-28, not landed on 11-03 (new RustSec    advisories, log 498).

## Context

| Date | Instance | Source |
|---|---|---|
| 10-29 to 11-01 | `scripts/vm-smoke` developed on three branches at once (`steward/s28c-dyno-1.0`, `spotter/update-smoke`, `feat/vm-smoke-gui`); the Spotter batch was blocked until they were unified on `tools/vm-smoke-unified` (`a456014a9`, `20ba5a4b1`), reviewed and re-tested | log 371, 373; lesson commit `0727f2085` |
| 10-31 | A fourth line of the same harness: `vm-win.sh` `gui-setup` and `job` (`22fef8b07`) on `tools/win-update-test`, used by `vm-smoke-unified`'s `update-win.mjs` but never merged; the unified README calls it "not built" | ADR 0040 |
| 10-31 to 11-01 | Pitwall line `s23b`: CI on it was dispatched at least seven times (7242, 7256-7257, 7274, 7276, 7297-7298, 7302) over a day, failing on shared CSS, fmt, a Dyno race fixed elsewhere and a degraded Windows runner; then **121 commits conflicted with the Dyno batch `s28c` in 12 files**, and `s23b` carried Dyno fixes `s28c` lacked | log 295-362, 425 |
| 11-01 | "Correction: the Spotter 1.0 line (`s24e` plus run-state plus fix plus tray-widgets, about 166 commits) was never on main"; only its shared parts were. It was merged onto `s28c` with hunk-by-hunk decisions in 9 files and an independent review of the resolutions | log 375 |
| 10-31 to 11-01 | Landing copies to reword subjects: `s22b` (4 over-long subjects), `s28b` and `s28d` (Dyno), `s29b` (12 subjects), `s29c` (8 over 100 characters plus 2 `wip` commits), `s30` (about 10). Each copy has the same tree and was dispatched to CI again | log 277, 339, 378, 429, 439, 460 |
| 11-01 | `ci/artifacts-v4` failed the `commits` job on one 108-character subject (`fd44404c0`): a CI run for a message | log 408 |
| 11-01 | `s29b`: CI dispatched four times on four heads in three hours (7340-7342, 7343-7345, 7346-7348, 7350-7352) | log 462-479 |
| 11-03 | `land-1` (Spotter, 222 commits) red on new advisories; the batch waits whole | log 498 |

**Pattern.** When a shared harness or a release line lives on several branches at once, the copies diverge, and the
divergence surfaces late, as large merges with many conflicts, re-reviews and repeated CI runs of the same tree. Batch
size and age multiply the cost: every extra day of a line adds commits that conflict with the next line, and every CI run
of a 150-commit batch costs about 1.5 hours of the one Mac runner (memory `ci-economy`).

**Invariant.** (a) Every change to a shared harness is made on that harness's one home branch, and every other branch
gets it by merging that home. (b) No line holds more than a bounded amount of unlanded work, by size or by age. (c) The
same tree is tested by CI once.

## Decision

### 1. The harness register: `docs/harnesses.toml`

A new register beside `known-copies.toml` (that one is about copies on `main`; this one is about who may change shared
tooling on branches):

```toml
version = 1

[[harness]]
id = "vm-smoke"
title = "VM start, GUI and update checks (macOS Tart, Windows Fusion), including vm-win"
paths = ["scripts/vm-smoke/**", "scripts/vm-win.sh", "scripts/vm-win/**", "tools/macos-vm-smoke.sh", "apps/*/vm-smoke.json"]
home = "tools/vm-smoke"          # the one branch where its files change
mode = "home"                     # "home": every change here; "additive": others may add new files only
lands = "48h"                     # the home lands on main at least this often while it is ahead
note = "apps/*/vm-smoke.json: an app's own block may change on the app's branch (declared exception below)"
exceptions = ["apps/*/vm-smoke.json"]
```

Seeded with the harnesses measured above: `vm-smoke` (home), the CI kit (`.gitea/actions/**`, `.gitea/workflows/ci.yml`,
`nightly.yml`, `app-release.yml`, `scripts/affected.mjs`; home), `paddock-dev` (`tools/paddock-dev/**`, `dev`; home),
`conformance` (`conformance/src/**`; **additive**: a feature branch may add a new check file, while edits to the runner,
the index and existing checks go through the home), `paddock-e2e` (`ui/projects/paddock-e2e/**`; home) and
`paddock-testkit` (`crates/paddock-testkit/**`; home). Product shared code (`crates/`, `ui/projects/paddock-ui`) is not a
harness: it changes with its adopting apps in one change (rules 39, 42).

**The owner** is the home branch plus a lease: the coordinator gives one agent at a time a harness's lease in its brief
("owns harness vm-smoke on tools/vm-smoke"), and logs it. An agent without the lease who needs a harness change asks
the lease holder, or the coordinator moves the lease. Agents are short-lived; the home branch is not.

### 2. The check: C72, harness changes come from the home

`paddock-conformance harness --range <base>..HEAD`, run by the steward before a landing and by CI's `commits` job (it adds
seconds, no new run):
- every non-merge commit in the range that touches a harness's paths (except its `exceptions`) must be reachable from
  `origin/<home>`, or the range is the home itself; in `additive` mode a commit that only adds files passes;
- two harnesses may not claim the same path; a harness path that no entry claims but that matches the seeded kinds
  (`scripts/vm-*`, `.gitea/actions/*`) is reported, so a new harness gets its entry;
- `harness --stale` lists every home ahead of `main` by more than its `lands` age, for the coordinator's check-in.

It fails today on the `vm-smoke` commits of `s28c`, `spotter/update-smoke` and `feat/vm-smoke-gui`, and on
`tools/win-update-test`'s `vm-win` commits (a second home for one harness).

### 3. Batch caps

A landing batch onto `main` holds at most:
- **60 non-merge commits and 200 changed files**, and
- work whose oldest unlanded commit is at most **72 hours** old.

These come from the landings above: the batches of 3 to 47 commits landed after one CI run; the batches of 121 to 222
commits each took several runs, conflict rounds or a stall. The caps are values in `docs/harnesses.toml`
(`[batch] max_commits`, `max_files`, `max_age`), so they can move with data (rule 21 spirit: tunables, not code).

How lines stay under them:
- **A release line lands in parts, untagged** (already the practice for the tray widgets, 2026-10-31): each green part lands
  on `main`; the tag comes from `main` later. A line never holds finished parts back for the tag.
- **Shared first.** Commits in a line that change shared code or a harness are split out and landed ahead of the line's
  app work, so the next line sees them on `main`, not in a sibling branch (`s23b`'s shared CSS, the Dyno fixes it carried).
- **Merge `main` into every open line daily** (not rebase: the reviewed commits keep their ids); conflicts appear one day
  at a time.
- **ER-1 decides the order, not the size:** the release's batches take the landing windows first; other lines land in
  windows the release does not need, and only if they do not change shared code the release depends on.

A batch over a cap lands only with a trailer on its merge commit, `Batch-size-waived: <reason>; approved: <coordinator|user>`.
C73 checks the caps over the landing range and the trailer.

### 4. No landing copies: messages right at the source

- A `commit-msg` hook in `lefthook.yml` runs `paddock-conformance commit-lint --file {1}` (the same code as CI's `commits`
  job), so an over-long subject or a `wip` type is refused when it is written, not at landing.
- The steward lands the reviewed head itself (fast-forward, or a merge commit), never a reworded copy. A history that
  still needs rewording is reworded **on its branch before review**, and reviewed as that.

### 5. CI once per tree

The plan job (`affected.mjs --ci`, later `./dev plan`) looks up the head's **tree hash** among successful runs of the same
workflow in the last 24 hours. If one ran on an identical tree (the tree includes `.gitea/`, so the workflows are identical
too), the run marks every gate job "tree already green in run <id>" with the link, skips them, and still runs the
`commits` job (messages differ). The 24-hour bound exists because some gates read the world (new RustSec advisories failed
`land-1` on an unchanged tree on 11-03). This removes the second run of a landing copy or a reworded history; ADR 0038's
local stamp is the same idea before the push.

### 6. Landing cadence

The steward lands when a batch is green, at most every 8 hours on working days (three main runs a day at most, each about
1.5 h of the Mac runner), and a harness home at least every 48 hours while it is ahead. Docs-only batches land without CI
(rule 50 (d)) and do not count. Rule 50's "one main run per batch" stands; smaller batches mean more main runs, which the
removed repeats (§4, §5) and the avoided conflict rounds are expected to outweigh. WP-6 measures both before the caps are
tuned.

## Checks that make the class detected

| Check | What it enumerates | Fails today on |
|---|---|---|
| **C72** harness homes | every commit in a landing range against every harness's paths and home | the three `vm-smoke` lines, `tools/win-update-test` |
| C72 `--stale` | every home ahead of `main` past its `lands` age | `tools/vm-smoke-unified` (ahead since 11-01), `tools/win-update-test` (since 10-31) |
| **C73** batch caps | the landing range's commits, files and oldest commit date, and the waiver trailer | `land-1` (222 commits, 524 files, oldest 10-28) |
| `commit-msg` hook and `commits` job | every new commit message | an over-long subject before it is pushed |
| Plan job tree lookup | every dispatch against the last day's green runs | a landing copy's repeated run |

## Rollout and order

1. **WP-1:** `docs/harnesses.toml` seeded, the proposed rule 61 (below), AGENTS.md "Working in the monorepo" points at it;
   the `commit-msg` hook. The leases start by hand in briefs at once.
2. **WP-2:** consolidate the live duplicates: `tools/vm-smoke-unified` plus `tools/win-update-test` become
   `tools/vm-smoke` (shared with ADR 0040 WP-1), and land.
3. **WP-3:** C72 and C73 with fixtures and tests; wired into the `commits` job and the steward's landing checklist.
4. **WP-4:** the plan job's tree lookup, with a test against a fake Gitea API.
5. **WP-5:** split `land-1` into landings under the caps (shared parts first), as the first use.
6. **WP-6:** after two weeks, measure main runs per day, CI runs per landed commit and conflict rounds, record them
   (rule 48), and tune the caps.

The proposed **rule 61**: "One owner per shared harness, and small batches. A shared harness (`docs/harnesses.toml`)
changes only on its home branch, by the agent holding its lease; other branches merge the home. A landing holds at most
the register's caps (60 commits, 200 files, 72 hours) unless waived with `Batch-size-waived:`; a release line lands its
green parts untagged and its shared parts first; open lines merge `main` daily; the same tree runs CI once. Checked by C72
and C73. *Why:* three copies of vm-smoke blocked a release batch, and 121- to 222-commit batches took repeated CI runs and
12-file conflict rounds (ADR 0042)."

**Rollback.** The register's caps and `mode` are data; C72 and C73 can be warnings by one line; the tree lookup can be
switched off in the plan job.

## Costs (agent-days, estimates)

| WP | Days |
|---|---|
| WP-1 register, rule, hook | 0.5 |
| WP-2 consolidate vm-smoke and vm-win (counted in ADR 0040 WP-1) | (1) |
| WP-3 C72 and C73 | 1.5 |
| WP-4 tree lookup in the plan job | 1.5 |
| WP-5 split `land-1` into capped landings (steward work) | 1 |
| WP-6 measure and tune | 0.5 |
| **Total (new work)** | **5** |

## Consequences

- An agent who needs a harness change waits for, or asks, the lease holder; small harness fixes may take a few hours
  longer to reach a feature branch. Feature branches never carry harness edits that could diverge.
- More, smaller landings: more main runs per day (bounded by the cadence), fewer repeated runs and conflict rounds.
- A release tag can come from a `main` that already holds the release's parts, so the tag run tests what was landed.
- The register is one more file to keep true; C72's report of unclaimed harness-like paths keeps it from going stale.

## Alternatives considered

- **CODEOWNERS as the registry.** With one human owner it names no agent or branch, and Gitea's code-owner review is not a
  gate here. Rejected as the registry; it keeps its role for paths.
- **Extend `known-copies.toml`.** Copies on `main` and changes on branches are different questions with different
  checks; one file would mix two schemas. Rejected; the two registers link each other.
- **Trunk-based development: every agent lands straight on `main` behind flags.** It would remove batches, but every push
  to `main` starts CI (rule 50) and a red `main` blocks everyone; the release flow (ER-1) needs a steward. Rejected for now.
- **Squash each branch to one commit at landing.** It removes the rewording and shrinks history, but it breaks ER-4's
  changelogs, bisecting and the `Copies-*` trailers of rule 55. Rejected.
- **A merge queue (bors-style) on Gitea.** Gitea has no built-in merge queue (not re-checked today); the cadence plus the
  tree lookup gives most of the effect without new infrastructure. Kept as a later option.

## Decisions for the user

- **D1:** the caps: 60 commits, 200 files, 72 hours (recommended), or tighter (30, 100, 48 h) at the cost of more main runs?
- **D2:** the landing cadence: at most every 8 hours (recommended), or once a day?
- **D3:** CI skipped for a tree already green within 24 hours (recommended), or always run (simpler, costs a run per copy)?
- **D4:** `conformance` as an additive harness (feature branches may add checks; recommended), or a strict home like the
  others?
