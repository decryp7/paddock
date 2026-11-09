# 73. No test double in a shipped build: one gate, one check per app, a dated register

- Status: Proposed (2026-11-09). Not yet independently reviewed; the author does not review it (ER-23 practice, the
  recurring-bug-adr skill, step 6). Nothing in "Work packages" starts before the user accepts it (step 7).
- Date: 2026-11-09
- Trigger: the independent review of 2026-11-09 found that Pitwall's released sidecar `pitwall-server` ships a fake LLM
  engine behind a command-line flag. Dyno's ADR 0033 had fixed the same shape in Dyno alone the day before.
- Related:
  - Engineering rules: ER-21 / rule 40 (a recurring bug means a missing design rule), ER-22 / rule 42 (share first),
    ER-27 / rule 56 (logs explain every failure), ER-33 / rule 71 (a check counts only if its probe can see), ER-20 /
    rule 51 (parallelism and cost), ER-11 / rule 5 (failing test first), rule 48 (quoted numbers are recorded), rule 54
    (an installer build before a release-path change lands), rule 55 / ADR 0034 (one implementation per mechanism).
  - Family ADRs: 0022 (the Rust developer and CI tool, `./dev`), 0037 (the app composition contract), 0038 (CI parity
    before push), 0039 (release gates inspect the artifacts, `release.toml`), 0049 (feature switches are declared; a test
    switch is not a feature switch).
  - Dyno ADR 0033 (no test fake in a release build), accepted 2026-11-08, implemented on `fix/dyno-no-fakes-in-release`
    (tip `5f6aadd4c`) and carried into `feat/dyno-health-not-checked` (tip `176b5972b`). This ADR generalises its §1 to
    §3 to the family and leaves its §4 and §5 (Dyno's Windows readers and Health) to Dyno.

All paths and lines below are at `origin/main` `b51c72509` unless a branch is named.

## Context

### The incidents

1. **Dyno, every release up to 1.1.0** (Dyno ADR 0033, Context). `windows::platform()` filled `locks`, `startup` and
   `facts` from `fake::Fake` through a struct-update spread (`apps/dyno/crates/dyno-platform/src/windows/mod.rs:23-38`,
   `..f`). Windows users saw "No startup items", a Health score of 100 and an OS named "Fake OS 1.0". The fake answered
   in the shape of real data, so neither the page nor any test could tell "nothing there" from "never read".
2. **Pitwall, released and on the 1.1 line.** `pitwall-server` is the app's only bundled sidecar
   (`apps/pitwall/app/src-tauri/tauri.conf.json:36`, `"externalBin": ["binaries/pitwall-server"]`). Its library declares
   `pub mod fake_engine;` with no gate (`apps/pitwall/crates/pitwall-server/src/lib.rs:14`), and its `main` accepts
   `--fake-engine` and `--die-after-ms` in every build (`src/main.rs:20-23`). With `--fake-engine` the shipped binary
   serves canned OpenAI-compatible answers instead of starting the real server (`src/main.rs:40-46`). The flag exists
   because the integration tests start the real binary as a child engine (`tests/common/mod.rs:440-463`).
   `fix/pitwall-e2e-engine-health` (tip `7a73cdc59`) adds a third test-only flag, `--ready-when <path>`, to the same
   binary (`src/main.rs:24-30` on that branch). The same lines are on `pitwall/1.1-b1b-b6` (`e20ae50c3`).

Both are one pattern at different depths: in Dyno a test double was **wired** into production; in Pitwall it is
**reachable** from production by an argument. In both, nothing in the build or in CI could tell a shipped binary from a
test binary.

### The pattern (ER-21)

**Test doubles, test-only modules and test-only switches are compiled into shipped binaries whenever they are declared
in a crate's normal source without a test gate. Once compiled in, a spread, a fallback, a re-export or a flag can reach
them, and no test notices, because the tests use the same doubles on purpose.**

The invariant: *a build that ships contains no test double, no test-only module and no test-only switch. The only
in-memory or no-op implementations it contains are deliberate production adapters with production names, listed in a
register.*

Dyno's ADR 0033 made the invariant true for Dyno's own crates and checked it there. It left out the family crates
(`no_ungated_doubles.rs` header, "Family crates are out of scope", and ADR 0033 review P2-4) and every other app.

### The survey: every instance today

**Method.**
- I generalised Dyno's `syn` declaration scan (`apps/dyno/crates/dyno-platform/tests/no_ungated_doubles.rs` on
  `feat/dyno-health-not-checked`, the same `eval_cfg` and `gated` rules) into a read-only survey tool. The tool lived in
  this ADR's scratch folder and is not committed (WP-0 builds the real check).
- It parses every lib, bin and proc-macro target of every package in each Cargo workspace (`crates/`, each
  `apps/<app>`, the demos' `src-tauri`). It follows `mod` declarations and `#[path]`, and skips anything gated by
  `cfg(test)`, `#[test]` or a `cfg` that is false when `test` and `feature = "test-support"` are off.
- It reports these items when they are not gated:
  - public items named `Fake*`, `Mock*`, `Stub*`, `Scripted*` or `Dummy*`;
  - modules named `fake`, `fakes`, `mock`, `mocks`, `testing`, `test_support` or `fake_*`;
  - trait impls for such types;
  - public functions named `fake*`, `mock*`, `stub*`, `test_*` or `*_for_test(s)`;
  - string literals that look like test switches (`--` flags or environment names containing FAKE, MOCK, TEST or E2E);
  - use sites of the doubles.
- An advisory pass lists public items whose doc comment says "for tests". It found 59 hits, triaged by hand below; most
  are ports or real code whose docs mention tests.
- Shipped sets come from each app's `mainBinaryName` plus every `bundle.externalBin` in its `tauri*.conf.json`.
- For each of the 18 shipped packages on main, I ran `cargo tree -p <pkg> -e normal --target all -f '{p} {f}'` to see
  which crates and which features ship.
- Branches surveyed as well: `pitwall/1.1-b1b-b6` (`e20ae50c3`), `fix/pitwall-e2e-engine-health` (`7a73cdc59`),
  `debrief/1.2-batch1`, `pacenotes/stage1-land` (`8576ce25c`), `sighter/sw3c-vnc-2` and `feat/dyno-health-not-checked`
  (`176b5972b`). Timings: [benchmarks.md, 2026-11-09](../benchmarks.md#2026-11-09-test-double-survey-scan-and-shipped-feature-trees).

**Classes.**

| Class | Meaning |
| --- | --- |
| **B** | Broken: wired into, or selectable by an argument or fallback of, a shipped binary. |
| **R** | At risk: compiled into a shipped binary, not wired today; any spread, re-export or flag can reach it. |
| **G** | Gated by a feature other than `test-support`. That feature is not enabled on any normal edge today (measured), so it does not ship, but the name differs. |
| **N** | Not shipped: a test-only crate or binary that no shipped package builds; listed so the check can enumerate it. |
| **E** | Deliberate: a production in-memory adapter, a demo adapter or a guard. These are register candidates (§4). |

**Measured fact for the feature graph.** No shipped package on main enables `test-support`, `testing`, `test-hooks`,
`testdb` or `mock-runtime` on any normal edge. The same holds on `pitwall/1.1-b1b-b6`. The 18 packages measured:

- Dyno: 6 (`dyno-app`, `-agent`, `-cli`, `-bench`, `-bench-gpu`, `-helper`);
- Debrief: 4 (`debrief-app`, `-ai-worker`, `-plugins`, `-store-worker`);
- Pitwall: 2; Pitcrew: 2; Spotter: 1;
- `shell-demo`: 2; `workbench-demo`: 1.

The test-only crates (`paddock-testkit`, `paddock-plugin-test`, `debrief-plugin-test`) are in none of the 18 trees. So today the leak is in **declarations**, not in features. That is why the declaration layer of the check
is the one that fails first.

#### Pitwall (release first: 1.1)

| # | Where | Instance | Class |
| --- | --- | --- | --- |
| P1 | `crates/pitwall-server/src/lib.rs:14` | `pub mod fake_engine` (a fake OpenAI-compatible engine) in the shipped sidecar's library | B |
| P2 | `crates/pitwall-server/src/main.rs:21` | `--fake-engine`: the shipped binary becomes the fake | B |
| P3 | `crates/pitwall-server/src/main.rs:22` | `--die-after-ms`: the fake exits on a timer | B |
| P4 | `crates/pitwall-server/src/main.rs:24` on `fix/pitwall-e2e-engine-health` | `--ready-when <path>` (`fake_engine::parse_ready_when`) | B (branch) |
| P5 | `app/src-tauri/src/server_client.rs:274` (`:288` on 1.1) | `pub mod fake` with `FakeServer` (`:290`) and `impl ServerClient for FakeServer` (`:330`), in `pitwall-app` | R |
| P6 | `app/src-tauri/src/credentials.rs:118` | `StaticKeys` ("A fixed key, for tests"); the name rules miss it | R |
| P7 | the 1.1 graph on `pitwall/1.1-b1b-b6` | `pitwall-app` and `pitwall-server` link 9 of the family crates with ungated doubles below (`paddock-service`, `-jobs`, `-health`, `-channels`, `-net`, `-watch`, `-commands`, `-mcp`, `-secrets`). On main Pitwall links only `pitwall-core` and `pitwall-loopback`. | R (1.1) |

Pitwall counts: **4 B and 2 R** in its own crates, plus **9 shared crates** on its 1.1 line.

#### Debrief (1.2; `debrief/1.2-batch1` scans the same as main)

| # | Where | Instance | Class |
| --- | --- | --- | --- |
| D1 | `crates/debrief-ai/src/llm/mod.rs:3` | `pub mod fake`: `ScriptedProvider` (`fake.rs:20`, `impl LlmProvider` `:64`), re-exported at `mod.rs:12` | R |
| D2 | `crates/debrief-ai/src/engine/pitwall/mod.rs:9` | `pub mod fake`: `FakePitwall` (`fake.rs:84`), a fake Pitwall server | R |
| D3 | `crates/debrief-ai/src/lib.rs:29` | `pub mod testing` (synthetic GGUF builders) | R |
| D4 | `crates/debrief-ai/src/vision/image.rs:246` | `pub mod testing` (test images) | R |
| D5 | `crates/debrief-dump/src/lib.rs:31` | `#[doc(hidden)] pub mod testing` ("Small documents for tests") | R |
| D6 | `crates/debrief-symbols/src/lib.rs:28-30` | `pub mod testing`: `FakeServer` (`testing.rs:10`). It is gated only by `feature = "server"`, which is a default and is on in `debrief-app` and `debrief-plugins` (measured). | R |
| D7 | `crates/chromium-driver/src/discover.rs:97,104` | `find_for_tests` and `CHROMIUM_TEST_PATH` | R |
| D8 | `crates/chromium-driver/src/lib.rs:30` | `DEBRIEF_FAKE_BROWSER`: a guard that makes the app refuse to run as a fake browser. It must ship. | E |
| D9 | `crates/debrief-ai/Cargo.toml:13` | `[[bin]] debrief-ai-fake-worker` (`--crash-on-load`, `--crash-on-chat`, `--crash-once`). It is not bundled, because `build-sidecars.mjs:27` selects `--bin`, but any `cargo build -p debrief-ai` builds it: there is no `required-features`. | N |
| D10 | `crates/debrief-plugin-host/Cargo.toml:12` | `[[bin]] debrief-plugin-host-fake`, the same shape | N |
| D11 | `crates/debrief-plugin-test/src/host.rs:46` | `FakeHost` in a test-only crate | N |

Debrief counts: **7 R, 1 E and 3 N**. Four of the seven R are in `debrief-ai`, which Debrief 1.2 is to remove in favour of
Pitwall (D8 below).

#### Dyno (fixed by its ADR 0033)

| # | Where (main) | Instance | Class on main | On `feat/dyno-health-not-checked` |
| --- | --- | --- | --- | --- |
| Y1 | `dyno-platform/src/windows/mod.rs:24-25` | `..crate::fake::Fake::new().platform()` | B | gone (every port named) |
| Y2 | `dyno-platform/src/hardware.rs:62` | `FakeBattery` as the answer on other OSes | B | gone (`NotReadHere`) |
| Y3 | `dyno-platform/src/lib.rs:17`, `fake.rs` | `pub mod fake`: `Fake`, `FakeOp`, `FakeState`, `FakePowerActions`, `FakeIdleSignals` | R | gated |
| Y4 | `dyno-platform/src/counters/mod.rs:63`; `hardware.rs:560,573,582`; `wol/mod.rs:56,134` | `FakeCounters`, `FakeThrottle`, `FakeWear`, `FakeBattery`, `FakeWakeTimer`, `WolPorts::fake` | R | gated |
| Y5 | `dyno-core/src/wol/mod.rs:8`, `wol/fake.rs` | `pub mod fake` with 7 `Fake*` types | R | gated |
| Y6 | `dyno-agent/src/build_id.rs:136` | `#[doc(hidden)] pub fn set_watch_every_for_tests` | R | still R |
| Y7 | `dyno-agent/Cargo.toml:74`, `geo_db.rs:325` | feature `testdb` gating `mod testdb` (fuzz corpus writer, one example) | G | still G |

Dyno counts on main: **2 B and 3 R groups** (17 types and one constructor). On the branch: **1 R and 1 G** remain. Dyno also
links 8 shared crates with doubles: `paddock-service` (the privileged `dyno-helper` included), `-jobs`, `-health`,
`-channels`, `-net`, `-watch`, `-commands` and `-power`. The branch gates `paddock-power`.

#### Pitcrew, Spotter, the demos, Pacenotes, Sighter

| # | Where | Instance | Class |
| --- | --- | --- | --- |
| C1 | `apps/pitcrew/crates/pitcrew-service/src/net.rs:26` | `Http::for_tests()`: lifts the HTTPS-only rule to allow loopback `http://`, in the shipped service. Only `cfg(test)` code calls it (`work.rs:258`, `net.rs:204`). | R |
| C2 | `apps/pitcrew/crates/notify-channels/src/model.rs:49-51` | `MemorySecrets` ("for tests"), re-exported at `lib.rs:17`, in `pitcrew-app` | R |
| S1 | `apps/spotter/crates/spotter-render/src/discover.rs:131-150` | `SPOTTER_TEST_BROWSER`, `SPOTTER_TEST_BROWSER_DIR`, `test_browser_dir`, `find_for_tests` | R |
| S2 | `apps/spotter/crates/spotter-render/src/discover.rs:171,185-198` | `SPOTTER_TEST_BROWSER_NO_SANDBOX` → `--no-sandbox` in `test_browser_flags(_with)`. Only `tests/real_browser.rs` calls it today (read). | R |
| S3 | `apps/spotter/crates/spotter-engine/src/secrets.rs:19-21` | `MemorySecrets` ("tests use") | R |
| S4 | `apps/spotter/crates/spotter-engine/src/clock.rs:151`, `notifier.rs:72` | `SequenceRandom`, `RecordingNotifier` ("for tests") | R |
| X1 | `apps/shell-demo/src-tauri/src/lib.rs:27,182` | `FakeToaster` wired as the demo's toaster | E (wired on purpose) |
| X2 | `apps/shell-demo/src-tauri/src/lib.rs:135` | `paddock_service::testing::FakeSystem` wired as the demo's service backend | E (wired on purpose) |
| X3 | `apps/workbench-demo/src-tauri/src/lib.rs:97` | `paddock_secrets::MemorySecrets` (Dyno ADR 0033 D2b: a legitimate ephemeral store) | E |
| N1 | `pacenotes/stage1-land`: `pacenotes-core/src/lib.rs:20`, `pacenotes-function-api`, `pacenotes-worker` | the right pattern under the names `testing` and `test-hooks`, enabled from `[dev-dependencies]` only | G (3) |
| — | `sighter/sw3c-vnc-2` | nothing found | — |

Counts: **Pitcrew 2 R. Spotter 4 R** (S1 to S4; S1 is one group of four items). **shell-demo 2 E. workbench-demo 1 E.
Pacenotes 3 G. Sighter 0.**

#### Shared crates (`crates/`)

| # | Crate and place | Doubles, ungated | Shipped by (normal graph) |
| --- | --- | --- | --- |
| F1 | `paddock-service`: `lib.rs:63` `pub mod fake_systemd`, `lib.rs:88` `pub mod testing`, `privileged/mod.rs:24` `pub mod trust_fake` | `FakeSystemd`, `FakeSystem`, `FakeAppService`, `FakeElevator`, `FakeFiles`, and `FakePort` (a fake machine for the privileged `TrustPort`) | `dyno-app`, `dyno-agent`, `dyno-helper`, both demos, Pitwall 1.1 (app and server) |
| F2 | `paddock-jobs`: `time.rs:121`, re-exported at `lib.rs:53` | `FakeClock`, re-exported again by `paddock-log` (`clock.rs:4`, `lib.rs:51`), `paddock-health` (`lib.rs:52`) and `paddock-testkit` | Dyno (all 6), demos, Pitwall 1.1 |
| F3 | `paddock-health`: `notify/os/mod.rs:24,35` | `mod testing` with `pub use testing::FakeToaster` | Dyno, demos, Pitwall 1.1 |
| F4 | `paddock-channels`: `lib.rs:23,40`; `control.rs:55` | `FakePoster`, `FakeMail`, `MemorySettings` ("for tests") | `dyno-app`, demos, Pitwall 1.1 |
| F5 | `paddock-net`: `lib.rs:21,31` | `FakeWire`, `StaticLookup`, `Reply`, `Seen` | Dyno, demos, Pitwall 1.1 |
| F6 | `paddock-watch`: `source.rs:122`, `lib.rs:31` | `FakeSource`, re-exported by `paddock-packs` (`watch/mod.rs:10`) | Dyno, demos, Pitwall 1.1 |
| F7 | `paddock-commands`: `lib.rs:62` | `pub mod testing` (assertions over an app's commands) | Dyno, demos, Pitwall 1.1 |
| F8 | `paddock-power`: `lib.rs:15,22` | `FakePower`, `FakeEvents` (gated on Dyno's branch, ADR 0033 WP-2) | Dyno |
| F9 | `paddock-mcp`: `control.rs:84`, `key_list.rs:232` | `MemorySettings`, `MemoryKeys` ("for tests") | Pitwall 1.1 |
| F10 | `paddock-secrets`: `memory.rs:11` (and `:59`) | `MemoryVault` ("for tests"); `MemorySecrets` is E (X3) | Dyno via `WolPorts`, Pitwall 1.1, `workbench-demo` |
| F11 | `paddock-chromium`: `discover.rs:10,174`; `lib.rs:31` | `find_for_tests`, `CHROMIUM_TEST_PATH`; `PADDOCK_FAKE_BROWSER` is a guard (E) | no shipped graph today |
| G1 to G4 | `paddock-pitwall` `lib.rs:39` and `paddock-siblings` `lib.rs:47` (`testing`); `paddock-shell` `lib.rs:43`, `paddock-dialogs` and `paddock-panels` (`mock-runtime`) | gated, under other feature names | no test feature on a normal edge (measured) |
| N2 | `paddock-testkit` (`FakeRunner`), `paddock-plugin-test` (`FakeHost`, also used by the dev tool `cargo-paddock-plugin`) | test-only crates | in no shipped graph (measured) |

Shared counts: **10 crates with ungated doubles** (F1 to F10, of which F8 is fixed on Dyno's branch), 1 latent (F11), 5
crates gated under other names, and 2 test-only crates.

**False positives of the name rules** (they size the check's rules; §2.2 drops the `test_*` prefix because of them):

- Spotter `Engine::test_source`, `test_source_job` and `test_pattern`, and `commands.rs:116,223`: the product's "Test"
  actions;
- Dyno `IpLookupSettings::test_url` (the Test button) and `dyno wake --test`;
- Debrief `runtimes/selftest.rs:28` `test_image`;
- Debrief `dbghelp.rs:20` `fake_handle`: a real DbgHelp session handle, `pub(crate)`. It is the one remaining false
  positive of the kept rules; WP-D renames it.
- The `--self-test` probes (`paddock-proc/src/self_test.rs:23`, `debrief-store-worker`, `dyno-helper`) are release probes
  (ADR 0039 `probe = ["--self-test"]`) and must ship. They are E.

**Totals:**

- 6 B: Dyno 2 on main, fixed on the branch; Pitwall 4, one of them on a branch.
- 30 R: Pitwall 2, Debrief 7, Dyno 4 (3 groups plus Y6), Pitcrew 2, Spotter 4, and the shared crates 11 (10 plus 1
  latent, counted per crate).
- 6 E: D8, X1 to X3, the `PADDOCK_FAKE_BROWSER` guard and the `--self-test` probes.
- 9 G: Y7, Pacenotes 3 and 5 shared crates.
- 5 N: D9 to D11 and the 2 shared test-only crates.

### The UI has the same shape (read, not built)

Every app's Angular frontend selects a `MockBackend` at run time when `__TAURI_INTERNALS__` is missing:

- Dyno: `apps/dyno/app/src/app/core/backend-factory.ts:4,17,23`;
- Pitcrew: `apps/pitcrew/app/src/app/app.config.ts:5,22,40`;
- Pitwall: `apps/pitwall/app/src/app/app.config.ts:6,46`;
- Spotter: `apps/spotter/app/src/app/app.config.ts:5,17,27`;
- Debrief: `apps/debrief/app/src/app/core/backend.ts:299`, through a chunk that `mock-registry.ts:10` imports lazily.

The release bundle therefore carries the mock. A release window that lost its Tauri bridge would show plausible demo data
instead of an error, the same failure as "Fake OS 1.0". I read this in the code; I neither built a dist nor saw it happen
in a release. §5 handles it as phase 2.

## Decision

### 1. The rule (proposed rule 73) and the one mechanism

Proposed text for `docs/rules.md`, under a new heading "Shipped builds". The number is the next free one; rule 72 is
taken by family ADR 0057's branch.

> **73. No test double ships.** A build that ships contains no test double, test-only module or test-only switch:
> (a) a double inside a crate sits behind `#[cfg(any(test, feature = "test-support"))]`, and `test-support` is enabled
> only from `[dev-dependencies]` (a crate's own integration tests use a self dev-dependency); (b) a test program that must
> be its own process is its own `[[bin]]` with `required-features = ["test-support"]`, or lives in a test-only crate,
> never behind a flag of a shipped binary; (c) a composition names every port, with no struct-update spread of a port
> bundle, and an OS or build that cannot read a port gets an honest adapter that says so; (d) an in-memory or no-op
> implementation that production uses on purpose has a production name and an entry in `docs/test-doubles.toml`.
> `./dev check test-doubles` enforces it for every app in CI. *Why:* Dyno shipped a fake platform to Windows users in every
> release up to 1.1.0, and Pitwall's released sidecar turns into a fake engine with one flag.

The mechanism, in detail:

1. **One feature name: `test-support`.** Every crate that holds doubles has it. A crate's `test-support` enables the
   `test-support` of the crates whose doubles it re-exports or builds on, as Dyno's `dyno-platform` does for `dyno-core`
   and `paddock-power`. The other names that exist today are handled in D3:
   - `testing`: `paddock-pitwall`, `paddock-siblings`, `pacenotes-core`, `pacenotes-function-api`;
   - `test-hooks`: `pacenotes-worker`;
   - `testdb`: `dyno-agent`;
   - `mock-runtime`: `paddock-shell`, `paddock-dialogs`, `paddock-panels`.
2. **Test programs are separate binaries.** Pitwall's fake engine becomes `pitwall-fake-engine`, a `[[bin]]` with
   `required-features = ["test-support"]` that takes `--port`, `--die-after-ms` and `--ready-when` (D2). The tests start
   it through `CARGO_BIN_EXE_pitwall-fake-engine`. `pitwall-server` loses all three flags, so no build of the shipped
   binary has them. Debrief's two fake binaries (D9, D10) get the same `required-features`.
   *Not verified:* that `CARGO_BIN_EXE_<name>` is set for a bin whose required feature comes from a self dev-dependency.
   WP-0's fixture proves it, or the tests' crate enables the feature explicitly.
3. **Compositions name every port.** This is Dyno ADR 0033 §1.3, family-wide, and fits ADR 0037's `compose`. `NotReadHere`
   stays in Dyno: it is one implementation, below the rule of three.
4. **Production in-memory adapters are not doubles.** They get a production name and doc. `MemorySecrets` keeps its name
   by the user's decision D2b of 2026-11-08 and gets a register entry (§4).
5. **The non-test build is the proof for use sites.** Once a double is gated, every production reference to it is a
   compile error in a build without test targets, on the OS that code compiles on. Any path, alias, re-export or flag
   arm counts. The check therefore runs that build on every OS leg (§2.3).

### 2. One shared check, run per app in CI: `./dev check test-doubles --app <app>`

The check is written in Rust in `tools/paddock-dev` (ADR 0022: generic checks move to Rust). It reuses
`paddock-repo-graph`'s `cargo-metadata` support. It is generalised from Dyno's three pieces:

- the `syn` scan with `cfg` evaluation (`no_ungated_doubles.rs`, commits `e8547c0c2` and `37e1f1466`);
- the shipped feature graph with a positive control (`scripts/check-shipped.mjs`: `shippedPackages` `:25`,
  `testSupportEnabledIn` `:67`, `assertProbeSees` `:91`);
- `hostRustChecks` (commits `259bec2a6` and `9bdc26f3b`: `.gitea/actions/app-gates/action.yml` "Host Rust checks",
  `scripts/affected.mjs` `host_rust_checks`).

1. **The shipped set is derived, never hand-kept.**
   - It is the app's `mainBinaryName` plus every `bundle.externalBin` in all its `tauri*.conf.json`. Debrief's sidecars
     are in `tauri.sidecars.conf.json:7`, not in `tauri.conf.json`.
   - Each name maps to the package that builds that binary, through `cargo metadata`. A name that no package builds fails.
   - When the app's `release.toml` (ADR 0039 §1) exists, its `[[executable]]` list is read too, and the two sets must
     be equal.
   - The scope is the normal-dependency closure of the shipped set over **path** packages. It includes the family
     crates under `crates/`, which closes Dyno ADR 0033's review P2-4.
2. **Layer 1, declarations.** The scan uses Dyno's `syn` rules and its `cfg` evaluation (`any`, `all`, `not`; `test` and
   `test-support` off; every other predicate "maybe"). It reports these when not gated:
   - a public item named `Fake*`, `Mock*`, `Stub*`, `Scripted*` or `Dummy*` (Dyno: `Fake*` only);
   - a module `fake`, `fakes`, `mock`, `mocks`, `testing`, `test_support` or `fake_*`;
   - a trait impl for such a type;
   - **new:** a non-private function or method named `fake*`, `mock*`, `stub*` or `*_for_test(s)`. This catches
     `WolPorts::fake`, `Http::for_tests`, `find_for_tests` and `set_watch_every_for_tests`. `test_*` is left out because
     of the false positives above.
   - **new:** a `[[bin]]` of an in-scope package whose name or path contains `fake`, `mock` or `stub`, without
     `required-features` that include `test-support`. This catches D9 and D10.
   - **new:** an item gated only by a feature from the register's list of test features (D3) that a normal edge
     enables. This links layer 1 to layer 2.

   Items in `docs/test-doubles.toml` are allowed until their date (§4). Doubles with production-sounding names (C2, P6,
   S3, S4, F4, F9, F10) are not found by name. The migration gates them, and the doc-comment pass stays an advisory list
   in the report, not a failure (its 59 hits were mostly ports).
3. **Layer 2, the shipped feature graph.**
   - The check runs one `cargo tree -e normal --target all -f '{p} {f}'` for the shipped packages of each build
     invocation. Dyno's sidecars build in one `cargo build -p ... -p ...`, so their features unify there; one call per
     invocation sees exactly that union.
   - It fails when any crate in that graph has `test-support` (or a registered test feature) enabled.
   - It also fails when a crate marked `[package.metadata.paddock] test-only = true` is in the graph. These are
     `paddock-testkit`, `paddock-plugin-test`, `debrief-plugin-test` and any later one. (`paddock-testplan` is a product
     crate, the test-plan format, not a test-only one.)
4. **Layer 3, a non-test build on every OS leg.** `cargo check --locked --lib --bins -p <shipped packages that link the
   gated crates>` runs on the Linux leg and, through the generalised `hostRustChecks`, on the Windows and macOS legs of
   every app that has them.
   - `app-gates` derives the package list from the shipped set, so an app needs no contract line it could forget.
   - Clippy `--all-targets` and nextest unify `test-support` in, so they never see the shipped build (Dyno ADR 0033 §3,
     "Rejected").
5. **Positive controls (ER-33).** A clean result counts only if each probe showed it can see.
   - **Fixture workspace.** The check first runs on `conformance/fixtures/test-doubles/`, a two-crate workspace with one
     double of each kind, ungated and gated, a fake `[[bin]]` without `required-features`, and a dev-dependency that
     enables `test-support`. It must report exactly the expected offences. Through the dev edges it must see
     `test-support`, and through the normal edges it must not. Otherwise it reports `check broken` and fails.
     This works for an app with no doubles at all, where Dyno's control (dyno-agent's dev edges) has nothing to see.
   - **Coverage of the tree.** The scan must parse at least 90 % of the `.rs` files under the in-scope crates' `src/`.
     The unreached files are listed, since a file no `mod` reaches is not compiled. Otherwise: `check broken`. This
     replaces Dyno's hand-kept `MIN_FILES = 100`.
   - **Per app.** If the register names gated doubles for the app, the scan must have met each of them gated.
6. **Output (ER-27).**
   - One `::error::` per offence, with `file:line`, the rule and the fix. For example: "gate it with
     `#[cfg(any(test, feature = "test-support"))]` and enable `test-support` from `[dev-dependencies]`".
   - A summary line: packages, crates and files scanned, the register entries applied with their dates, and each
     control's result.
   - `check broken` names the control that failed and its next step.
   - No source text and no user data appear in the output.
7. **Where it runs.**
   - In `app-gates` on the Linux leg for every app with a Cargo workspace. A conformance test asserts that every app
     with `cargo` in `paddock.ci.json` is covered.
   - In the nightly for all apps.
   - In `./dev check --pre-push` (ADR 0038) when the plan reaches an app's Rust half.
   - Layer 3 runs on every OS leg, as Dyno's D10 decided for Dyno.
   - A shared-crate change runs the check for every app whose shipped graph contains that crate, through the planner's
     Cargo graph, as for any test.
8. **Layer 4, the artifacts (D6).** ADR 0039's `release.toml` gains `forbidden_strings`.
   - The family list holds `--fake-`, `--mock-`, `--ready-when` and `--die-after-`; an app adds its own test switch
     names (`SPOTTER_TEST_BROWSER_NO_SANDBOX`).
   - `./dev release verify` searches every extracted executable for them.
   - Its positive control searches the test build of the binary that declares them (for example `pitwall-fake-engine`,
     built by the test job) and must find them.
   - This layer sees a string switch that layer 1's name rules cannot. It runs only in release workflows.
9. **Dyno's own pieces become copies (rule 55).** `no_ungated_doubles.rs` and the `test-support` part of
   `check-shipped.mjs` are entered in `docs/known-copies.toml`, with the shared check as canonical and delete-by
   `dyno-v1.1.2` (or Dyno's next tag after WP-0 lands). Dyno's per-OS no-fake wiring tests and its `Platform wired` log
   line stay: they are Dyno's readable failure for its wiring, not the guarantee.

### 3. What this does not do

- It does not forbid in-memory adapters that production needs; they are named and registered.
- It does not police test code: `tests/`, `benches/`, examples and `cfg(test)` modules may hold anything.
- It does not replace Dyno ADR 0033 §2's per-OS wiring tests. Those stay where a composition has many ports.

### 4. The exception register: `docs/test-doubles.toml`

The register is data, read by the check:

```toml
version = 1
test_features = ["test-support", "mock-runtime", "testdb"]   # D3: the names a normal edge must never enable

[[exception]]
id = "paddock-secrets-memory"                 # unique, kebab case
path = "crates/paddock-secrets/src/memory.rs"
item = "MemorySecrets"
kind = "production-in-memory"                 # | "guard" | "release-probe" | "demo-adapter" | "temporary"
apps = ["workbench-demo", "dyno"]
reason = "An ephemeral store the workbench demo uses on purpose (Dyno ADR 0033 D2b)."
approved = "user 2026-11-08"
review_by = 2027-05-09                        # permanent kinds: re-reviewed, the check warns 30 days ahead and fails after
# remove_by = "pitwall-v1.1.0" | 2026-12-31   # temporary kind only: a tag or a date
```

Its rules mirror rule 55 (d), so a temporary entry cannot become permanent by drift:

- A `temporary` entry fails the check after its date.
- A tag is refused while its app still has a `temporary` entry whose `remove_by` tag it has reached.
- A date moves only with the trailer `Doubles-date: <id>: <old> -> <new>: approved: <coordinator|user>: <reason>`. A move
  to a later release line needs `approved: user`.
- A permanent entry needs `approved: user` and a `review_by` date at most 6 months ahead.
- An entry leaves the register with its code, or with `Doubles-removed: <id>: <reason>`.

Proposed first entries (dates are proposals, D7):

| id | Item | Kind | Date |
| --- | --- | --- | --- |
| `paddock-secrets-memory` | `paddock_secrets::MemorySecrets` (X3) | production-in-memory | review 2027-05-09 |
| `fake-browser-guard` | `PADDOCK_FAKE_BROWSER`, `DEBRIEF_FAKE_BROWSER` (D8, F11): the app refuses to run as a fake browser | guard | review 2027-05-09 |
| `self-test-probe` | `--self-test` of `paddock-proc`, `debrief-store-worker` and `dyno-helper` (ADR 0039 probes) | release-probe | review 2027-05-09 |
| `shell-demo-fakes` | `FakeSystem` and `FakeToaster` wired in `shell-demo` (X1, X2), and the `test-support` features they will need on its normal edges | demo-adapter (D5) | review 2027-05-09 |
| `pitwall-1-1` | P1 to P6 and Pitwall's 9 shared crates (F1 to F7, F9, F10) | temporary | remove by `pitwall-v1.1.0` |
| `debrief-1-2` | D1 to D7, D9, D10 | temporary | remove by `debrief-v1.2.0` |
| `dyno-next` | Y6, and F1 to F7 for Dyno | temporary | remove by Dyno's next tag after WP-S |
| `pitcrew-1-1` | C1, C2 | temporary | remove by `pitcrew-v1.1.0` |
| `spotter-next` | S1 to S4 | temporary | remove by Spotter's next tag |

The check therefore goes on at once, green with the register, and ratchets: no new instance can land, and each existing
one has a date. This is the C13 ratchet the user approved for Debrief on 2026-11-08.

### 5. Phase 2: the UI mock backends (D9)

Each app gets two Angular build configurations:

- The `production` configuration swaps the mock module, through `fileReplacements`, for
  `mock-backend.release.ts`. That module exports a factory that throws: "This build has no mock backend: it runs only
  inside <App>'s window." The release build then fails fast instead of showing demo data.
- The dev and e2e configurations keep the mock.

The check (`./dev check test-doubles --ui`):

- every mock module exports a marker constant `PADDOCK_MOCK_BACKEND = '<app>'`;
- the release `dist` must not contain the marker;
- its positive control: the e2e build must contain it.

The configurations come from a shared preset in `ui/`, as the Angular presets do today. Estimated in WP-11; its timing is
D9.

## Migration per app, ordered by release

1. **Pitwall 1.1 (first; tag window about 2026-12-18 to 2027-01-13).**
   - WP-P: move the fake engine out of `pitwall-server` into `pitwall-fake-engine` (§1.2). Delete P2 to P4 from `main.rs`.
     Gate P5 and P6. Land `fix/pitwall-e2e-engine-health` with `--ready-when` on the fake binary, not on
     `pitwall-server` (D2).
   - WP-S: gate the shared crates Pitwall 1.1 links (D4).
   - The check goes on for Pitwall, and the installers are built before the change lands (rule 54).
2. **Debrief 1.2.**
   - WP-D: rename `fake_handle`. Add `required-features` to D9 and D10. Gate D5 to D7. Gate D1 to D4, or delete them with
     `debrief-ai` if 1.2 removes it first (D8).
3. **Dyno.** Done for its own crates by ADR 0033 (Dyno 1.1.1). WP-Y:
   - gate Y6;
   - register `testdb` (D3);
   - delete the copies (§2.9) when the shared check is on.
   Dyno's share of the shared crates comes with WP-S.
4. **Pitcrew 1.1.** WP-C: gate C1 (`Http::for_tests` under `cfg(any(test, feature = "test-support"))`) and C2.
5. **Spotter, next release.** WP-T: gate S1 to S4. S2 first: it is a sandbox switch.
6. **Pacenotes and Sighter.** WP-N: rename `testing` and `test-hooks` to `test-support` (D3). The check is on from each
   app's first shipped package (the new-app template carries it).
7. **The demos.** `shell-demo-fakes` (D5) and `paddock-secrets-memory` are registered; the demos need no code change
   unless D5 chooses the alternative.
8. **UI mock backends.** Phase 2 (§5, D9).

**Rollback.**
- The check is one `app-gates` step and one register file. Removing the step (integrating agent) turns it off for
  everyone; a dated `temporary` entry turns it off for one instance.
- The gates are per crate and revert per commit.
- The Pitwall fake-engine move reverts as one commit; its tests move with it.

## Parallelism and cost (ER-20, rule 51)

Measurements, from [benchmarks.md, 2026-11-09](../benchmarks.md#2026-11-09-test-double-survey-scan-and-shipped-feature-trees)
and Dyno's `docs/performance.md` on `fix/dyno-no-fakes-in-release`:

- **Declaration scan.** It took 0.10 to 0.82 s per Cargo workspace (Debrief, the largest, 0.81 to 0.82 s; `crates/`
  0.45 to 0.48 s), release build, in two consistent rounds. A third round took 2.7 to 50 s per workspace. That was not
  diagnosed; the likely cause is `cargo metadata` waiting on a shared Cargo lock while other agents built, so the check
  holds no lock longer than one `cargo metadata` call.
  - **More parallelism:** not worth it. Parsing is the cheap part, and the 1-second scale is dominated by `cargo
    metadata`.
  - **Cheaper:** one `cargo metadata` per workspace, shared with layer 2's package mapping.
- **Shipped feature graph.** One `cargo tree` per shipped package took 0.2 to 0.4 s; Dyno's six took 1.3 to 2.7 s in a
  row.
  - **Cheaper:** one call per build invocation (`-p a -p b ...`), which is also the more faithful probe (§2.3).
  - **Parallel:** not needed; separate `cargo tree` processes in one workspace contend for the same lock.
- **Compiling the check.** Dyno's in-crate scan cost 46 s of compile on the Linux leg (Dyno `performance.md`, run 7767).
  In `./dev` the binary is built or fetched once per input hash (ADR 0022), so an app pays nothing per run. This is the
  main saving over copying Dyno's test into each app.
- **Layer 3, the non-test build** (Dyno only, measured).
  - Linux leg: 49.6 s (run 7767).
  - Warm host legs (run 7774): macOS 46 s, about 11 % of the Rust step; Windows 2 min 10 s, about 31 %.
  - Cold: one local `cargo xwin check`, 6 min 24 s.
  - Other apps are not measured. WP-1 measures each app per leg, records it (rule 48), and returns to the user any app
    whose host-leg check adds more than 3 minutes warm (D10).
  - **Cheaper:** only the shipped packages that link a gated crate are checked, not the workspace. The leg's build
    folder already holds most of the graph.
- **Layer 4.** A byte search over each extracted executable, linear in size (tens of MB per app), in release workflows
  only. Not measured.
- **Settings.** No thread-count setting: this is a CI tool with one bounded run per app, not a product operation.

## Work packages

Estimates are raw days of agent work, before review rounds (the family's convention since 2026-11-04: firm once the ADR
has work packages).

| WP | Package | Depends on | Days |
| --- | --- | --- | --- |
| WP-0 | `./dev check test-doubles`, failing fixture first: the fixture workspace with every offence kind and the self-dev-dependency `CARGO_BIN_EXE` case; shipped-set derivation (`tauri*.conf.json`, `release.toml` when present); layer 1 (Dyno's `syn` and `cfg` code moved, plus the function, `[[bin]]` and test-feature rules); layer 2 (one `cargo tree` per build invocation, `test-only` metadata); register parsing with dates and trailers; controls; `::error::` output and summary | – | 2–3 |
| WP-1 | CI (integrating agent, `.gitea/`): `app-gates` runs the check on Linux for every app with a Cargo workspace; layer 3 derived from the shipped set on host legs (generalised `hostRustChecks`); the conformance coverage test; nightly; `./dev check --pre-push`; cost per app and leg measured and recorded | WP-0, Dyno's `hostRustChecks` landed | 1–1.5 |
| WP-2 | `docs/test-doubles.toml` with the first entries (§4); the tag refusal in the release kit; rule 73 text, lessons entry, `docs/adr/README.md` row, wiki notes | WP-0 | 0.5 |
| WP-P | Pitwall: `pitwall-fake-engine` binary with `--die-after-ms` and `--ready-when`, tests moved to `CARGO_BIN_EXE_pitwall-fake-engine`, three flags removed from `pitwall-server`, P5 and P6 gated, the check on, installers built (rule 54) | WP-0 | 1–1.5 |
| WP-S | Shared crates, one change (rules 39, 43): `test-support` gates in `paddock-service` (`testing`, `fake_systemd`, `trust_fake`), `-jobs` (and the re-exports in `-log`, `-health`, `-testkit`), `-health`, `-channels`, `-net`, `-watch` (and `-packs`), `-commands`, `-mcp`, `-secrets` (`MemoryVault`), `-chromium`; self dev-dependencies for their `tests/`; every consumer's dev-dependencies in Dyno, Pitwall, the demos and the tools; CHANGELOG entries; demos registered | WP-0; ADR 0033 WP-2 (`paddock-power`) | 2–3 |
| WP-D | Debrief: `required-features` on the two fake binaries, D5 to D7 gated, `fake_handle` renamed, D1 to D4 gated or deleted (D8), host legs on | WP-0 | 1–1.5 |
| WP-Y | Dyno: Y6 gated, `testdb` registered, its copies deleted, the check on | WP-0, WP-1 | 0.5 |
| WP-C | Pitcrew: C1 and C2 gated, the check on | WP-0 | 0.5 |
| WP-T | Spotter: S1 to S4 gated (S2 first), the check on | WP-0 | 0.5–1 |
| WP-N | Pacenotes and Sighter: feature rename (D3), the check in the new-app template (`templates/`) | WP-0 | 0.25–0.5 |
| WP-4 | Layer 4: `forbidden_strings` in `release.toml` and `./dev release verify`, with the test-build control (D6) | ADR 0039 WP-1, WP-P | 0.5–1 |
| WP-11 | Phase 2, UI: the shared `fileReplacements` preset, `mock-backend.release.ts` per app (5), marker check with the e2e-build control (D9) | WP-0 | 1.5–2 |
| WP-R | Independent review of this ADR and of WP-0's check (fixtures and controls), one round each | – | 0.5–1 |

**Total: 11.75 to 17.5 days raw.** Without phase 2 (WP-11): 10.25 to 15.5. On the Pitwall 1.1 path: WP-0,
WP-1, WP-2, WP-P, WP-S and WP-R, 7 to 10.5 days. That fits the 1.1 tag window if it starts after the Pitwall PoC (about
2026-11-19 to 25), but it is not small against it.

Two agents can run in parallel after WP-0:

- an implementer on WP-P, then WP-D, then WP-C and WP-T;
- the integrating agent on WP-1, then WP-S.

## Decisions for the user

| # | Decision | Recommendation |
| --- | --- | --- |
| D1 | Make "no test double ships" a family rule (proposed rule 73) for every app and shared crate, Rust now, the UI as phase 2 | **Yes.** Two apps shipped the shape within two days of each other, and every app that ships Rust has instances. |
| D2 | Pitwall's fake engine: a separate test-only binary (`pitwall-fake-engine`, `required-features`), or the flags kept in `pitwall-server` behind `cfg(feature = "test-support")` | **Separate binary.** The shipped binary then never has the flags in any build, and the tests no longer depend on a test mode of the product. Land `--ready-when` on the fake binary, not as a new flag of `pitwall-server`. |
| D3 | One feature name: rename `testing` (4 crates) and `test-hooks` (1) to `test-support`; keep `mock-runtime` (a Tauri mock runtime) and `testdb` (a fuzz corpus writer) as registered test features | **Yes.** One name everywhere; the two kept names gate different things and are listed in the register's `test_features`. |
| D4 | Gate the family doubles (WP-S) inside Pitwall 1.1, or register them temporary until `pitwall-v1.1.1` | **Inside 1.1.** Pitwall 1.1 links 9 of them, including `paddock-service`'s privileged `trust_fake`, in a per-machine install. |
| D5 | The demos: register their wired `FakeSystem` and `FakeToaster` as permanent `demo-adapter` exceptions, or add named demo adapters to the shared crates | **Register them.** A demo adapter API in `paddock-service` and `paddock-health` would add shared API for two demos (YAGNI); review in 6 months. |
| D6 | Layer 4 (the artifact string search) now, as part of ADR 0039's verifier | **Yes, after WP-P.** It is small, and it sees switches that name rules cannot. |
| D7 | Turn the check on at once with dated `temporary` entries per app (ratchet), or turn it on per app as each migrates | **Ratchet now.** No new instance can land from day one (C13 precedent). Each app's entry expires at its next tag. |
| D8 | Debrief's `debrief-ai` doubles (D1 to D4): gate them now, or let the removal of Debrief's own AI in 1.2 delete them | **Register them with remove-by `debrief-v1.2.0`, whichever comes first.** Gate them only if the removal leaves 1.2's scope. |
| D9 | Phase 2 (UI mock backends): with each app's next release after Pitwall 1.1, or Pitwall 1.1 included | **Pitwall 1.1 included if WP-11's shared preset is ready by its feature freeze; otherwise each app's next release.** |
| D10 | Layer 3 on host legs on every branch run for every app (Dyno's D10 generalised), with a return to the user above 3 minutes warm per leg | **Yes.** Dyno measured 46 s (macOS) and 2 min 10 s (Windows) warm; other apps are measured in WP-1. |

## Alternatives rejected

- **Clippy `disallowed-types` or `disallowed-methods`.** Clippy lints `--all-targets`, which unifies `test-support` in,
  so it never sees the shipped build (Dyno ADR 0033 §3).
- **A grep for use sites** (`fake::`, `..`). It misses aliases and re-exports, and it cannot tell a data spread from a
  port spread (Dyno ADR 0033 §3). Layer 3's compile is the use-site check.
- **Every double in its own dev-only crate (`*-testkit`).** This is the strongest separation, and layer 2's `test-only`
  rule covers such crates. It would need about 15 new crates, and doubles that need crate-private access cannot move.
  It is allowed, not required.
- **A run-time guard** (a double panics in a release build). It fails on the user's machine after shipping.
- **Only the artifact check.** It runs at release time, too late for a branch, and it cannot see a double that leaves
  no string in the binary.
- **One copy of Dyno's test per app.** That is eight copies of one mechanism (rule 55), and each costs about 46 s of
  compile per run.

## Consequences

- A test double can no longer reach a user through a spread, a fallback or a flag; the compiler refuses the reference
  in the build that ships.
- Tests that start a fake as a process use a separate binary. Integration tests of a crate need a self dev-dependency to
  see its own doubles, as `paddock-power`'s do on Dyno's branch.
- The shared-crate change touches every consumer's `[dev-dependencies]` in one change (rule 43); the integrating agent
  lands it.
- The register adds dated paperwork for the few deliberate in-memory adapters; in return they are visible.
- Host legs run a non-test build. Its cost is measured per app and comes back to the user when large (D10).

## Sources

- Dyno ADR 0033 and its implementation: `git show origin/fix/dyno-no-fakes-in-release:apps/dyno/docs/adr/0033-no-test-fake-in-a-release-build.md`;
  `origin/feat/dyno-health-not-checked`: `apps/dyno/crates/dyno-platform/tests/no_ungated_doubles.rs`,
  `apps/dyno/scripts/check-shipped.mjs`, `apps/dyno/scripts/check-shipped.test.mjs`, `apps/dyno/paddock.ci.json`
  (`rustChecks`, `hostRustChecks`), `.gitea/actions/app-gates/action.yml`, `scripts/affected.mjs`; Dyno
  `docs/performance.md` on `origin/fix/dyno-no-fakes-in-release` (runs 7767 and 7774).
- Pitwall: `apps/pitwall/crates/pitwall-server/src/{main.rs,lib.rs,fake_engine.rs}`, `tests/common/mod.rs`,
  `apps/pitwall/app/src-tauri/tauri.conf.json` at `origin/main`; `origin/fix/pitwall-e2e-engine-health` (`7a73cdc59`);
  `origin/pitwall/1.1-b1b-b6` (`e20ae50c3`).
- Cargo: resolver 2 does not unify dev-dependency features into a build with no test targets
  (<https://doc.rust-lang.org/cargo/reference/resolver.html#feature-resolver-version-2>), and `required-features` on
  `[[bin]]` targets (<https://doc.rust-lang.org/cargo/reference/cargo-targets.html#the-required-features-field>). These
  are relied on as Dyno ADR 0033 relied on them, not re-read for this ADR.
- The survey tool's per-item output and the 18 `cargo tree` outputs are in the author's scratch folder
  (`/Volumes/d7sst7/dev-targets/scratch/adr-0073/`), not in the repository; every row above cites its `file:line` so a
  reader can check it with `git show`.
