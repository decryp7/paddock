# Amendment to ADR 0081 (2026-11-11): zombies, process groups and `tools/heavy` go through the one look

- Status: Accepted 2026-11-11 (user), all decisions as recommended (D-Z1, D-Z2). The user accepted it on 2026-11-11 in chat, as relayed by the coordinator on 2026-11-11 (the user's words are not in
  the repository; ADR 0097's signed approvals are not built). History: Proposed
  2026-11-11; revision 2 (`f3d313123`) answered the independent review of revision 1 (`dbd7028a1`: see "Review answers")
  and was approved. An amendment to ADR 0081 (accepted 2026-11-10 by the user, decisions as recommended), which is
  **not on `origin/main`**: it is on `origin/docs/adr-0081-0082` (`9dc480ef6`) only, and ADR 0096's copy to main has
  not happened yet. This file is merged into the ADR's text when 0081 lands on main. Nothing here is built yet. ER-23
  does not apply.
- Amends: ADR 0081 §1 (the look: process groups), §3 (the check: group shapes and the `#[expect]` reason), §4 (a fourth
  user with its own check: `tools/heavy`), the instance table, and "Order" (what of WP-1 the train already has).
- Related: ER-21 / rule 40, ER-33 (a probe must be shown to see the thing), rule 53 and ADR 0033, ADR 0076 and its
  orphan-guardian amendment (`0076-orphan-guardian-2026-11-09.md`: the guardian that waits for groups), ADR 0096
  (register row `docs/adr/register/R-2026-11-11-0081-zombies-groups-heavy.toml`).
- Overlap checked: ADR 0081 itself already defines the zombie rule for single pids (§1's table) and the `kill(<pid>, 0)`
  shape of its check (§3.2); this amendment adds only groups, `tools/heavy` and the `#[expect]` rule. ADR 0033 and C53
  cover kills, not looks. The orphan-guardian amendment of ADR 0076 owns the guardian's waits; its group look is the one
  item 1 keeps. No other ADR or amendment on `origin/main`, the train or the pinned ADR branches names `killpg(.., 0)`
  or `tools/heavy`'s liveness.
- Evidence: read on 2026-11-11 with `git show`, `git grep`, `git log`, `git merge-base --is-ancestor` and `git tag
  --contains` after `git fetch`. Paths are from `origin/train/2026-11-11-5` unless named. No user data was read. Heads,
  re-read right before the commit:

```evidence-heads
read: 2026-11-11
origin/main ea94d9f1e
origin/train/2026-11-11-5 55c056089
origin/docs/adr-0081-0082 9dc480ef6
origin/fix/guardian-zombie-wait 1f9029dbc
origin/fix/liveness-broken-sites 7ea9fa08b
origin/feat/sysinfo-0-40 2ea59896b
```

## Trigger: the instances

| # | Where | What happened | Source |
|---|---|---|---|
| 1 | Debrief 1.1.1 tests, Linux CI container (PID 1 is `sleep`, it never reaps) | "the grandchild is gone" passed only when the shell reaped it first: `kill(pid, 0)` reports a zombie as existing | lesson "A CI job container whose PID 1 is `sleep` never reaps orphans" (`docs/lessons.md:494`, `2bebe158c`) |
| 2 | `paddock-proc` orphan guardian, Linux container | `killpg(group, 0)` counts zombies on Linux (not on macOS), so the guardian waited its full 10 s bound per SIGKILLed group; its test **passed** at 32 s because its hang guard (40 s) was longer than the three waits: a probe that could not see (ER-33), not a red run | `c8d2f6423`; `620bb7adb` reproduced it on purpose (uid 65534, test binary as PID 1, arm64 container: three "still had members" lines, 32.4 s) and made the test assert the outcome; `1f9029dbc` made an unreadable or non-UTF-8 `/proc/<pid>/stat` count as running, never gone |
| 3 | `tools/heavy` `procs.rs:68` | `if start.is_some() && !paddock_proc::is_alive(pid) { return Presence::Gone }`: `is_alive` folds `Unknown` into "not alive", so a holder whose start time is known but whose look is `Unknown` reads as gone, and `lock.rs` `break_if_stale` then breaks its slot lock | code reading; not in ADR 0081's survey (S1 to S22) |
| 4 | `tools/heavy` `probe.rs:17` `exists` | raw `kill(pid, 0)`, EPERM counted alive, zombie-unaware (procs.rs adds the zombie case through #3's fold) | code reading |

The fixes of #2 are on the train (`git merge-base --is-ancestor origin/fix/guardian-zombie-wait
origin/train/2026-11-11-5`), not on `origin/main`, in no tag. #3 and #4 are unfixed. `tools/heavy` ships in no app.

## What the train already has, and what it does not

ADR 0081 §1 asks for a three-valued look from OS primitives, with a zombie counted as gone (its table: Linux state `Z`
or `X`, macOS `SZOMB`). On the train, from `origin/fix/liveness-broken-sites` and `origin/feat/sysinfo-0-40`:

- **Built (an interim of WP-1):** `paddock_proc::Presence { Running, Gone, Unknown }` (`tree.rs:294`), `presence(pid)`
  (`:326`), `ProcessId::presence` and `recorded_presence` (`identity.rs:93`, `:136`); `classify` (`tree.rs:356`) makes a sysinfo `Zombie`/`Dead` status `Gone`, start time 0 or an unknown status `Unknown`, and an unlisted pid
  `Gone` only on the primitive `sys::pid_exists` answering "no such process" (`sys.rs:108`, ESRCH; Windows
  `ERROR_INVALID_PARAMETER` or an exited handle, `sys.rs:305`) or on `sys::is_zombie` (`sys.rs:339`/`:362`: macOS
  `kp_proc.p_stat == SZOMB` by `sysctl`, Linux `/proc/<pid>/stat` state `Z`/`X`). For groups: `tree::group_exists`
  (`tree.rs:171`, crate-private): `killpg(group, 0)` and, on Linux, every member's `/proc/<pid>/stat` read once.
  **So the zombie-aware "is alive" helper exists: it is `paddock_proc::presence`**, three-valued, with a zombie as
  `Gone`. None of this is on `origin/main` (`main`'s `group_exists`, `tree.rs:161`, is `signal_group(group, 0)` alone, "zombies included", and
  `main` has no `Presence`).
- **Not built:** sysinfo still decides first (§1 wants the primitives to decide); the `bool` folds `is_alive` and
  `may_be_alive` remain (D1 (a) removes them; ADR 0081 WP-1b counts about 30 call sites, `tools/heavy`'s two among
  them); no proof tokens (D2); no `docs/liveness.toml` and no check (WP-3).

## Pattern (unchanged, one more shape)

ADR 0081's pattern holds. This amendment adds the **group** shape: "is anything left in group G?" answered by
`killpg(G, 0)`, which on Linux counts zombies; and a fourth tool with its own two-valued liveness type
(`tools/heavy`'s `Presence { Gone, Present }`, beside Debrief's `Look` and Dyno's `Option<bool>` that §4 already names).

## Change

1. **§1: groups.** A group look is part of the one look: `paddock_proc` keeps `group_exists` as the only group probe,
   with its documented precondition (valid once the group can gain no members, after SIGKILL). It stays crate-private
   until a second user needs it (D-Z2); when one does, it becomes `group_presence(group) -> Presence`, whose `Unknown`
   is "the signal said yes and `/proc` could not be read" (today that case answers `true`, the safe direction for a
   waiter).
2. **§3: the check finds group probes and marked signal-0 calls.** The liveness check's shapes (ADR 0081 §3.2) gain
   `killpg(<g>, 0)`, `kill(-<g>, 0)`, `nix::sys::signal::killpg(.., None)` and `paddock_proc`'s own
   `signal_group(<g>, 0)` outside `tree.rs`. And because `libc::kill` is already in the Clippy preset
   (`lint/clippy.toml`, ADR 0033), every raw signal-0 site today carries `#[expect(clippy::disallowed_methods, reason =
   "...")]` (chromium-driver `process.rs:67`, debrief-ipc `process.rs:695` and `:733` (tests), heavy `probe.rs:7`,
   `paddock-proc` `sys.rs:104`): the check requires each such `#[expect]` over a signal-0 call to be a `docs/liveness.toml` entry
   (`kind = "primitive"` only for `crates/paddock-proc/src/sys.rs`; `migrating` with a `delete_by` for the rest).
3. **§4: `tools/heavy` is a user with its own check.** `probe.rs` `exists` and `procs.rs` `OsProcs::probe` call
   `paddock_proc::presence` (later `ProcessId::look`); heavy's `Presence` gains `Unknown`, and `pid_alive`,
   `child_alive` and `break_if_stale` treat `Unknown` as alive (a slot lock is broken only on `Gone`), logging the reason
   once (ER-27). heavy's raw `kill(pid, 0)` goes. heavy already depends on `paddock-proc` (`tools/heavy/Cargo.toml`).
4. **Positive controls for zombies (ER-33), added to WP-1's list.** On Linux and macOS CI legs: a child spawned, killed
   and **not** reaped by the test is a zombie of the test process; for it, `libc::kill(pid, 0)` succeeds (the control
   shows the case is real on that host) and `presence(pid)` is `Gone`; a process group whose only members are such
   zombies reads as empty in `group_exists` on Linux. The guardian's outcome assertion of `620bb7adb` ("emptied", not
   "still had members") stays the control for the bounded wait.
5. **Instance table additions** (§ "Instances"): H1 `tools/heavy/src/procs.rs:68` (**broken**: breaks a lock on
   `Unknown`), H2 `tools/heavy/src/probe.rs:17` and its non-Unix twin `:26` (at risk: a raw probe and an
   `is_alive || of().is_some()` fold), G1 `crates/paddock-proc/src/tree.rs:171` `group_exists` (fixed, zombie-aware on
   Linux). T6 (chromium-driver `process_exists`, EPERM read as gone, zombie-unaware) and T7 (debrief-ipc `running`,
   whose Linux `is_zombie` folds an unreadable `/proc` into "zombie", `process.rs:711` `is_none_or`) are already rows and
   are unchanged.

## What this does not cover

- Windows: no zombies; a process object kept by a handle is "exited" by `GetExitCodeProcess` (`sys.rs:305`, fixed in
  `867add2de`), already in §1's table.
- Held children (`try_wait`), which ADR 0081 D6 and ADR 0076 own.
- Containers whose `/proc` is mounted `hidepid=2`: §1's control already covers them.

## Release impact (ER-31)

`tools/heavy` ships in no app. The guardian fix (`group_exists`) is in shared code every app with supervised children
builds, and is in no tag yet; it lands with the train. Nothing here gates a tag beyond ADR 0081's D5 rows.

## Work packages (raw agent-days, one agent; on top of ADR 0081's 14)

| WP | Content | Days |
|---|---|---|
| WP-Z0 | Add H1, H2, G1 to ADR 0081 WP-0's survey and `docs/liveness.toml`; re-read the four `#[expect]`ed signal-0 sites | 0.25 |
| WP-Z1 | The zombie controls of item 4 in `paddock-proc` (Linux and macOS), part of WP-1's controls | 0.5 to 0.75 |
| WP-Z2 | `tools/heavy` onto `paddock_proc::presence`, three-valued `Presence`, `Unknown` never breaks a lock, once-per-reason log; failing test first | 0.5 to 0.75 |
| WP-Z3 | The group shapes and the `#[expect]`-must-be-registered rule in WP-3's check, with fixtures (`killpg(g, 0)`, `kill(-g, 0)`, an unregistered `#[expect]` over `kill(pid, 0)`) | 0.25 to 0.5 |
| WP-Z4 | Fold this text into ADR 0081 when it lands on main; lesson line beside the 2026-10-25 zombie lesson | 0.25 |
| | **Total** | **1.75 to 2.5** |

**CI runs (rule 50).** WP-Z2 is one branch run (the `tools` job for `tools/heavy`). WP-Z1 and WP-Z3 ride in ADR 0081
WP-1's and WP-3's landings and add no run. WP-Z0 and WP-Z4 are docs-only. So **1 extra branch run**, plus a fix round
if red.

WP-Z2 can land at once (it touches only `tools/heavy`); WP-Z1 and WP-Z3 ride on ADR 0081 WP-1 and WP-3.

## Failing tests the work starts with (ER-11; not written in this change)

- `tools/heavy`: `a_holder_whose_look_is_unknown_keeps_its_lock` (a `Procs` answering a known start time and an
  `Unknown` look for the holder; `break_if_stale` must return false; today `OsProcs::probe` returns `Gone` through
  `!is_alive`, so the equivalent real-OS path breaks it).
- `paddock-proc`: `an_unreaped_killed_child_is_gone_though_signal_zero_finds_it` (Linux, macOS).

## Decisions (accepted 2026-11-11 as recommended)

| # | Question | Options | Recommended and chosen |
|---|---|---|---|
| D-Z1 | `tools/heavy` | (a) migrate now (WP-Z2, 0.5 days, touches no app); (b) wait for ADR 0081 WP-1b's one-landing migration | **(a)**: it can break a live job's slot lock, and every agent's heavy build goes through it (ER-24) |
| D-Z2 | A public group look | (a) keep `group_exists` crate-private until a second user; (b) publish `group_presence` now | **(a)** (YAGNI); item 1 fixes its shape for when it is needed |

## Review answers (revision 1, `dbd7028a1`)

| Finding | Answer in revision 2 |
|---|---|
| P2 pin `origin/feat/sysinfo-0-40` (ADR 0091) | pinned in the evidence heads (`2ea59896b`) |
| P2 estimate ranges and CI runs | ranges (1.75 to 2.5); one extra branch run |
| P2 overlap | "Overlap checked" line |

## Rollback

Revert WP-Z2 (heavy keeps its own probe); the check shapes leave WP-3's list with one line each.
