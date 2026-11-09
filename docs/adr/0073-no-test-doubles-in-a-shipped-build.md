# 73. No test double in a shipped build: one gate, one check per app, a dated register

- Status: Proposed (2026-11-09), revision 3. Nothing in "Work packages" starts before the user accepts it.
- Revision 2 (2026-11-09) answers the independent review of revision 1 (`1daeb0fff`, verdict CHANGES, 3 P1 and P2s). The
  review verified the mechanism in a fixture: a self dev-dependency with `required-features` and `CARGO_BIN_EXE_<name>`
  works within one package. It found nothing exploitable in a released app today.
  - P1-1: a wired environment switch was missed (Debrief's `DEBRIEF_BROWSER_NO_SANDBOX`), and the check was blind to
    environment and flag switches. Answered by new survey rows and the inventory layer, §2.3.
  - P1-2: the shipped set missed `paddock-setup.exe`, which per-machine installers embed. Answered in §2.1: the shipped
    set is now every binary an installer carries.
  - P1-3: a conflict with ADR 0071, now accepted. `7a73cdc59` lands as is, and WP-P moves the flags afterwards (Migration
    1).
  - The mechanism caveats, the controls, the survey corrections, the estimate, the decisions and the P2s are folded in
    where they belong.
- Revision 3 (2026-11-09) answers the re-review of revision 2 (one security P1, four P2s):
  - DB9 is class B. Debrief's three sidecar-path overrides pick which executable runs, and `restart_elevated`
    (`apps/debrief/app/src-tauri/src/system.rs:120`) relaunches Debrief elevated with the user's environment, which
    `HKCU\Environment` lets the user set without elevation. That makes them an environment-variable UAC bypass (§1.3).
  - Fix in Debrief 1.2, WP-D first item; user chose no 1.1.x patch on 2026-11-09.
- Date: 2026-11-09
- Trigger: the independent review of Pitwall on 2026-11-09 found that its released sidecar `pitwall-server` ships a fake
  LLM engine behind a command-line flag. Dyno's ADR 0033 had fixed the same shape in Dyno alone the day before.
- Related:
  - Engineering rules: ER-21 / rule 40 (a recurring bug means a missing design rule), ER-22 / rule 42 (share first),
    ER-27 / rule 56 (logs explain every failure), ER-33 / rule 71 (a check counts only if its probe can see), ER-20 /
    rule 51 (parallelism and cost), ER-11 / rule 5 (failing test first), rule 48 (quoted numbers are recorded), rule 54
    (an installer build before a release-path change lands), rule 55 / ADR 0034 (one implementation per mechanism).
  - Family ADRs:
    - 0022: the Rust developer and CI tool, `./dev`;
    - 0037: the app composition contract;
    - 0038: CI parity before push;
    - 0039: release gates inspect the artifacts, `release.toml`; WP-0 and WP-4 depend on it;
    - 0049: a feature switch is declared, and a test switch is not a feature switch;
    - 0053: per-machine installers and `paddock-setup.exe`;
    - 0071: ready or exit; accepted 2026-11-09, its row 1 lands `fix/pitwall-e2e-engine-health` as is;
    - 0074: declared hosts and machine leases; it names this ADR.
  - Dyno ADR 0033 (no test fake in a release build), accepted 2026-11-08, implemented on `fix/dyno-no-fakes-in-release`
    (tip `5f6aadd4c`) and carried into `feat/dyno-health-not-checked` (tip `176b5972b`). This ADR generalises its §1 to
    §3 to the family and leaves its §4 and §5 (Dyno's Windows readers and Health) to Dyno.
- Survey tool and raw method: [research note](../research/2026-11-09-test-doubles-survey.md).

All paths and lines below are at `origin/main` `b51c72509` unless a branch is named.

## Context

### The incidents

1. **Dyno, every release up to 1.1.0** (Dyno ADR 0033, Context). `windows::platform()` filled `locks`, `startup` and
   `facts` from `fake::Fake` through a struct-update spread (`apps/dyno/crates/dyno-platform/src/windows/mod.rs:23-38`,
   `..f`). Windows users saw:
   - "No startup items";
   - a Health score of 100;
   - an OS named "Fake OS 1.0".

   The fake answered in the shape of real data, so neither the page nor any test could tell "nothing there" from "never
   read".
2. **Pitwall, released and on the 1.1 line.** `pitwall-server` is the app's only bundled sidecar
   (`apps/pitwall/app/src-tauri/tauri.conf.json:36`). Its library declares `pub mod fake_engine;` with no gate
   (`apps/pitwall/crates/pitwall-server/src/lib.rs:14`), and its `main` accepts `--fake-engine` and `--die-after-ms` in
   every build (`src/main.rs:20-23`). With `--fake-engine`, the shipped binary serves canned OpenAI-compatible answers
   instead of starting the real server (`src/main.rs:40-46`).
   - The flag exists because the integration tests start the real binary as a child engine
     (`tests/common/mod.rs:440-463`).
   - `fix/pitwall-e2e-engine-health` (tip `7a73cdc59`) adds a third test-only flag, `--ready-when <path>`
     (`src/main.rs:24-30` on that branch).
   - The same lines are on `pitwall/1.1-b1b-b6` (`e20ae50c3`).
3. **Debrief, released (found by the review of revision 1).** `chromium-driver` turns off the browser's sandbox when the
   process runs as root and `DEBRIEF_BROWSER_NO_SANDBOX` is set to anything but empty or `0`:
   - the constant: `apps/debrief/crates/chromium-driver/src/launch.rs:121`;
   - the decision: `Sandbox::decide` at `:134-140`;
   - the read: `from_environment` at `:143-145`;
   - the wiring: `Chromium::launch` at `:233`.

   The switch exists for a CI job that runs as root in a container. It is in `debrief-v1.1.1` too: `launch.rs:91` and
   `:208` at that tag. A user has to run Debrief as root and set the variable, so the review rates it not exploitable.
   It is still a test switch wired into the product.

The three are one pattern at different depths:

- In Dyno, a test double was **wired** into production.
- In Pitwall, a double is **reachable** from production by an argument.
- In Debrief, a CI-only switch changes the product's **security posture** through the environment.

In all three, nothing in the build or in CI could tell a shipped binary from a test binary.

### The pattern (ER-21)

**Test doubles, test-only modules and test-only switches are compiled into shipped binaries whenever they are declared
in a crate's normal source without a test gate. Once compiled in, a spread, a fallback, a re-export, a flag or an
environment variable can reach them. No test notices, because the tests use the same doubles and switches on purpose.**

The invariant: *a build that ships contains no test double, no test-only module and no test-only switch. The only
in-memory or no-op implementations and the only non-product environment or flag switches it contains are deliberate,
carry production names, and are listed in a register with a kind and a date.*

Dyno's ADR 0033 made the invariant true for Dyno's own doubles and checked it there. It left out the family crates
(`no_ungated_doubles.rs` header, "Family crates are out of scope"; ADR 0033 review P2-4), environment and flag switches,
and every other app.

### The survey: every instance today

**Method.** The tool and its exact commands are in the [research note](../research/2026-11-09-test-doubles-survey.md).

- **The scan.** It is a generalisation of Dyno's `syn` scan (`no_ungated_doubles.rs` on `feat/dyno-health-not-checked`:
  the same `eval_cfg` and `gated`).
  - It parses every lib and bin target of every package in each Cargo workspace (`crates/`, each `apps/<app>`, the
    demos' `src-tauri`), following `mod` and `#[path]`.
  - It skips what `cfg(test)`, `#[test]` or a `cfg` that is false with `test` and `test-support` off excludes.
- **What it reports when not gated:**
  - doubles by name: `Fake*`, `Mock*`, `Stub*`, `Scripted*` and `Dummy*` types, double modules, trait impls for them,
    and `fake*`, `mock*`, `stub*`, `test_*` and `*_for_test(s)` functions;
  - switch-looking literals;
  - every `env::var` or `env::var_os` argument and every `"--…"` match arm (the inventory pass, added in revision 2);
  - use sites of doubles;
  - an advisory list of items whose doc says "for tests" (59 hits, triaged by hand).
- **The shipped scope.** For each shipped package I ran `cargo tree -p <pkg> -e normal --target all -f '{p} {f}'`.
  - The shipped set was first taken from the Tauri configs only. That missed `paddock-setup.exe` (review P1-2).
  - Revision 2 adds `paddock-setup` (from `crates/`) and derives the set as §2.1 specifies.
  - That gives 19 shipped packages on main: Dyno 6, Debrief 4, Pitwall 2, Pitcrew 2, Spotter 1, `shell-demo` 2,
    `workbench-demo` 1, `paddock-setup` 1.
  - Their normal-dependency closure contains 101 path packages: the scope.
- **Branches** surveyed as well:
  - `pitwall/1.1-b1b-b6` (`e20ae50c3`) and `fix/pitwall-e2e-engine-health` (`7a73cdc59`);
  - `debrief/1.2-batch1`;
  - `pacenotes/stage1-land` (`8576ce25c`) and `sighter/sw3c-vnc-2`;
  - `feat/dyno-health-not-checked` (`176b5972b`).
- **Timings:** [benchmarks.md, 2026-11-09](../benchmarks.md#2026-11-09-test-double-survey-scan-and-shipped-feature-trees).

**Classes.**

| Class | Meaning |
| --- | --- |
| **B** | Broken: wired into, or selectable by an argument, an environment variable or a fallback of, a shipped binary. |
| **L** | Latent B: wired the same way, but in a shared crate that no shipped graph links today. |
| **R** | At risk: compiled into a shipped binary, not wired today; any spread, re-export or flag can reach it. |
| **G** | Gated by something other than `test-support` (another feature name, `debug_assertions`). It does not ship today (measured), but the gate differs. |
| **N** | Not shipped: a test-only crate, or a test binary no installer carries. Listed so the check can enumerate it. |
| **E** | Deliberate: a production in-memory adapter, a demo adapter, a guard, a release probe or a CI opt-in. These are register candidates (§4). |

**Measured fact for the feature graph.** No shipped package enables `test-support`, `testing`, `test-hooks`, `testdb`,
`mock-runtime` or `probe` on any normal edge. This holds on main and on `pitwall/1.1-b1b-b6`.

No third-party test feature (`test-util`, `test`, `mock*`) is in any shipped tree either. `paddock-workspace-hack`
(cargo-hakari) carries none. Hakari unifies third-party features across the workspace, dev-dependencies included
(read in `crates/.config/hakari.toml`; whether hakari counts dev-dependencies by default is not re-read). Layer 2
therefore also checks third-party test features (§2.4).

The test-only crates (`paddock-testkit`, `paddock-plugin-test`, `debrief-plugin-test`) are in none of the trees.

So today the leak is in **declarations and switches**, not in features.

#### Pitwall (release first: 1.1)

| # | Where | Instance | Class |
| --- | --- | --- | --- |
| P1 | `crates/pitwall-server/src/lib.rs:14` | `pub mod fake_engine` (a fake OpenAI-compatible engine) in the shipped sidecar's library | B |
| P2 | `crates/pitwall-server/src/main.rs:21` | `--fake-engine`: the shipped binary becomes the fake | B |
| P3 | `crates/pitwall-server/src/main.rs:22` | `--die-after-ms`: the fake exits on a timer | B |
| P4 | `crates/pitwall-server/src/main.rs:24` on `fix/pitwall-e2e-engine-health` | `--ready-when <path>` (`fake_engine::parse_ready_when`); lands as is under ADR 0071 (Migration 1) | B (branch) |
| P5 | `app/src-tauri/src/server_client.rs:274` (`:288` on 1.1) | `pub mod fake` with `FakeServer` (`:290`) and `impl ServerClient for FakeServer` (`:330`), in `pitwall-app` | R |
| P6 | `app/src-tauri/src/credentials.rs:118` | `StaticKeys` ("A fixed key, for tests"); the name rules miss it | R |
| P7 | `crates/pitwall-server/src/paths.rs:15` | `PITWALL_HOME`: moves the data folder (tests, isolation) | E (CI opt-in) |
| P8 | the 1.1 graph on `pitwall/1.1-b1b-b6` | `pitwall-app` and `pitwall-server` link 9 of the shared crates with ungated doubles (F1 to F7, F9, F10). Pitwall 1.1's per-machine installer also embeds `paddock-setup.exe` (F14). On main Pitwall links only `pitwall-core` and `pitwall-loopback`. | R (1.1) |

**Pitwall: 4 B, 2 R and 1 E** in its own crates, plus 9 shared crates and `paddock-setup` on its 1.1 line.

#### Debrief (1.2; `debrief/1.2-batch1` scans the same as main)

| # | Where | Instance | Class |
| --- | --- | --- | --- |
| DB1 | `crates/chromium-driver/src/launch.rs:121,134-145,233` | `DEBRIEF_BROWSER_NO_SANDBOX`: root plus the variable starts the browser without its sandbox. Also in `debrief-v1.1.1`. | **B** |
| DB2 | `crates/debrief-ai/src/llm/mod.rs:3` | `pub mod fake`: `ScriptedProvider` (`fake.rs:20`, `impl LlmProvider` `:64`), re-exported at `mod.rs:12` | R |
| DB3 | `crates/debrief-ai/src/engine/pitwall/mod.rs:9` | `pub mod fake`: `FakePitwall` (`fake.rs:84`), a fake Pitwall server | R |
| DB4 | `crates/debrief-ai/src/lib.rs:29` | `pub mod testing` (synthetic GGUF builders) | R |
| DB5 | `crates/debrief-ai/src/vision/image.rs:246` | `pub mod testing` (test images) | R |
| DB6 | `crates/debrief-dump/src/lib.rs:31` | `#[doc(hidden)] pub mod testing` ("Small documents for tests") | R |
| DB7 | `crates/debrief-symbols/src/lib.rs:28-30` | `pub mod testing`: `FakeServer` (`testing.rs:10`). Its only gate is `feature = "server"`, which is a default and is on in `debrief-app` and `debrief-plugins` (measured). | R |
| DB8 | `crates/chromium-driver/src/discover.rs:97,104` | `find_for_tests`, `CHROMIUM_TEST_PATH` | R |
| DB9 | `app/src-tauri/src/engine/mod.rs:96`; `crates/debrief-plugin-host/src/builtins.rs:36`; `crates/debrief-ai/src/worker/frames.rs:538` | `DEBRIEF_STORE_WORKER`, `DEBRIEF_PLUGINS` and `DEBRIEF_AI_WORKER` (3 rows): development overrides of which sidecar executable runs, honoured in release builds and in `debrief-v1.1.1` (re-review). `restart_elevated` (`system.rs:120`) relaunches Debrief through `runas` with the user's environment, and `HKCU\Environment` is writable without elevation, so an unelevated process can choose the program Debrief starts elevated: an environment-variable UAC bypass. | **B (3)** |
| DB10 | `crates/chromium-driver/src/lib.rs:30` | `DEBRIEF_FAKE_BROWSER`: a guard that makes the app refuse to run as a fake browser; it must ship | E (guard) |
| DB10b | `crates/debrief-ipc/src/endpoint.rs:95,129` | `PADDOCK_SOCKET_BASE`: a short socket folder that CI's job-tmpdir sets (test isolation) | E (CI opt-in) |
| DB11 | `crates/debrief-ai/Cargo.toml:13` | `[[bin]] debrief-ai-fake-worker` (`--crash-*`) without `required-features` | N |
| DB12 | `crates/debrief-ai-worker/Cargo.toml:22` | `[[bin]] debrief-fake-llama-server`, in the **shipped** `debrief-ai-worker` package; not bundled because `build-sidecars.mjs:60` selects `--bin` | N |
| DB13 | `crates/debrief-plugin-host/Cargo.toml:13`; `crates/debrief-ipc/Cargo.toml:13`; `crates/debrief-video/Cargo.toml:13` | `debrief-plugin-host-fake`, `debrief-ipc-fake-child`, `debrief-video-fake-ffmpeg`, all in in-scope packages, without `required-features` | N (3) |
| DB14 | `crates/debrief-plugin-test/src/host.rs:46` | `FakeHost` in a test-only crate | N |

**Debrief: 4 B, 7 R, 2 E and 6 N.** Four of the seven R are in `debrief-ai`, which Debrief 1.2 is to remove in favour of
Pitwall (D8).

Product switches, not counted: `DEBRIEF_SAFE_MODE` (`lib.rs:584`), `DEBRIEF_LLAMA_SERVER`, `_DIR` and `_BUILD`
(`debrief-ai-worker/src/locate.rs:10-14`), `DUCKDB_JE_MALLOC_CONF` (`debrief-store-client/src/allocator.rs:20`). WP-D
confirms each is a product setting.

#### Dyno (fixed by its ADR 0033)

| # | Where (main) | Instance | Class on main | On `feat/dyno-health-not-checked` |
| --- | --- | --- | --- | --- |
| Y1 | `dyno-platform/src/windows/mod.rs:24-25` | `..crate::fake::Fake::new().platform()` | B | gone (every port named) |
| Y2 | `dyno-platform/src/hardware.rs:62` | `FakeBattery` as the answer on an OS other than macOS, Linux and Windows: no shipped target compiles it | R | gone (`NotReadHere`) |
| Y3 | `dyno-platform/src/lib.rs:17`, `fake.rs` | `pub mod fake`: `Fake`, `FakeOp`, `FakeState`, `FakePowerActions`, `FakeIdleSignals` | R | gated |
| Y4 | `dyno-platform/src/counters/mod.rs:63`; `hardware.rs:560,573,582`; `wol/mod.rs:56,134` | `FakeCounters`, `FakeThrottle`, `FakeWear`, `FakeBattery`, `FakeWakeTimer`, `WolPorts::fake` | R | gated |
| Y5 | `dyno-core/src/wol/mod.rs:8`, `wol/fake.rs` | `pub mod fake` with 7 `Fake*` types | R | gated |
| Y6 | `dyno-agent/src/build_id.rs:136` | `#[doc(hidden)] pub fn set_watch_every_for_tests` | R | still R |
| Y7 | `app/src-tauri/src/shell.rs:104-106` | `DYNO_TRAY_ITEMS` overrides the tray items ("development and measurements") in release builds | R (switch) | still R |
| Y8 | `dyno-agent/Cargo.toml:74`, `geo_db.rs:325` | feature `testdb` gating `mod testdb` (fuzz corpus writer, one example) | G | still G |
| Y9 | `app/src-tauri/src/login.rs:426-430` | `DYNO_AGENT_BIN`, honoured only when `cfg!(debug_assertions)` (`agent_override`) | G (`debug_assertions`) | still G |
| Y10 | `dyno-agent/src/paths.rs:165`, `logging.rs:30,40` | `DYNO_DATA_DIR`: moves the data folder (tests, isolation) | E (CI opt-in) | still E |
| Y11 | `dyno-plugin-host/Cargo.toml:13` | `[[bin]] dyno-test-plugin` in an in-scope package, without `required-features` | N | still N |

**Dyno: 1 B, 6 R, 2 G, 1 E and 1 N on main** (Y3 to Y5 are 17 types and one constructor). On the branch: 2 R, 2 G, 1 E
and 1 N. Dyno also links 8 shared crates with doubles (F1 to F8, the privileged `dyno-helper` included); the branch gates
F8. Its per-machine Windows installer (Dyno 1.1.0) embeds `paddock-setup.exe` (F14).

#### Pitcrew, Spotter, the demos, Pacenotes, Sighter

| # | Where | Instance | Class |
| --- | --- | --- | --- |
| C1 | `apps/pitcrew/crates/pitcrew-service/src/net.rs:26,49-55` | `Http::for_tests()` lifts the HTTPS-only rule to allow loopback `http://`, in the shipped service. Only `cfg(test)` code calls it (`work.rs:258`, `net.rs:204`). Its `check` is a prefix test (`url.starts_with("http://127.0.0.1:")`), so `http://127.0.0.1:@evil.host/` would pass when the flag is on. Gating it must parse the URL and compare the host. | R |
| C2 | `apps/pitcrew/crates/notify-channels/src/model.rs:49-51` | `MemorySecrets` ("for tests"), re-exported at `lib.rs:17`, in `pitcrew-app` | R |
| S1 | `apps/spotter/crates/spotter-render/src/discover.rs:131-150` | `SPOTTER_TEST_BROWSER`, `SPOTTER_TEST_BROWSER_DIR`, `test_browser_dir`, `find_for_tests` | R |
| S2 | `apps/spotter/crates/spotter-render/src/discover.rs:171,185-198` | `SPOTTER_TEST_BROWSER_NO_SANDBOX` adds `--no-sandbox` in `test_browser_flags(_with)`. Only `tests/real_browser.rs` calls it today (read). | R |
| S3 | `apps/spotter/crates/spotter-engine/src/secrets.rs:19-21` | `MemorySecrets` ("tests use") | R |
| S4 | `apps/spotter/crates/spotter-engine/src/clock.rs:151`, `notifier.rs:72` | `SequenceRandom`, `RecordingNotifier` ("for tests") | R |
| S5 | `apps/spotter/app/src-tauri/src/state.rs:11`, `lifecycle.rs:89` | `SPOTTER_DATA_DIR` (tests, e2e, a second profile) | E (CI opt-in) |
| X1 | `apps/shell-demo/src-tauri/src/lib.rs:27,182` | `FakeToaster` wired as the demo's toaster | E (wired on purpose) |
| X2 | `apps/shell-demo/src-tauri/src/lib.rs:135` | `paddock_service::testing::FakeSystem` wired as the demo's service backend | E (wired on purpose) |
| X3 | `apps/workbench-demo/src-tauri/src/lib.rs:97` | `paddock_secrets::MemorySecrets` (Dyno ADR 0033 D2b: a legitimate ephemeral store) | E |
| N1 | `pacenotes/stage1-land`: `pacenotes-core/src/lib.rs:20`, `pacenotes-function-api`, `pacenotes-worker` | the right pattern under the names `testing` and `test-hooks`, enabled from `[dev-dependencies]` only | G (3) |
| — | `sighter/sw3c-vnc-2` | nothing found | — |

**Pitcrew: 2 R. Spotter: 4 R and 1 E** (S1 is one group of four items). **shell-demo: 2 E. workbench-demo: 1 E.
Pacenotes: 3 G. Sighter: 0.**

#### Shared crates (`crates/`)

| # | Crate and place | Instance | Shipped by (normal graph) | Class |
| --- | --- | --- | --- | --- |
| F1 | `paddock-service`: `lib.rs:63`, `lib.rs:88`, `privileged/mod.rs:24` | `pub mod fake_systemd` (`FakeSystemd`), `pub mod testing` (`FakeSystem`, `FakeAppService`, `FakeElevator`, `FakeFiles`), `pub mod trust_fake` (`FakePort`, a fake machine for the privileged `TrustPort`) | Dyno (helper included), both demos, `paddock-setup`, Pitwall 1.1 | R |
| F2 | `paddock-jobs`: `time.rs:121`, `lib.rs:53` | `FakeClock`, re-exported by `paddock-log` (`clock.rs:4`, `lib.rs:51`), `paddock-health` (`lib.rs:52`), `paddock-testkit` | Dyno, demos, `paddock-setup`, Pitwall 1.1 | R |
| F3 | `paddock-health`: `notify/os/mod.rs:24,35` | `mod testing`, `pub use testing::FakeToaster` | Dyno, demos, `paddock-setup`, Pitwall 1.1 | R |
| F4 | `paddock-channels`: `lib.rs:23,40`; `control.rs:55` | `FakePoster`, `FakeMail`, `MemorySettings` ("for tests") | `dyno-app`, demos, Pitwall 1.1 | R |
| F5 | `paddock-net`: `lib.rs:21,31` | `FakeWire`, `StaticLookup`, `Reply`, `Seen` | Dyno, demos, Pitwall 1.1 | R |
| F6 | `paddock-watch`: `source.rs:122`, `lib.rs:31` | `FakeSource`, re-exported by `paddock-packs` (`watch/mod.rs:10`) | Dyno, demos, Pitwall 1.1 | R |
| F7 | `paddock-commands`: `lib.rs:62` | `pub mod testing` (assertions over an app's commands; reads `WRITE_VAR` at `testing.rs:31`) | Dyno, demos, `paddock-setup`, Pitwall 1.1 | R |
| F8 | `paddock-power`: `lib.rs:15,22` | `FakePower`, `FakeEvents` (gated on Dyno's branch, ADR 0033 WP-2) | Dyno | R |
| F9 | `paddock-mcp`: `control.rs:84`, `key_list.rs:232` | `MemorySettings`, `MemoryKeys` ("for tests") | Pitwall 1.1 | R |
| F10 | `paddock-secrets`: `memory.rs:9-11,58-59`, `lib.rs:5` | `MemoryVault` and `MemorySecrets`. Their docs say "for tests", but `MemorySecrets` wraps `MemoryVault` and is a production adapter (X3), so neither can be gated. The fix is the doc (WP-S), plus a register entry. | Dyno via `WolPorts`, Pitwall 1.1, `workbench-demo` | E |
| F11 | `paddock-chromium`: `discover.rs:10,174` | `find_for_tests`, `CHROMIUM_TEST_PATH` | no shipped graph today | R (latent) |
| F12 | `paddock-chromium`: `launch.rs:44,101-105` | `PADDOCK_BROWSER_NO_SANDBOX`, wired into `Sandbox::from_environment` as in DB1 | no shipped graph today | **L** |
| F13 | `paddock-secrets`: `os/mod.rs:19-27` | `PADDOCK_SECRETS_FORBID_REAL` and `PADDOCK_SECRETS_REAL`: a guard that makes `OsVault` panic in tests instead of touching the user's keychain (rule 7). It is read through `env_is_one(name)`, a helper that a literal-only inventory misses. | wherever `paddock-secrets` ships | E (guard) |
| F14 | `crates/paddock-setup` (the bin `paddock-setup`), built from `crates/` with `crates/Cargo.lock` by `scripts/stage-paddock-setup.mjs:93-96`, staged by `app-release.yml:245-256` (`windows-pre-bundle`) | Not a double: a **shipped binary**, embedded in every per-machine Windows installer (Dyno 1.1.0, Pitwall 1.1) and run as administrator. It links F1, F2, F3 and F7. | Dyno, Pitwall 1.1 installers | in scope |
| F15 | `paddock-chromium` `lib.rs:31` `PADDOCK_FAKE_BROWSER` | a guard: the app refuses to run as a fake browser | no shipped graph today | E |
| F16 | `paddock-proc/src/self_test.rs:23` (and `debrief-store-worker`, `dyno-helper`) | `--self-test`: release probes (ADR 0039 `probe = ["--self-test"]`) | as linked | E |
| G1 to G6 | `paddock-pitwall` `lib.rs:39` and `paddock-siblings` `lib.rs:47` (`testing`); `paddock-shell` `lib.rs:43`, `paddock-dialogs` and `paddock-panels` (`mock-runtime`); `paddock-dialogs` `lib.rs:70` (`probe`: `PADDOCK_REAL_WINDOWS`, `PADDOCK_PROBE_*`) | gated under other feature names | no such feature on a normal edge (measured) | G (6) |
| N2 | `paddock-testkit` (`FakeRunner`), `paddock-plugin-test` (`FakeHost`, fixture bins), `paddock-ipc` `[[bin]] paddock-ipc-test-child` (`Cargo.toml:15`, in-scope package, no `required-features`) | test-only crates, a test bin | in no installer (measured) | N (3) |

**Shared: 9 R** (F1 to F9; F8 fixed on Dyno's branch), **1 latent R (F11), 1 L (F12), 4 E (F10, F13, F15, F16)**,
6 G and 3 N. `paddock-setup` joins the shipped set.

**False positives of the name rules.** They size the rules; §2.2 drops the `test_*` prefix because of them.

- Spotter's product "Test" actions: `Engine::test_source`, `test_source_job` and `test_pattern`, and `commands.rs:116,223`.
- Dyno `IpLookupSettings::test_url` (the Test button) and `dyno wake --test`.
- Debrief `runtimes/selftest.rs:28` `test_image`.
- Debrief `dbghelp.rs:20` `fake_handle`: a real DbgHelp session handle, `pub(crate)`. It is the one remaining false
  positive of the kept rules, and WP-D renames it.

#### The switch inventory (revision 2)

The inventory covers the shipped scope: 101 path packages, 251 records.

- **68 `"--…"` match arms.** 3 are test-only: P2 to P4 (P4 on its branch). The rest are product flags (`--help`,
  `--version`, `dyno` subcommand options, `--self-test`, `--service`).
- **183 `env::var` reads.** Most are OS variables (`HOME`, `PATH`, `XDG_*`, proxies) or parameters of helper functions
  (`var(k)`). A `var(h)` in the nettrace writers is a false match of a local function named `var`.
- **The named app and family switches.** 22 names, plus the 6 behind `probe`. They are classified in the tables above:
  - B: DB1 and DB9 ×3;
  - L: F12;
  - R (switch): Y7, S1 and S2;
  - G: Y9 and G6;
  - E: P7, Y10, S5, DB10b, F13, F15 and DB10;
  - product (not counted): `DEBRIEF_SAFE_MODE`, `DEBRIEF_LLAMA_*`, `DUCKDB_JE_MALLOC_CONF`, `PITWALL_SERVICE`
    (supervised marker), `DYNO_MCP_KEY` (a key off the command line).
- **Blind spots of the survey's inventory**, which WP-0 closes:
  - clap-derived flags;
  - `==` comparisons with `"--…"` (Debrief's `--safe-mode` parser);
  - names passed to a helper (F13).

**Totals (revision 2):**

| Class | Count | Made up of |
| --- | --- | --- |
| B | 9 | Pitwall 4 (one on a branch), Debrief 4 (DB1 and the three DB9 variables), Dyno 1 (on main; fixed on the branch) |
| L | 1 | F12 |
| R | 31 | Pitwall 2, Debrief 7, Dyno 6, Pitcrew 2, Spotter 4, shared 10 (9 plus latent F11), counted per crate or row |
| E | 12 | Pitwall 1, Debrief 2, Dyno 1, Spotter 1, the demos 3, shared 4 |
| G | 11 | Dyno 2, Pacenotes 3, shared 6 |
| N | 10 | Debrief 6, Dyno 1, shared 3 |

### The UI has the same shape (read, not built)

Every app's Angular frontend selects a `MockBackend` at run time when `__TAURI_INTERNALS__` is missing:

- Dyno: `apps/dyno/app/src/app/core/backend-factory.ts:4,17,23`;
- Pitcrew: `apps/pitcrew/app/src/app/app.config.ts:5,22,40`;
- Pitwall: `apps/pitwall/app/src/app/app.config.ts:6,46`;
- Spotter: `apps/spotter/app/src/app/app.config.ts:5,17,27`;
- Debrief: `apps/debrief/app/src/app/core/backend.ts:299`, through a lazily imported chunk (`mock-registry.ts:10`).

So the release bundle carries the mock. A release window that lost its Tauri bridge would show plausible demo data
instead of an error. I read this in the code; I neither built a dist nor saw it happen. §5 handles it as phase 2.

## Decision

### 1. The rule and the one mechanism

The proposed rule text for `docs/rules.md`, under a new heading "Shipped builds". Its number is assigned on acceptance:
73 is the next free one today, and 72 is taken by family ADR 0057's branch.

> **No test double or test switch ships.** A build that ships contains no test double, test-only module or test-only
> switch:
> (a) a double inside a crate sits behind `#[cfg(any(test, feature = "test-support"))]`, and `test-support` is enabled
> only from `[dev-dependencies]` (a crate's own integration tests use a self dev-dependency);
> (b) a test program that must be its own process is a `[[bin]]` with `required-features = ["test-support"]` in the
> package whose tests start it, or lives in a test-only crate, never behind a flag of a shipped binary;
> (c) an environment variable or flag that only tests, CI or development use is gated the same way, or is registered
> with its kind (`guard`, `ci-opt-in`, `dev-override`) and a review date;
> (d) a composition names every port, with no struct-update spread of a port bundle, and an OS or build that cannot
> read a port gets an honest adapter that says so;
> (e) an in-memory or no-op implementation that production uses on purpose has a production name and an entry in
> `docs/test-doubles.toml`.
> `./dev check test-doubles` enforces it for every app in CI.
> *Why:* Dyno shipped a fake platform to Windows users in every release up to 1.1.0. Pitwall's released sidecar turns
> into a fake engine with one flag. Debrief's released browser driver turns its sandbox off on a CI variable.

The mechanism, in detail:

1. **One feature name: `test-support`.** Every crate that holds doubles has it. A crate's `test-support` enables the
   `test-support` of the crates whose doubles it re-exports or builds on, as Dyno's `dyno-platform` does for `dyno-core`
   and `paddock-power`. For the other names (D3):
   - `testing` is renamed in `paddock-pitwall`, `paddock-siblings` (WP-G), `pacenotes-core` and
     `pacenotes-function-api` (WP-N), and `test-hooks` in `pacenotes-worker`;
   - `mock-runtime` (a Tauri mock runtime), `probe` (a real-window probe) and `testdb` (a fuzz corpus writer) stay as
     registered test features.
2. **Test programs are separate binaries in the package whose tests start them.** `CARGO_BIN_EXE_<name>` is set only
   for bins of the package under test (Cargo docs; the review verified it in a fixture with a self dev-dependency and
   `required-features`). So:
   - **Pitwall:** `pitwall-fake-engine` is a `[[bin]]` of the `pitwall-server` package, with
     `required-features = ["test-support"]` and the flags `--port`, `--die-after-ms` and `--ready-when`.
     `pitwall-server`'s `main` loses all three flags (D2).
   - **Debrief and Dyno:** their fake binaries get `required-features` (DB11 to DB13, Y11).
   - **Shared:** `paddock-ipc-test-child` gets `required-features` too.
   - **ADR 0071's scripted program**, a test binary of `paddock-proc` (`--ready-when`, `--exit`, `--never-ready`), must
     use `required-features = ["test-support"]` or live in `paddock-testkit`. It is the first new instance the check sees.
3. **Test switches are gated or registered.**
   - DB1, F12, S2, DB9 and Y7 move under `cfg(any(test, feature = "test-support"))`, or become a registered
     `dev-override` with a review date.
   - **Never registered:** any switch that picks an executable or library path, or weakens a protection, is gated, or
     honoured only under `cfg!(debug_assertions)` (as Dyno's `DYNO_AGENT_BIN`, Y9). It is never `ci-opt-in` or
     `dev-override`. That covers the no-sandbox switches (DB1, F12, S2) and the executable-path overrides (DB9).
     The environment of a shipped build is user-writable (`HKCU\Environment` on Windows), and it survives an elevation
     that relaunches the same user, so such a switch is a privilege boundary, not a convenience. A root CI container gets a test build or passes the flag through the test
     harness's launch options instead (`adapter.rs:44` already has `extra_args` for that).
   - Data-folder overrides used for test isolation (P7, Y10, S5, DB10b) are `ci-opt-in`. They stay, because the e2e
     runs drive the shipped app with them (rule 7) and they never weaken a protection.
   - **No data-folder override is read by an elevated or service process.** A service, a helper or a relaunch through
     `runas` resolves its folders from the OS (the per-machine location or the service account), never from the
     environment. WP-0's inventory flags such a read in a crate that a service or elevated entry point links.
4. **Compositions name every port.** This is Dyno ADR 0033 §1.3, family-wide, and fits ADR 0037's `compose`.
   `NotReadHere` stays in Dyno: it is one implementation, below the rule of three.
5. **Production in-memory adapters are not doubles.** `MemorySecrets` keeps its name by the user's decision D2b of
   2026-11-08. Its inner `MemoryVault` stays public with it. Both doc lines (`memory.rs:9`, `:58` and `lib.rs:5`) change
   from "for tests" to "an in-memory store that forgets on exit (tests, demos)", and both get a register entry.
6. **`debug_assertions` is not a gate for a double.** `#[cfg(debug_assertions)]` and `cfg!(debug_assertions)` count as
   "maybe" in layer 1, so a double behind them is an offence. They are accepted only for a registered `dev-override`
   (Y9), and then only if:
   - every profile that ships keeps `debug-assertions = false` (the check reads `[profile.release]` and `[profile.dist]`
     from the workspace manifest and fails otherwise);
   - layer 3 builds with the shipping profile (§2.5).
7. **The non-test build is the proof for use sites.** Once a double is gated, every production reference to it is a
   compile error in a build without test targets, on the OS that code compiles on. That covers every path, alias,
   re-export and flag arm.

### 2. One shared check, run per app in CI: `./dev check test-doubles --app <app>`

The check is written in Rust in `tools/paddock-dev` (ADR 0022) and reuses `paddock-repo-graph`'s `cargo-metadata`. It is
generalised from Dyno's three pieces:

- the `syn` scan with `cfg` evaluation (`no_ungated_doubles.rs`, commits `e8547c0c2` and `37e1f1466`);
- the shipped feature graph with its positive control (`scripts/check-shipped.mjs`: `shippedPackages` `:25`,
  `testSupportEnabledIn` `:67`, `assertProbeSees` `:91`);
- `hostRustChecks` (commits `259bec2a6` and `9bdc26f3b`).

**Without the register, the check fails on main.** It would report every R, B and L row above. §4's first entries are what
let it go on at once (D7).

1. **The shipped set is every binary an installer carries.**
   - The Tauri main binary: `mainBinaryName`. When it is absent, as in both demos, it is the bin target of the Cargo
     package that holds the app's `tauri.conf.json` (`paddock-shell-demo` and `paddock-workbench-demo`).
   - Every `bundle.externalBin` in all the app's `tauri*.conf.json`. Debrief's sidecars are in
     `tauri.sidecars.conf.json:7`.
   - Every executable or library the release workflow stages before bundling:
     - each app declares them in its `windows/per-machine.json` (and later in `release.toml`) as
       `staged = [{ file = "paddock-setup.exe", package = "paddock-setup", workspace = "crates" }]`;
     - the check cross-reads every `File` directive of an `.exe` or `.dll` in the app's installer hooks
       (`windows/*.nsh`), every executable in `bundle.resources`, and the `windows-pre-bundle` input of its release
       workflow;
     - a staged binary it cannot map to a declared package fails the check.
   - When ADR 0039's `release.toml` exists, its `[[executable]]` list must equal this set. `./dev release verify` already
     counts the executables of the built artifact both ways (ADR 0039 §1), which closes the loop on the real installer.
   - Each name maps to its package in **its own** workspace through `cargo metadata`: `paddock-setup` maps in `crates/`
     with `crates/Cargo.lock`. A name no package builds fails.
   - The scope is the normal-dependency closure of the shipped set over path packages, family crates included (Dyno ADR
     0033 review P2-4).
2. **Layer 1, declarations.** The scan uses Dyno's `syn` rules and its `cfg` evaluation (`any`, `all`, `not`; `test` and
   `test-support` off; every other predicate, `debug_assertions` included, "maybe"). It reports these when not gated:
   - a public item named `Fake*`, `Mock*`, `Stub*`, `Scripted*` or `Dummy*` (Dyno: `Fake*` only);
   - a module `fake`, `fakes`, `mock`, `mocks`, `testing`, `test_support` or `fake_*`;
   - a trait impl for such a type;
   - a non-private function or method named `fake*`, `mock*`, `stub*` or `*_for_test(s)`;
   - a `[[bin]]` of an in-scope package whose name or path contains `fake`, `mock`, `stub` or `test`, without
     `required-features` that include `test-support`;
   - an item gated only by a registered test feature that a normal edge enables.

   Doubles with production-sounding names (C2, P6, S3, S4, F4, F9) are not found by name. The migration gates them, and
   the doc-comment pass stays an advisory list, not a failure.
3. **Layer 1b, the switch inventory (new).** In the non-test code of every in-scope crate, each of these must be either
   in the register (kinds `guard`, `ci-opt-in`, `dev-override`, `release-probe`) or a declared product setting:
   - every `env::var`, `env::var_os` and `env::vars` read whose name is a literal or a `const`;
   - every helper that passes a parameter on to such a read: the helper is listed, and so is each literal at its call
     sites (F13's `env_is_one`);
   - every `"--…"` literal in a match arm or an `==` comparison;
   - every clap `#[arg(long …)]` field;
   - every environment value that reaches `Command::new`, a `PathBuf` given to a process start, or a library load
     (`libloading`, `LoadLibrary*`, `dlopen`), followed within the function and through a returned path. Each is a
     **candidate** that needs a review: gated, or `cfg!(debug_assertions)` only (§1.3).

   A product setting is one the app's `docs/` names, recorded as `product = true` in the same register file, so the list
   is complete in one place. OS variables (`HOME`, `PATH`, `XDG_*`, `TMPDIR`, proxies and similar) are a fixed family
   allow list in the check.
4. **Layer 2, the shipped feature graph.**
   - The check runs one `cargo tree -e normal --target all -f '{p} {f}'` per build invocation of the shipped packages.
     Dyno's sidecars build in one `cargo build -p … -p …`, so their features unify; one call per invocation sees that
     union.
   - It fails when any crate in that graph enables `test-support`, a registered test feature, or a third-party test
     feature (`test-util`, `test`, `mock*`, `testing`) that `paddock-workspace-hack` or anything else unified in.
   - It fails when a crate marked `[package.metadata.paddock] test-only = true` is in the graph. Today those are
     `paddock-testkit`, `paddock-plugin-test` and `debrief-plugin-test`. `paddock-testplan` is a product crate.
   - Per-app allowances for a feature edge (the demos, D5) come from the register.
5. **Layer 3, a non-test build on every OS leg.** `cargo check --locked --bins -p <shipped packages that link a gated
   crate>`, with the shipping profile (`--profile dist` where the app ships from `dist`, otherwise `--release`).
   - There is no `--lib`: with `--bins`, the libraries the bins use are checked anyway, and a lib-only package is not
     shipped.
   - It runs on the Linux leg and, through the generalised `hostRustChecks`, on the Windows and macOS legs.
   - `app-gates` derives the package list from the shipped set.
   - Clippy `--all-targets` and nextest unify `test-support` in, so they never see the shipped build (Dyno ADR 0033 §3).
6. **Positive controls (ER-33).** A clean result counts only if each probe has shown it can see.
   - **Fixture workspaces**, in `conformance/fixtures/test-doubles/`, run before the app:
     - layers 1 and 1b: one offence of each kind, ungated and gated (a double, a fake `[[bin]]` without
       `required-features`, an `env::var` literal, a `"--…"` arm, an unmapped staged binary), must give exactly the
       expected findings;
     - layer 2: a dev-dependency that enables `test-support` must be seen on dev edges and not on normal edges;
     - layer 3: a crate whose production code names a gated double must fail the non-test `cargo check`, and its fixed
       twin must pass (seconds; once per leg).
   - **Per app** (Dyno's control, generalised):
     - every in-scope crate that declares `test-support` must show it enabled on the `-e normal,dev` tree **of its own
       workspace**: `crates/` for a shared crate, the app's workspace for an app crate. A feature nobody enables is
       dead, or the probe is broken;
     - the root set of the normal tree must equal the derived shipped set;
     - every gated double the register names for the app must have been met gated.
   - **Tree coverage: 100 %, reached or listed.** Every `.rs` file under the in-scope crates' `src/` is either parsed
     from a crate root or printed by name as unreached (a file no `mod` reaches is not compiled). A parse error is
     `check broken`. This replaces Dyno's hand-kept `MIN_FILES = 100`.
   - A control that fails reports `check broken` and fails the step.
7. **Output (ER-27).**
   - One `::error::` per offence, with `file:line`, the rule and the fix. For example: "gate it with
     `#[cfg(any(test, feature = "test-support"))]` and enable `test-support` from `[dev-dependencies]`".
   - A summary of the packages, crates and files scanned, the unreached files, the register entries applied with their
     dates, and each control's result.
   - No source text and no user data.
8. **Where it runs.**
   - In `app-gates` on the Linux leg for every app with a Cargo workspace, and for `paddock-setup` from `crates/` when
     an app stages it. A conformance test asserts that every app with `cargo` in `paddock.ci.json` is covered.
   - In the nightly for all apps, and in `./dev check --pre-push` (ADR 0038) when the plan reaches an app's Rust half.
   - Layer 3 on every OS leg (D10).
   - A shared-crate change runs the check for every app whose shipped graph contains that crate.
9. **Layer 4, the artifacts (D6; depends on ADR 0039 WP-1).** `release.toml` gains `forbidden_strings`: the family list
   (`--fake-`, `--mock-`, `--ready-when`, `--die-after-`, `_NO_SANDBOX`) plus each app's own names.
   - `./dev release verify` searches every extracted executable for them.
   - **The control stays in one job.** The release job builds the app's test-switch binary itself, for example
     `cargo build -p pitwall-server --bin pitwall-fake-engine --features test-support --profile dist`, and the search
     must find the strings in it.
     - That build changes the feature set of every crate whose `test-support` it enables, so those crates and their
       dependents recompile, not only `pitwall-server`.
     - It runs **after the bundle is built and sealed**, or in its own `CARGO_TARGET_DIR`, so it can never replace a
       binary that the bundle step copies (lesson "tauri-build copies every sidecar over the binary cargo just linked").
     - Its cost is measured in WP-4.
     - An app with no test-switch binary builds the family fixture binary from `conformance/fixtures/test-doubles/`
       instead.
   - No artifact crosses jobs.
10. **Dyno's own pieces become copies (rule 55).** `no_ungated_doubles.rs` and the `test-support` part of
    `check-shipped.mjs` are entered in `docs/known-copies.toml`, with the shared check as canonical and delete-by Dyno's
    next tag after WP-0 lands. Dyno's per-OS wiring tests and its `Platform wired` log line stay.

### 3. What this does not do

- It does not forbid in-memory adapters or switches that production needs; they are named and registered.
- It does not police test code: `tests/`, `benches/`, examples and `cfg(test)` modules may hold anything.
- It does not replace Dyno ADR 0033 §2's per-OS wiring tests.

### 4. The exception register: `docs/test-doubles.toml`

The register is data, read by the check:

```toml
version = 1
test_features = ["test-support", "mock-runtime", "probe", "testdb"]   # D3: names a normal edge must never enable

[[exception]]
id = "paddock-secrets-memory"                 # unique, kebab case
path = "crates/paddock-secrets/src/memory.rs"
item = "MemorySecrets, MemoryVault"
kind = "production-in-memory"                 # | "guard" | "release-probe" | "ci-opt-in" | "dev-override"
                                              # | "demo-adapter" | "temporary"
apps = ["workbench-demo", "dyno", "pitwall"]
reason = "An ephemeral store the workbench demo uses on purpose (Dyno ADR 0033 D2b); MemoryVault is its inner store."
approved = "user 2026-11-08"
review_by = 2027-05-09                        # permanent kinds: the check warns 30 days ahead and fails after
# remove_by = "pitwall-v1.1.0" | 2026-12-31   # temporary kind only: a tag or a date

[[feature_edge]]                              # a per-app allowance on a normal edge (D5); the only way past layer 2
app = "shell-demo"
crate = "paddock-service"
feature = "test-support"
reason = "The demo wires FakeSystem so it never touches the machine."
approved = "user"
review_by = 2027-05-09

[[setting]]                                   # layer 1b: a product environment variable or flag
name = "DEBRIEF_SAFE_MODE"
app = "debrief"
doc = "apps/debrief/docs/user-guide.md"
product = true
```

Its rules mirror rule 55 (d), so a temporary entry cannot become permanent by drift:

- A `temporary` entry fails the check after its date.
- A tag is refused while its app has a `temporary` entry whose `remove_by` tag it has reached.
- A date moves only with `Doubles-date: <id>: <old> -> <new>: approved: <coordinator|user>: <reason>`. A move to a later
  release line needs `approved: user`.
- A permanent kind needs `approved: user` and a `review_by` at most 6 months ahead.
- An entry leaves with its code, or with `Doubles-removed: <id>: <reason>`.

Proposed first entries. The dates are proposals (D7).

| id | Item | Kind | Date |
| --- | --- | --- | --- |
| `paddock-secrets-memory` | F10 (X3) | production-in-memory | review 2027-05-09 |
| `fake-browser-guard` | `PADDOCK_FAKE_BROWSER`, `DEBRIEF_FAKE_BROWSER` (F15, DB10) | guard | review 2027-05-09 |
| `secrets-real-guard` | `PADDOCK_SECRETS_FORBID_REAL`, `PADDOCK_SECRETS_REAL` (F13) | guard | review 2027-05-09 |
| `self-test-probe` | `--self-test` (F16) | release-probe | review 2027-05-09 |
| `data-dir-overrides` | `PITWALL_HOME`, `DYNO_DATA_DIR`, `SPOTTER_DATA_DIR`, `PADDOCK_SOCKET_BASE` (P7, Y10, S5, DB10b) | ci-opt-in | review 2027-05-09 |
| `dyno-agent-bin` | `DYNO_AGENT_BIN` (Y9), `debug_assertions` only | dev-override | review 2027-05-09 |
| `shell-demo-fakes` | X1, X2 and their `[[feature_edge]]`s | demo-adapter (D5) | review 2027-05-09 |
| `pitwall-1-1` | P1 to P6 (P4 included once `7a73cdc59` lands), the 9 shared crates in Pitwall's graph, and `paddock-setup`'s F1 to F3 and F7 | temporary | remove by `pitwall-v1.1.0` |
| `debrief-1-2` | DB1 to DB9, DB11 to DB13 | temporary | remove by `debrief-v1.2.0` |
| `dyno-next` | Y6, Y7, Y11, and F1 to F8 for Dyno | temporary | remove by Dyno's next tag after WP-S |
| `pitcrew-1-1` | C1, C2 | temporary | remove by `pitcrew-v1.1.0` |
| `spotter-next` | S1 to S4 | temporary | remove by Spotter's next tag |
| `shared-latent` | F11, F12, `paddock-ipc-test-child` | temporary | remove with WP-S |

With these entries the check goes on at once and ratchets: no new instance can land, and each existing one has a date
(the C13 ratchet the user approved for Debrief on 2026-11-08). **`pitwall-1-1` expiring at `pitwall-v1.1.0` makes WP-P,
WP-S and WP-G a Pitwall 1.1 tag blocker** (D4, D7).

### 5. Phase 2: the UI mock backends (D9)

- Each app's `production` Angular configuration swaps the mock module, through `fileReplacements`, for
  `mock-backend.release.ts`. That module's factory throws: "This build has no mock backend: it runs only inside <App>'s
  window."
- The dev and e2e configurations keep the mock.
- The check (`./dev check test-doubles --ui`): every mock module exports a marker `PADDOCK_MOCK_BACKEND = '<app>'`, and
  the release `dist` must not contain it. Its control: the e2e build of the same job must contain it.
- The configurations come from a shared preset in `ui/`.

## Migration per app, ordered by release

1. **Pitwall 1.1 (first; tag window about 2026-12-18 to 2027-01-13). Its tag is blocked until these land.**
   - `7a73cdc59` (`fix/pitwall-e2e-engine-health`, in train 1) **lands as is**, as accepted ADR 0071 row 1 decides. Its
     `--ready-when` is covered by `pitwall-1-1` until WP-P.
   - WP-P then:
     - moves `--fake-engine`, `--die-after-ms` and `--ready-when` with `fake_engine` into `pitwall-fake-engine` (§1.2);
     - moves the tests to `CARGO_BIN_EXE_pitwall-fake-engine`;
     - gates P5 and P6;
     - builds the installers (rule 54).
   - WP-S gates the shared crates, `paddock-setup`'s included.
   - WP-G renames `paddock-pitwall`'s `testing`.
2. **Debrief 1.2.** WP-D, in this order:
   - **DB9 and DB1 first**, the wired switches of a released app:
     - DB9: honour the three executable-path overrides only under `cfg!(debug_assertions)`, or gate them. Fix in Debrief
       1.2, WP-D first item; user chose no 1.1.x patch on 2026-11-09.
     - DB1: gate `DEBRIEF_BROWSER_NO_SANDBOX`. `chromium-driver` gains a launch option that passes `--no-sandbox` from
       the test harness, which the root CI job uses instead of the variable;
   - DB6 to DB8 gated;
   - `required-features` on DB11 to DB13;
   - `fake_handle` renamed;
   - DB2 to DB5 gated, or deleted with `debrief-ai` (D8);
   - the installer build (rule 54).
3. **Dyno.** Done for its doubles by ADR 0033 (Dyno 1.1.1). WP-Y:
   - gate Y6 and Y7;
   - give Y11 `required-features`;
   - register Y8 to Y10;
   - delete the copies (§2.10).

   Its shared crates come with WP-S.
4. **Pitcrew 1.1.** WP-C: gate C1 and fix its prefix check by parsing the URL and comparing the host; gate C2.
5. **Spotter, next release.** WP-T: S2 first (a sandbox switch), then S1, S3 and S4.
6. **Pacenotes and Sighter.** WP-N: rename `testing` and `test-hooks`. The check is on from each app's first shipped
   package; the new-app template carries it.
7. **The demos.** Registered (D5); no code change unless D5 picks another option.
8. **UI mock backends.** Phase 2 (§5, D9).

**Rollback.**
- The check is one `app-gates` step and one register file. Removing the step (integrating agent) turns it off for
  everyone; a dated `temporary` entry turns it off for one instance.
- The gates revert per commit.
- The Pitwall fake-engine move reverts as one commit, with its tests.

## Parallelism and cost (ER-20, rule 51)

Measurements come from [benchmarks.md, 2026-11-09](../benchmarks.md#2026-11-09-test-double-survey-scan-and-shipped-feature-trees)
and Dyno's `docs/performance.md` on `fix/dyno-no-fakes-in-release`.

- **Declaration scan.** 0.10 to 0.82 s per Cargo workspace (Debrief, the largest, 0.81 to 0.82 s), release build, in two
  consistent rounds.
  - A third round was about 15 to 120 times slower in every workspace at once (2.7 to 50.5 s). This was not diagnosed;
    most likely `cargo metadata` waited on a Cargo lock that another agent's build held. So the check holds no lock
    longer than one `cargo metadata` call.
  - Parallelism does not pay: parsing is the cheap part.
  - The cheaper path: one `cargo metadata` per workspace, shared with layer 2's mapping.
- **Switch inventory.** The same parse, with one more visitor pass. Not measured separately; in the survey it did not
  change the scan's time visibly.
- **Shipped feature graph.** 0.2 to 0.4 s per `cargo tree`; Dyno's six in a row took 1.3 to 2.7 s.
  - Cheaper: one call per build invocation (§2.4).
  - Parallel: no; separate `cargo tree` processes in one workspace contend for a lock.
- **Compiling the check.** Dyno's in-crate scan cost 46 s of compile on the Linux leg (run 7767). In `./dev` the binary is
  built or fetched once per input hash (ADR 0022), so an app pays nothing per run.
- **Layer 3** (only Dyno is measured):
  - Linux leg: 49.6 s (run 7767);
  - warm host legs (run 7774): macOS 46 s, about 11 % of the Rust step; Windows 2 min 10 s, about 31 %;
  - one local cold `cargo xwin check`: 6 min 24 s.
  - With `--profile dist` instead of the dev profile, layer 3 compiles in another profile folder. Its cost is not
    measured. WP-1 measures it per app and leg, records it (rule 48), and returns to the user any leg above 3 minutes
    warm (D10).
  - The layer-3 fixture costs seconds.
- **Layer 4.**
  - The byte search is linear in the executables' size. Not measured.
  - The control's extra build recompiles one package with `test-support`. Not measured.
  - It runs in release workflows only.
- **Settings.** No thread-count setting: this is a bounded CI tool, not a product operation.

## Work packages

Estimates are raw days of agent work, before review rounds.

| WP | Package | Depends on | Days |
| --- | --- | --- | --- |
| WP-0 | `./dev check test-doubles`, failing fixtures first: the four fixture workspaces (layers 1, 1b, 2, 3); the shipped-set derivation (Tauri main binary, demos without `mainBinaryName`, `externalBin`, staged binaries from `per-machine.json`, `.nsh` `File` directives, `bundle.resources` and `windows-pre-bundle`, multi-workspace mapping); layer 1 with the bin rule; layer 1b (env, helpers, `"--…"` arms and comparisons, clap); layer 2 (third-party and hakari test features, `test-only` metadata, feature-edge allowances); the per-app controls and 100 % tree coverage; register parsing with dates and trailers; the `debug_assertions` profile assertion; `::error::` output | – | 3–4.5 |
| WP-1 | CI (integrating agent, `.gitea/`): `app-gates` runs the check on Linux for every app and for staged `paddock-setup`; layer 3 with the shipping profile on host legs; the conformance coverage test; nightly; pre-push; cost per app and leg measured and recorded | WP-0, Dyno's `hostRustChecks` landed | 1.25–1.75 |
| WP-2 | `docs/test-doubles.toml` with §4's first entries and the product `[[setting]]`s found by WP-0; the tag refusal in the release kit; the rule text with its number, a lessons entry, wiki notes | WP-0 | 0.5 |
| WP-P | Pitwall: `pitwall-fake-engine` in the `pitwall-server` package, the three flags moved from `pitwall-server`, tests on `CARGO_BIN_EXE_pitwall-fake-engine`, P5 and P6 gated, the check on | WP-0, `7a73cdc59` landed | 1–1.5 |
| WP-S | Shared crates, one change (rules 39, 43): `test-support` gates in `paddock-service`, `-jobs` (and the re-exports in `-log`, `-health`, `-testkit`), `-health`, `-channels`, `-net`, `-watch` (and `-packs`), `-commands`, `-mcp`, `-chromium` (F11, and F12 gated); `required-features` on `paddock-ipc-test-child`; the `paddock-secrets` docs; self dev-dependencies for their `tests/`; every consumer's dev-dependencies (Dyno, Pitwall, the demos, the tools); CHANGELOG entries | WP-0; ADR 0033 WP-2 (`paddock-power`) | 2–3 |
| WP-G | Rename `testing` to `test-support` in `paddock-pitwall` and `paddock-siblings` and in every consumer's dev-dependencies (D3) | – | 0.25–0.5 |
| WP-D | Debrief: DB9 and DB1 first (the `--no-sandbox` launch option in `chromium-driver` for CI), then DB6 to DB8, `required-features` on DB11 to DB13, `fake_handle`, DB2 to DB5 (or deletion, D8), host legs on | WP-0 | 1.5–2 |
| WP-Y | Dyno: Y6 and Y7 gated, Y11 `required-features`, Y8 to Y10 registered, the copies deleted, the check on | WP-0, WP-1 | 0.5–0.75 |
| WP-C | Pitcrew: C1 gated with a parsed-URL host check (its failing test first: `http://127.0.0.1:@evil.host/` must be refused), C2 gated | WP-0 | 0.5–0.75 |
| WP-T | Spotter: S2 first, then S1, S3 and S4 | WP-0 | 0.5–1 |
| WP-N | Pacenotes and Sighter: the feature rename (D3), the check in the new-app template | WP-0 | 0.25–0.5 |
| WP-B | Rule 54: the installer builds with gates on for every app whose release path changes (Pitwall, Debrief, Dyno, Pitcrew, Spotter, and `paddock-setup` through Dyno's and Pitwall's per-machine installers) | each app's WP | 1–1.5 |
| WP-4 | Layer 4: `forbidden_strings` in `release.toml` and `./dev release verify`, with the in-job control build (D6) | ADR 0039 WP-1, WP-P | 0.5–1 |
| WP-11 | Phase 2, UI: the shared `fileReplacements` preset, `mock-backend.release.ts` per app (5), the marker check with the e2e-build control (D9) | WP-0 | 1.5–2 |
| WP-R | Independent re-review of this ADR, and review of WP-0's check (fixtures and controls) | – | 0.5–1 |

**Total: 14.75 to 22.25 days raw.** Without phase 2 (WP-11): 13.25 to 20.25.

**The Pitwall 1.1 path is 9 to 13.5 days.** It is WP-0, WP-1, WP-2, WP-P, WP-S, WP-G, WP-R, and Pitwall's share of WP-B
(0.5–0.75). It fits the 1.1 tag window only if it starts right after the Pitwall PoC (about 2026-11-19 to 25), and
**it blocks the tag** (D4, D7).

**ADR 0039 dependency.** WP-0 works without `release.toml` (from the Tauri configs and `per-machine.json`). The equality
with `release.toml` and WP-4 wait for ADR 0039 WP-1.

After WP-0, two agents can run in parallel:

- an implementer on WP-P, then WP-D, then WP-C and WP-T;
- the integrating agent on WP-1, then WP-S and WP-G.

## Decisions for the user

| # | Decision | Recommendation |
| --- | --- | --- |
| D1 | Make "no test double or test switch ships" a family rule (number assigned on acceptance) for every app and shared crate: Rust now, the UI as phase 2 | **Yes.** Three apps shipped the shape (a wired fake, a fake behind a flag, a sandbox switch and executable-path overrides that cross an elevation), and every app that ships Rust has instances. |
| D2 | Pitwall's fake engine: a separate test-only binary in the `pitwall-server` package (`pitwall-fake-engine`, `required-features`), or the flags kept in `pitwall-server` behind `cfg(feature = "test-support")` | **Separate binary.** `7a73cdc59` lands as is first (ADR 0071); WP-P moves all three flags afterwards. The shipped binary then has the flags in no build. |
| D3 | One feature name: rename `testing` (`paddock-pitwall`, `paddock-siblings`, `pacenotes-core`, `pacenotes-function-api`) and `test-hooks` (`pacenotes-worker`) to `test-support`; keep `mock-runtime`, `probe` and `testdb` as registered test features | **Yes.** One name everywhere; the three kept names gate different things and are listed in `test_features`. |
| D4 | Gate the family doubles (WP-S, WP-G) inside Pitwall 1.1, or register them temporary until `pitwall-v1.1.1` | **Inside 1.1.** Pitwall 1.1 links 9 of them, and its per-machine installer embeds `paddock-setup.exe`, which runs as administrator and links F1 to F3 and F7. `trust_fake` is unreachable today; the worst case is a future change that wires a fake machine into the privileged trust check, and that is what the gate rules out. |
| D5 | The demos' wired `FakeSystem` and `FakeToaster`: (a) a permanent `demo-adapter` exception with `[[feature_edge]]` allowances per demo; (b) named demo adapters in the shared crates; (c) demo-local adapters in each demo's own crate (a small no-op `System` and `Toaster` there) | **(a) now, (c) when a demo is next touched.** (b) adds shared API for two demos (YAGNI). (c) removes the feature edges entirely at about 0.25 day per demo. |
| D6 | Layer 4 (the artifact string search) now, as part of ADR 0039's verifier | **Yes, after WP-P and ADR 0039 WP-1.** It is small, and it sees switches that name rules cannot. |
| D7 | Turn the check on at once with dated `temporary` entries per app (a ratchet), or per app as each migrates | **Ratchet now.** No new instance can land from day one. `pitwall-1-1` expires at `pitwall-v1.1.0`, which makes WP-P, WP-S and WP-G **a Pitwall 1.1 tag blocker**: accepting D7 as proposed accepts that. |
| D8 | Debrief's `debrief-ai` doubles (DB2 to DB5): gate them now, or let the removal of Debrief's own AI in 1.2 delete them | **Register them with remove-by `debrief-v1.2.0`, whichever comes first.** DB9 and DB1 are fixed regardless, first in WP-D; for DB9 the user chose no 1.1.x patch (2026-11-09). |
| D9 | Phase 2 (UI mock backends): with each app's next release after Pitwall 1.1, or Pitwall 1.1 included | **Pitwall 1.1 included if WP-11's shared preset is ready by its feature freeze; otherwise each app's next release.** |
| D10 | Layer 3 on host legs on every branch run for every app (Dyno's D10 generalised), with a return to the user above 3 minutes warm per leg | **Yes.** Dyno measured 46 s (macOS) and 2 min 10 s (Windows) warm; the other apps and the `dist` profile are measured in WP-1. |

## Alternatives rejected

- **Clippy `disallowed-types` or `disallowed-methods`.** Clippy lints `--all-targets`, which unifies `test-support` in
  (Dyno ADR 0033 §3).
- **A grep for use sites.** It misses aliases and re-exports. Layer 3's compile is the use-site check.
- **Every double in its own dev-only crate (`*-testkit`).** The strongest separation, and layer 2's `test-only` rule
  covers such crates. But it needs about 15 new crates, and doubles that need crate-private access cannot move. Allowed,
  not required.
- **A run-time guard** (a double panics in a release build). It fails on the user's machine after shipping.
- **Only the artifact check.** It runs too late for a branch, and it cannot see a double that leaves no string.
- **One copy of Dyno's test per app.** That is eight copies (rule 55), each with about 46 s of compile per run.
- **Treating `debug_assertions` as a test gate.** A release profile can turn it on, and `cfg!` still compiles both
  branches (§1.6).

## Consequences

- A test double or a test switch can no longer reach a user through a spread, a fallback, a flag or an environment
  variable without a dated, approved entry.
- Tests that start a fake as a process use a separate binary of the same package. Integration tests use a self
  dev-dependency.
- The shared-crate change touches every consumer's `[dev-dependencies]` in one change (rule 43).
- Root CI containers that need `--no-sandbox` pass it through test launch options instead of an environment variable
  the product reads.
- The register adds dated paperwork for the deliberate adapters and switches; in return they are visible.
- Pitwall 1.1's tag waits for WP-P, WP-S and WP-G (D7).

## Sources

- Dyno ADR 0033 and its implementation:
  - `git show origin/fix/dyno-no-fakes-in-release:apps/dyno/docs/adr/0033-no-test-fake-in-a-release-build.md`;
  - on `origin/feat/dyno-health-not-checked`: `no_ungated_doubles.rs`, `scripts/check-shipped.mjs` and its test,
    `paddock.ci.json`, `.gitea/actions/app-gates/action.yml`, `scripts/affected.mjs`;
  - Dyno `docs/performance.md` on `fix/dyno-no-fakes-in-release` (runs 7767 and 7774).
- Pitwall: `apps/pitwall/crates/pitwall-server/src/{main.rs,lib.rs,fake_engine.rs}`, `tests/common/mod.rs`, at
  `origin/main`; `origin/fix/pitwall-e2e-engine-health` (`7a73cdc59`); `origin/pitwall/1.1-b1b-b6` (`e20ae50c3`).
- ADR 0071: `origin/docs/adr-0071-ready-or-exit`, the row 1 table and §"Tests of the module".
- Per-machine installers: `.gitea/workflows/app-release.yml:245-256`, `scripts/stage-paddock-setup.mjs:93-96`,
  `apps/dyno/app/src-tauri/windows/per-machine.json` and its `.nsh` hooks.
- Debrief's sandbox switch: `apps/debrief/crates/chromium-driver/src/launch.rs` at `origin/main` and at `debrief-v1.1.1`.
- The review's fixture: `/Volumes/d7sst7/dev-targets/scratch/review-adr-0073/{fx,ws}`.
- Cargo:
  - resolver 2 does not unify dev-dependency features into a build with no test targets
    (<https://doc.rust-lang.org/cargo/reference/resolver.html#feature-resolver-version-2>);
  - `required-features` on `[[bin]]` (<https://doc.rust-lang.org/cargo/reference/cargo-targets.html#the-required-features-field>);
  - `CARGO_BIN_EXE_<name>` for the package's own bins in integration tests
    (<https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-crates>).

  These are relied on as Dyno ADR 0033 and the review's fixture relied on them, not re-read for this revision.
- The survey tool is committed as text in the [research note](../research/2026-11-09-test-doubles-survey.md). Its
  per-item output and the `cargo tree` outputs are in the author's scratch folder; every row above cites its
  `file:line`.
