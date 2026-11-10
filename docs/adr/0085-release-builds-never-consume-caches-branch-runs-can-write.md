# 85. Release builds never consume caches that branch runs can write

- Status: Proposed (2026-11-10). Drafted at the user's request of 2026-11-10; its fix gates the next app tag (WP-1 and WP-2,
  §8).
- Date: 2026-11-10
- Related: rule 9 / ER-15 (releases only from a tag in CI), rule 23 (ad-hoc seal), rule 25 (before tagging), rule 54
  (installers before landing; its dispatches are the "branch runs" here), rule 71 / ER-33 (a check counts only if its
  probe has shown it can see the thing), rule 48 (benchmarks recorded), ER-27 (logs explain every failure); ADR 0022
  (`./dev`; "release and tag jobs bypass every cache", which the workflows do not implement, §2.4), ADR 0025 (job
  isolation: slots, kept folders, one job per Linux host), ADR 0080 (CI speed; its D4 shared cache service must not reach a
  tag run, D9 below), the kept-clone `fresh` input of `030ef0d8e` on `train/2026-11-09-3` (a first step, §2.5).
- Number: free on every `origin` branch on 2026-11-10 (`git ls-tree` of every remote branch: no `008[1-9]` ADR).
- Evidence read at `origin/train/2026-11-09-3` `66b600669` unless a row names a run. CI facts come from the Gitea job records
  and job logs of the runs named (read through the API on 2026-11-10); the poisoning experiments are synthetic (§2.6).
  Nothing was read from the runners themselves; the runners' `config.yaml` is not in the repository, so the Linux bind
  mount is known only from what the job logs and scripts show.

## 1. Pattern

**Every cache CI keeps is shared by branch runs and tag runs, and nothing a tag run reads from one is verified.** A job of
any branch (a pull request, a rule 54 dispatch, a Renovate branch) runs as the same OS user on the same host as a tag run,
writes the same kept folders and the same compile cache, and a later tag build reuses what it wrote. Whatever code ran in
that branch job (the branch's own, or any build script, proc macro, npm install script or test of its dependencies) can
leave compiler output, sources or tools that the signed installers then carry.

**The invariant that should hold.** A tag run's build reads only: the tagged commit fetched for this run, the pinned CI
image (Linux), the toolchain, and downloads verified against the tagged lockfiles or a pinned checksum. Every folder it
builds in is created by the run and dies with it. A check in the tag run proves this and fails the run otherwise.

## 2. What a tag run reads today, and who can write it

### 2.1 The tag run of `dyno-v1.1.0` used every shared cache (verified)

Run 7695 (`dyno-release.yml`, tag `dyno-v1.1.0`, commit `8efce3fc3`), the bundle jobs' logs:

| Leg | Build folder | Compile cache | npm / Angular | Crates |
|---|---|---|---|---|
| macOS (job 31477) | `/Volumes/d7sst7/ci/home/target-dyno-bundle-s1` (kept, slot s1) | `sccache-s1`: 173 requests, **150 hits**, 0 misses | `npm-s1`; `.angular/cache` linked to `angular/dyno-bundle-s1` | 0 downloaded: the runner user's `~/.cargo` |
| Linux (job 31478) | `apps/dyno/target` (new container) | `/sccache` mount: 1,039 requests, **821 hits** | `/sccache/npm`; `.angular/cache` linked to `/sccache/angular/dyno-bundle` | 515 downloaded: the image's `CARGO_HOME`, new each run |
| Windows (job 31479) | `C:/Users/<runner>/.paddock-ci/target-dyno-bundle-s1` (kept) | `sccache-s1`: 307 requests, **210 hits**, 19 misses | `npm-s1` | 0 downloaded: `~/.cargo` |

The same kept folder names (`<app>-bundle` plus the slot) are written by every `dyno-release.yml` dispatch from a branch
(rule 54): runs 7848 to 7865 on `train/2026-11-09-2` built `dyno-bundle` in the same slots two days later. This is not
evidence that 1.1.0 was poisoned; it shows that the path was open.

### 2.2 Inventory: what a tag run reads, per OS

"Writable by a branch run" means: a job of any branch can write it as part of its normal steps (same names or same
paths), and in any case as the same OS user (host runners) or through the same bind mount (Linux).

| State | Linux container | macOS host | Windows host | Read by the tag build | Verified by |
|---|---|---|---|---|---|
| Compile cache (sccache, local disk) | `/sccache` (bind mount shared by every container on the host, ADR 0025 §8) | `<ci-home>/sccache-s<n>` | same | yes, `setup-rust` default `sccache-backend: local` | run 7695 stats; `setup-rust/action.yml:143-165` |
| Shared sccache backend (Redis/Dragonfly `172.16.1.188:6379` db 5, no password) | only if a job passes `sccache-backend: auto\|redis` | same | same | **no** today: no workflow passes it (`git grep sccache-backend: .gitea` finds only the input's definition) | grep; `docs/dev-setup.md:73` |
| act_runner cache server (`172.16.1.188:8089`, `actions/cache`) | used by nightly, Debrief bench/corpus/mutants, fuzz, plan | same | same | **not by the bundle path**; Debrief's `licences` job (a tag gate) restores `registry/index`, `registry/cache` and `advisory-dbs` with a prefix `restore-keys` | `debrief.yml:418-430` |
| Rust build folder | `target/` in the new container (clean) | `<ci-home>/target-<app>-bundle-s<n>` (kept) | same | yes: cargo reuses any artifact whose fingerprint looks fresh | `build-folder.sh`; run 7695 |
| Checkout | fresh `actions/checkout` | kept clone `src/<name>-<slot>`; since `030ef0d8e` made again on a tag (`fresh`) | same | tag: the fresh clone, at the slot's path | `kept-clone.sh` |
| Debrief checkout | fresh | `stable-workspace` moves the job's checkout (fresh) | `stable-workspace` keeps a clone: `clean -ffd` without `-x` (ignored files stay), git hooks not disabled (a planted `post-checkout` runs) | yes | `stable-workspace/action.yml:88-100` |
| Cargo home (registry index, `.crate` archives, extracted sources, `~/.cargo/bin`) | image's `/usr/local/cargo`, new each run | the runner user's `~/.cargo` | same | yes, sources and archives are **not re-verified** (§2.6) | run 7695 (0 downloads on hosts) |
| npm download cache | `/sccache/npm` | `npm-s<n>` | same | `npm ci` reads it (integrity-checked by npm against the lockfile; read, not measured here) | `node-caches` |
| Angular build cache | `/sccache/angular/<name>` | `angular/<name>-s<n>` | same (not for Debrief Windows) | yes, linked into the app as `.angular/cache` | `node-caches`; run 7695 |
| Debrief `node_modules` | npm ci | npm ci | **kept per lockfile hash** in `node-modules/debrief-s<n>`, junctioned in; `npm ci` skipped when it exists | yes (Windows) | `windows-node-modules.sh` |
| Debrief `libduckdb` (shipped in the installer) | image's `/opt/libduckdb` or job temp | `libduckdb/<job>-s<n>`; the SHA-256 is checked only when the folder has no `.verified` marker | same | yes; the marker is a file anyone can write | `fetch-libduckdb.sh:34` |
| `paddock-dev` (`./dev`, run by `job-isolation` as the slot holder for the whole job) | `/sccache/paddock-dev/bin-source/<hash>` | `<ci-home>/tools/paddock-dev/bin-source/<hash>` (scope `shared`) | same | yes: a cached binary that answers `--version` is run; `self hash` is checked only for a new build or a download | `dev:280-312`; `job-isolation.sh:186-193`; no `dev: built` line in run 7695's host logs |
| sccache binary | image's | first `sccache` 0.19.0 on `PATH` (the shared Mac: `/opt/homebrew/bin`, user-writable); the checksum is checked only on a download | same | yes | `setup-rust/action.yml:72-109` |
| Toolchain, Node, git and its global config, shell profile, login items | image | the runner user's (the macOS runner runs as the developer's own account: `/Users/decryp7/gitea-runner`) | the runner user's | yes | job logs (work folder paths) |
| CI image | `paddock-ci:v2`, `garage:ubuntu-24.04-…`, pulled by **mutable tag**; `ci-image.yml` can push `:v2` from any branch's dispatch | n/a | n/a | yes | `ci-image.yml`; `app-release.yml:121` |
| Updater signing key, `RELEASE_TOKEN` | passed to the bundle job of **every** dispatch, not only tags | same | same | in the environment of `tauri build`, so of every build script and proc macro it runs | `dyno-release.yml:129-134` |

### 2.3 Two different exposures

1. **The data path (this ADR's core).** State that a branch job leaves through the normal cache mechanisms: sccache
   entries, kept build folders, `~/.cargo`, npm and Angular caches, kept `node_modules`, `libduckdb`, the cached
   `paddock-dev`. Closing it needs no new machine: the tag run must not read any of it.
2. **The code-execution path.** On a host runner a branch job runs as the same OS user as the tag job. It can change
   anything that user can: the toolchain under `~/.rustup`, `~/.cargo/bin`, Homebrew, `~/.gitconfig` (for example
   `url.insteadOf` to redirect the tag run's fetch), the shell profile, or install a login agent that waits for the next tag
   job and edits its outputs or reads its environment (the signing key). No choice of folders inside the job closes this;
   only another identity does (a separate OS user, host or throw-away VM for tag jobs, D6). On Linux the container leaves
   nothing behind except through the `/sccache` mount (unless the runner also mounts the Docker socket or runs privileged:
   not visible from the repository, to confirm on the host).

### 2.4 ADR 0022 claims more than the workflows do

ADR 0022 says "Release and tag jobs bypass every cache. They set `PADDOCK_DEV_FRESH=1`". No workflow sets
`PADDOCK_DEV_FRESH`; the release jobs set `PADDOCK_DEV_DOWNLOAD: "0"`, which still runs a cached binary from
`bin-source/<hash>` on the shared mount or tools folder, and the rest of the bundle job (sccache, build folder, caches)
was never covered. The comment "release jobs bypass every cache" is repeated on eight jobs (`app-release.yml:149,404`,
`ci.yml:238,706,923,1095`, the demo release workflows, `debrief.yml:860`). WP-8 corrects ADR 0022 by amendment.

### 2.5 What `030ef0d8e` (kept clone `fresh` on tags) does and does not do

It makes the tag run's source tree come from objects fetched in that run, so a planted object, ref, hook or config in the
slot's clone is gone. It does not touch the compile cache, the build folder, `~/.cargo`, the npm and Angular caches or
`paddock-dev`, which are where compiled code comes from. It also leaves Debrief's bundle (on `stable-workspace`) unchanged.

### 2.6 Poisoning shown on synthetic crates (verified; ER-33)

Three experiments on this Mac (cargo 1.99.1, sccache 0.19.0), in a scratch folder, with invented crates; nothing of a
real build or a runner was touched. Each shows that a writer of one cache changes what a clean source builds:

| Cache | What the writer did | Clean rebuild | Result |
|---|---|---|---|
| sccache local disk | built `victim` (`answer() -> 42`) and a changed `victim` (`-> 666`) through one `SCCACHE_DIR`, then copied the second entry's file over the first entry's key | source says 42, **empty target folder**, same cache | prints `answer=666`; stats: 1 hit, 0 misses |
| Kept build folder | built 42 into folder A and 666 into folder B, copied B's `libvictim-*.rlib` into A | source says 42, cargo finds A fresh, relinks `app` | prints `answer=666` |
| Cargo home: extracted source | appended `compile_error!("TAMPERED…")` to `registry/src/…/itoa-1.0.11/src/lib.rs` | **empty target folder**, `Cargo.lock` unchanged | cargo compiles the changed source: `error: TAMPERED registry source used` |
| Cargo home: `.crate` archive | deleted `registry/src`, rewrote `registry/cache/…/itoa-1.0.11.crate` with the same change | `cargo build --locked`, empty target | cargo unpacks the changed archive without a checksum error: `TAMPERED crate archive used` |

So neither a fresh build folder alone (sccache, `~/.cargo` still poison it) nor `--locked` protects a build whose cargo
home or compile cache another job can write. The commands are in §11.

## 3. Threat cases, and what this ADR changes

The apps are personal and non-commercial, and every branch is the owner's (or an agent's working for the owner). The
updater installs signed updates on the owner's machines without asking; Dyno ships a privileged helper and per-machine
installers. So the impact of a poisoned release is high even with one user, and the likelihood is low.

| # | Case | Today | After WP-1, WP-2 | Residual |
|---|---|---|---|---|
| T1 | **A compromised dependency in a branch build** (a Renovate branch with a new version, a crate or npm package an agent adds to try something): its build script, proc macro, install script or test runs as the runner user during a pull request or rule 54 dispatch | it can write every row of §2.2 and the next tag ships the result; during a dispatch it also reads the updater signing key from `tauri build`'s environment | the data path is closed on every OS (the tag run reads no row of §2.2 except the toolchain and the image); with D7 a dispatch build never sees the real key | on hosts, persistence through the runner user (§2.3 item 2) until D6; the dependency must target Paddock's layout, which generic malware does not, but it may still steal what the user's account holds |
| T2 | **A malicious or mistaken branch of the owner or an agent** (a hijacked agent: prompt injection from a web page, an issue or a dependency's docs): it controls the workflow files | it can do all of T1, and also dispatch a workflow that hands it the signing key, push the CI image `:v2`, or run anything as the runner user | the cache path is closed, but this attacker does not need it | **not closed by this ADR**: Gitea gives repository secrets to any branch's dispatch; closing it needs secrets bound to tags on the server (to research: whether the Gitea version in use scopes secrets by ref or environment, unverified) and D6 |
| T3 | A compromised dependency **in the tagged lockfile** | ships | ships | out of scope: cargo-deny advisories (rule 25), lockfile review, ADR 0073 |
| T4 | A LAN device or job writes the Redis compile cache (no password) or the act_runner cache server | not read by the bundle path; the Debrief `licences` tag gate restores a cargo registry and the advisory database by prefix, so a planted stale database can hide an advisory | no `actions/cache` and no remote sccache in any job a tag reaches (D9, a conformance check) | the shared cache service of ADR 0080 D4 is named in D9 so it never reaches a tag |
| T5 | **Accidental** reuse: a branch's leftovers (another feature set, an old `ui/dist`, a stale `node_modules`) end up in a tag's installers (the rule 54 incident was this kind) | possible on hosts | gone: the tag builds from an empty folder | none |

Honest weighing: T1 is the case this ADR is for, and the data path is the cheap part of it (about 3.5 agent-days and
tens of runner minutes per tag, §6). T2 is stronger and is not stopped by any cache rule; D6 and the secret scoping are
separate, larger decisions. T5 alone (reproducible tag builds) would justify WP-1.

## 4. Decision

A tag run (`github.ref` starts with `refs/tags/`) builds in **release mode**:

1. **Nothing kept is read.** Every folder the build uses is under the runner's per-job folder (`$RUNNER_TEMP`), created
   by the run: the clone (the kept-clone `fresh` clone moves from the slot's `src/` to the job folder), the build folder,
   `CARGO_HOME` (a fresh registry download against the tagged lockfiles; a cold `cargo fetch` of all eight workspaces took
   34.3 s on this Mac, `docs/benchmarks.md` 2026-11-08 C99 block, and the Linux leg already downloads its 515 crates
   every run), the npm cache, `node_modules` (`npm ci`), and
   Debrief's `libduckdb` (always downloaded and checked against the pinned SHA-256; the `.verified` marker is ignored).
   The kept Angular cache is not linked; the build writes its cache inside the job's own checkout.
2. **No compile cache.** `setup-rust` leaves `RUSTC_WRAPPER` unset and starts no sccache server; no Redis, no WebDAV, no
   `actions/cache`.
3. **`paddock-dev` is built from source in the job**, with no compiler wrapper, into an empty folder made for this run,
   so the slot holder is not a cached binary and ADR 0022's text then holds. `./dev`'s existing `PADDOCK_DEV_FRESH=1` builds
   into a temp folder inside the job folder, which the slot holder refuses on purpose (`slots.rs`
   `check_program_outside_owner`: a program there keeps Windows from deleting the job folder). So release mode points
   `PADDOCK_DEV_HOME` and `PADDOCK_DEV_TARGET_DIR` at a new `release-<run id>-<job>` folder under the CI home's `tools`
   (removed first if it exists, swept by age like the others), outside the job folder and never read by another run.
4. **The image is pinned by digest** for the release jobs (D8).
5. **The real signing key reaches only tag runs** (D7); a rule 54 dispatch signs with a throw-away key generated in the
   job, so the signing path is still exercised.
6. **A check fails the tag run if any of this did not hold** (§5), and the release job needs it.

Release mode is one mechanism, `conformance/ci/release-isolation.sh setup|verify` (with a thin action), read by the
existing actions through one variable, `PADDOCK_RELEASE_MODE=1`, and the overrides they already have (`PADDOCK_SCCACHE_MOUNT`
in `setup-rust`, `PADDOCK_CI_MOUNT` in `node-caches` and `./dev`). Branch and dispatch runs keep every cache: the speed
of ADR 0080 is untouched outside tags.

## 5. The check

`release-isolation verify`, the last step of every tag-run job that builds or publishes (`if: always()`), fails the job
with one `::error::` per finding (the kind, the path, the next step) and writes a summary line to the job summary.

1. **The ledger.** Every action that chooses a folder or a cache appends one line `kind<TAB>path<TAB>mode` to
   `$RUNNER_TEMP/release-isolation/ledger.tsv` (`setup-rust`: `sccache off`; `build-folder`: `target <path> job`;
   `kept-clone`: `workspace`; `node-caches`: `npm`, `angular`; `./dev`: `dev-tool`; Debrief: `libduckdb`, `node-modules`).
   Verify fails when a line's mode is not `job`, `run` or `off`, when a `job` path (canonical; `cygpath -u` on Windows) is
   not under `$RUNNER_TEMP`, or when a `run` path (only `dev-tool`, §4 item 3) holds a file older than the job's start. A kind the job's declared list expects but the ledger lacks is **check broken**, never a pass (rule 71).
2. **Probes that do not trust the ledger.**
   - `RUSTC_WRAPPER`, `CARGO_BUILD_RUSTC_WRAPPER` and `RUSTC_WORKSPACE_WRAPPER` are empty, and no sccache server answers on
     the job's port (or `--show-stats` shows 0 compile requests).
   - `PADDOCK_BUILD_FOLDER`, `PADDOCK_WORKSPACE`, `CARGO_HOME` and `npm_config_cache` resolve under `$RUNNER_TEMP`, and so
     does the `target-dir` that `.cargo/ci.toml` names.
   - No file in the build folder or in `CARGO_HOME/registry` is older than the marker `setup` wrote at the job's start
     (`find … ! -newer marker`): the folders were empty when the build began. The count of files checked is logged, and a
     count of 0 is check broken (the probe saw nothing).
   - `<app-dir>/.angular/cache` is absent or a real folder under the job, never a link or junction.
   - Linux: `/sccache` (or `PADDOCK_CI_MOUNT`) holds no file newer than the marker that this job wrote (a write would mean
     a cache was used).
3. **Output.** `release isolation: 9 kinds job-local, 0 shared; 41,212 build files and 6,830 registry files newer than the
   job start; no compile cache` (numbers per job), or the `::error::` list. The `release` job `needs` the bundle jobs, so a
   red check stops the publish.
4. **Static check (conformance, next free C number).** Every job reachable from a tag (`app-release.yml` bundle, keychain,
   release; `debrief.yml` bundle, keychain, release, and the gates' jobs that feed a published file) runs `setup` first and
   `verify` last; none of them uses `actions/cache*`, `sccache-backend: auto|redis`, `stable-workspace`, or a kept-folder
   kind of `kept-folders.txt` outside release mode; no release workflow names an image by tag.

### 5.1 How it is shown to see the thing (rule 71 / ER-33)

- **Unit tests** (`conformance/test/release-isolation.test.mjs`, `node --test`, synthetic folders under the test scratch):
  for each kind, a ledger or environment pointing into a fake CI home makes verify fail and name it; a build-folder file
  older than the marker fails; an Angular cache that is a link fails; an empty ledger or a probe that checked 0 files
  reports check broken; a clean fixture passes. A mutation check (delete each probe in turn) makes a test fail.
- **A positive control in CI, once, before the first isolated tag:** the release-mode dispatch (`release-mode: true`,
  WP-3) with `isolation-probe: leak` keeps one cache shared on purpose (the Angular link and sccache); verify must go red
  on every OS and name both. The run number goes into this ADR's verification section; without it the check does not
  count.
- **The real proof:** a green release-mode dispatch on the release candidate (WP-3), then the tag run itself, each showing
  the summary line with non-zero file counts and `no compile cache`.

## 6. Cost

From the CI records in [docs/benchmarks.md, 2026-11-10 release bundle jobs](../benchmarks.md) (rule 48). No fully cold
bundle exists in the records, so the cold side is an estimate, to be replaced by WP-3's measurement.

| Leg | Warm, tag 7695 | Cold, closest record | Estimate isolated |
|---|---|---|---|
| Linux | 5.5 min (821 sccache hits; crates already downloaded every run) | 7851: 80 misses, cargo 1 min 43 s + 3 min 11 s, job 8.2 min | 10 to 15 min (all 1,039 compiles on 16 cores) |
| macOS | 3.9 min (150 hits, kept build folder) | 7728 Pitwall: 520 misses, cargo 9.4 min, job 20.1 min; 7729 Dyno on a shared Mac: 31 min of cargo, 90.8 min job | 20 to 35 min on a quiet Mac, more when the Mac's three runners are busy |
| Windows | 8.3 min (210 hits) | 7810 cold-slot test build 21.3 min (debug, not a bundle) | 25 to 40 min |

So a tag run's bundle stage goes from about 8 min (the slowest warm leg) to an estimated 25 to 40 min wall, and from
about 18 to about 55 to 90 runner-minutes over the three OS. The last two app tags (Dyno 1.0.0 and 1.1.0) were a week
apart; at a few tags a month the cost is an estimated 1 to 6 runner-hours a month, at times the owner chooses; it does not touch branch
or `main` runs. Two ways to cut it later, neither needed now: run the tag's bundle legs when the host runners are idle
(the coordinator already chooses the tag time), and D6's dedicated release identity could keep a **release-only** compile
cache that only tag runs can write (a cache is safe when its only writer is the thing it feeds).

## 7. Decisions for the user

- **D1. Scope of release mode.** (a) tag runs only; (b) tag runs and rule 54 dispatches; (c) every release workflow run.
  *Recommended: (a).* Dispatch artifacts are never published; (b) and (c) would slow every rule 54 run by 20 to 30 min for
  no gain. A release-mode dispatch exists for proving the path (WP-3).
- **D2. Linux.** (a) the tag job ignores the `/sccache` mount through the existing overrides and verify shows it wrote
  nothing there; (b) a second Linux runner instance on the same host, label `ubuntu-release`, whose config has no mount,
  for release jobs. *Recommended: (a) now, (b) as a follow-up host action:* (a) closes the read; (b) also makes the mount
  invisible. Any job may name the label, but nothing persists on a runner without a mount.
- **D3. Compile cache for tags.** (a) none; (b) a separate namespace (another folder, Redis database or key prefix) used
  only by tag runs; (c) the shared cache read-only. *Recommended: (a).* (b) is a naming convention, not a boundary: on a
  host every job can write every folder, Redis has no password, and tags are rare enough that the namespace would be cold
  anyway; (c) protects nothing, because the read is the attack (§2.6).
- **D4. Host runners.** (a) job-local everything (§4 item 1), the toolchain and Node kept as installed; (b) (a) plus the
  toolchain installed into the job (`RUSTUP_HOME` in the job: about 1 to 2 min more, still the same OS user). *Recommended:
  (a);* (b) adds time without closing §2.3 item 2, which only D6 closes.
- **D5. The check.** (a) ledger plus probes, failing the run (§5); (b) a warning annotation only. *Recommended: (a);* a
  warning on a tag nobody watches is not a gate (rule 71).
- **D6. A separate identity for tag builds on the hosts** (the code-execution path, §2.3): (a) a separate OS user per host
  with its own runner, toolchain and Node, registered with a label only release workflows name; (b) a throw-away VM per
  tag build (Tart on the Mac, already used by the VM smoke; a Windows VM snapshot); (c) accept the residual risk and write
  it down. *Recommended: decide after the next tag on a research note (WP-7, 1 day); until then (c), recorded here.* (b) is
  the strongest and the slowest; (a) still lets any workflow file name the label, so it needs a runner-side guard that
  refuses non-tag jobs (whether act_runner can do that is unverified).
- **D7. Signing secrets only on tags.** (a) the release callers pass the updater key, its password and `RELEASE_TOKEN`
  only when the ref is a tag; a dispatch signs with a key pair generated in the job (the signing and `latest.json` path is
  still exercised); (b) keep. *Recommended: (a), before the next tag (WP-4, half a day).* It closes T1's key theft in a
  dispatch; it does not stop T2, which can edit the caller.
- **D8. Release images by digest.** (a) release jobs name `paddock-ci` and `garage` by `@sha256:` digest, updated by the
  image workflow's output and Renovate; (b) keep tags. *Recommended: (a)* (a quarter day): `ci-image.yml` can push `:v2`
  from any branch's dispatch.
- **D9. No shared remote cache in a tag run, ever.** A conformance rule: no `actions/cache*` and no `sccache-backend` other
  than release mode's `off` in any job a tag reaches; the Debrief `licences` gate fetches the advisory database and crates
  fresh on a tag. ADR 0080 D4's shared cache service inherits this. *Recommended: yes.*
- **D10. A rule.** Add to `docs/rules.md` (next free number) and propose as an ER: "A tag run reads no state that a
  non-tag run can write: it builds from folders it creates, without a compile cache, and a check fails it otherwise."
  *Recommended: yes,* with the lesson (ER-6).

## 8. Work packages

Estimates in agent-days, raw. Ownership: `.gitea/` and the shared actions are integration points (AGENTS.md): one
integrating branch.

| WP | What | Gates | Days | Depends on |
|---|---|---|---|---|
| **WP-1a** | `release-isolation.sh setup` and its action; release mode in `setup-rust` (no wrapper), `build-folder` (job folder), `kept-clone` (fresh clone in the job folder), `node-caches` (job npm cache, no Angular link), `./dev` (a per-run build folder outside the job folder, §4 item 3); `CARGO_HOME` in the job; wired into `app-release.yml` bundle, keychain and release; ADR 0022 comments corrected on those jobs | **the next tag of Dyno, Pitcrew, Pitwall, Spotter or a demo** | 1.5 | — |
| **WP-1b** | the same for `debrief.yml` bundle: `kept-clone` instead of `stable-workspace` in release mode (and the Windows `clean -ffd` gap closed with hooks off for every mode), `libduckdb` checked every time in release mode, no kept `node_modules`; the `licences` gate without `actions/cache` on tags (D9) | **Debrief's next tag** | 1 | WP-1a |
| **WP-2** | `release-isolation.sh verify`: ledger, probes, summary, `::error::` per finding; unit tests with failing fixtures and the mutation check; the static conformance check (§5 item 4) | **the next tag** (with WP-1a or WP-1b) | 1.5 | WP-1a |
| **WP-3** | a `release-mode` and an `isolation-probe` input on the release callers' dispatch; the positive-control run (red on every OS) and a green release-mode run on the release candidate; their timings as a results block in `docs/benchmarks.md`; the run numbers in this ADR | **the next tag** (the proof rule 71 asks) | 0.5 + runner time | WP-2 |
| WP-4 | D7: secrets only on tags, throw-away key on dispatch | recommended before the next tag, not gating unless the user says so | 0.5 | — |
| WP-5 | D8: release images by digest, the image workflow prints the digest | no | 0.25 | — |
| WP-6 | D2 (b): the `ubuntu-release` runner without the mount (a host action by the user, then a label change) | no | 0.25 + host action | WP-1a |
| WP-7 | D6: research note on a release identity for the hosts (Tart VM per tag on macOS; Windows options; act_runner job guards; Gitea secret scoping by ref), then an ADR | no; after the next tag | 1 | — |
| WP-8 | D10: the rule, the lesson, ADR 0022 amendment (`PADDOCK_DEV_FRESH` really set; "bypass every cache" now true for tags) | no | 0.25 | WP-2 |

**What gates the next tag: WP-1a (or WP-1b for Debrief), WP-2 and WP-3,** about 3.5 agent-days for a non-Debrief app
and 4.5 with Debrief. WP-4 is the cheapest next step and is recommended in the same batch.

## 9. Consequences

- Tag runs are slower (§6) and download their crates and npm packages each time; a registry outage stops a tag (the
  tag waits; nothing is published from a partial build).
- A tag build is reproducible from the tag alone: no leftover of an earlier branch can reach an installer (T5).
- Release mode is one more mode in five actions; the ledger and the static check keep it from drifting, and the actions'
  own tests gain the release-mode case.
- The residual risk (T2 and the host code-execution path) is written down in §3 and owned by D6 and D7.

## 10. Verification (to fill in)

- Positive control run (WP-3): not run yet.
- Release-mode dispatch on the release candidate: not run yet.
- First isolated tag run: not run yet.

## 11. Appendix: the synthetic poisoning experiments (§2.6)

Run on 2026-11-10 on the Mac mini (cargo 1.99.1, sccache 0.19.0 from Homebrew), in a session scratch folder, with
`CARGO_HOME`, `CARGO_TARGET_DIR` and `SCCACHE_DIR` inside it, `CARGO_BUILD_JOBS=2`, sccache on its own port (4299),
stopped afterwards. A workspace with two invented crates: `victim` (a library, `pub fn answer() -> u32`) and `app` (prints
`victim::answer()`).

```sh
# sccache entry swap (RUSTC_WRAPPER=sccache, CARGO_INCREMENTAL=0)
echo 'pub fn answer()->u32{42}'  > victim/src/lib.rs && cargo run -q -p app      # answer=42, one new cache entry E1
echo 'pub fn answer()->u32{666}' > victim/src/lib.rs && rm -rf "$CARGO_TARGET_DIR" && cargo run -q -p app   # entry E2
echo 'pub fn answer()->u32{42}'  > victim/src/lib.rs && cp "$E2" "$E1" && rm -rf "$CARGO_TARGET_DIR" && cargo run -q -p app
# -> answer=666; sccache --show-stats: Cache hits 1, Cache misses 0

# kept build folder (no wrapper): build 666 into tB, build 42 into tA, then
cp tB/debug/deps/libvictim-*.rlib tA/debug/deps/ && touch app/src/main.rs && CARGO_TARGET_DIR=tA cargo run -q -p app
# -> answer=666

# cargo home: a probe crate depending on itoa = "=1.0.11", built once, then
printf '\ncompile_error!("TAMPERED registry source used");\n' >> "$CARGO_HOME"/registry/src/*/itoa-1.0.11/src/lib.rs
rm -rf "$CARGO_TARGET_DIR" && cargo build          # -> error: TAMPERED registry source used
# and with registry/src removed and the same change packed into registry/cache/*/itoa-1.0.11.crate:
cargo build --locked                               # -> error: TAMPERED crate archive used (no checksum error)
```
