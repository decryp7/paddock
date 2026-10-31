# 34. One implementation per mechanism: copies are registered, fixed together and expire

- Status: Proposed (2026-10-31). The direction was accepted by the user on 2026-10-31 (family survey
  `family-redesign-and-sharing-2026-10-31`); this text waits for an independent review. The binding rule is rule 55.
- Date: 2026-10-31
- Related: rule 40 / ER-21 (recurring bugs mean a missing design rule), rules 39, 42 and 43 / ER-22 (share first, shared
  behaviour tested once), rule 1 (one release at a time, the shared-code freeze), rule 53 / ADR 0033; ADR 0001 (plugin
  platform: "Dyno 1.0 ships on its own copy"), ADR 0030 §5 ("one driver"), ADR 0031 (C51); checks C13 and C23 (the two
  kinds of copy that are already checked).
- Evidence (all read in git at `origin/main` `c3404cdc7` and the app release branches, 2026-10-31):
  - **launchd job report, four implementations.** `crates/paddock-service/src/parse.rs:25-54` skips launchd's nested
    blocks; the copies in `apps/{dyno,pitwall,pitcrew}/app/src-tauri/src/service_repair.rs:102-127` do not. Dyno's copy
    read every healthy agent as "not running" on macOS 26 and replaced it four times at each login (Dyno 1.0 blocker B4,
    root-caused 2026-10-31). The shared `stale()` (`parse.rs:62-67`) still carries the copies' second defect.
  - **Browser driver, three implementations.** The Windows network-sandbox switch (ADR 0030) is in
    `crates/paddock-chromium` only, which no app depends on; `spotter-render` and Debrief's `chromium-driver` lack it.
    EINTR, wall-clock bounds and the launch gate had to be fixed per copy (merge-steward 2026-10-30 12:20, 17:05).
  - **IPC endpoint, two implementations.** The macOS `SS_DRAINING` EBADF fix (poll-based waits) is needed in
    `crates/paddock-ipc/src/endpoint/unix.rs` and in `apps/dyno/crates/dyno-ipc/src/endpoint/unix.rs`; the Windows
    endpoints differ in 22 of 245 lines.
  - **Notification channels, three implementations**: `crates/paddock-channels` (built from the other two, adopted by no
    app), `apps/pitcrew/crates/notify-channels`, `apps/spotter/crates/spotter-engine/src/mail.rs`.
  - **Adoption.** About 15 shared crates have no direct dependency from any app, even on the release branches
    (`paddock-chromium`, `paddock-ipc`, `paddock-channels`, the `paddock-plugin-*` family, ...). A fix made in the shared
    crate therefore reaches no user; a fix made in an app reaches one.
  - The issues research of 2026-10-30 (pattern D, 9 catalogue rows) found the same: "a fix in one copy does not raise
    the others", and proposed this register.

## Context

Rules 42 and 43 say what to do when code is shared: build it once in Paddock and delete the app copy. They do not say
what happens in between. In practice three things keep copies alive and make them drift:

1. **The freeze.** During a release (rule 1) shared code is frozen, so the quick fix goes into the app copy only. ADR 0031
   even records a "rule 42 exception" for Debrief.
2. **No list.** Nobody can see that `service_repair.rs` exists four times; a reviewer of a Dyno fix has no reason to
   open Pitwall's copy.
3. **"Later" has no date.** ADR 0001 says Dyno's IPC copy "is deleted after Dyno moves", without a tag. Adoption slips
   behind every release.

The result is the class this ADR names: **a mechanism with several implementations, fixed in one.** It caused the worst
bug of the Dyno 1.0 release (B4) although the shared crate had the fix.

## Decision

### 1. The copies register: `docs/known-copies.toml`

One TOML file lists every mechanism that exists more than once. It is the only place that says a copy exists; anything
not in it is either the one implementation or a violation of rule 42.

```toml
version = 1

[[mechanism]]
id = "launchd-job-report"                       # kebab-case, unique, stable (waivers name it)
title = "Reading launchd's job report and replacing a stale registration (R03)"
canonical = "paddock-service"                   # crate or package that is the one implementation
paths = ["crates/paddock-service/src/parse.rs"] # the canonical files (globs allowed)
symbols = []                                    # optional narrowing, see "How copies are identified"
status = "built"                                # "built" | "planned" (planned: canonical paths may not exist yet)
adr = "0034"                                    # the ADR or plan that decided the canonical home (optional)

  [[mechanism.copy]]
  paths = ["apps/dyno/app/src-tauri/src/service_repair.rs"]
  symbols = ["item:parse_job", "item:JobReport", "item:plan"]
  owner = "dyno"                                # the app (or "paddock") whose lead deletes the copy
  delete_by = "dyno-v1.1.0"                     # the tag that may not be created while the copy exists
  note = "stale() and the parser fixed in this copy on dyno/login-blockers; the shared stale() still needs it"
```

Fields:

- `owner`: an app folder name under `apps/` or `paddock`. It names who does the deletion, not who may edit.
- `delete_by`: an app tag `<app>-vX.Y.Z` (normally the owner's next minor release), or `vX.Y.Z` for the shared packages.
  Moving a date is a reviewed register change with a reason in the commit body, never a silent edit.
- `note`: free text; the reason the copy still exists, and what differs.

Out of scope, because another check already keeps them identical: the `paddock:block` sections of the `AGENTS.md` files
(C23) and the per-app `clippy.toml` presets (C13). Domain code that only looks alike (Dyno's socket lister, Pitcrew's
and Pitwall's own transports, supervisor transitions) is not a copy and is not listed (rule 42: domain logic stays in the
app).

### 2. How copies are identified

A **member** of a mechanism is the canonical entry or one copy. A change **touches** a member when one of these is true:

- a changed file matches one of the member's `paths` and the member has no `symbols`; or
- the member has `symbols`, and a changed line (added or removed, `git diff -U0`) lies inside one of them:
  - `item:<Name>` (Rust or TypeScript): the span of the item named `Name`, from its `fn`/`struct`/`enum`/`trait`/`impl`/
    `mod`/`const`/`static` (Rust) or `function`/`class`/`interface`/`export const`/`export function` (TypeScript) line to
    its closing brace, found with the brace walker over masked source (comments and strings blanked, as
    `conformance/src/checks/C39-freed-port.mjs`'s `maskRust` does). `impl X` and `impl Trait for X` both count for `X`.
    The span is taken in the old file for removed lines and in the new file for added lines.
  - `text:<literal>`: any changed line that contains the literal, for wiring that is not an item (for example
    `text:tauri_plugin_autostart::init`).
- the change deletes a file of the member, or edits the member's own entry in the register.

### 3. The check (C58, follow-up package WP-1)

`paddock-conformance.mjs copies` gets three modes. The implementation is a follow-up package; this is its specification.

**a. Register validity** (runs in every conformance run, `--app paddock`): the file parses; `version = 1`; ids are unique
and kebab-case; every `owner` is `paddock` or a folder under `apps/`; every `delete_by` matches
`^(debrief|dyno|pacenotes|pitcrew|pitwall|sighter|spotter|shell-demo|workbench-demo)-v\d+\.\d+\.\d+$` or `^v\d+\.\d+\.\d+$`,
and an app tag's app equals the `owner`; every path of a copy and of a `built` canonical matches at least one tracked
file (`git ls-files`); every `item:` symbol is found in at least one of its member's files. A copy whose files are all
gone must be removed from the register in the same change. Errors name the entry and say what to fix.

**b. Range check** (`copies --range <base>..HEAD`; CI runs it with `base = $(git merge-base origin/main HEAD)` in the plan
job, next to commit-lint, and the steward runs it before pushing a batch):

1. Compute the touched members of every mechanism (§2).
2. For a mechanism with at least one touched member, every other member must be touched too, or waived (below).
3. Each untouched, unwaived member is an **error**: `C58 launchd-job-report: apps/pitcrew/app/src-tauri/src/service_repair.rs
   was not changed with crates/paddock-service/src/parse.rs. Make the same fix there (rule 55), or waive it with
   "Copies-waived: launchd-job-report apps/pitcrew/app/src-tauri/src/service_repair.rs: <why>".`
4. Whitespace-only and comment-only hunks do not count as touching (masked comparison), so a reformat of one copy does not
   force edits elsewhere.

**c. Tag check** (`copies --tag <tag>`; the app release workflow runs it in its tag-check step, and the steward runs it
before tagging): for the tag's app and version, every copy whose `owner` is that app and whose `delete_by` version is
lower than or equal to the tag's version is an **error** ("dyno-v1.1.0 may not be created: apps/dyno/crates/dyno-ipc is
still a copy of paddock-ipc; delete it or move its date in a reviewed change"). A shared-packages tag `vX.Y.Z` checks the
`paddock` copies the same way.

**Waiver syntax.** A git trailer in any commit of the range, one per member:

```
Copies-waived: <mechanism-id> <member-path-or-*>: <reason, at least 10 characters>
```

`*` waives every other member of that mechanism. Typical reasons: "the copy does not have this code path (Windows-only
branch)", "the copy is deleted in this same batch by <branch>". The check prints every waiver it used, so the reviewer
sees it. Trailers are used rather than a file because a waiver belongs to one change, not to the tree.

**Not detected.** A brand-new copy that nobody registers. That stays a review finding (rule 42) and the survey at each
app move. A report-only similarity scan is a possible later step, not part of this ADR.

### 4. The freeze rule

During a shared-code freeze (rule 1) **a fix to a registered mechanism is not frozen**: it lands in the canonical
implementation and in every copy in the same change, with one failing test per member (rule 11). What stays frozen is the
shared API: a fix may not rename or remove a public item of the canonical crate during the freeze. If the canonical crate
has no user yet, its fix is still made (the next adopter gets it), and its test still runs in the crate's own CI legs.

A new copy during a freeze (to ship an app before the shared adoption is ready, as Dyno 1.0 did with `dyno-ipc`) is
allowed only with its register entry, a `delete_by` tag and the reason, in the same change.

### 5. Seed and dates

`docs/known-copies.toml` is seeded with every copy found by the 2026-10-31 survey (24 mechanisms, 57 copies). The `delete_by` dates
follow the agreed order (Dyno 1.0, Spotter 1.0, Pitwall 1.1, Pitcrew 1.1, Debrief 1.2, Pacenotes, Sighter) and are
proposals for the review and the user to confirm:

| Owner | Deleted by | Copies |
|---|---|---|
| Pitwall | `pitwall-v1.1.0` | `native-feel.ts`, `sparkline.ts` (already gone on `steward/s23-pitwall-1.1`) |
| Spotter | `spotter-v1.0.0` | `native-feel.ts`, `copy-menu.ts` (already gone on `steward/s24c-spotter-1.0`) |
| Pitcrew | `pitcrew-v1.1.0` | `service_repair.rs`, `service_install.rs`, `notify-channels`, its UI copies, login and single-instance wiring |
| Dyno | `dyno-v1.1.0` | `service_repair.rs`, the login and service parts of `login.rs`/`login_item.rs`, `dyno-ipc` endpoint, frames and peer, `sparkline.ts`, wiring |
| Dyno | `dyno-v1.2.0` | `dyno-plugin-api`, `dyno-plugin-host` (Dyno tier 3, ADR 0001) |
| Spotter | `spotter-v1.1.0` | `spotter-render`, `mail.rs` and its notifier, `schedule-editor.ts`, `clipboard.ts`, wiring |
| Pitwall | `pitwall-v1.2.0` | the rest of `service_repair.rs`, `service_install.rs`, wiring |
| Debrief | `debrief-v1.2.0` | `chromium-driver`, `debrief-ipc` parts, the `debrief-plugin-*` platform crates, `debrief-agent` lock, updater, start-up watchdog, `native-feel.ts`, `clipboard.ts`, the dock (shared workbench move), `check-icons.mjs` |
| Pacenotes, Sighter | `pacenotes-v1.0.0`, `sighter-v1.0.0` | `scripts/release/publish.mjs`, `export-diagrams.sh` |

**The Pitwall 1.1 exception.** Pitwall 1.1 already uses the shared `parse_job`, so its copy is only the repair sequence
(`plan`, `run_repair`). The shared `stale()` fix (B4 defect 2) is a separate change before the Pitwall 1.1 tag. Under this
ADR it is one change: `crates/paddock-service/src/parse.rs` and the Dyno and Pitcrew copies together.

## Consequences

- A fix to a listed mechanism costs more at once (every copy, each with a test) and less overall: no second root-cause
  hunt for a bug that was already fixed elsewhere (B4, the browser abort rounds).
- Release tags can be refused for an expired copy. That is intended: it is the only point where "later" becomes a date.
  Moving a date is cheap but visible.
- The register shrinks to empty as the apps adopt the shared crates. An empty register plus C58 still guards new copies
  made during freezes.
- The check needs a git range. It runs in the plan job (which has the history) and in the steward's pre-push step, not in
  the per-file conformance runs.

## Alternatives considered

- **A rule without a check.** Rules 42 and 43 already exist; the copies drifted under them. Rejected.
- **Forbid copies outright, no freeze exception.** Then a release waits for a shared adoption (Dyno 1.0 would have waited
  for `paddock-ipc` and the plugin platform). Rejected: one release at a time (rule 1) needs the escape, but a dated one.
- **Detect copies by similarity (token or AST clones).** It would also flag intended domain look-alikes and miss copies
  that were already rewritten (the four launchd parsers are similar but not identical). Kept as a possible report-only
  helper for the survey, not as the gate.
- **The register in each app's `paddock.ci.json`.** A mechanism spans apps and crates; one family file keeps every member
  of a mechanism in one entry, which is what the check compares.

## Work packages

| WP | What | Size | When |
|---|---|---|---|
| WP-1 | C58 in `conformance/src/checks/C58-known-copies.mjs` plus the `copies` subcommand (modes a-c), good and bad fixtures for each mode (a member touched alone, a waiver, a whitespace-only hunk, an expired copy, a stale entry, an unknown owner), wiring: mode a in the `--app paddock` run, mode b in the plan job and the steward's pre-push, mode c in the app release tag step | 2 d | right after the Dyno tag |
| WP-2 | `fix/service-stale-shared`: the shared `stale()` and `plan()` fix in `paddock-service` with the Dyno and Pitcrew copies, test first with an untrimmed real `launchctl print` | 0.5 d | before the Pitwall 1.1 tag |
| WP-3 | Each owner's deletions by its `delete_by` tag, through the app's normal work packages | per app | per tag |
