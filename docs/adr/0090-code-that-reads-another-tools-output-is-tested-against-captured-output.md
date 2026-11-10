# 90. Code that reads another tool's output is tested against captured output: a register of readers and fixtures, a capture command at the pinned version, and C104

- Status: Proposed 2026-11-10, revision 3. Revision 1 (`0cca8dd30`) was reviewed independently ("changes": P1-1 to
  P1-6 and ten P2s), revision 2 (`9687d644b`) answered them, and its re-review ("changes": P1-A, P1-B and six P2s) is
  answered here (see "Review answers"). Drafted at the coordinator's request of 2026-11-10
  (ER-21: one class, seen twice that day in RT-8 and twice before). Nothing here is built yet. ER-23 does not apply (no
  drawings).
- Date: 2026-11-10
- Related: ER-11, ER-12, ER-14 (no real user data in fixtures; "invent same-shape data"), ER-21 / rule 40, ER-22 / rule
  55 (`docs/known-copies.toml`), ER-33 / rule 71; ADR 0022 (developer tooling is Rust in `./dev`), ADR 0074 (leased test
  VMs), ADR 0078 (one shared test scratch), ADR 0088 (accepted 2026-11-10: one ratchet engine in `./dev check`,
  `docs/ratchets.toml`, kinds Ratchet and Register), ADR 0089 (proposed: not-measured outcomes, C103, and §4b's single
  runner-log reader `runlog`); the RT-5 check host (`./dev check run`).
- Number: 0090 was free on every `origin` branch on 2026-11-10 (0089 is the highest, on `origin/docs/adr-not-measured`;
  no ADR worktree holds an uncommitted 009x file). Check id C104: C103 (ADR 0089 §5) is the highest named on any `origin`
  branch.
- Evidence: read on 2026-11-10 with `git show`, `git grep` and `git log -S` at these branch heads, re-read for revision 2:
  `origin/train/2026-11-09-3` `67a633ef3` (this ADR's base; paths and lines without a branch are from it),
  `origin/main` `b51c72509`, `origin/feat/rt8-ci-flakes` `da05b54e8`, `origin/feat/rt5-check-host` `ec8530a95`,
  `origin/docs/adr-windows-gate-ratchets` `db7782e01` (ADR 0088), `origin/docs/adr-not-measured` `0df6835a9` (ADR 0089, its status commit after `bd9f9eea4`; accepted
  2026-11-10 by the user). Claims about shipped code were checked with `git tag --contains` on the commit that
  introduced the code.
  One measurement: Pitcrew's `parse_job` state loop (`apps/pitcrew/app/src-tauri/src/service_repair.rs:104-107`),
  copied into a 5-line program in the researcher's scratch folder and run on paddock-service's real capture
  (`crates/paddock-service/tests/fixtures/launchctl-print-running.txt`), returns `found = true, running = false` for a
  running job (that loop is on the train and `origin/main`, but in no Pitcrew or Pitwall tag; see below). One web page: the nextest retries docs. No user data was read.

## Context: the incidents

`./dev ci flakes` (RT-8, `tools/paddock-dev/src/ci_runs/` on `origin/feat/rt8-ci-flakes`) reads CI job logs and says
which tests were flaky, failed, or failed and then passed. Its first fixtures were written from reasoning about what
nextest and Playwright print, not taken from the tools.

1. **Invented nextest endings.** The first fixtures (`tests/ci-runs/nextest-flaky.log`, `nextest-hardfail.log`,
   `playwright-flaky.log`, `vitest-failed.log`, deleted in `d95a47ceb`) began with an invented runner banner
   (`Gitea Action Runner (inventedrunner)`) and assumed a hard failure ends on a bare `FAIL`. Captured with
   cargo-nextest 0.9.151, a hard failure with retries ends on `TRY n FAIL` and never on a bare `FAIL`, so the reader
   counted a final failure as flaky. Fixed in `d95a47ceb` with six captured logs.
2. **A missed label.** The reasoned timeout ending was `TIMEOUT`. With `retries = 1` nextest prints `TRY n TMT` (and
   `TRMNTG` while it kills the test), so a timed-out test made the job a "broken check". Found only when a real capture
   was made; fixed in `5da33f8cf`. That commit says "SIG* endings stay unverified against a real log", and the same
   regex also matches `ABORT` and `LEAK-FAIL`, which no fixture shows (`logfind.rs:58` at `5da33f8cf`).

RT-8 then re-captured every log at CI's pin, cargo-nextest **0.9.149** (`.gitea/actions/host-tools/action.yml:12`), in
the `paddock-ci:v2` image, and replaced the invented clippy log with a capture from clippy 0.1.99 / rustc 1.99.1 in the
same image (first line of each fixture in `tools/paddock-dev/tests/ci-runs/` at `da05b54e8`). Its one nextest reader,
`tools/paddock-dev/src/nextest_log.rs`, says which labels have no capture: `SIG*`, `ABORT` and `LEAK-FAIL` (`:9`,
`:67`). That is the shape this ADR asks of every reader. The nextest documentation would not have caught either bug: its
retries page names `TRY 1 FAIL`, `TRY 2 PASS`, `FLAKY 2/3` and `(1 flaky)`, but not `TMT` or `FLKY-FL`
(https://nexte.st/docs/features/retries/, read 2026-11-10). Only the tool's output, or its source, is the reference.

On the train (`67a633ef3`) the class still lives next door: `./dev plan --since-run` has its own matcher,
`FAIL|TIMEOUT|SIG*|ABORT|LEAK-FAIL|FLAKY`, without `TMT` or `FLKY-FL` (`tools/paddock-dev/src/plan_since.rs:67-71`), so
with `retries = 1` (allowed by ER-12) a timed-out test is not rerun. At RT-8's head `plan_since` calls the shared reader
instead (`plan_since.rs:60` at `da05b54e8`; `nextest_log.rs:70` matches `TMT` and `FLKY`), so the gap closes when RT-8
lands.

## The class, seen before

- **2026-10-25, launchd simulation.** "The launchd simulation said 'never registered = NotRegistered', so every scenario
  passed while the real first install failed. Fakes copy the real OS's answers, captured from a VM"
  (`docs/lessons.md:387-389`).
- **2026-10-25, `launchctl print` read line by line.** macOS 26 adds `resource coalition` and `jetsam coalition` blocks,
  each with its own `state = active`, after the job's `state = running`, and a line scan called a healthy agent refused
  (`docs/lessons.md:381-384`). Fixed in `paddock-service` (`3024f2938`), which now keeps the family's one whole capture
  that names its OS: `crates/paddock-service/src/parse.rs:719` (macOS 26.6.2, 2026-10-31).
  - Pitcrew's copy still scans every `state = ` line (`apps/pitcrew/app/src-tauri/src/service_repair.rs:104-107`, the
    same on `origin/main`). Run on the real capture it returns `running = false` for a running job (Evidence).
  - **It never shipped.** The line scan came in with `5ff3b0a39` (Pitcrew) and `638a61a6b` (Pitwall), both 2026-10-23;
    `git tag --contains` lists only Dyno tags for either, no Pitcrew or Pitwall tag. `pitcrew-v1.0.1` and `pitwall-v1.0.3`
    use `stale_registration` (`service_repair.rs:80-83` at those tags), which reads three phrases and is not tripped by
    the coalition blocks. Pitwall's copy is removed on the train in `b7151a57f`.
  - Its tests pass because their input is a real capture with the nested blocks cut out ("The real `launchctl print` ...
    (trimmed)", `:429-431`). A trimmed capture showed the shape the author expected, not the one macOS prints.
  - The copies register lists the defect (`docs/known-copies.toml:44-48`, "Both B4 defects", deleted by
    `pitcrew-v1.1.0`).
- **2026-11-10, RT-8:** the two incidents above.

Four instances, one shape. The test input for a reader of another program's output was written, or cut down, from what
the author believed the program prints. The test then agrees with the reader, because both encode the same belief.

## Survey: readers of external output and the provenance of their test input

**Method.** Two passes, then reading each candidate's module doc and tests:
- **Structural.** 97 non-test Rust files spawn a program (`Command::new`, `paddock_proc`, a `runner.run`/`run_tool`
  port); 70 of them read its output (stdout, `from_utf8`, a JSON decode). Add every Gitea API client, every non-test
  `scripts/vm-smoke` file that parses guest output (53), and the JS readers of CI logs.
- **By name.** `git grep` for the tools in the brief, which finds the pure reader modules that spawn nothing (Dyno's
  `parse/` folder).

A unit is one reader module with its tests. A spawn adapter that hands output to a counted reader is not a unit of its
own; it is named with that reader (for example `linux/gpu.rs` with `parse/linux_hw.rs`, `links.rs` with `parse/wifi.rs`,
`smart_reader.rs` with `parse/smart.rs`).

This is a sweep, not an audit. WP-3 repeats the structural pass and seeds the register from it.

**54 Rust units classified:**

| Provenance | Count | Units |
|---|---|---|
| Whole capture, tool or OS version named | 2 | RT-8's readers at `da05b54e8`: `nextest_log.rs` with its callers `ci_runs/logfind.rs` and `plan_since.rs`, and logfind's Playwright and clippy parts (nextest 0.9.149 at the pin and clippy 0.1.99, both in `paddock-ci:v2`; Playwright 1.64.1; first line of each fixture); `crates/paddock-service/src/parse.rs` for `launchctl print` (`:719`). Its `sc query`/`sc qc` mode is invented text (`:493`), so the Windows mode has no capture |
| A note that falls short (trimmed, no version or command, or inconsistent) | 9 | Pitcrew `service_repair.rs:429` (real, trimmed, the cut not declared); `tools/paddock-dev/src/plan_since.rs` on the train (its own matcher, real CI logs in `tests/since-run/`, no note; at RT-8's head it becomes a caller of `nextest_log`, but its vitest part and its logs stay); Dyno `parse/mac_pcblist.rs` with `tests/fixtures/mac_pcblist/make.py` (a capture script, but its note says "macOS 27.2, build 26B123", `make.py:1`, and a 26B build is macOS 26); `apps/debrief/crates/debrief-plugin-windows/tests/fixtures/NOTICE.txt` ("recorded on a test machine", no OS build); Dyno `parse/smart.rs:153,233,246,287`, `parse/ioreg_disk.rs:85`, `parse/btm.rs:166` ("invented" or "shape of a real reading"); `dyno-platform/src/counters/mac.rs:1-2` ("tested with real output", none named); `dyno-platform/src/issues/macos.rs` (`log show`, one "invented" note) |
| No note | 43 | **paddock-dev and shared:** RT-8 `ci_runs/client.rs` and `timings.rs`, one unit (Gitea JSON `tests/ci-runs/timings/*.json`); `tools/paddock-dev/src/input_hash.rs` (`git`); `crates/paddock-repo-graph/src/cargo.rs` (`cargo metadata`), `history.rs` (`git log`); `crates/paddock-download/src/proxy_os.rs` (`scutil`, `reg query`, `netsh`); `crates/paddock-service/src/privileged/windows_acl.rs` (`icacls`, whose words change with the display language); `apps/debrief/crates/cargo-debrief-plugin/src/lib.rs` (cargo JSON). **Debrief:** `debrief-video/src/parse.rs` (ffprobe, ffmpeg; its stand-in `src/bin/fake_ffmpeg.rs` answers "like FFmpeg does" from a script). **Pitcrew:** `pitcrew-core/src/gitea_api.rs`, `distribution.rs` (Gitea), `hygiene.rs` (`docker`, `podman`). **Spotter:** `spotter-core/src/source/forge.rs`, `json_api.rs`. **Pitwall:** `pitwall-core/src/pressure.rs` (`sysctl`, `pmset`), `hardware.rs` (`nvidia-smi`). **Dyno agent and app:** `dyno-agent/src/service/mod.rs:235` (`launchctl print` by substring), `helper_readers/counters.rs` (`powermetrics`), `app/src-tauri/src/link/launchd.rs`. **Dyno core:** `extras/boot.rs` (`wevtutil`, `systemd-analyze`), `extras/sleep.rs` (`pmset`, `powercfg`), `extras/gpu.rs` (`nvidia-smi pmon`, `typeperf`), `extras/usage.rs` (`nettop`, `top`), `extras/software.rs` (`system_profiler`, `reg query`), `network_traffic.rs` (`nettop`), `wol/readiness.rs` (`pmset`, `powercfg`, `reg query`; adapter `dyno-platform/src/wol/readiness.rs`), `wol/neighbours.rs` (`arp`, `ip neigh`, `ndp`), `region.rs` (locale, `reg query` Geo). **Dyno platform:** `issues/windows.rs` (`wevtutil`), `issues/linux.rs` (`journalctl`, `coredumpctl`), `inventory/windows.rs` (CIM JSON), `inventory/macos.rs` (`system_profiler` JSON), `inventory/linux.rs`, `wakes.rs` (`pmset`, `systemctl`), `helper_counters.rs`, and in `parse/` `firewall.rs`, `hardware.rs`, `ioreg.rs`, `linux_hw.rs`, `linux_tools.rs`, `lsof.rs`, `mac_tools.rs`, `powercfg.rs`, `wifi.rs` |

**JavaScript, not classified here (WP-3 does it):**
- **9 named files:** `scripts/affected.mjs` and `conformance/src/commit-lint.mjs` (`git log`); `scripts/ci-timings.mjs`,
  `conformance/release/publish.mjs`, `conformance/ci/release-verdict.mjs`, `apps/pacenotes/scripts/release/publish.mjs`
  and `apps/sighter/scripts/release/publish.mjs` (Gitea API); `conformance/src/checks/C04-macos-signing.mjs`
  (`codesign`); `apps/debrief/app/e2e/retried-reporter.mjs` (Playwright results, a copy of ADR 0089 §4b's `runlog`).
- **The 53 vm-smoke files.** Their one fixture note, `scripts/vm-smoke/fixtures/README.md`, names run ids but no tool or
  OS version.

**Out of scope (D7):** Debrief's import formats, which are user files of many programs. Their samples are generated on
purpose (`apps/debrief/samples/README.md`), and real specimens are kept apart (`apps/debrief/samples/real-libraries/`).
Debrief's collector (`debrief-collect/src/collector.rs`) runs `wevtutil`, `journalctl`, `log` and `typeperf` but only
writes their files for import, so it is covered there.

Good precedents exist and are the model: a first-line capture note in a log fixture (RT-8), a capture script that keeps
the shape and replaces every value (`make.py`), a folder note naming source and licence (`NOTICE.txt`), and a whole
capture with its OS (`paddock-service`).

## Pattern

A reader of another program's output has two sources of truth that can disagree: the program and the test input. When
the test input is written from belief, or a real capture is cut down to what the author expects, a wrong belief is
tested in, not out, and the reader looks finished. The fix is not "be more careful". The test input must come whole from
the pinned program, say so, and be easy to refresh.

## Decision

### 1. A register of readers and their fixtures: `docs/captured-output.toml` (an ADR 0088 Register)

One family file, like `docs/known-copies.toml`, declared in `docs/ratchets.toml` as kind **Register** with a positive
control (§6). Its records:
- One `[[reader]]` per unit, with these fields:
  - `id`: the key every other list uses.
  - `paths`: the reader's source and its spawn adapters.
  - `tool`: the program or service.
  - `modes`: the settings that change the output.
  - `pin`: the key in `ci/tool-pins.toml` (§2), when the tool is pinned.
  - `labels`: see §5.
- One `[[reader.fixture]]` per test input, with these fields:
  - `path`: a tracked file, or `path#CONST` for an inline string.
  - `mode`.
  - `kind`: `captured` is the program's whole output with values replaced; `invented` is written by hand for a hostile
    or bounds case.
  - `trimmed`: a list of what was removed and why. It is empty for a whole capture; anything cut must be declared here.
- For a `captured` fixture, also:
  - `with`: tool version, OS and build.
  - `command`: the exact command and its config.
  - `date`: from the clock.
  - `where`: `ci-image:<VERSION>`, `vm:<name>`, `metal:<model>` or `host`.
  - `by`: `user`, or `agent, approved by user <date>` for a host capture.
  - `kept`: the fields the reader reads, which the sanitiser keeps.

A text fixture may also carry a first-line note (the RT-8 convention); C104 does not require it.

**Modes.** A mode is anything that changes the output:
- nextest `retries` 0 and 1;
- Playwright with and without projects;
- each OS a cross-OS reader claims, with the **OS major version** a mode of its own (macOS 26 added the coalition
  blocks);
- **VM and metal**: a VM has no battery, NVMe SMART or discrete GPU, so `smartctl`, `powermetrics`, `pmset -g batt` and
  `nvidia-smi` captures come from metal;
- the display language for readers of localised text (`sc`, `icacls`).

### 2. A captured fixture's version equals the pin, and the pin is data

- Pinned tool versions move into one data file, `ci/tool-pins.toml` (nextest first: today `VERSION=0.9.149` in
  `.gitea/actions/host-tools/action.yml:12`).
  - `host-tools` reads it through `./dev pins get nextest`.
  - `ci-image/verify.sh` fails, instead of only printing (`verify.sh:36-38`), when the image's nextest (from the external
    garage image, `ci-image/Dockerfile:13`) differs from the pin.
  - C104 reads the same file.
- Tools without a pin are tied to the Linux CI image version. Rust is the main one: CI uses `stable`
  (`.gitea/actions/setup-rust/action.yml:4-6`), and there is no `rust-toolchain` file. A clippy or rustc row records
  `rustc -V` and `where = ci-image:<VERSION>`, and C104 compares it with `ci-image/VERSION`. **This covers the Linux
  image only.** On the macOS and Windows runners `setup-rust` installs whatever `stable` is that day, so no row can name
  their version. Rows of Rust tools are therefore scoped to the Linux image's mode; readers that need the other runners'
  output (none today) wait for D5 (b).
- **What a nextest bump costs.** The Linux image takes nextest from the garage image, so a pin bump means a garage and
  `paddock-ci` rebuild and a `ci-image/VERSION` bump. That bump makes every Rust-tool row stale as well (clippy, cargo
  metadata, rustc), and they are re-captured in the same change. With `./dev capture` that is one command per reader,
  four today.
- OS tools have no pin. Their mode (OS major, VM or metal) is the version.
- C104 fails when a `captured` row of a pinned tool names another version than the pin.
- A bump re-captures in the same change. For host tools that is one command (§3), which Renovate's lock PRs run before
  they are opened for review.

### 3. Capturing: `./dev capture <reader-id>`, at the pin, field-allowlist sanitised, scratch first

**Tools that run in the CI image** (nextest, Playwright, vitest, clippy, `cargo metadata`, `git`):
- `./dev capture` runs the command inside `paddock-ci:<VERSION>` or with the pinned binary, never with whatever the host
  has. Docker is used only for a container it creates (ER-14).
- The run is on a small scratch project in the shared test scratch (ADR 0078), outside the worktree. For nextest that is
  a crate with a passing, a failing, a flaky and a hanging test.
- Output goes to the scratch only. `./dev capture --accept` copies it into the fixture folder and writes the row.

**OS tools** (`wevtutil`, `powercfg`, `sc`, `icacls`, `journalctl`, `launchctl`, `pmset`):
- Captured on a leased test VM (ADR 0074) by the same command. The row records the VM.

**Metal-only tools** (`smartctl` on NVMe, `powermetrics`, battery, GPU) and tools that show the user's own state
(`nettop`, `lsof`, `system_profiler`, Wi-Fi names, `launchctl` of the user's agents):
- Captured on the user's Mac.
- They are run by the user, or by an agent only with the user's approval for that capture (D8).
- Output lands in the scratch first and enters the repository only after the user or the coordinator has read the diff.
- The row records `by`.

**The sanitiser keeps an allowlist, not a denylist.** Following `make.py`, it keeps the bytes of the fields the reader
reads (the row's `kept`), keeps the layout, line order, labels and lengths, and replaces every other value:
- documentation addresses (192.0.2.0/24, 2001:db8::/32);
- invented names, serials and ids;
- the scratch path.

A field the reader reads that holds personal data (a Wi-Fi name, a peer address, a serial) gets an invented value of the
same shape. When a local denylist exists (`~/.config/paddock/confidential.txt`, the C25 proposal's file,
`docs/conformance-proposal.md:70`), `./dev capture` also scans its output against it before `--accept`. C25 itself is
not built and this ADR does not depend on it.

### 4. C104 in the RT-5 host, with its list in `docs/ratchets/c104.toml` (ADR 0088)

C104 reads **tracked files only** (`git ls-files`), so captures waiting in the scratch never count as fixtures. This
differs from the RT-5 host's own `Tree`, which walks the file system like the JS host (`tools/paddock-dev/src/check/tree.rs:1-2`
at `ec8530a95`); C104 asks git for its list and uses `Tree` only to read the files. It fails
when:
- a tracked file under a reader's declared fixture folder has no row;
- a row's file or constant is gone;
- a reader has no `captured` row for a declared mode;
- a `captured` row lacks `with`, `command`, `date`, `where` or `by`;
- a pinned row's version differs from `ci/tool-pins.toml` (§2);
- a capture differs from the program's output by more than its declared `trimmed` entries. This is checked only for
  host tools, when `./dev capture --verify` re-runs them in the image or with the pinned binary; for OS and metal
  captures the `trimmed` list is reviewed in the diff.
- a declared label has no evidence (§5).

**Today's gaps.** Readers with no `captured` row for a declared mode go into `docs/ratchets/c104.toml`, keyed by reader
id and declared in `docs/ratchets.toml` (`id = "c104-uncaptured"`, `file`, `kind = "Ratchet"`, `readers = ["C104"]`,
`key = "reader"`, `control`). The list only shrinks (ADR 0088), and C104 checks that every key in it is a reader id in
the register. Its first filling is growth, so it follows ADR 0088 §5's seed rules:
- a `[[seed]]` record names the list, this ADR, the rule id `C104`, `count` and `entries_sha256`;
- every entry carries `rule = "C104"`;
- the seed is honoured only once this ADR reads `Accepted` on `origin/main`;
- the seed stays until WP-3's commits are on `origin/main`.

**New readers** are found by review: a `rules-review` checklist line and the RT brief template line. WP-3's structural
sweep is the seed. A heuristic that finds new readers automatically is left out (D4).

### 5. Known labels, where a reader matches a closed set of words

A reader that matches status words declares them in its row's `labels`. Examples:
- nextest `PASS`, `FAIL`, `TIMEOUT`, `TMT`, `SIG*`, `ABORT`, `LEAK-FAIL`, `FLAKY`, `FLKY-FL`;
- Playwright section names;
- launchd `state =` values.

Each label is one of:
- `seen`: C104 finds it in a captured fixture of the reader;
- `source`: a URL to the tool's source at a commit;
- `unverified`: the reader matches it and nobody has seen it, and the reader's doc says so.

Today `SIG*`, `ABORT` and `LEAK-FAIL` are `unverified` in RT-8's reader.

### 6. Positive controls (ER-33), each a test of C104

Each case below is a test of C104:
- a planted fixture without a row fails;
- a row whose file is gone fails;
- a reader with only `invented` rows fails;
- a capture at 0.9.151 against a 0.9.149 pin fails;
- a host-tool capture with an undeclared cut fails `--verify`;
- a label with no evidence fails;
- a file in the scratch is ignored;
- a c104 list key that is not a register id fails;
- a clean planted register passes.

### 7. Relation to ADR 0089 §4b (`runlog`)

ADR 0089 §4b (`0df6835a9`, accepted) makes one runner-log reader, `runlog`, for nextest, Playwright and vitest. RT-8 has
already built its nextest part: `tools/paddock-dev/src/nextest_log.rs` is the one nextest reader, and `ci_runs/logfind.rs`
and `plan_since.rs` are its callers (`da05b54e8`). In the register it is one reader, `runlog`, with modes per runner
(nextest retries 0 and 1, Playwright with and without projects, vitest), and its captured logs are this register's
fixtures. ADR 0089 WP-2c adds the Playwright and vitest parts and a vitest capture; it comes before this ADR's WP-3, and
the vitest capture is made once. The train's `plan_since` label gap closes when RT-8 lands.

## Parallelism and cost (rule 51)

C104 reads two TOML files, lists tracked files and greps captured fixtures for declared labels: milliseconds, no
threads. `./dev capture` runs on demand, one tool at a time. A nextest capture builds one scratch crate in the CI image,
which takes seconds once the image is pulled. A tool bump costs one capture per reader of that tool; for nextest that is
`runlog` alone. Nothing is added to CI time beyond the check.

## Alternatives

- **Only a per-file header, no register.** It cannot cover JSON, binary or inline fixtures without breaking them, and a
  lint cannot tell a missing header from a file that is not tool output.
- **Live contract tests that run the real tool in CI.** They need the tool on every runner (ADR 0089's not-measured
  problem). `./dev capture --verify` gives the same evidence at bump time. A nightly live run for nextest and Playwright
  can be added later.
- **Read the tool's documentation.** Insufficient: nextest's docs do not list `TMT` (above).
- **A review checklist only.** All four instances passed review.

## Consequences

- A reader is not done until it has a whole captured fixture per declared mode, at the pinned version.
- Tool bumps cost a re-capture in the same change, which is one command for host tools.
- Captures of the user's Mac need the user. That is deliberate: a capture of `nettop` or `system_profiler` is the user's
  machine data until the sanitiser and a reader of the diff say otherwise.

## Rollback

Set `c104-uncaptured` to report-only in `docs/ratchets.toml`, or revert WP-1. The register, the pins file and the
fixtures stay useful as data.

## Review answers (revision 2)

| Finding | Answer |
|---|---|
| P1-1 pins | §2: the version must equal the pin; `ci/tool-pins.toml` is read by host-tools, verify.sh and C104; unpinned tools are tied to the CI image version (D5); WP-0 re-captures RT-8 at 0.9.149 |
| P1-2 capture safety | §3: allowlist sanitiser, scratch first, user-run or user-approved host captures, `by` recorded; no claim on C25 (D8) |
| P1-3 missed readers | Structural sweep; 13 units added; counts redone (54 Rust; JS listed) |
| P1-4 Pitcrew | D11 (restated in revision 3) |
| P1-5 trimmed capture | Instance 3 restated; `trimmed` field; OS major is a mode; Pitcrew reclassified; Pitwall named |
| P1-6 ADR 0088 fit | §1 Register, §4 `docs/ratchets/c104.toml` keyed by reader id; WP-1 depends on 0088 WP-2 and WP-3 |
| P2-1 runlog | §7 |
| P2-2, P2-3 | `make.py` note and the recount in the table |
| P2-4 labels | §5 and the `plan_since` paragraph |
| P2-5 VM vs metal | §1 modes, §3, WP-4 3.5 days |
| P2-6 ER-37 | D9 |
| P2-7 Renovate, D3 + D8 | §2 and D3 (merged with the old D8) |
| P2-8 citations | `lessons.md:387-389`, `parse.rs:493` |
| P2-9 README | blank line removed |
| P2-10 scratch | §4: tracked files only |

## Review answers (revision 3)

| Finding | Answer |
|---|---|
| P1-A RT-8 facts at `da05b54e8` | Context and the survey table: RT-8's logs are captured at the 0.9.149 pin in `paddock-ci:v2`, and clippy at 0.1.99; the `plan_since` gap is stated for the train `67a633ef3` only; §7: `nextest_log.rs` is the one nextest reader with two callers; WP-0 is 0.25 days; ADR 0089 cited at its head `0df6835a9` (the acceptance commit after `bd9f9eea4`) |
| P1-B D11 premise | The line scan came in with `5ff3b0a39` and `638a61a6b`, which no Pitcrew or Pitwall tag contains (`git tag --contains`); the shipped versions use `stale_registration`. The claims of a shipped bug, the 1.0.x option and the release-notes note are gone; D11 restated |
| P2-a | §2: Rust-tool rows are scoped to the Linux image's mode |
| P2-b | §2: what a nextest bump costs; tag gating: WP-1 runs Debrief's gates before landing |
| P2-c | `client.rs` and `timings.rs` are named as one unit (the total stays 54) |
| P2-d | §4 and §6: `--verify` covers host tools only |
| P2-e | §4: C104 lists files with `git ls-files`, unlike RT-5's file-system `Tree` |
| P2-f | §4: the c104 list's first filling follows ADR 0088 §5's seed rules |

## Decisions for the user

| # | Decision | Options | Recommended |
|---|---|---|---|
| D1 | Where provenance lives | (a) a header in every fixture; (b) one register `docs/captured-output.toml` (an ADR 0088 Register), a first-line note optional for logs; (c) a README per fixture folder | **(b)**: it works for JSON, binary and inline strings, and C104 can check it |
| D2 | What a reader must have | (a) captured fixtures only; (b) a whole `captured` fixture per declared mode (OS major, VM or metal, language), cuts declared in `trimmed`, `invented` allowed beside it for hostile and bounds cases; (c) any labelled fixture | **(b)** |
| D3 | How and where captures run | (a) a script per tool beside its fixtures; (b) `./dev capture` in the CI image or with the pinned binary for host tools, on a leased VM for OS tools, on the user's Mac for metal-only tools; scratch first, `--accept` after a read diff; (c) all by hand | **(b)** |
| D4 | The check | (a) C104 in the RT-5 host over tracked files, today's gaps as `docs/ratchets/c104.toml`, new readers found in review; (b) also a heuristic for new readers; (c) review only | **(a)**; (b) later if review misses one |
| D5 | Versions | (a) a captured row equals the pin in `ci/tool-pins.toml`; unpinned Rust tools are tied to `ci-image/VERSION` (re-capture when the image changes; Linux image mode only, since the macOS and Windows runners take moving `stable`); (b) as (a), and also pin Rust with a `rust-toolchain.toml` (a family change, Renovate-bumped); (c) a notice on mismatch only | **(a)**: it needs no toolchain policy change; (b) is a separate decision the family can take later |
| D6 | Known labels | (a) declared per reader, each `seen`, `source` or `unverified`, checked by C104; (b) parse the tool's source; (c) none | **(a)**, only for readers that match a closed set of words |
| D7 | Scope | (a) programs Paddock runs, services it calls, and CI logs; Debrief's import formats stay under its samples policy; (b) include the import formats | **(a)** |
| D8 | Captures on the user's Mac | (a) never: VM and container only, metal-only modes stay `invented` in the list; (b) allowed for metal-only and user-state tools, run by the user or by an agent with the user's approval per capture, allowlist sanitiser, scratch first, diff read before `--accept`, `by` recorded; plus the local denylist scan when the file exists | **(b)** |
| D9 | Engineering rule | (a) amend ER-14 only; (b) a new **ER-37** "Code that reads another program's output is tested against that program's whole output, captured at the pinned version on a test machine with values replaced, recording tool, version, command and who captured it; invent only hostile and bounds cases", plus one sentence in ER-14: "same-shape" means a real capture with its values replaced, not a shape reasoned from memory | **(b)** (ER-34 and ER-35 are reserved) |
| D10 | Tag gating | (a) none: the list only shrinks, and closed-label readers go first as each app is next touched; (b) each app's readers captured before its next tag | **(a)** |
| D11 | Pitcrew's unshipped `launchctl print` line scan (`service_repair.rs:104-107` on the train and `origin/main`, from `5ff3b0a39`; in no Pitcrew tag): on macOS 26 it reads a running job as not running | (a) fix the copy on main now: about 5 lines (read the job's own level, as `paddock-service` does since `3024f2938`), with a failing test fed by `crates/paddock-service/tests/fixtures/launchctl-print-running.txt` first; about 0.25 days on its own branch; (b) leave it until `pitcrew-v1.1.0` deletes the copy, with a release check that no Pitcrew tag is cut while the copy is present | **(a)**: it is cheap, it keeps any Pitcrew tag before 1.1.0 from shipping the bug, and rule 55 asks for a copy to be fixed with its canonical |

## Work packages (raw agent-days, one agent)

| WP | Content | Depends on | Days |
|---|---|---|---|
| WP-0 | RT-8 on its branch (its nextest logs are already at the 0.9.149 pin and its clippy log is a capture): capture a `SIG*` ending and record the provenance of `timings/*.json`; a failing test first where a reading changes | none | 0.25 |
| WP-1 | `ci/tool-pins.toml` with `./dev pins get`, host-tools and `verify.sh` reading it; the `docs/captured-output.toml` schema; C104 with the §6 controls; `docs/ratchets/c104.toml` and its `docs/ratchets.toml` declaration | RT-5 landed; ADR 0088 WP-2, WP-3 | 2.5 |
| WP-2 | `./dev capture` (`--accept`, `--verify`) in the CI image or with the pinned binary, scratch projects for nextest, Playwright, vitest and clippy, the allowlist sanitiser, the local denylist scan | WP-1 | 2 |
| WP-3 | Seed the register from a structural sweep (every spawn, every Gitea client, every vm-smoke file, the JS log readers), honest `kind` and `trimmed` per fixture, the c104 list | WP-1; ADR 0089 WP-2c for `runlog` | 1 |
| WP-4 | First burn-down of closed-label readers: Dyno agent `launchctl print` (by level), `pmset` and `powercfg` (`extras/sleep.rs`, `parse/powercfg.rs`, `wol/readiness.rs`), `wevtutil`, `sc` and `icacls` in two display languages on VMs; `smartctl` on metal (D8); ffprobe | WP-2, WP-3 | 3.5 |
| WP-5 | ER-37 and the ER-14 sentence (D9), the Paddock rule line, lessons entry, `rules-review` checklist and RT brief template lines, Renovate's lock-PR step | acceptance | 0.5 |

First cut (WP-0, WP-1, WP-3, WP-5): 4.25 days. All: 9.75 days. D11 (a) is 0.25 days more, on its own branch. The rest of
the list shrinks as apps are touched.

Tag gating: none (D10). WP-0 lands with RT-8. WP-1 to WP-3 change `tools/paddock-dev`, `ci/`, `ci-image/verify.sh`,
`.gitea/actions/host-tools`, `docs/` and fixtures. `host-tools` runs in Debrief's tag workflow
(`.gitea/workflows/debrief.yml:194`, `:296`), so it is on a release path (ER-31 (b)): WP-1 runs Debrief's gates on its
change before it lands.
