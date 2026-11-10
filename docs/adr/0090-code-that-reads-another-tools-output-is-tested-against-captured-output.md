# 90. Code that reads another tool's output is tested against captured output: a register of readers and fixtures, a capture command, and C104

- Status: Proposed 2026-11-10. Drafted at the coordinator's request of 2026-11-10 (ER-21: one class, seen twice that day
  in RT-8 and at least twice before). Nothing here is built yet. Needs an independent review before the user decides
  (ER-23 does not apply: no drawings).
- Date: 2026-11-10
- Related: ER-11 (a bug fix starts with a failing test), ER-14 (no real user data in fixtures; "invent same-shape data"),
  ER-21 / rule 40, ER-22 / rule 55 (`docs/known-copies.toml`), ER-33 / rule 71; ADR 0022 (developer tooling is Rust in
  `./dev`, no new shell or Node tooling), ADR 0074 (leased test VMs), ADR 0088 (accepted 2026-11-10 on
  `origin/docs/adr-windows-gate-ratchets`: one ratchet engine in `./dev check`, `docs/ratchets.toml`), ADR 0089 (proposed
  on `origin/docs/adr-not-measured`: not-measured outcomes, check C103); the RT-5 check host (`./dev check run`,
  `1691a35e5` on `origin/feat/rt5-check-host`, with "shrink-only baselines by text").
- Number: 0090 was free on every `origin` branch on 2026-11-10 (family ADRs end at 0089 on
  `origin/docs/adr-not-measured`; no locked ADR worktree holds an uncommitted 009x file). Check id C104: C103 is the
  highest id named on any `origin` branch (ADR 0089 §5).
- Evidence: read at `origin/train/2026-11-09-3` `67a633ef3` and `origin/feat/rt8-ci-flakes` on 2026-11-10 with
  `git grep`, `git show` and `git log -S`; one page of the nextest docs. No tool was run, no user data was read.

## Context: the incidents

`./dev ci flakes` (RT-8, `tools/paddock-dev/src/ci_runs/`, on `origin/feat/rt8-ci-flakes`) reads CI job logs and says
which tests were flaky, failed, or failed and then passed. Its first fixtures were written from reasoning about what
nextest and Playwright print, not taken from the tools.

1. **Invented nextest endings.** The first fixtures (`tests/ci-runs/nextest-flaky.log`, `nextest-hardfail.log`,
   `playwright-flaky.log`, `vitest-failed.log`, deleted in `d95a47ceb`) started with an invented runner banner
   (`Gitea Action Runner (inventedrunner)`) and assumed a hard failure ends on a bare `FAIL`. Captured with
   cargo-nextest 0.9.151, a hard failure with retries ends on `TRY n FAIL` and never on a bare `FAIL`, so the reader
   counted a final failure as flaky. Fixed in `d95a47ceb` with six captured logs (`nextest-r0-*.log`,
   `nextest-r1-*.log`, `playwright-project.log`, `playwright-noproject.log`).
2. **A missed label.** The reasoned timeout ending was `TIMEOUT`. With `retries = 1` nextest prints `TRY n TMT` (and
   `TRMNTG` while it kills the test), so a timed-out test made the job a "broken check". Found only when a real capture
   was made; fixed in `5da33f8cf` (`logfind.rs` regex gains `TMT`, two captured fixtures). The same commit says "SIG*
   endings stay unverified against a real log": the reader still matches labels nobody has seen.

The tool's documentation would not have caught either: the nextest retries page names `TRY 1 FAIL`, `TRY 2 PASS`,
`FLAKY 2/3` and `(1 flaky)`, but neither `TMT` nor `FLKY-FL` (https://nexte.st/docs/features/retries/, read
2026-11-10). Only the tool's output (or its source) is the reference.

## The class, seen before

- **2026-10-25, launchd simulation.** "The launchd simulation said 'never registered = NotRegistered', so every scenario
  passed while the real first install failed. Fakes copy the real OS's answers, captured from a VM"
  (`docs/lessons.md:388`, section at `:372`).
- **2026-10-25, `launchctl print` by line.** macOS 26 adds `resource coalition` and `jetsam coalition` blocks with their
  own `state = active` after the job's `state = running`; a line scan called a healthy agent refused
  (`docs/lessons.md:381-384`). Fixed in `paddock-service` (`3024f2938`), which now has the one real capture in the family
  that names its OS version: `crates/paddock-service/src/parse.rs:719` ("A real `launchctl print` of a healthy job
  (macOS 26.6.2, 2026-10-31)"), `tests/fixtures/launchctl-print-running.txt`. Pitcrew's copy
  (`apps/pitcrew/app/src-tauri/src/service_repair.rs:102-107`) still sets `running = line == "state = running"` on every
  `state = ` line; its tests feed reports without coalition blocks (`:430-431`), so they pass. The copies register
  already records the defect (`docs/known-copies.toml:44-48`, "Both B4 defects", deleted by `pitcrew-v1.1.0`).
- **2026-11-10, RT-8:** the two incidents above.

Four instances, one shape: a test input for a reader of another program's output was written from what the author
believed the program prints. The test then agrees with the reader, because both encode the same belief.

## Survey: readers of external output and the provenance of their test input (2026-11-10)

Method: `git grep` for the tools in the brief and for parse functions in the same files, then reading each file's module
doc to confirm it parses output of a program Paddock runs or a service it calls, then `grep` for provenance words
(`captured`, `real`, `recorded`, `invented`, `same-shape`) in the reader and its fixture folder. A unit is one reader
module with its tests. This is a grep-and-read count, not a full audit: a reader with no tool name in its source is missed,
and a note can be wrong.

**42 units classified. 4 have a capture note that names the tool or OS version; 5 have a note that says captured or
invented without a version or command; 33 have no note at all.** 5 JavaScript readers were found but not classified.

| Provenance | Units |
|---|---|
| Captured, version named (4) | RT-8 `ci_runs/logfind.rs` (nextest 0.9.151, Playwright 1.64.1, first line of each fixture; but `tests/ci-runs/clippy-failed.log` is still the invented `inventedrunner` log, read at `logfind.rs:202` and `flakes.rs:543`); `crates/paddock-service/src/parse.rs:719` (one fixture; others "invented same-shape text", `:494`); `apps/dyno/crates/dyno-platform/src/parse/mac_pcblist.rs` with its capture script `tests/fixtures/mac_pcblist/make.py` (macOS 27.2, values replaced); `scripts/vm-smoke/fixtures/README.md` (run ids named) |
| Note without version or command (5) | `apps/debrief/crates/debrief-plugin-windows/tests/fixtures/NOTICE.txt` ("recorded on a test machine", no OS build); Dyno `parse/smart.rs:153,233,246,287`, `parse/ioreg_disk.rs:85`, `parse/btm.rs:166` (each "invented" or "shape of a real reading"); `apps/dyno/crates/dyno-platform/src/counters/mac.rs:1-2` ("tested with real output", none named) |
| No note (33) | `tools/paddock-dev`: `plan_since.rs` (CI logs in `tests/since-run/`, real runner banners, no note), RT-8 `ci_runs/client.rs`/`timings.rs` (Gitea API JSON `tests/ci-runs/timings/*.json`); `crates/paddock-repo-graph/src/cargo.rs` (`cargo metadata`), `history.rs` (`git log`); `crates/paddock-download/src/proxy_os.rs` (`scutil`, `reg query`, `netsh`); `apps/pitcrew/app/src-tauri/src/service_repair.rs` and `apps/dyno/crates/dyno-agent/src/service/mod.rs:235` (`launchctl print`); `apps/dyno/app/src-tauri/src/link/launchd.rs`; `apps/debrief/crates/debrief-video/src/parse.rs` (ffprobe, ffmpeg; its stand-in `src/bin/fake_ffmpeg.rs` answers "like FFmpeg does" from a script); `apps/pitcrew/crates/pitcrew-core/src/gitea_api.rs`; `apps/spotter/crates/spotter-core/src/source/forge.rs`, `json_api.rs`; Dyno core `extras/boot.rs` (`wevtutil`, `systemd-analyze`), `extras/sleep.rs` (`pmset`, `powercfg`), `extras/gpu.rs` (`nvidia-smi pmon`, `typeperf`), `extras/usage.rs` (`nettop`, `top`), `extras/software.rs` (`system_profiler`, `reg query`), `network_traffic.rs` (`nettop`); Dyno platform `issues/windows.rs` (`wevtutil`), `helper_counters.rs`, dyno-agent `helper_readers/counters.rs` (`powermetrics`); Dyno platform `parse/` `firewall.rs`, `hardware.rs`, `ioreg.rs`, `linux_hw.rs`, `linux_tools.rs`, `lsof.rs`, `mac_tools.rs`, `powercfg.rs`, `wifi.rs`; Pitwall `crates/pitwall-core/src/pressure.rs` (`sysctl`, `pmset`), `hardware.rs` (`nvidia-smi`) |
| Not classified (5) | `scripts/affected.mjs` and `conformance/src/commit-lint.mjs` (`git log`), `scripts/ci-timings.mjs` (Gitea API), `scripts/vm-smoke/win-judges.mjs` (`Get-CimInstance`), `conformance/src/checks/C04-macos-signing.mjs` (`codesign`) |

Out of scope: Debrief's import formats (user files of many programs). Their samples are generated on purpose
(`apps/debrief/samples/README.md`, "Do not edit the samples by hand"), with real specimens kept apart
(`apps/debrief/samples/real-libraries/README.md`). D7 asks whether to include them.

Good precedents already exist and are the model for this ADR: a first-line capture note in a log fixture (RT-8), a
capture script that keeps the shape and replaces every value (`make.py`), and a folder note naming source and licence
(`NOTICE.txt`).

## Pattern

A reader of another program's output has two sources of truth that can disagree: the program and the test input. When
the test input is written from belief, a wrong belief is tested in, not out, and the reader looks finished. The fix is
not "be more careful": the test input must come from the program, say so, and be easy to refresh.

## Decision

### 1. A register of readers and their fixtures: `docs/captured-output.toml`

One family file, like `docs/known-copies.toml`. One `[[reader]]` per unit: `id`, `paths` (the reader's source),
`tool` (program or service), `version_source` (where the pinned version is read, when there is one: `rust-toolchain`,
`package-lock.json` for `@playwright/test`, the CI image's nextest line, `crates/paddock-runtime` catalogue for FFmpeg;
none for OS tools), and `labels` (D6). One `[[reader.fixture]]` per test input: `path` (a file, or `path#CONST` for an
inline string), `kind` (`captured`: the program's bytes with values replaced; `invented`: written by hand, for a hostile
or bounds case), and for `captured`: `with` (tool and version, OS and build), `command` (the exact command and config),
`date` (from the clock), `where` (`vm:<name>`, `ci-image:<tag>`, `scratch`, `host`), `sanitised` (what was replaced).

The register holds provenance, so it works for JSON and binary fixtures and for inline strings; a text fixture may
also carry a first-line note (the RT-8 convention), which C104 does not require.

### 2. Every reader has at least one captured fixture per mode it claims to read

A mode is a setting that changes the output (nextest retries 0 and 1; Playwright with and without projects; macOS and
Windows for a cross-OS reader). Invented fixtures stay for what a capture cannot show (truncation, hostile bytes, size
caps: "parsers never panic", PRINCIPLES), always next to a captured one, never instead of it.

### 3. Capturing is one command where the tool runs on the host: `./dev capture <reader-id>`

It runs the register's `command` in the shared test scratch (ADR 0078) on a scratch project (a tiny crate with a passing,
a failing, a flaky and a hanging test is enough for nextest), applies the reader's sanitiser (paths, host names, user
names, ids; documentation IP ranges as `make.py` does), writes the fixture and updates its row. For OS tools
(`wevtutil`, `powercfg`, `pmset`, `smartctl`, `launchctl`) the command is run on a leased test VM (ADR 0074) or, where
no VM has the hardware (NVMe SMART, `powermetrics`), on the host, by hand, with the same row fields and the diff
reviewed for machine data (ER-14, `C25` denylist scan).

### 4. C104 in the RT-5 host, with a shrink-only ratchet (ADR 0088)

C104 fails when: a file under a declared fixture folder of a registered reader has no row; a row's file or constant is
gone; a reader has no `captured` row for a declared mode; a `captured` row lacks `with`, `command` or `date`; a pinned
`version_source` moved past a row's `with` (D5); a reader's label list (D6) names a label that is neither in a captured
fixture nor marked `unverified`. Today's readers without a captured fixture are seeded as one list in
`docs/ratchets.toml`, which only shrinks. New readers are found by review (a `rules-review` checklist line and the RT
brief template line) and by the register itself; automatic discovery is left out (D4).

### 5. Known labels, where a reader matches a closed set of words

A reader that matches status words (nextest `PASS`/`FAIL`/`TMT`/`SIG*`/`FLKY-FL`, Playwright section names, launchd
`state =` values) declares them in its row's `labels`. Each label is `seen` (C104 finds it in a captured fixture),
`source` (a URL to the tool's source at a commit), or `unverified` (the reader matches it, nobody has seen it; the
reader's doc says so, as `logfind.rs` does for `SIG*`).

### 6. Positive controls (ER-33), each a test of C104

A planted fixture file without a row fails; a row whose file is gone fails; a reader with only `invented` rows fails; a
`version_source` bumped past `with` fails (or warns, per D5); a label with no evidence fails; a clean planted register
passes.

## Parallelism and cost (rule 51)

C104 reads one TOML file, stats the listed files and greps the captured fixtures for the declared labels: milliseconds,
no threads needed. `./dev capture` runs only on demand, one tool at a time; a nextest capture builds one scratch crate
(seconds after the first build). Nothing is added to CI run time beyond the check.

## Alternatives

- **Only a per-file header, no register.** Cannot cover JSON, binary or inline fixtures without breaking them, and a
  lint cannot tell a missing header from a file that is not tool output. Kept as an optional convention for logs.
- **Live contract tests that run the real tool in CI.** Strongest for host tools, but they need the tool on every runner
  (ADR 0089's not-measured problem, ffmpeg is already missing on a leg) and run on every change. `./dev capture` gives
  the same evidence at bump time; a nightly live run can be added later for nextest and Playwright if bumps slip.
- **Read the tool's documentation.** Insufficient: nextest's docs do not list `TMT` (above).
- **A review checklist only.** The four instances passed review.

## Consequences

- A reader is not done until it has a captured fixture; writing one costs a capture, not a guess.
- Tool bumps cost a re-capture of the readers of that tool (cheap where `./dev capture` covers it).
- Capturing on real machines touches ER-14; the sanitiser and the reviewed diff are the guard, and the ER-14 text needs
  one sentence so "invent same-shape data" is read as "replace the values of a real capture", not "reason about the
  shape" (D10).

## Rollback

Set C104's list to report-only in `docs/ratchets.toml`, or revert WP-1. The register and fixtures stay useful as data.

## Decisions for the user

| # | Decision | Options | Recommended |
|---|---|---|---|
| D1 | Where provenance lives | (a) per-fixture header in every file; (b) one register `docs/captured-output.toml` per reader and fixture, a first-line note optional for text logs; (c) a README per fixture folder | **(b)**: works for JSON, binary and inline strings, and C104 can check it |
| D2 | What a reader must have | (a) captured fixtures only; (b) at least one `captured` fixture per declared mode, `invented` allowed beside it for hostile and bounds cases; (c) any labelled fixture | **(b)** |
| D3 | How fixtures are captured | (a) a script per tool beside the fixtures; (b) `./dev capture <id>` for host tools (nextest, Playwright, vitest, clippy, `cargo metadata`, `git`), by hand on a leased VM with the command recorded for OS tools; (c) all by hand | **(b)**: ADR 0022 keeps tooling in `./dev`; OS tools need VMs anyway |
| D4 | The check | (a) C104 in the RT-5 host over the register and declared folders, today's gaps as a shrink-only ADR 0088 list, new readers found in review; (b) also a heuristic for new readers (a file that runs a program and has a `parse` function); (c) review checklist only | **(a)**; (b) later if review misses one |
| D5 | Tool version bumps | (a) a pinned `version_source` moving past a row's `with` fails C104 until re-captured (or the row is re-dated after a capture showed no diff); OS tools: a notice when the test VM's OS build changes; (b) a notice only; (c) nothing | **(a)** |
| D6 | Known labels | (a) declared per reader, each `seen`, `source` or `unverified`, checked by C104; (b) parse the tool's source; (c) none | **(a)**, only for readers that match a closed set of words |
| D7 | Scope | (a) programs Paddock runs, services it calls, and CI logs; Debrief's import formats stay under its samples policy; (b) include Debrief's import formats | **(a)** |
| D8 | Where captures run | (a) only VMs, containers and the scratch; (b) also the host for read-only tools no VM can serve, sanitised, diff reviewed | **(b)** |
| D9 | Engineering rule | (a) amend ER-14: "Test input for a reader of another program's output is captured from that program on a test machine, with values replaced, and records the tool, version and command; invent only hostile and bounds cases"; (b) a new ER-37 | **(a)**: it resolves the "invent same-shape data" reading in the same place |
| D10 | Tag gating | (a) none: a shrink-only list, burned down by risk (closed-label readers first) as each app is next touched; (b) each app's readers captured before its next tag | **(a)** |

## Work packages (raw agent-days, one agent)

| WP | Content | Depends on | Days |
|---|---|---|---|
| WP-0 | RT-8 leftovers on `feat/rt8-ci-flakes`: capture the clippy failure (replace `clippy-failed.log`), a `SIG*` nextest ending, and note the provenance of `timings/*.json`; failing test first where a reading changes | none | 0.5 |
| WP-1 | `docs/captured-output.toml` schema; C104 in the RT-5 host with the §6 controls; the ratchet list in `docs/ratchets.toml` | RT-5 landed, ADR 0088 WP-1 | 2 |
| WP-2 | `./dev capture` for host tools (nextest, Playwright, vitest, clippy, `cargo metadata`, `git log`) with scratch projects and sanitisers | WP-1 | 1.5 |
| WP-3 | Seed the register from this survey (42 units plus the 5 JS readers), honest `kind` per fixture | WP-1 | 0.5 |
| WP-4 | First burn-down of closed-label readers: Dyno agent `launchctl print`, `pmset`/`powercfg` (`extras/sleep.rs`, `parse/powercfg.rs`), `smartctl`, `wevtutil`, ffprobe; on leased VMs | WP-1, WP-3 | 2 |
| WP-5 | ER-14 text (D9), the Paddock rule line, lessons entry, `rules-review` checklist and RT brief template lines | acceptance | 0.5 |

First cut (WP-0, WP-1, WP-3, WP-5): 3.5 days. All: 7 days. The rest of the list shrinks as apps are touched.

Tag gating: none (D10). WP-0 lands with RT-8; WP-1 to WP-3 change only `tools/paddock-dev`, `docs/` and fixtures, so they
touch no app's release path (ER-31).
