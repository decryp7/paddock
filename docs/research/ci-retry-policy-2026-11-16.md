# CI retry policy: the standard and a plan for Paddock (2026-11-16)

Research note for the user's request "adopt the standard practice for retries in CI". It is not an ADR: the decisions at
the end go to the user, and an ADR follows only for the shared mechanism if D1 picks it.

**Evidence heads (ADR 0091).** Read at `origin/train/2026-11-15-13` = `3585bcab12d5216d2f63c5828b42382a80abf151` (the branch
base) and `origin/main` = `23213a35eb51058a015625077ef3b6ca32adcba2`. File:line references are at the train head. CI
history: the 400 newest runs on the Gitea server (runs 7575 to 8175, 2026-11-05 to 2026-11-16; 186 failed, 58 cancelled,
156 green), the 503 failed jobs of the failed runs, and their logs (read through the API on 2026-11-16, scratch only,
nothing from them is committed except run ids and error texts of our own tools).

**Nothing here is shipped code.** The mechanism proposed below (`./dev ci retry`, the `paddock_dev::retry` module) is not
built on `origin/main` or any train; it is work package WP-2 below, a prerequisite of the rule text (ADR 0096).

## 1. The standard, with sources

All pages read 2026-11-16. "Read" means the vendor page itself; "unverified" means only a secondary source or a search
snippet was seen.

| System | What it retries | What it never retries (or not by default) | Attempts and backoff | How a retry shows |
|---|---|---|---|---|
| GitLab CI `retry` ([yaml reference](https://docs.gitlab.com/ee/ci/yaml/#retry), read) | A whole job, only for the failure classes in `retry:when` (`runner_system_failure`, `api_failure`, `stuck_or_timeout_failure`, `scheduler_failure`, `data_integrity_failure`, `runner_unsupported`, `unmet_prerequisites`, `archived_failure`, `stale_schedule`, newer `runner_interrupted`, `server_timeout_*`, ...), or the exit codes in `retry:exit_codes` (GA in 17.5) | Nothing by default: "If not defined, defaults to `0` and jobs do not retry". `script_failure` (the job's own commands failed) is a class you must opt into | `retry:max` 0 to 2; no backoff documented | The job shows as retried; the failure class is the server's own field |
| GitLab runner stage attempts ([configure runners](https://docs.gitlab.com/ee/ci/runners/configure_runners.html), read) | The infrastructure phases of a job only: `GET_SOURCES_ATTEMPTS`, `ARTIFACT_DOWNLOAD_ATTEMPTS`, `RESTORE_CACHE_ATTEMPTS`, `EXECUTOR_JOB_SECTION_ATTEMPTS` (Docker "No Such Container") | The script phase | Default one attempt each; the page's example sets 3 | In the job log |
| GitHub Actions ([re-running workflows and jobs](https://docs.github.com/en/actions/managing-workflow-runs-and-deployments/managing-workflow-runs/re-running-workflows-and-jobs), read) | A person re-runs all jobs or the failed jobs (dependents too), same SHA, within 30 days, at most 50 times; `github.run_attempt` counts them | No automatic retry and no step-level `retry:` keyword (none on the pages read) | Manual | A new attempt of the same run |
| `nick-fields/retry` ([README](https://github.com/nick-fields/retry), read), the common step wrapper on GitHub | One step's command, `retry_on: any | timeout | error`, or only `retry_on_exit_code` | Other exit codes when `retry_on_exit_code` is set | `max_attempts` required, `retry_wait_seconds` 10, a required per-attempt timeout | `warning_on_retry: true` by default: a `::warning::` per retry; `on_retry_command` |
| Buildkite ([retry](https://buildkite.com/docs/pipelines/configure/retry), read) | A job by `exit_status` (0-255, `-1` = agent lost), `signal`, `signal_reason` (`agent_stop`, `agent_refused`, `process_run_error`, ...); rules in order, first match wins | Unmatched statuses; cancelled jobs; agent lost (-1) unless named; `"*"` covers 1-255 only | `limit` default 2, max 10, per rule; no backoff documented | A retried job in the UI; `retry.manual.permit_on_passed` and `reason` |
| Bazel ([user manual, `--flaky_test_attempts`](https://bazel.build/docs/user-manual), read) | A failed test, 1 attempt normally, 3 for `flaky = True`, an explicit value up to 10 | | | A pass after a failure is `FLAKY`, but counts as passed for the exit code |
| Google ([2016 blog](https://testing.googleblog.com/2016/05/flaky-tests-at-google-and-how-we.html)) | Comment thread by Google staff: reruns "only used for tests that are marked as flaky or when users specifically request it" (read, comment thread only) | | | The 1.5 % flaky runs and 16 % tests-with-flakiness figures are **unverified** (search snippets only; the article body was not read) |
| Chromium CQ ([cq.md](https://chromium.googlesource.com/chromium/src/+/master/docs/infra/cq.md), read) | "Retry each shard that has a test failure", same build; then a without-patch run of the failing suite | A CL fails only if a test fails with the patch, on retry, and passes without it; `Disable-Retries: true` opts out | One retry per shard | Tests that failed and then passed feed the flake detector and LUCI Analysis (read: [flake_detector.md](https://chromium.googlesource.com/infra/infra/+/26a1ad59d316419b96a98ecfd87dfe4ccb8a6513/appengine/findit/docs/flake_detector.md)); quarantine via `DISABLED_` and TestExpectations **unverified** |
| cargo-nextest ([retries](https://nexte.st/docs/features/retries/), [changelog](https://nexte.st/changelog/), read) | A failed test, `retries` count, `fixed` or `exponential` backoff, `jitter`, `max-delay` | | | `TRY 2 PASS`, `FLAKY 2/3`; flaky passes the run unless `flaky-result = "fail"` (0.9.131, 2026-03-18); JUnit `<flakyFailure>` |
| Playwright ([test retries](https://playwright.dev/docs/test-retries), [TestConfig](https://playwright.dev/docs/api/class-testconfig), read) | A failed test (`retries`, since 1.10; a serial group retries whole) | | | A separate "flaky" category; `failOnFlakyTests` (1.52) fails the run |
| npm ([config](https://docs.npmjs.com/cli/v10/using-npm/config), read) | Idempotent registry reads on network errors and 5xx | | `fetch-retries` 2, `fetch-retry-factor` 10, min 10 s, max 60 s; env `npm_config_fetch_retries` | npm's own log |
| cargo ([config](https://doc.rust-lang.org/cargo/reference/config.html#netretry), read) | "possibly spurious network errors", and a transfer under `http.low-speed-limit` for `http.timeout` | Errors it does not classify as spurious (the exact list in cargo's `retry.rs` is **unverified**: not read) | `net.retry` 3, env `CARGO_NET_RETRY` | `warning: spurious network error (N tries remaining)` |
| rustup ([environment variables](https://rust-lang.github.io/rustup/environment-variables.html), read) | | | `RUSTUP_DOWNLOAD_TIMEOUT` 180 s (unstable); no retry variable is documented there (a `RUSTUP_MAX_RETRIES` is **unverified**) | |
| git ([git-config](https://git-scm.com/docs/git-config), read) | Only HTTP 429 (`http.maxRetries`, default 0, `GIT_HTTP_MAX_RETRIES`) | Every other fetch failure | | |
| curl ([manpage `--retry`](https://curl.se/docs/manpage.html#--retry), read) | Timeouts and HTTP 408, 429, 500, 502, 503, 504, 522, 524 | Connection refused unless `--retry-connrefused`; other errors unless `--retry-all-errors` ("sledgehammer") | Off by default; 1 s doubling to 10 min; `--retry-max-time` caps the whole | stderr only |
| Docker ([dockerd](https://docs.docker.com/reference/cli/dockerd/), read) | Layer downloads, `max-download-attempts` 5 | | | |
| Backoff guidance ([AWS, Exponential Backoff And Jitter](https://aws.amazon.com/blogs/architecture/exponential-backoff-and-jitter/); [Google Cloud Storage retry strategy](https://docs.cloud.google.com/storage/docs/retry-strategy), read) | Idempotent operations only; 408, 429, 5xx, socket timeouts, TCP resets | Permanent errors (authorisation, configuration) | Truncated exponential backoff with jitter (gcloud: base 1 s, factor 2, max delay 32 s) | |
| Flaky tests ([Fowler, Eradicating Non-Determinism in Tests](https://martinfowler.com/articles/nonDeterminism.html), read) | | "Once you start ignoring a regression test failure, then that test is useless"; quarantine, capped in count and time, and fix quickly | | |
| Gitea Actions | Re-run of all jobs, or of one job and its dependents ([PR 29955](https://github.com/go-gitea/gitea/pull/29955), read). "Re-run failed jobs only" and any step or job retry keyword: **unverified** (not in Gitea's own docs; [overview](https://docs.gitea.com/usage/actions/overview) read). Our server reports version 28.3.0 (`/api/v1/version`, read 2026-11-16) and `./dev ci verdict` already reads run attempts (`tools/paddock-dev/src/ci_runs/client.rs:29-31`) | | | Another attempt of the same run |

**What the standard is, in four lines.** (1) Retries are opt-in, few (GitLab at most 2 reruns, Buildkite default 2,
cargo 3, npm 2, `actions/checkout` 3 attempts) and with backoff, preferably jittered. (2) They are scoped to an
infrastructure class that the system can name from its own signals (GitLab's failure classes, Buildkite's exit status
-1 and signal reasons, curl's HTTP code list, cargo's spurious-error classifier), never to "any failure" of the job's own
commands. (3) Test retries exist, but the established systems keep the result visible as FLAKY and feed a flake tracker
(Bazel, Chromium, nextest, Playwright); turning a run green with them is the default in most tools and the thing ER-12
forbids. (4) Every retry is visible: a warning per attempt (nick-fields), a FLAKY status, or an attempt counter.

## 2. Classification

The class is decided by the **signal**, read in this order: the runner's own (job cancelled, runner lost, exit -1), the
tool's exit code and documented classification (curl exit 6, 7, 28, 35, 52, 56 and HTTP 408/429/5xx; cargo's "spurious
network error"; npm `ECONNRESET`, `ETIMEDOUT`, `EAI_AGAIN`), then a stderr pattern from the allow-list. A deny-list is
checked first and always wins (it is how a "failed to download" that is really `--offline` stays a product failure).

| Class | Examples (signal) | Retried? | Who retries | Paddock evidence |
|---|---|---|---|---|
| `network` (transient) | DNS (`EAI_AGAIN`, `Could not resolve host`, curl 6), connect refused or reset (curl 7, `ECONNRESET`), timeout (curl 28, `ETIMEDOUT`), HTTP 408, 429, 500, 502, 503, 504, TLS handshake reset, git `early EOF` from a 5xx | **Yes**, bounded, logged | The tool's own retry first; the shared wrapper where the tool has none | Run 7585 (nightly on main, 2026-11-06): the Gitea server answered 503, then refused connections; `actions/checkout`'s three attempts (15 s and 20 s apart, 02:02:26 to 02:03:43, 77 s) did not outlast it and job 30317 failed. sccache downloads saw HTTP 504 four times in a row in at least 20 failed jobs (for example job 33238, run 7819) and degraded to no cache, as designed (`.gitea/actions/setup-rust/action.yml:141-142`); `taiki-e/install-action` retried cargo-about's download 8 times and then succeeded (job 33284, run 7822) |
| `remote-corrupt` (server says data is bad) | `aborting due to possible repository corruption on the remote side`, a checksum mismatch | **No** | Nobody; fail with the cause | Runs 7872 and 7879 (`paddock-dev` macOS and Windows): a golden test cloned the job's partial checkout and the promisor object was missing: a test-setup defect, not a blip. A changed release asset is a supply-chain question (`conformance/ci/tauri-tools.mjs:291`) |
| `runner` (host or runner state) | slot not acquired, a slot holder killed, workspace move timeout, a toolchain install that conflicts or crosses devices, the runner's own `context deadline exceeded` on cleanup | **No** automatic retry; each has a root-cause fix (ER-44) | The coordinator reruns the job by hand after the cause is known | `job-isolation: no slot`: runs 7943, 8060; `the holder of sN ended before the job did`: 7773, 7774, 7819, 7861, 8019; `stable-workspace: 'move the checkout ...' did not finish within 300 s`: 7585, 7712, 7728, 7761, 7762, 7772, 7793, 7851, 7855, 7919, 7965; rustup `Invalid cross-device link`: 8029, 8043; rustup `detected conflict` (two installs at once): 8060, fixed by `02aa10a14` |
| `product` (code, config, dependency data) | compiler, clippy, `cargo deny` advisory, lockfile stale, offline download refused | **Never** | Nobody | `cargo deny` advisories FAILED, runs 7824 (pitwall, spotter): a real advisory, not a fetch problem. `failed to download ... --offline was specified` in a paddock-shell test, runs 7716, 7795: deterministic, looks like network |
| `test` (assertion, timeout, crash, budget) | nextest FAIL, Playwright failure, start-up budget, fuzz crash | **One** retry at most, flaky-as-failure (ER-12); never green | nextest `retries = 1` + `flaky-result = "fail"`, Playwright `retries: 1` + `failOnFlakyTests` | The large majority of the 503 failed jobs (nextest and Playwright `::error::` lines lead the tally) |
| `measurement` (a sample taken under load) | start-up benchmark discarding samples above load 12 | Re-sampled, never passed: too few samples is `NOT MEASURED` (ER-33) | `apps/debrief/scripts/ci/startup-bench.mjs:17,109-111` | Two `start-up benchmark NOT MEASURED` lines in the window |

**Measured frequency.** In 11 days and 186 failed runs, a transient network error ended exactly one run (7585), and that
was a server outage of at least 77 s rather than a blip. Every other network blip seen was absorbed by a tool's own
retry or degraded on purpose. Runner-state failures (18 runs above) are ten times as common, and none of them is
transient in the sense the standard means: each came back until its cause was fixed. So the gain of more retrying is
small; the gain of **one classifier and honest logging** is in triage (telling the 1 from the 18 and the rest).

## 3. Paddock today: every place that retries

| Where | What | Class handled | Logged? | Gap |
|---|---|---|---|---|
| `apps/*/.config/nextest.toml`, `crates/.config/nextest*.toml`, `tools/*/.config/nextest.toml` (`[profile.ci]`) | `retries = 1`, `flaky-result = "fail"` | test | FLAKY in the report; `./dev ci flakes` | None. Enforced by `scripts/nextest-flaky.test.mjs:37-48` and `tools/paddock-dev/src/ci_runs/preconditions.rs:44-82` |
| `apps/{debrief,dyno,pitcrew,spotter}/app/playwright.config.ts` | `retries` with `failOnFlakyTests` | test | flaky category | None. Enforced by C71 (`conformance/src/checks/C71-test-timing.mjs:503-522`) |
| `actions/checkout@v4` (about 90 uses in `.gitea/workflows`) | 3 attempts, 10-20 s apart (seen in job 30317's log) | network | `Waiting N seconds before trying again` | 77 s outage outlasted it (run 7585) |
| `taiki-e/install-action` (`.gitea/actions/app-gates/action.yml:82`, `.gitea/workflows/debrief.yml:476`, `.gitea/workflows/publish.yml:92`) | its own curl retries | network | curl lines only | No `::warning::` |
| `.gitea/actions/setup-rust/action.yml:141` | `curl -fsSL --retry 3` sccache; on failure `::warning::` and no cache | network | yes (warning) | No `--retry-connrefused`, no `--retry-max-time`; the attempt count is not in the warning |
| `.gitea/actions/gitleaks/action.yml:21` | `curl -fsSL --retry 3` | network | no | Fails the job without an `::error::` naming the class |
| `conformance/ci/host/host-tools.sh:20` and its port `tools/paddock-dev/src/ci_host/host_tools.rs:180` | `curl -fsSL --retry 3` nextest | network | command logged | As above |
| `tools/paddock-dev/src/ci_setup_rust.rs:384` | `curl -fsSL --retry 3` sccache | network | yes | As above |
| `apps/debrief/crates/debrief-store-duck/scripts/fetch-libduckdb.sh:37` | `curl -fsSL --retry 3` | network | no | As above |
| `conformance/ci/tauri-tools.mjs:233,267-291` | 3 attempts, 2 s doubling, only `retryable` (no status, 408, 429, 5xx); annotation on the last | network | yes, per try | Good; the best of the copies |
| `apps/debrief/app/scripts/libduckdb.mjs:38-48`, `apps/debrief/app/e2e-tauri/scripts/fetch-msedgedriver.mjs:75-85` (from `1c7426ca5`) | linear 2 s backoff on network errors and 5xx | network | `console.error`, not an annotation | Copies of one mechanism, unregistered (rule 55) |
| `tools/paddock-dev/src/ci_runs/client.rs:185-195` | every Gitea API read: 4 attempts, 0.5/1/1.5 s, **any error but 404** | everything | **no** | Retries 401 and 403 (not transient) silently; ER-27 |
| `tools/paddock-dev/src/ci_runs/dispatch.rs:550-590` | `ci runner-wait` reads its own run `OWN_RUN_READS` times on top of the client's retries | network | yes | Nested retry (attempts multiply) |
| `scripts/ci-timings.mjs:44-52` | 4 attempts, linear | everything | no | Retired by `docs/plans/ci-in-rust.md` WP-9; no CI user |
| `dev:440-460` | the prebuilt `paddock-dev` download: **one** attempt, then build it (minutes) | network | `::warning::` with HTTP code | One blip costs a whole tool build |
| `dev:115,133,270` | `rustup toolchain install`: one attempt, bounded, one install at a time | runner | `::error::` with cause | Correct not to retry (8029, 8043, 8060 were not transient) |
| `tools/paddock-dev/src/ci_host/kept_clone.rs` | `git fetch` of the kept clone: no retry, the job falls back to its own checkout | network | `git fetch failed` line | Fine (degrades) |
| `npm ci` (`.gitea/workflows/ci.yml:207,302,347,401,769,812,1134,1300`; `nightly.yml:92,158,520`; `debrief.yml:540,635,811,824,1087,1088,1290`; `canary.yml:106,118`) | npm's own `fetch-retries` 2 (no `.npmrc` override) | network | npm log | None seen failing in the window |
| `cargo install` and builds (`ci.yml:1165,1455`, `nightly.yml:531`, `debrief.yml:439,788,1267,1287`, `app-fuzz.yml:107`, `debrief-fuzz.yml:110`, `debrief-mutants.yml:117`, `app-release.yml:469`) | cargo `net.retry` 3 | network | `spurious network error` warnings | None seen failing |
| `npx playwright install` (`ci.yml:357`, `shell-demo-release.yml:74`, `workbench-demo-release.yml:74`) | Playwright's own downloader (its retry behaviour **unverified**) | network | | No bound we control |
| `continue-on-error` (`debrief.yml:307` memory budget, `ci.yml:506` hooks tests, `ci.yml:591` engineering-rules checkout) | not retries: report-only steps by decision (ER-41) or an optional checkout | | | `ci.yml:591` fails softly with no `::warning::` of its own |
| `apps/debrief/scripts/ci/startup-bench.mjs:17,109-111` | re-samples under load, `NOT MEASURED` when short | measurement | yes | None |
| Reruns of failed jobs | by hand; `./dev ci verdict` counts a run green after a rerun as red when an earlier attempt failed (`tools/paddock-dev/src/ci_runs/verdict.rs:111-130`) | any | yes | Right for ER-15; makes an infra-only failure cost a full rerun |

## 4. Proposed policy

1. **Only the `network` class is retried automatically.** At most 3 attempts in all, truncated exponential backoff with
   full jitter (base 10 s, factor 3, so about 10 s and 30 s, each capped at 90 s), and a whole-retry cap of 180 s. The
   cap is the measured cause ER-12 asks for: the one outage in the window lasted at least 77 s (run 7585, job 30317), and
   180 s is about twice that; the attempt count matches cargo (3) and `actions/checkout` (3).
2. **Use the tool's own retry first** and set it explicitly in one place: `CARGO_NET_RETRY=3`, `npm_config_fetch_retries=2`
   (both their defaults, written down so a change is deliberate), every `curl` with
   `--retry 3 --retry-connrefused --retry-max-time 180`. No `--retry-all-errors`.
3. **One shared mechanism where a tool has none or where our code retries**: `./dev ci retry --class network --max 3
   [--max-time 180s] -- <cmd>` in CI steps, and the same module (`paddock_dev::retry`) in-process for the `./dev` Gitea
   client and the kept clone. It: runs the command with the caller's environment (ER-47); classifies a failure by exit
   code, then a deny-list, then the class's allow-list of stderr patterns (the table in §2); retries only a match;
   prints `::warning::retry 1/3 (network: HTTP 503 from <host>) in 12 s: <cmd>` per retry and, on the last failure, one
   `::error::` with the class, the attempts, the last stderr lines and the next step (ER-27); appends one line to the job
   summary and to `$RUNNER_TEMP/paddock-retries.jsonl` so `./dev ci triage` can show "passed after a network retry".
   It refuses, with exit 2, to wrap a test runner (`cargo nextest`, `cargo test`, `npx playwright test`, `vitest`,
   `npm test`, `./dev ci run`): tests have their own one-retry, flaky-as-failure rule.
4. **The release path keeps shell only** (a tag run never downloads `./dev`, `.gitea/actions/setup-rust/action.yml:93-95`):
   there the curl flags of point 2 are the mechanism; no hand-written retry loops in shell.
5. **`runner`, `remote-corrupt`, `product` and `test` failures are never retried by any wrapper.** A runner failure gets
   its root fix; a person may rerun the job after the cause is known, and `./dev ci verdict` keeps counting that run red
   (unchanged).
6. **Tests: unchanged** (ER-12): one retry with flaky-as-failure, and a FLAKY result blocks only when the change reaches
   the test (the user's decision of 2026-11-16).
7. **The JS copies** (`tauri-tools.mjs`, `libduckdb.mjs`, `fetch-msedgedriver.mjs`) are registered in
   `docs/known-copies.toml` against the canonical Rust module, with `tauri-tools.mjs`'s `retryable` as the JS reference
   until they move to `./dev` (rule 55).

## 5. Steps to change

| WP | File:line | Change |
|---|---|---|
| WP-1 | `.gitea/actions/setup-rust/action.yml:141`, `.gitea/actions/gitleaks/action.yml:21`, `conformance/ci/host/host-tools.sh:20`, `tools/paddock-dev/src/ci_host/host_tools.rs:180`, `tools/paddock-dev/src/ci_setup_rust.rs:384`, `apps/debrief/crates/debrief-store-duck/scripts/fetch-libduckdb.sh:37` | `--retry 3 --retry-connrefused --retry-max-time 180`; the failure message names the class and the attempts. Parity test `tools/paddock-dev/tests/it/host_steps.rs` updated with the shell form |
| WP-1 | `dev:440-460` | The prebuilt `paddock-dev` download gets the same curl flags (a 5xx or refused connection no longer costs a tool build) |
| WP-1 | One env block for CI jobs (the `build-env` action, `.gitea/actions/build-env/action.yml`) | `CARGO_NET_RETRY=3`, `npm_config_fetch_retries=2`, written down with their sources |
| WP-1 | `.gitea/workflows/ci.yml:589-593` | The optional checkout prints a `::warning::` when it fails |
| WP-2 | new `tools/paddock-dev/src/retry.rs` and `ci retry` subcommand | The classifier, backoff, logging, refusal of test runners, the JSONL record; table tests from invented same-shape stderr (ER-14) |
| WP-3 | `tools/paddock-dev/src/ci_runs/client.rs:185-195` | Use `retry.rs`: retry only network, never 401/403, log each retry to stderr |
| WP-3 | `tools/paddock-dev/src/ci_runs/dispatch.rs:550-590` | One retry layer: the outer loop keeps its own reason (waiting for its own run to appear), the inner client stops multiplying it |
| WP-3 | `.gitea/workflows/ci.yml:357`, `shell-demo-release.yml:74` (release path: curl-less, so unchanged), `workbench-demo-release.yml:74` (same) | `./dev ci retry --class network -- npx playwright install webkit` on the non-release step |
| WP-3 | `docs/known-copies.toml` | Entry for the retry mechanism: canonical `retry.rs`; copies `conformance/ci/tauri-tools.mjs`, `apps/debrief/app/scripts/libduckdb.mjs`, `apps/debrief/app/e2e-tauri/scripts/fetch-msedgedriver.mjs` (delete-by: their move to `./dev`); `scripts/ci-timings.mjs` leaves with WP-9 of `docs/plans/ci-in-rust.md` |
| WP-4 | `tools/paddock-dev/src/ci_runs/triage.rs` (the `triage` command) | Read `paddock-retries.jsonl` from the job's artifacts or log lines and print "network retry: N" per job, so a reviewer sees retried infrastructure |
| WP-4 | A conformance check (id from `./dev check new-id`) | Fails a `curl` in `.gitea/`, `conformance/ci/`, `dev` or `tools/` without `--retry`, any `--retry-all-errors`, and a shell `until`/`for` loop around curl, npm, cargo or git in a workflow. Report-only first (ER-41) |

Not changed, on purpose: `dev:115,133,270` (rustup install; not transient, runs 8029, 8043, 8060), the kept clone's
fetch (degrades), the nextest and Playwright retries (already the standard plus ER-12), the start-up benchmark's
re-sampling (ER-33), and the three `continue-on-error` report-only steps (decisions, not retries).

## 6. Rule text to add

As a Paddock rule (next free number on the open rules branch is 85; `origin/docs/rules-2026-11-16` holds 80 to 84) and
as ER-48 if the user wants it family-wide:

> **85. Only infrastructure is retried, by one mechanism, and every retry is logged.** A CI step retries automatically
> only a failure of the `network` class (DNS, refused or reset connection, timeout, HTTP 408, 429 and 5xx), recognised by
> the tool's exit code or classifier, or by `./dev ci retry --class network`'s pattern list: at most 3 attempts with
> jittered exponential backoff and a 180 s cap (measured cause: run 7585, a 77 s server outage). Use the tool's own retry
> first (`curl --retry 3 --retry-connrefused --retry-max-time 180`, `CARGO_NET_RETRY`, npm `fetch-retries`); never
> `--retry-all-errors`, never a hand-written retry loop. Each retry prints a `::warning::` with the class and the attempt,
> and the last failure one `::error::` with the cause and the next step. Runner, product and test failures are never
> retried by a wrapper; tests keep ER-12's one retry with flaky-as-failure, and a rerun of failed jobs never makes a run
> count as green (`./dev ci verdict`). Why: on 2026-11-16 a survey found seven separate retry loops, one of them silently
> retrying refused credentials, while in 186 failed runs only one was a network failure and eighteen were runner state
> that retries would have hidden.

## 7. Cost against common practice (ER-37)

| Option | What | Agent-days |
|---|---|---|
| A, common practice | WP-1 only: tool-native retries set explicitly, curl flags everywhere, messages that name the class; fix `client.rs` not to retry 401/403 and to log (ER-44: a found defect is fixed in the same round) | **0.75** (0.5 to 1) |
| B, recommended | A + WP-2 (shared classifier and `./dev ci retry`, about 1.25) + WP-3 (adopt in `./dev`, register copies, about 0.5) | **2.5** (2 to 3.25) |
| C, full | B + WP-4 (triage shows retries; a conformance check, report-only first) | **3.5** (3 to 4.5) |
| D, beyond the standard | C + `./dev ci verdict` accepts a rerun whose earlier failures were all classified `network` (GitLab's `retry:when: runner_system_failure` equivalent) | +1 = **4.5**; changes ER-15's reading |

What comparable systems do: GitHub and Gitea offer only manual reruns plus actions with their own retry (option A is
exactly that); GitLab and Buildkite add a declared, class-scoped job retry (closest to B's classifier); Chromium and
Bazel retry tests but keep FLAKY visible (Paddock already does this). B costs about 1.75 agent-days more than A, so it
needs the user's yes (ER-37).

## Decisions for the user

- **D1.** Option A (0.75 d, common practice) or B (2.5 d, one shared classifier and wrapper) or C (3.5 d, plus the check
  and triage view). Recommendation: **B**, because seven hand-made copies already exist (rule 55) and one of them hid
  credential errors; take WP-4 later if the copies come back. Fallback: A.
- **D2.** The 180 s cap and 3 attempts (from run 7585's 77 s outage and the cargo and checkout defaults), or another
  number with its own measured cause.
- **D3.** Keep `./dev ci verdict` counting a rerun as red even when every earlier failure was `network` (recommended:
  keep; 1 such run in 186), or adopt option D.
- **D4.** Paddock rule 85 only, or also ER-48 in the engineering rules.

## Risks

- A pattern list can misclassify: "failed to download" under `--offline` (runs 7716, 7795) looks like network. The
  deny-list is checked first, and the table tests start from these real shapes (invented paths and hosts).
- Retrying a non-idempotent step (a publish, a tag request, a release upload) can duplicate it; the wrapper is for reads
  and downloads only, and `publish.yml` and `tag.yml` steps stay unwrapped.
- Nested retries multiply waits (`dispatch.rs` over `client.rs`); WP-3 removes the inner layer.
- A longer cap slows a real outage's failure by up to 3 minutes per step; jobs fail at their first such step, so a run
  pays it once.

Recurring: yes. Class "hand-made network retry loops": `1c7426ca5` (libduckdb, msedgedriver), `conformance/ci/tauri-tools.mjs`,
`tools/paddock-dev/src/ci_runs/client.rs`, `scripts/ci-timings.mjs`, `dispatch.rs`; the mechanism above is its redesign.
