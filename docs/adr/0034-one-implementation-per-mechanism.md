# 34. One implementation per mechanism: copies are registered, fixed together and expire

- Status: Accepted (2026-10-31, by the user), after independent review rounds 1-2. The binding rule is rule 55. The
  direction came from the family survey `family-redesign-and-sharing-2026-10-31`; round 1 added the counted-files rule,
  the entry guard, the wiring kind and register fixes; round 2 added the copies found in `ui/`, the narrowed tray
  entries, the approver on date moves and the exact commands.
- Date: 2026-10-31
- Related: rule 40 / ER-21 (recurring bugs mean a missing design rule), rules 39, 42 and 43 / ER-22 (share first, shared
  behaviour tested once), rule 1 (one release at a time, the shared-code freeze), rule 50 (CI runs only what a change
  needs), rule 53 / ADR 0033; ADR 0001 (plugin platform: "Dyno 1.0 ships on its own copy"), ADR 0012 (the app shell, whose
  crates were "taken from" Debrief, Pitwall, Spotter and Dyno), ADR 0030 §5 ("one driver"), ADR 0031 (C51); check C13
  (the per-app clippy presets, a kind of copy that is already checked).
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
  - **Shell crates taken from the apps, originals still there.** The doc comments of `paddock-window`, `paddock-startup`,
    `paddock-commands`, `paddock-dialogs`, `paddock-menu`, `paddock-panels`, `paddock-health`, `paddock-packs` and
    others name the app files they were "taken from" or "ported from"; most of those files still exist in Debrief,
    Pitwall and Spotter.
  - **Adoption.** 37 of the 51 shared crates have no direct dependency from any app at its release head (several reach
    the apps through `paddock-shell`); among them `paddock-chromium`, `paddock-ipc`, `paddock-channels`, `paddock-packs`
    and the `paddock-plugin-*` family. A fix made in such a crate reaches no app's own mechanism; a fix made in an app
    reaches one.
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
id = "launchd-job-report"                       # kebab-case, unique, stable (trailers name it)
title = "Reading launchd's job report and replacing a stale registration (R03)"
canonical = "paddock-service"                   # crate or package that is the one implementation
paths = ["crates/paddock-service/src/parse.rs"] # the canonical files (globs allowed, see §2)
symbols = []                                    # optional narrowing, see §2
status = "built"                                # "built" | "planned" (the canonical files may not exist yet)
adr = "0034"                                    # the ADR or plan that decided the canonical home (optional)

  [[mechanism.copy]]
  paths = ["apps/dyno/app/src-tauri/src/service_repair.rs"]
  symbols = ["item:parse_job", "item:JobReport", "item:plan"]
  kind = "copy"                                 # "copy" (default) | "wiring" (see §3)
  owner = "dyno"                                # the app (or "paddock") whose lead deletes the copy
  delete_by = "dyno-v1.1.0"                     # the tag that may not be released while the copy exists
  note = "what differs, and why the copy still exists"
```

Fields:

- `owner`: a folder name under `apps/`, or `paddock`. It names who does the deletion, not who may edit.
- `delete_by`: an app tag `<owner>-vX.Y.Z` (normally the owner's next minor release), or `vX.Y.Z` for owner `paddock`.
- `kind`: `copy` is a second implementation of the mechanism. `wiring` is a few lines that hook a third-party plugin up
  by hand where the canonical offers the hook (for example `tauri_plugin_single_instance::init(|app, args, _| ...)`
  instead of `paddock_window::tauri::second_instance`). Wiring is covered by the tag check only.
- `note`: free text; what differs and why the copy still exists. Notes state facts, not branch names.

Out of scope: documentation (the `AGENTS.md` sections synced from `docs/blocks/`, ADR copies, READMEs); `templates/**`,
which is the deliberate source of new standalone repositories (its `scripts/export-diagrams.sh` is a template, not a
fifth copy); the per-app `clippy.toml` presets, which C13 governs; and domain code that only looks alike (Dyno's socket
lister, Pitcrew's and Pitwall's transports, supervisor transitions; rule 42 keeps domain logic in the app).

### 2. How copies are identified

A **member** of a mechanism is its canonical entry or one of its copies. A change **touches** a member when a counted
line changes in it:

- **Counted files.** Only non-test source files count. Under a member's `paths`, these never count: test and fixture
  folders (`tests/`, `benches/`, `examples/`, `fuzz/`, `fixtures/`, `testing/`, `e2e/`), test files (`tests.rs`,
  `*_tests.rs`, `*.spec.ts`, `*.test.mjs`, `*.test.ts`), in-file `#[cfg(test)]` modules (blanked the way C51 does),
  manifests and lockfiles (`Cargo.toml`, `Cargo.lock`, `package.json`, `package-lock.json`, `ng-package.json`), docs
  (`*.md`) and generated files (`generated/`). So a dependency bump (Renovate) or a test-only change never triggers the
  check, and bots never need a trailer.
- **Whole-crate members are written as `<crate>/src/**`** (or the source folder of a UI entry point), never as the crate
  root. The validity mode rejects `crates/<name>/**` and `apps/<app>/crates/<name>/**`.
- **Counted lines.** Lines added or removed (`git diff -U0`), ignoring whitespace-only and comment-only changes (compared
  on masked source).
- **Symbols** narrow a member to parts of a file:
  - `item:<Name>` (Rust or TypeScript): the span of the item named `Name`, from its `fn`/`struct`/`enum`/`trait`/`impl`/
    `mod`/`const`/`static` (Rust) or `function`/`class`/`interface`/`export const`/`export function` (TypeScript) line to
    its closing brace, found with the brace walker over masked source (comments and strings blanked, as
    `conformance/src/checks/C39-freed-port.mjs`'s `maskRust` does). `impl X` and `impl Trait for X` both count for `X`.
    The span is taken in the old file for removed lines and in the new file for added lines.
  - `text:<literal>`: any counted line that contains the literal.
- **Not touches.** Deleting a copy's files together with removing its register entry. Editing the register. Both are
  checked by §3a and §3b's entry guard instead.

### 3. The check (C58, follow-up package WP-1)

The subcommand `copies` of `conformance/bin/paddock-conformance.mjs` gets three modes, one flag each. The
implementation is a follow-up package; this is its specification, and rule 55 names the same commands.

| Mode | Command |
|---|---|
| a. register validity | `node conformance/bin/paddock-conformance.mjs copies --check-register` |
| b. range check | `node conformance/bin/paddock-conformance.mjs copies --range <base>..HEAD` |
| c. tag check | `node conformance/bin/paddock-conformance.mjs copies --tag <tag>` |

The `--app paddock` conformance run also runs mode a as check C58.

**a. Register validity** (`copies --check-register`: in every conformance run with `--app paddock`, and locally before a
register-only change lands; see rule 55(e)):

- the file parses; `version = 1`; ids are unique and kebab-case;
- every `owner` is `paddock` or a folder under `apps/` (the list is read from the tree, not written in the check);
- every `delete_by` is `<owner>-vX.Y.Z`, or `vX.Y.Z` for owner `paddock`;
- `kind` is `copy` or `wiring`; whole-crate globs end in `src/**` (§2);
- every path of a copy, and of a `built` canonical, matches at least one tracked file (`git ls-files`); every `item:`
  symbol is found in at least one of its member's files; every `text:` literal occurs in one of them;
- a copy whose files are all gone must be removed from the register in the same change.

Errors name the entry and say what to fix.

**b. Range check** (`copies --range <base>..HEAD`; CI runs it in the plan job with `base = $(git merge-base origin/main
HEAD)`, next to commit-lint, and the steward runs it before pushing a batch):

1. **Entry guard.** Compare the register at `base` and at `HEAD`. A copy entry (mechanism id plus its first path) that
   exists at `base` and not at `HEAD` is an error unless none of its paths match a tracked file at `HEAD` (the copy was
   deleted in the range), or a commit in the range carries `Copies-removed: <mechanism-id> <path>: <reason>` (for
   example a copy that turned out to be domain code). A changed `delete_by` is an error unless a commit carries
   `Copies-date: <mechanism-id> <path>: <old> -> <new>: approved: <who>: <reason>`, where `<old>` and `<new>` match
   the register at `base` and `HEAD`, and `<who>` is `coordinator` or `user`. When `<new>` is a later release line than
   `<old>` (a higher major or minor version), `<who>` must be `user`; otherwise the check reports the missing approval
   (rule 55(d)).
2. **Touches.** For every mechanism, compute the touched members (§2), leaving out `wiring` copies.
3. **Touch alike.** For a mechanism with at least one touched member, every other `copy` member and a `built` canonical
   must be touched too, by any commit of the range, or waived. A `planned` canonical is never required (its files do not
   exist); the copies of a planned canonical still move together.
4. Each untouched, unwaived member is an **error**: `C58 launchd-job-report:
   apps/pitcrew/app/src-tauri/src/service_repair.rs was not changed with crates/paddock-service/src/parse.rs. Make the same
   fix there (rule 55), or waive it with "Copies-waived: launchd-job-report apps/pitcrew/app/src-tauri/src/service_repair.rs:
   <why>".`

**Waivers.** A git trailer, one per member:

```
Copies-waived: <mechanism-id> <member-path>: <reason, at least 10 characters>
```

- A waiver counts only in a commit that itself touches that mechanism, so a waiver cannot leak to another fix in the same
  steward batch.
- `<member-path>` may be `*` (every other member) only when the mechanism has at most two members besides wiring; for a
  larger mechanism each member is named.
- The check prints every waiver it used, and the reviewer of the change confirms each one in the review.

Typical reasons: "the copy does not have this code path (Windows-only branch)", "the copy is deleted in this same batch".

**c. Tag check** (`copies --tag <tag>`):

- A copy **exists** while one of its paths matches a tracked file and, when it has `symbols`, while one of its `item:`
  spans or `text:` literals is still found in those files. So a copy narrowed to items (Pitwall's hand-built tray) is
  gone once those items are, even if the file stays.
- For the tag's app and version, every existing `copy` or `wiring` entry whose `owner` is that app and whose `delete_by`
  version is lower than or equal to the tag's version is an error ("dyno-v1.1.0: apps/dyno/crates/dyno-ipc/src/endpoint/** is
  still a copy of paddock-ipc; delete it, or move its date (rule 55(d))"). When the mechanism's canonical is `planned`,
  the message names that as the cause ("the canonical icons/check.mjs is not built yet").
- A shared-packages tag `vX.Y.Z` checks the `paddock` entries the same way.
- **The steward runs it before creating the tag;** that is where it is meant to stop a release. The app release workflow
  runs it again in its tag-check step, where it **fails the release workflow** (the tag already exists by then) and no
  installer is published.
- A release branch that deletes copies also removes their entries (mode a requires it), so the batch that becomes a tag
  carries its own register.

**Not detected.** A brand-new copy that nobody registers. That stays a review finding (rule 42) and the survey at each
app move. The shared crates' own "Taken from" and "Ported from" doc comments are a cheap source for that survey (they
seeded part of this register). A report-only similarity scan is a possible later step, not part of this ADR.

### 4. The freeze rule

During a shared-code freeze (rule 1) **a fix to a registered mechanism is not frozen**: it lands in the canonical
implementation and in every copy in the same change, with one failing test per member (rule 11). What stays frozen is the
shared API: a fix may not rename or remove a public item of the canonical crate during the freeze. If the canonical crate
has no user yet, its fix is still made (the next adopter gets it), and its test still runs in the crate's own CI legs.

A new copy during a freeze (to ship an app before the shared adoption is ready, as Dyno 1.0 did with `dyno-ipc`) is
allowed only with its register entry, a `delete_by` tag and the reason, in the same change.

### 5. Seed and dates

`docs/known-copies.toml` is seeded with every copy found by the 2026-10-31 survey and by the shared crates' own "Taken
from" notes in `crates/` and `ui/`: 46 mechanisms, 87 entries. The `delete_by` dates follow the agreed order (Dyno 1.0, Spotter 1.0,
Pitwall 1.1, Pitcrew 1.1, Debrief 1.2, Pacenotes, Sighter) and are proposals for the review and the user to confirm:

| Owner | Deleted by | Copies |
|---|---|---|
| Spotter | `spotter-v1.0.0` | `native-feel.ts`, `copy-menu.ts`, `updater.rs`, the hand-built tray and popover, single-instance wiring (all deleted by the Spotter 1.0 batch) |
| Pitwall | `pitwall-v1.1.0` | `native-feel.ts`, `sparkline.ts`, `updater.rs`, `window.rs`, the hand-built tray, single-instance wiring (all deleted by the Pitwall 1.1 batch) |
| Pitcrew | `pitcrew-v1.1.0` | `service_repair.rs`, `service_install.rs`, `notify-channels`, `updater.rs`, the hand-built tray, its UI copies, login wiring |
| Dyno | `dyno-v1.1.0` | `service_repair.rs`, the login and service parts of `login.rs`/`login_item.rs`, `dyno-ipc` endpoint, frames, peer and child, the redaction in `diagnostics.rs`, `sparkline.ts`, login wiring |
| Dyno | `dyno-v1.2.0` | `dyno-plugin-api`, `dyno-plugin-host` (Dyno tier 3, ADR 0001) |
| Spotter | `spotter-v1.1.0` | `spotter-render`, `mail.rs`, `pack.rs`, `time.rs`, `schedule.rs`, `schedule-editor.ts`, `clipboard.ts`, `check-icons.mjs`, login wiring |
| Pitwall | `pitwall-v1.2.0` | the rest of `service_repair.rs`, `service_install.rs`, `packs.rs`, the tray model types, `check-icons.mjs`, login wiring |
| Debrief | `debrief-v1.2.0` | `chromium-driver`, the `debrief-ipc` parts, the `debrief-plugin-*` platform crates, the agent lock, and the shell move: window manager, launch plan, lifecycle, tray, commands, dialogs, menu, panels, health, notification centre, updater, start-up watchdog, settings schema, model download, prompted tools, the loopback HTTP clients, UI copies (palette, regions, locale and more), the dock, scripts |
| Pacenotes, Sighter | `pacenotes-v1.0.0`, `sighter-v1.0.0` | `scripts/release/publish.mjs`, `scripts/export-diagrams.sh` |

**The Pitwall 1.1 exception.** Pitwall 1.1 already uses the shared `parse_job`, so its copy is only the repair sequence
(`plan`, `run_repair`). The shared `stale()` fix (B4 defect 2) is a separate change before the Pitwall 1.1 tag. Under this
ADR it is one change: `crates/paddock-service/src/parse.rs` and the Dyno and Pitcrew copies together.

## Consequences

- A fix to a listed mechanism costs more at once (every copy, each with a test) and less overall: no second root-cause
  hunt for a bug that was already fixed elsewhere (B4, the browser abort rounds).
- Release tags can be refused for an expired copy. That is intended: it is the only point where "later" becomes a date.
  Moving a date is cheap but visible and approved.
- Debrief's entries are the largest group: Debrief 1.2's shell move is where most of them end, so its plan can use the
  register as its deletion checklist.
- The register shrinks as the apps adopt the shared crates. An empty register plus C58 still guards new copies made
  during freezes.
- The range check needs git history. It runs in the plan job (which has the history) and in the steward's pre-push step,
  not in the per-file conformance runs.

## Alternatives considered

- **A rule without a check.** Rules 42 and 43 already exist; the copies drifted under them. Rejected.
- **Forbid copies outright, no freeze exception.** Then a release waits for a shared adoption (Dyno 1.0 would have waited
  for `paddock-ipc` and the plugin platform). Rejected: one release at a time (rule 1) needs the escape, but a dated one.
- **Detect copies by similarity (token or AST clones).** It would also flag intended domain look-alikes and miss copies
  that were already rewritten (the four launchd parsers are similar but not identical). Kept as a possible report-only
  helper for the survey, not as the gate.
- **The register in each app's `paddock.ci.json`.** A mechanism spans apps and crates; one family file keeps every member
  of a mechanism in one entry, which is what the check compares.
- **Count every file under a member's paths.** Simpler, but a lockfile bump or a test-only change would demand edits in
  every copy and trailers from bots. Rejected for the counted-files rule in §2.

## Work packages

| WP | What | Size | When |
|---|---|---|---|
| WP-1 | C58 in `conformance/src/checks/C58-known-copies.mjs` plus the `copies` subcommand (modes a-c). Good and bad fixtures for each rule: a member touched alone; a test-only, manifest-only and whitespace-only change (no error); a waiver in a commit that does not touch the mechanism (ignored); `*` on a mechanism with three members (error); a wiring copy touched alone (no error); a planned canonical (not required; tag message names it); a dropped entry with and without its files and with `Copies-removed`; a moved `delete_by` with and without `Copies-date`, with `approved: coordinator` on a later release line (error) and `approved: user` (passes); a copy narrowed to items whose items are gone while the file stays (gone at the tag); an expired copy at a tag; a stale entry; an unknown owner; a crate-root glob. Wiring: mode a in the `--app paddock` run, mode b in the plan job and the steward's pre-push, mode c in the steward's pre-tag step and the app release tag step | 2.5 d | right after the Dyno tag |
| WP-2 | `fix/service-stale-shared`: the shared `stale()` and `plan()` fix in `paddock-service` with the Dyno and Pitcrew copies, test first with an untrimmed real `launchctl print` | 0.5 d | before the Pitwall 1.1 tag |
| WP-3 | Each owner's deletions by its `delete_by` tag, through the app's normal work packages | per app | per tag |
