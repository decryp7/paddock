# 42. One owner per shared harness, and smaller merge batches: a home branch per harness, size and age caps, CI once per tree

- Status: Accepted (2026-11-03), by the user, who approved this redesign on 2026-11-03 (ER-21 redesigns, item 6)
  and decided its open points the same day (Decisions of the user), after the independent review approved it (round 1
  and a delta re-review, 2026-11-03).
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
    `steward/land-1` (Spotter 1.0) **222 commits, 524 files**, oldest commit 2026-10-28, not landed on 11-03 (new RustSec
    advisories, log 498).

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
lands = "48h"                     # the home lands at least this often while ahead, outside release windows (below)
note = "apps/*/vm-smoke.json: an app's own block may change on the app's branch (declared exception below)"
exceptions = ["apps/*/vm-smoke.json"]
```

Seeded with the harnesses measured above: `vm-smoke` (home), the CI kit (`.gitea/actions/**`, `.gitea/workflows/ci.yml`,
`nightly.yml`, `app-release.yml`, `scripts/affected.mjs`; home), `paddock-dev` (`tools/paddock-dev/**`, `dev`; home),
`conformance` (`conformance/src/**`; a strict **home** until checks are auto-discovered, then **additive**: a feature
branch may add a new check file, while edits to the runner and existing checks go through the home. Additive mode needs
the runner to discover `conformance/src/checks/C*.mjs` by itself, so adding a check never edits `checks/index.mjs`; that
change is WP-3's first step, decision D4), `paddock-e2e` (`ui/projects/paddock-e2e/**`; home) and
`paddock-testkit` (`crates/paddock-testkit/**`; home). Product shared code (`crates/`, `ui/projects/paddock-ui`) is not a
harness: it changes with its adopting apps in one change (rules 39, 42).

**Release windows (ADR 0022 G.7).** A home whose paths are in `crates/` or `ui/` (`paddock-testkit`, `paddock-e2e`) or
on a release path (the CI kit's `app-release.yml` and shared actions) lands **only outside a release window**, whatever
its `lands` age; the register marks such a harness `release_path = true`, and `harness --stale` reports it as "held by
the <app> release window" instead of overdue. The other homes (`vm-smoke`, the parts of `paddock-dev` off the release
path) keep the 48-hour age.

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
C73 checks the caps over the landing range and the trailer. It runs **at landing**: in the steward's landing step
(`paddock-conformance batch --range origin/main..<landing head>`) and in the coordinator's dispatch of the landing's CI
(ADR 0038's dispatch guard); a feature branch's own pushes are not landings and are not checked against the caps.

### 4. No landing copies: messages right at the source

- A `commit-msg` hook in `lefthook.yml` runs `paddock-conformance commit-lint --file {1}` (the same code as CI's `commits`
  job), so an over-long subject or a `wip` type is refused when it is written, not at landing.
- The steward lands the reviewed head itself (fast-forward, or a merge commit), never a reworded copy. A history that
  still needs rewording is reworded **on its branch before review**, and reviewed as that.

### 5. CI once per tree

**Scope: branch runs only.** Reuse applies only to runs started by a push to a branch, or by a `workflow_dispatch`
without inputs on a branch. It **never** applies to a tag push, any `*-release.yml` run, a scheduled or nightly run, a run
on `main`, or a dispatch with inputs (gates on, bundles, a chosen app): those always run in full, because a release is
built and proven from its own tag (ER-15, rule 9, ADR 0039) and the nightly is the full suite (rule 50 (e)).

For a branch run, the plan job (`affected.mjs --ci`, later `./dev plan`) looks up the head's **tree hash** among successful
branch runs of the same workflow in the last 24 hours. If one ran on an identical tree (the tree includes `.gitea/`, so the workflows are identical
too), the run marks every gate job "tree already green in run <id>" with the link, skips them, and still runs the
`commits` job (messages differ). The 24-hour bound exists because some gates read the world (new RustSec advisories failed
`land-1` on an unchanged tree on 11-03). This removes the second run of a landing copy or a reworded history; ADR 0038's
local stamp is the same idea before the push, and the two share **one key** (rule 55: one implementation per
mechanism): G26's input hash, computed by one function in the generic tools (`devkit`), over the tree hash plus the tool
versions file. The plan job and the local stamp both call it; C58 then treats any second hash function as a copy.

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
3. **WP-3:** the conformance runner discovers its checks (no `index.mjs` edit per check; D4), then C72 and C73 with
   fixtures and tests; C72 wired into the `commits` job, C73 into the steward's landing step and the dispatch guard.
4. **WP-4:** the plan job's tree lookup, with a test against a fake Gitea API.
5. **WP-5:** split `land-1` into landings under the caps (shared parts first), as the first use.
6. **WP-6:** after two weeks, measure main runs per day, CI runs per landed commit and conflict rounds, record them
   (rule 48), and tune the caps.

The proposed **rule 61**: "One owner per shared harness, and small batches. A shared harness (`docs/harnesses.toml`)
changes only on its home branch, by the agent holding its lease; other branches merge the home. A landing holds at most
the register's caps (60 commits, 200 files, 72 hours) unless waived with `Batch-size-waived:`; a release line lands its
green parts untagged and its shared parts first; open lines merge `main` daily; the same tree runs a branch CI
run once (never a tag, release, nightly or `main` run). Checked by C72
and C73. *Why:* three copies of vm-smoke blocked a release batch, and 121- to 222-commit batches took repeated CI runs and
12-file conflict rounds (ADR 0042)."

**Rollback.** The register's caps and `mode` are data; C72 and C73 can be warnings by one line; the tree lookup can be
switched off in the plan job.

## Costs (agent-days, estimates)

| WP | Days |
|---|---|
| WP-1 register, rule, hook | 0.5 |
| WP-2 consolidate vm-smoke and vm-win (counted in ADR 0040 WP-1) | (1) |
| WP-3 check auto-discovery, C72 and C73 | 1.75 |
| WP-4 tree lookup in the plan job, on G26's shared input hash | 1.5 |
| WP-5 split `land-1` into capped landings (steward work) | 1 |
| WP-6 measure and tune | 0.5 |
| **Total (new work)** | **5.25** |

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

## Decisions of the user (2026-11-03)

The user accepted the reviewer's recommendations on 2026-11-03. The delta re-review approved the ADR with them.
- **D1:** the caps are **60 commits, 200 files and 72 hours** (§3).
- **D2:** the steward lands **at most every 8 hours** (§6).
- **D3:** CI is **skipped for a tree already green within 24 hours**, for branch runs only, with the exclusions of §5
  (never tags, `*-release.yml`, nightly, `main`, or dispatches with inputs).
- **D4:** `conformance` becomes an **additive** harness only once checks are auto-discovered (no `index.mjs` edit to add
  one); until then it is a strict home like the others.

## Amendment (2026-11-08, proposed): the `vm-smoke` home built now, a shared probe kit, preconditions first, one `--only`, a cleanup registry and a harness lint

- Status: **Proposed** (2026-11-08), ER-21 / rule 40. The user decided on 2026-11-08 to do all redesigns now; the
  coordinator log lists "one VM harness + probe helper" among the foundation redesigns (log 1361). This amendment does not
  reopen §§1-6: it builds §1 and §2 for the `vm-smoke` harness first, adds a second pattern (probes that cannot see what
  they judge) with its own mechanism, and re-plans the work packages. Draft on branch `docs/adr-vm-harness`, not yet
  reviewed. It also amends ADR
  [0040](0040-windows-gui-test-session.md) §1 and §3 (see that ADR's amendment of the same date) and uses ADR
  [0041](0041-deterministic-timing-in-tests.md) §1 (b) and §4 (`observe(window, reason)`) as written.
- Related: ER-11, ER-12 / rule 5, ER-27 / rule 56, ER-33 / rule 71, rule 53 and ADR 0033 (processes by identity), rule 55
  and ADR 0034 (copies), ADR 0049 (which lowered §3's caps to 25 commits and 48 hours; unchanged here).
- Evidence (read 2026-11-08 at `origin/main` `9e18230c5`, on the named branches, in the coordinator log
  `/Volumes/d7sst7/dev-targets/handoff/2026-10-29-overnight.md`, cited "log" with its line, and in the VM run reports):
  - **Nothing of §1-§2 was built.** `docs/harnesses.toml` exists on no branch; C72 and C73 exist on no branch; the home
    branch `tools/vm-smoke` was never made. ADR 0040 WP-1 (merge `vm-win`) was not done either (below).
  - **Seven lines of one harness**, all ahead of `main` (`git rev-list --count origin/main..<b>`):

    | Line | Ahead | Holds | Relation |
    |---|---|---|---|
    | `main` | 0 | 8 files, about 1,250 lines | the only landed copy |
    | `tools/vm-smoke-unified` | 42 | Spotter update rows (`update*.mjs`), macOS VNC `--gui`, `steplog.mjs`, `win/uia.ps1` (24 files not on any later line) | contains `feat/vm-smoke-gui`; last commit 11-03; based 820 commits behind `main` |
    | `tools/win-update-test` | 7 | `scripts/vm-win.sh`, `scripts/vm-win/` | contains `tools/win-test-vm` |
    | `test/dyno-vm-harness-1.1-r2` | 156 | 97 files under `scripts/vm-smoke` (+9,384 lines against its base `838564a5e`) | contains `test/dyno-vm-harness-1.1` and `test/dyno-vm-reset-and-seed`; re-implements unified's launchd fixes (`c930b902d` against `781c2e054`) instead of merging them |
    | `test/dyno-vm-harness-1.1-mac` (`a22492960`) | r2 + 17 | 13 files, adds `only.mjs` | forked from r2's tip `2dcafda62` |
    | `test/dyno-vm-harness-1.1-linux` (`6aca680e2`) | r2 + 8 | 6 files, adds `only-rows.mjs` | forked from r2's tip |
    | `test/dyno-vm-harness-1.1-win` (`330b7e356`) | r2 + 15 | 21 files, adds `win-rows.mjs` | forked from r2's tip |

    The split into three per-OS lines was a decision for speed: "Splitting into 3 parallel per-OS VM agents" (log 1303).
    The three overlap in only two files (`login-jobs-drivers.mjs`: mac and linux; `manifest.mjs`: mac and win) and
    `git merge-tree` merges all three onto r2 **with no conflict**; merging the result onto `main` conflicts only in
    `docs/lessons.md`. Merging `tools/vm-smoke-unified` onto that result conflicts in 7 harness files.
  - **The Windows runner's VM driver is not in git.** Six modules default to
    `PADDOCK_VM_WIN ?? '/Volumes/d7sst7/dev-targets/scratch/dyno-vm-t/vm-win.sh'` (`run-win.mjs:27`, `win-cdp.mjs`,
    `win-act.mjs`, `win-ui.mjs`, `win-helper.mjs`, `win-tray.mjs`); that file does not exist. Two untracked copies exist
    (`scratch/wake-vm/vm-win.sh`, `scratch/dyno-small-vm/scripts/vm-win.sh`, 181 lines, identical), and both differ from
    `tools/win-update-test`'s 238-line `scripts/vm-win.sh` in 59 lines.
  - **Three `--only` schemes**, one per OS: macOS takes manifest ids from a fixed list of 10 (`only.mjs`); Linux takes the
    row numbers of one report table, 1 to 26 (`only-rows.mjs`); Windows takes names, aliases and numbers of "the 1.1.0
    table", or else passes the token to `win-suite.ps1 -Only` (`win-rows.mjs`). Only Linux checks preconditions first, as a
    step of the run (`firstBrokenPrecondition`); on macOS and Windows some rows check their own preconditions inside
    their bodies, each in its own way.
  - **Probe mechanics copied per module** (union of the four Dyno lines, 56 non-test modules): 16 private `sleep`
    definitions; at least 9 polling helpers (`until` in `login-jobs-row.mjs` and `power-checks.mjs`, `pollText` in
    `helper-readings.mjs`, `poll` in `login-start-rows.mjs` and `hung-page.mjs`, `gui.waitFor`, inline `waitReady`,
    `waitConsole`, `waitCountdown`) plus
    `Wait-Until` twice in PowerShell (`win-suite.ps1`, `win-gui-probe.ps1`) and once more in
    `crates/paddock-setup/nsis/test/run-harness.ps1`; 322 sleep-like call sites (fixed waits and poll intervals together);
    25 `pkill`/`killall` and 25 `Stop-Process` calls, nearly all by process name (`Get-Process dyno-app,dyno-agent |
    Stop-Process -Force` six times); three ways to compare a start time (`checks.mjs`
    trims trailing blanks, `run.mjs:463` squeezes runs of blanks since `a22492960`, `discovery-row.mjs` turns blanks into
    `_`); log reads by `tail -n <N>`, `log show --last 10m` or `--last 30m`, and host time; 17 `schtasks /create` against
    7 `schtasks /delete`; 16 `finally` blocks over 56 modules.
  - **The cost, measured.** 111 VM run reports written 2026-11-05 to 2026-11-08 (`*.json` under `local-*` run folders in
    the agents' scratch folders) hold 2,140 steps: 187 `check-broken` and 112 `not-measured` (14 % of all steps), against
    1,456 `pass` and 266 `fail`. By OS: macOS 51 check-broken of 867 steps, Linux 21 of 440, **Windows 115 of 833**. The
    reasons of the 183 required steps that ended check-broken, sorted by keywords (a rough sort, first match wins, not a
    classification): 37 a precondition or state from an earlier row or the snapshot, 21 a read too early or with no bound,
    18 a read or parse that failed, 14 a log line missing, 5 a process race or identity, 88 other (mostly "could not be
    triggered": the probe found no menu, window or DevTools page to act on). 195 distinct harness commit subjects on the
    four Dyno lines, by keyword (overlapping): 44 about logs, 24 about waits or retries, 14 about pids, kills or identity,
    12 about cleanup or restoring, 10 about `--only`, 6 about parsing.

### A second pattern: probes that cannot see what they judge

| Kind | Instance (Dyno 1.1.0 VM rounds, 2026-11-05 to 11-08) | Fix on the line |
|---|---|---|
| Read too early | row 5a pressed the Jobs list's Cancel before the job was cancellable | `cac72ffc6` (mac): "waits (bounded) for the Cancel button" |
| One look after a fixed sleep | second launch: `Start-Process ...; Start-Sleep -Seconds 8`, then one look; under Defender the forwarded duplicate (pid 8264) was still listed 9 s later | `221987501` (win): condition wait up to 40 s |
| Missed log lines | hung-page polls missed the app's lines although the app logged correctly (row 42 recorded NOT MEASURED in the release record, log 1354) | `d9eb74320`, `acf194431`, `4e741176b` (win): rescue from the log, array pattern, lines dated by a guest mark |
| A log held open | `setup.log` read while the installer held it | `win-judges.mjs:279`: read through a shared-read stream |
| Raced the app | MCP killed the agent while the app respawned it | `8755a033b`, `4e741176b` (win): "MCP ends the app before killing the agent", "holds the agent down" |
| Parse | the SIGCONT identity check compared `ps -o lstart=` text and failed on "Nov  8" (two blanks) | `a22492960` (mac) |
| State left behind | a benchmark left running blocked the next row | `0811625cc` (mac): "5a ends its benchmark" |
| The harness's own trace | "Background Items Added" was the harness's own log grep (log 1313); the reopen probe matched the log tool's own command line | `05b2b9c46` (mac) |
| Fixed sleep in a driver | Linux Processes row: "the Linux driver ignores minWidth and uses a fixed sleep" (log 1278) | linux line |

**Pattern.** Each probe owns its own waiting, log reading, process handling and cleanup, so each makes the same mistakes
on its own schedule, and the fix lands in one module on one line. **Invariant.** (d) A probe in `vm-smoke` observes
changing state only through one kit that bounds every wait, reads every log from a guest-clock mark without a line limit,
acts on a process only after re-reading its identity, and undoes what its row changed; a row whose precondition does not
hold never runs its body. (e) The harness has one line: §1-§2, built now.

### 7. The home now: one line, one owner (builds §1 and §2 for `vm-smoke`; replaces WP-2)

1. **Freeze.** The four Dyno lines, `tools/vm-smoke-unified` and `tools/win-update-test` take no more commits from the
   moment the merge starts; the coordinator says so in the VM agents' briefs.
2. **Merge, in this order, on a new `tools/vm-smoke` from `test/dyno-vm-harness-1.1-r2`:** the `-mac`, `-linux` and `-win`
   lines (merge commits, no rebase: the reviewed commits keep their ids); then `origin/main`, resolving `docs/lessons.md`
   by keeping both sides (the union ADR 0049 asks for; `merge=union` is not active on `main` yet, since `merge-tree`
   reports the conflict); then `tools/win-update-test`'s `scripts/vm-win.sh` and `scripts/vm-win/`, with the
   59-line difference of the scratch copy reconciled line by line and recorded in the merge message. The six
   `PADDOCK_VM_WIN` defaults become the repo path; the variable stays as an override and the runner logs which driver it
   used and its hash (ER-27). The merge then runs the harness's own tests (283 cases in 39 files on the union) and a
   no-VM dry run of each runner's argument parsing.
3. **Not merged by text: `tools/vm-smoke-unified`.** Its base is 820 commits behind `main` and 7 files conflict; r2 already
   re-implemented its Dyno fixes. Its 24 unique files (Spotter update rows, macOS VNC `--gui`, `steplog.mjs`, `win/uia.ps1`,
   the download fallback test) are ported onto the kit in WP-V8; until then the branch stays frozen as their source.
4. **Land** the home on `main` as its own batch (harness files only, so the affected plan runs the `tools` job;
   rule 50). Under §3 and ADR 0049's caps it is one waived batch (`Batch-size-waived: vm-smoke consolidation, ADR 0042
   amendment 2026-11-08`), because it gathers work that should have landed in 48-hour steps.
5. **The register entry and the owner.** `docs/harnesses.toml` is seeded with the `vm-smoke` entry of §1 (the rest of
   WP-1 stays as planned), plus a `users` list naming `crates/paddock-setup/nsis/test/run-harness.ps1`: it runs through
   the harness's Windows transport but stays owned by its crate, outside `paths`. `CODEOWNERS` gets one line after `scripts/.*`:
   `(scripts/vm-smoke/.*|scripts/vm-win\.sh|scripts/vm-win/.*|apps/[^/]+/vm-smoke\.json) @decryp7`, with a comment that
   names the home `tools/vm-smoke` and points at the register for the lease holder. CODEOWNERS can name only Gitea users,
   and the one user is the human owner, so the **agent** owner stays the lease in the register and the brief (§1);
   whether Gitea applies the last or every matching line is to be read in its docs in WP-V1 (not checked today); with one
   owner the result is the same.
6. **Per-OS work in parallel, without forks.** VM agents may work in parallel per OS, on topic branches made from the
   home (`tools/vm-smoke-<os>-<topic>`), each merged into the home by the lease holder within 24 hours or dropped. The
   kit, the row registry, `manifest.mjs`, `verdict.mjs`, `report.mjs` and the `--only` parser change only on the home
   itself. C72 (§2) then holds as written: every harness commit that reaches `main` is reachable from the home. Today's
   evidence says this is cheap: the three per-OS lines overlapped in two files and merged without a conflict; what went
   wrong was that they were never merged and each grew its own `--only`.

### 8. One probe kit: `scripts/vm-smoke/kit/`

Plain ES modules (the user decided on 2026-11-08 that the VM harness is not ported to Rust now: "fix its design instead",
log 1302), pure where possible, with a fake guest for tests. A row gets a `ctx` that exposes the kit, not a raw shell.

- **Guest port.** `exec(script, { timeoutMs })` returns `{ code, stdout, stderr, guestMs }`; `clock()` reads the guest's
  epoch milliseconds. Adapters: Tart (`tart exec`) for macOS and Linux, `vm-win.sh ssh` for Windows. Every call is logged
  with its exit code and elapsed guest time.
- **`until(probe, { max, every, what })`.** Polls `probe()` until it returns a value, at most `max` (required; taken from a
  named budget in the app's `vm-smoke.json`, each with a reason, as `settleSec`/`settleWhy` already are), every `every`
  (default 500 ms, growing to 4 s). The result carries `ok`, the value, the number of polls, the elapsed guest time and
  **every distinct observation** in order, so a timed-out row shows what it saw (ER-27). It is the only wait in rows.
  `observe(window, reason, probe)` (ADR 0041 §1 (b)) watches for something that must **not** happen; `stable(probe,
  { for, max })` asks that a value hold for a span (the "agent is steady" precondition of `5c0851590`).
- **Logs from a mark.** `mark(source)` records the guest clock and a cursor: a byte offset for a file (read on Windows
  through a stream opened with `FileShare.ReadWrite`, so a file held open by its writer, such as `setup.log`, reads),
  `journalctl --cursor` on Linux, `log show --start <guest time>` on macOS. `linesSince(mark)` returns every line after it,
  never a `tail -n` or `--last`, and drops lines whose process is the kit's own reader (the `05b2b9c46` and log 1313
  class). `untilLine(mark, pattern, { max })` is `until` over it, so no poll can miss a line between two reads.
- **Processes by identity (rule 53).** `procs(filter)` returns `{ pid, startMs, command }`, the start as a number read on
  the guest: `/proc/<pid>/stat` field 22 plus `btime` on Linux, `StartTime.ToFileTimeUtc()` on Windows, and on macOS
  `LC_ALL=C ps -o lstart=` parsed by one tolerant parser (any run of blanks, padded days, trailing blanks) with fixtures
  for "Nov  8", "Nov 18" and a padded line. `signal(id, sig)` and `end(id)` send one guest script that re-reads the start
  time and acts only if it matches, and returns the state before and after as evidence. Nothing in a row ends a process by
  name; leftovers of the app under test are found by `procs` and ended by identity. This is the JS form of
  `paddock_proc::ProcessId`; its tests mirror that crate's identity cases.
- **The cleanup registry.** `ctx.defer(label, undo, verify)` is called at the moment a row creates state (a scheduled
  task, a started benchmark or job, a stopped agent, a changed setting, a planted file). At the row's end, pass, fail or
  throw, the undos run last-in first-out, each followed by its `verify` through `until`. A failed undo is recorded on the
  row, and every later row that needs that state (by precondition, below) is `check-broken: <undo label> was not undone`.
  Each run ends with a sweep that compares the guest's processes, scheduled tasks and login items with the snapshot taken
  at the run's start and lists what is left.
- **Evidence.** Every kit call appends to the row's evidence; the evidence folder of `evidence.mjs` stays.
- **The guest half on Windows.** `kit/guest.psm1` holds the same verbs for in-guest PowerShell (`Wait-Until`,
  `Get-ProcIdentity`, `Stop-ByIdentity`, `Read-LinesSince`, `Register-Undo`) and absorbs `win-suite.ps1`'s and
  `win-gui-probe.ps1`'s `Wait-Until`, `win-act.ps1`'s verbs and unified's `uia.ps1` (ADR 0040 §3 amended).
  `run-harness.ps1` keeps its own `WaitUntil` until it imports the module (it must still run alone from the crate); the copy
  is registered in `docs/known-copies.toml` with `delete_by` the first Pitwall or Dyno tag after WP-V5.

### 9. Rows as data, preconditions first

A row is `defineRow({ id, os, title, needs, after, budget, run(ctx) })` in one registry per app. `id` is the manifest id
that exists today (`login-jobs`, `hung-page`, `upgrade-swap`, `tray-menu`, ...), and the report prints it on every row.
`needs` names preconditions from one module (`kit/preconditions.mjs`: installed, build-is-commit, helper-active,
login-item-on, window-readable, agent-steady, desktop-session, old-installer-given, ...), each a probe with a positive
control (rule 71). The runner checks a row's `needs` **before** its body, through `until` with the precondition's budget;
the first that does not hold ends the row as `check-broken: <precondition>: <what was seen>` and the body never runs.
`after` names the setup rows a row stands on (install, build binding, cold start) and the closing rows (Quit), which the
runner adds. The manifest's title regexes stay only for reading older reports.

### 10. One `--only` for every OS

`vm-smoke.sh --app <app> --os <macos|linux|windows> --only <id>[,<id>...]`. The ids are the registry's; an unknown id
fails at once and lists the ids for that OS; the runner adds each row's `after` rows and checks its `needs` first; a
partial run is never a release verdict (the manifest's `partial`, as today). Row numbers and aliases go: they named rows
of particular report tables, which changed between rounds. `win-suite.ps1 -Only` parts become rows of the registry.

### 11. The harness lint: C101 (C100 is ADR 0060's "every check runs somewhere")

`paddock-conformance` check over `scripts/vm-smoke/**`, `scripts/vm-win*`, guest scripts (`.ps1`, `.sh`, `.py`, `.swift`)
and the shell text inside JS template strings:

| Flagged | Allowed when |
|---|---|
| A fixed wait: a `setTimeout` promise, `sleep <n>` or `Start-Sleep` in a guest script, `time.sleep`, `usleep`/`Thread.sleep` | inside `kit/`, or an `observe(window, reason)` call (ADR 0041 §1 (b)) |
| A one-shot read of changing state: a row calling `ctx.guest.exec` (the raw port) instead of a kit probe | the line carries `// vm-smoke: one-shot — <why the value cannot change>` (a file hash, an installed version) |
| A kill by name: `pkill`, `killall`, `Stop-Process -Name` or by pipeline without an identity check, `taskkill /IM` | inside `kit/` (C53's harness half) |
| A truncated log read: `tail -n`, `log show --last`, `Get-Content -Tail` in a row | inside `kit/` |
| The host clock judging guest events: `Date.now()` in a row | never (use `ctx.clock()`) |
| A row that creates a scheduled task, login item, benchmark or job with no `ctx.defer` in the same row | never |

It ratchets like C71: today's sites are recorded as hashes in `conformance/baselines/C101.json`; new sites fail; WP-V6 brings
the baseline to zero and then the baseline file is deleted. Its fixtures prove each line can fire (a flagged and an allowed
sample per row). It runs in CI's conformance job (the integrator adds it to `ci.yml`'s `--only` list).

### 12. The kit is tested against the faults it exists for

`kit/*.test.mjs` run a fake guest that seeds each instance kind above, and each test is written failing first (ER-11): a
Cancel button that becomes enabled late; log lines that arrive between two polls; a log file held open by its writer; a
pid reused with a new start time; "Nov  8" and a padded `lstart`; an app that respawns the agent while the row ends it; an
undo that throws; a row whose precondition is false. A mutation check of the kit (rule of the 2026-11-05 redesigns) runs
before WP-V2 lands.

### Work packages after this amendment

WP-V1 comes first so that Pitwall 1.1's half-day Windows check (log 1358: `run-harness.ps1` cases P, Q, R, M1-M5 plus three
Pitwall rows) runs on the merged harness. If the check comes before WP-V4, it runs on WP-V1's home with the three rows
driven as its brief says, and WP-V4 then turns them into registry rows.

| WP | What | Days |
|---|---|---|
| **V1** (replaces WP-2; is ADR 0040 WP-1) | §7: freeze, merge the four Dyno lines and `vm-win` onto `tools/vm-smoke`, driver path into the repo, tests and dry runs, land; the `vm-smoke` register entry and the CODEOWNERS line | 1.5 |
| V2 | §8 kit core: guest port and adapters, `until`/`observe`/`stable`, logs from a mark, identity and the `lstart` parser, cleanup registry, evidence; §12 fake-guest tests | 2.5 |
| V3 | §9 and §10: row registry, preconditions module, preconditions-first runner, one `--only` across the three runners | 2 |
| V4 | Pitwall's Windows rows (install with the service on, upgrade over a running service, uninstall with the `sc.exe` fallback) and the `run-harness.ps1` cases as registry rows on the kit | 1 |
| V5 (reworks ADR 0040 WP-3) | `kit/guest.psm1`; `win-suite.ps1`, `win-act.ps1`, `win-gui-probe.ps1` and `uia.ps1` onto it; known-copies entry for `run-harness.ps1` | 1.5 |
| V6 | move the rows onto the kit, per OS on topic branches (§7.6): macOS 2, Linux 1.5, Windows 2.5; then one full VM round per OS on the latest Dyno build, 0.5 each | 7.5 |
| V7 | §11 C101 with fixtures and the ratchet baseline | 1 |
| V8 | port `tools/vm-smoke-unified`'s 24 unique files onto the kit (Spotter update rows, macOS VNC `--gui`, `steplog`, download fallback); then the coordinator asks the user before deleting the frozen branches | 2 |
| V9 | measure: check-broken and not-measured steps per VM round before (this amendment's 14 % baseline, Windows 115 of 833) and after V6; record in `docs/benchmarks.md` (rule 48) | 0.5 |
| **New work, total** | | **19.5** |

The other packages keep their scope; re-estimated where this amendment changes them:

| WP | Status on 2026-11-08 | Days |
|---|---|---|
| 0042 WP-1 register, rule 61, `commit-msg` hook | not built; `vm-smoke` entry moves into V1 | 0.5 |
| 0042 WP-2 consolidate vm-smoke and vm-win | replaced by V1 | (in V1) |
| 0042 WP-3 check auto-discovery, C72, C73 | not built | 1.75 |
| 0042 WP-4 tree lookup in the plan job | not checked by this amendment | 1.5 |
| 0042 WP-5 split `land-1` | the Spotter line's landing is the coordinator's to confirm; not re-estimated | (1) |
| 0042 WP-6 measure and tune | unchanged | 0.5 |
| 0040 WP-2, WP-4..WP-7 | left after V1 and V5; see ADR 0040's amendment | 5.75 |

**Proposed addition to rule 61** (its text is not in `docs/rules.md` yet): "A probe in a VM harness waits, reads logs, acts
on processes and cleans up only through the harness's kit; a row checks its preconditions first. Checked by C101. *Why:*
187 of 2,140 VM steps in four days ended check-broken, from probes that read too early, missed log lines, raced the app,
misparsed a date or left state behind (ADR 0042, amendment 2026-11-08)."

**Rollback.** The kit is additive: a row not yet moved keeps working on the home until V6. C101 can be a warning by one
line. The home can be split again only by a new decision; nothing in the kit depends on there being one line.

**Consequences.**
- A harness fix reaches every OS on the next merge of the home; VM agents wait at most a day for another OS's fix.
- Rows lose their private shortcuts: a row that needs a new kind of wait or read adds it to the kit, on the home, with a
  fake-guest test.
- The Windows runner can be run from any worktree of the home without a driver copied into scratch.
- Old reports keep their row numbers; new reports carry ids only, so a report from before V3 is read by its titles.

**Alternatives considered.**
- **Keep the per-OS lines and merge them at the end of each release round.** That is what happened (lines lived from 11-07
  to 11-08 and each added its own `--only`); a merge at the end does not stop three copies of the same mechanism.
  Rejected.
- **A lint alone, no kit.** It would flag sleeps but leave each module to write its own correct wait, log cursor and
  identity check: the seven copies above show how that goes. Rejected.
- **Port the harness to Rust (`./dev vm`) now.** The user decided on 2026-11-08 not to port it now (log 1302). Kept for later;
  the kit's ports make a later port mechanical.
- **Merge `tools/vm-smoke-unified` by text in V1.** Seven conflicting files on a base 820 commits old would make V1 the
  long pole before Pitwall's check; porting its unique files onto the kit costs about the same and lands the result on the
  kit. Rejected for V1.
