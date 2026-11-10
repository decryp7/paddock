# 85. Release builds never consume caches that branch runs can write

- Status: Proposed (2026-11-10), revision 2. Revision 2 answers the independent review of revision 1 (`4c2fcc109`).
  Drafted at the user's request of 2026-11-10. Its fix gates the next app tag; the one gating list is in §8.
- Date: 2026-11-10
- Related: rule 9 / ER-15 (releases only from a tag in CI), rule 23 (ad-hoc seal), rule 25 (before tagging), rule 54
  (installers before landing; its dispatches are the "branch runs" here), rule 71 / ER-33 (a check counts only if its
  probe has shown it can see the thing), rule 48 (benchmarks recorded), ER-27 (logs explain every failure). ADR 0022
  (`./dev`: "release and tag jobs bypass every cache", which the workflows do not implement, §2.4). ADR 0025 (job
  isolation: slots, kept folders); its §8 "one job per Linux host" is superseded by ADR 0074 I7 and ADR 0080 (line 448): up
  to four containers share `/sccache`. ADR 0080 (CI speed; its D4 shared cache service must not reach a tag run, D9). The
  kept-clone `fresh` input of `030ef0d8e` on `train/2026-11-09-3` (a first step, §2.5).
- Number: free on every `origin` branch on 2026-11-10 (`git ls-tree` of every remote branch: no `008[1-9]` ADR).
- Evidence read at `origin/train/2026-11-09-3` `66b600669` unless a row names a run. CI facts come from the Gitea job
  records and job logs of the runs named (read through the API on 2026-11-10). The poisoning experiments are synthetic
  (§2.6). Nothing was read from the runners themselves. The runners' `config.yaml` is not in the repository, so the Linux
  bind mount is known only from what the job logs and scripts show.

## 1. Pattern

**Every cache CI keeps is shared by branch runs and tag runs, and nothing a tag run reads from one is verified.** A job of
any branch (a pull request, a rule 54 dispatch, a Renovate branch) runs on the same host as a tag run: as the same OS user
on macOS and Windows, or with the same bind mount on Linux. It writes the same kept folders and the same compile cache, and
a later tag build reuses what it wrote. Whatever code ran in that branch job (the branch's own, or any build script, proc
macro, npm install script or test of its dependencies) can leave compiler output, sources or tools in the signed installers.

**The invariant that should hold.** A tag run's build reads only these: the tagged commit fetched for this run, the CI
image by digest (Linux), the toolchain, and downloads verified against the tagged lockfiles or a pinned checksum. Every
folder it builds in is created by the run and dies with it. A check in the tag run proves this and fails the run otherwise.

## 2. What a tag run reads today, and who can write it

### 2.1 The tag run of `dyno-v1.1.0` used the shared caches (verified)

Run 7695 (`dyno-release.yml`, tag `dyno-v1.1.0`, commit `8efce3fc3`), from the bundle jobs' logs. The host legs ran before
the kept clone existed: they used `stable-workspace`.

| Leg | Checkout | Build folder | Compile cache | npm / Angular | Crates |
|---|---|---|---|---|---|
| macOS (job 31477) | the job's checkout moved to `src/dyno-bundle-macOS-s1` (fresh) | `/Volumes/d7sst7/ci/home/target-dyno-bundle-s1` (kept, slot s1) | `sccache-s1`: 173 requests, **150 hits**, 0 misses | `npm-s1`; `.angular/cache` linked to `angular/dyno-bundle-s1` | 0 downloaded: the runner user's `~/.cargo` |
| Linux (job 31478) | `actions/checkout` in a new container | `apps/dyno/target` (new) | `/sccache` mount: 1,039 requests, **821 hits** | `/sccache/npm`; `.angular/cache` linked to `/sccache/angular/dyno-bundle` | 515 downloaded: the image's `CARGO_HOME`, new each run |
| Windows (job 31479) | the move failed; **a kept clone** at `src/dyno-bundle-Windows-s1`, brought up with `clean -ffd` (no `-x`: ignored files stay) and hooks enabled | `C:/Users/<runner>/.paddock-ci/target-dyno-bundle-s1` (kept) | `sccache-s1`: 307 requests, **210 hits**, 19 misses | `npm-s1` | 0 downloaded: `~/.cargo` |

Every `dyno-release.yml` dispatch from a branch (rule 54) writes the same kept folder names (`<app>-bundle` plus the slot):
runs 7848 to 7865 on `train/2026-11-09-2` built `dyno-bundle` in the same slots two days later. This is not evidence that
1.1.0 was poisoned. It shows that the path was open.

### 2.2 Inventory: what a tag run reads, per OS

"Writable by a branch run" means that a job of any branch can write it in its normal steps (same names or same paths), and
in any case as the same OS user (host runners) or through the same bind mount (Linux).

| State | Linux container | macOS host | Windows host | Read by the tag build | Source |
|---|---|---|---|---|---|
| Compile cache (sccache, local disk) | `/sccache` bind mount, shared by up to four containers at once (ADR 0074 I7) | `<ci-home>/sccache-s<n>` | same | yes (`setup-rust` default `sccache-backend: local`) | run 7695 stats; `setup-rust/action.yml:143-165` |
| Shared sccache backend (Redis/Dragonfly `172.16.1.188:6379` db 5, no password) | only if a job passes `sccache-backend: auto\|redis` | same | same | **no** today: no workflow passes it (`git grep sccache-backend: .gitea` finds only the input's definition) | grep; `docs/dev-setup.md:73` |
| act_runner cache server (`172.16.1.188:8089`, `actions/cache`) | nightly, Debrief bench/corpus/mutants, fuzz, plan | same | same | **not by the bundle path**. Debrief's `licences` job (a tag gate) restores `registry/index`, `registry/cache` and `advisory-dbs` by a prefix `restore-keys` | `debrief.yml:418-430` |
| Rust build folder | `target/` in the new container | `<ci-home>/target-<app>-bundle-s<n>` (kept) | same | yes: cargo reuses any artifact whose fingerprint looks fresh | `build-folder.sh`; run 7695 |
| Checkout (`app-release.yml`) | fresh `actions/checkout` | kept clone `src/<name>-<slot>`, made again on a tag since `030ef0d8e` (`fresh`) | same | tag: a fresh clone at the slot's path | `kept-clone.sh` |
| Checkout (`debrief.yml`) | fresh | `stable-workspace` moves the job's checkout (fresh) | `stable-workspace` keeps a clone: `clean -ffd` without `-x`, git hooks enabled (a planted `post-checkout` runs) | yes | `stable-workspace/action.yml:88-100`; job 31479 |
| Cargo home (registry index, `.crate` archives, extracted sources, `~/.cargo/bin`) | the image's `/usr/local/cargo`, new each run | the runner user's `~/.cargo` | same | yes; sources and archives are **not re-verified** (§2.6) | run 7695 (0 downloads on hosts) |
| **Cargo configuration in parent folders** (cargo reads `.cargo/config.toml` in every folder from the working folder up to the root, then `CARGO_HOME`'s): `build.rustc-wrapper`, `[source]` replacement, `linker`, `rustflags`, `[env]` | none outside the image | `/Users/decryp7/.cargo/config.toml` (above the CI home and the work folder), `/Users/.cargo`, `/.cargo` | `C:\gitea-runner\.cargo`, `C:\.cargo`, `C:\Users\<runner>\.cargo` | yes: any of them changes what every cargo command compiles or links | cargo configuration docs (hierarchical lookup) |
| npm download cache | `/sccache/npm` | `npm-s<n>` | same | `npm ci` reads it (integrity-checked against the lockfile; read, not measured here) | `node-caches` |
| npm user configuration (`~/.npmrc`: registry, `ignore-scripts`, `script-shell`) | image | the runner user's | same | yes | npm configuration docs |
| Angular build cache | `/sccache/angular/<name>` | `angular/<name>-s<n>` | same (not for Debrief Windows) | yes, linked into the app as `.angular/cache` | `node-caches`; run 7695 |
| Debrief `node_modules` | npm ci | npm ci | **kept per lockfile hash** in `node-modules/debrief-s<n>`, junctioned in; `npm ci` skipped when it exists | yes (Windows) | `windows-node-modules.sh` |
| Debrief `libduckdb` (shipped in the installer) | the image's `/opt/libduckdb` or the job's temp | `libduckdb/<job>-s<n>`; the SHA-256 is checked only when the folder has no `.verified` marker | same | yes; anyone can write the marker | `fetch-libduckdb.sh:34` |
| **Tauri's tool cache** (NSIS and its plugins, shipped inside the installer) | n/a | n/a | the runner user's `%LOCALAPPDATA%\tauri` | yes, reused once present | tauri-bundler (read, not verified on the runner) |
| **AppImage tools** (`linuxdeploy`, `linuxdeploy-plugin-appimage` from the moving `continuous` release, `AppRun` from `apprun-old`), part of the shipped AppImage | **downloaded in every run without a checksum** | n/a | n/a | yes | job 31478: three `Downloading https://github.com/…` lines |
| `paddock-dev` (`./dev`, run by `job-isolation` as the slot holder for the whole job) | `/sccache/paddock-dev/bin-source/<hash>` | `<ci-home>/tools/paddock-dev/bin-source/<hash>` (scope `shared`) | same | yes: a cached binary that answers `--version` runs. `self hash` is checked only for a new build or a download | `dev:280-312`; `job-isolation.sh:186-193`; no `dev: built` line in run 7695's host logs |
| sccache binary | the image's | the first `sccache` 0.19.0 on `PATH` (the shared Mac: `/opt/homebrew/bin`, user-writable); the checksum is checked only on a download | same | yes | `setup-rust/action.yml:72-109` |
| Toolchain (`~/.rustup`), Node, git and its global configuration (`core.hooksPath`, `url.insteadOf`), shell profile, LaunchAgents and login items | the image | the runner user's (the macOS runner runs as the developer's own account: `/Users/decryp7/gitea-runner`) | the runner user's | yes | job logs (work folder paths) |
| Actions | `actions/checkout@v4`, `actions/setup-node@v4`, `taiki-e/install-action@v2` by tag, not digest; act's action cache on each runner | same | same | yes | the workflows |
| CI image | `paddock-ci:v2`, `garage:ubuntu-24.04-…` by **mutable tag**, and `forcePull=false`: a tag already on the host is reused without a pull. `ci-image.yml` pushes `:v2` from any branch's dispatch, and its registry `buildcache` is writable by any branch | n/a | n/a | yes | job 31478 (`docker pull … forcePull=false`); `ci-image.yml:61-63` |
| Updater signing key and its password | the bundle job of **every** dispatch, not only tags; in the environment of `npm ci` (`tauri-bundle.sh:55`) and of `tauri build` | same | same | every npm install script and every build script and proc macro can read it | `dyno-release.yml:129-133`; `tauri-bundle.sh:55` |
| `RELEASE_TOKEN` | only the tag-only `release` job (`app-release.yml:400`, used at `:465`) | n/a | n/a | not by the build | `app-release.yml` |

Out of scope: the Playwright test browsers (they run the gates' tests and are not shipped).

### 2.3 Two different exposures

1. **The data path (this ADR's core).** State that a branch job leaves through the normal cache mechanisms: sccache
   entries, kept build folders, `~/.cargo`, npm and Angular caches, kept `node_modules`, `libduckdb`, the Tauri tool cache,
   the cached `paddock-dev`. Closing it needs no new machine: the tag run must not read any of it.
2. **The code-execution path.** On a host runner a branch job runs as the same OS user as the tag job. It can change
   anything that user can: the toolchain under `~/.rustup`, `~/.cargo/bin` and `~/.cargo/config.toml`, Homebrew, `~/.npmrc`,
   `~/.gitconfig` (a global `core.hooksPath`, or `url.insteadOf` to redirect the tag run's fetch), the shell profile, or a
   LaunchAgent that waits for the next tag job and edits its outputs or reads its environment. Generic malware does go for
   these files. Choosing folders inside the job does not close this; probes (§5) catch the configuration files, and only
   another identity closes the rest (D6). On Linux the container leaves nothing behind except through the `/sccache` mount
   and the host's image store (unless the runner also mounts the Docker socket or runs privileged: not visible from the
   repository, to confirm on the host).

### 2.4 ADR 0022 claims more than the workflows do

ADR 0022 says "Release and tag jobs bypass every cache. They set `PADDOCK_DEV_FRESH=1`". No workflow sets
`PADDOCK_DEV_FRESH`. The release jobs set `PADDOCK_DEV_DOWNLOAD: "0"`, which still runs a cached binary from
`bin-source/<hash>` on the shared mount or tools folder, and the rest of the bundle job (sccache, build folder, caches) was
never covered. The comment "release jobs bypass every cache" is repeated on eight jobs (`app-release.yml:149,404`,
`ci.yml:238,706,923,1095`, the demo release workflows, `debrief.yml:860`). WP-8 corrects ADR 0022 by amendment.

### 2.5 What `030ef0d8e` (kept clone `fresh` on tags) does and does not do

It makes the tag run's source tree come from objects fetched in that run, so a planted object, ref, hook or local config in
the slot's clone is gone. It does not touch the compile cache, the build folder, `~/.cargo`, the npm and Angular caches or
`paddock-dev`, which are where compiled code comes from. It leaves Debrief's bundle (on `stable-workspace`) and the demos'
bundles unchanged.

### 2.6 Poisoning shown on synthetic crates (verified; ER-33)

Three experiments on this Mac (cargo 1.99.1, sccache 0.19.0), in a scratch folder, with invented crates. Nothing of a real
build or a runner was touched. Each shows that a writer of one cache changes what a clean source builds:

| Cache | What the writer did | Clean rebuild | Result |
|---|---|---|---|
| sccache local disk | built `victim` (`answer() -> 42`) and a changed `victim` (`-> 666`) through one `SCCACHE_DIR`, then copied the second entry's file over the first entry's key | source says 42, **empty target folder**, same cache | prints `answer=666`; stats: 1 hit, 0 misses |
| Kept build folder | built 42 into folder A and 666 into folder B, copied B's `libvictim-*.rlib` into A | source says 42, cargo finds A fresh and relinks `app` | prints `answer=666` |
| Cargo home: extracted source | appended `compile_error!("TAMPERED…")` to `registry/src/…/itoa-1.0.11/src/lib.rs` | **empty target folder**, `Cargo.lock` unchanged | cargo compiles the changed source: `error: TAMPERED registry source used` |
| Cargo home: `.crate` archive | deleted `registry/src`, rewrote `registry/cache/…/itoa-1.0.11.crate` with the same change | `cargo build --locked`, empty target | cargo unpacks the changed archive without a checksum error: `TAMPERED crate archive used` |

So neither a fresh build folder alone (sccache and `~/.cargo` still poison it) nor `--locked` protects a build whose cargo
home or compile cache another job can write. The commands are in §11.

## 3. Threat cases, and what this ADR changes

The apps are personal and non-commercial, and every branch is the owner's (or an agent's working for the owner). The
updater installs signed updates on the owner's machines without asking. Dyno ships a privileged helper and per-machine
installers. So a poisoned release has a high impact even with one user, and the likelihood is low.

| # | Case | Today | After the gating list (§8) | Residual |
|---|---|---|---|---|
| T1 | **A compromised dependency in a branch build** (a Renovate branch with a new version, or a crate or npm package an agent adds to try something): its build script, proc macro, install script or test runs as the runner user during a pull request or rule 54 dispatch. It does not control the workflow files | it can write every row of §2.2. The next tag ships the result. During a dispatch it can also read the updater signing key (`npm ci` and `tauri build` have it in their environment) | **closed on Linux.** **On the hosts, only targeted cache poisoning is closed** (the data path), plus the cargo configuration files and the Tauri tool cache, which the probes check (§5). With D7 no dispatch build sees the real key | on the hosts, everything the runner user can change (§2.3 item 2) until D6 |
| T2 | **A malicious or mistaken branch of the owner or an agent** (a hijacked agent: prompt injection from a web page, an issue or a dependency's docs). It controls the workflow files | all of T1, plus: dispatch a workflow that hands it the signing key, push the CI image `:v2`, run anything as the runner user | the cache path is closed, but this attacker does not need it | **not closed by this ADR.** Gitea gives repository secrets to any branch's dispatch. Closing it needs secrets bound to tags on the server (to research in WP-7: whether the Gitea version in use scopes secrets by ref or environment; unverified) and D6 |
| T3 | A compromised dependency **in the tagged lockfile** | ships | ships | out of scope: cargo-deny advisories (rule 25), lockfile review, ADR 0073 |
| T4 | A device on the LAN or any job writes the Redis compile cache (no password) or the act_runner cache server | the bundle path does not read them. The Debrief `licences` tag gate restores a cargo registry and the advisory database by prefix, so a planted stale database can hide an advisory | no `actions/cache` and no remote sccache in any job a tag reaches (D9, a conformance check) | ADR 0080 D4's shared cache service is named in D9, so it never reaches a tag |
| T5 | **Accidental** reuse: a branch's leftovers (another feature set, an old `ui/dist`, a stale `node_modules`) end up in a tag's installers (the rule 54 incident was this kind) | possible on the hosts | gone: the tag builds in folders made by the run | none |
| T6 | **The signing key is already exposed**: every rule 54 dispatch since the release callers began passing the key gave it to `npm ci` and `tauri build`, so to every install script and build script of those branches | — | D7 stops new exposure | whether to rotate the updater key is the user's decision (§7, D7). A new key needs every installed copy updated by hand once |

Honest weighing: T1 is the case this ADR is for. On Linux the gating list closes it; on the hosts it closes the cheap,
targeted part (the caches) and the configuration files, and leaves the runner user's account open until D6. T2 is stronger
and no cache rule stops it. T5 alone (reproducible tag builds) would justify WP-1.

## 4. Decision

A tag run (`github.ref` starts with `refs/tags/`) and the pre-tag dispatch of rule 25 build in **release mode**:

1. **Nothing kept is read.** Every folder the build uses is created by the run with `mktemp -d` under the runner's per-job
   folder (`$RUNNER_TEMP`): the clone (the kept-clone `fresh` clone moves from the slot's `src/` into the job folder), the
   build folder, `CARGO_HOME`, the npm cache, `node_modules` (`npm ci`), and Debrief's `libduckdb` (always downloaded and
   checked against the pinned SHA-256; the `.verified` marker is ignored). `CARGO_HOME` starts empty: a fresh registry
   download against the tagged lockfiles (a cold `cargo fetch` of all eight workspaces took 34.3 s on this Mac,
   `docs/benchmarks.md` 2026-11-08 C99 block; the Linux leg already downloads its 515 crates every run). The kept Angular
   cache is not linked; the build writes its cache inside the job's own checkout. On Windows `LOCALAPPDATA` points into
   the job folder for the build steps, so Tauri downloads its NSIS tools again; WP-9 pins their hash.
2. **No compile cache.** `setup-rust` leaves `RUSTC_WRAPPER` unset and starts no sccache server. No Redis, no WebDAV, no
   `actions/cache`.
3. **`paddock-dev` is built from source for the run**, with no compiler wrapper, into an empty folder made for this run,
   so the slot holder is not a cached binary and ADR 0022's text holds. `./dev`'s existing `PADDOCK_DEV_FRESH=1` builds
   into a temp folder inside the job folder, which the slot holder refuses on purpose (`slots.rs`
   `check_program_outside_owner`: a program there keeps Windows from deleting the job folder). So on the hosts release mode
   points `PADDOCK_DEV_HOME` and `PADDOCK_DEV_TARGET_DIR` at a new `release-<run id>-<job>` folder under the CI home's
   `tools`, outside the job folder, removed first if it exists and never read by another run. On Linux it is built inside
   the container (the job folder; no slot holder there), never on the shared mount.
4. **The release images are named by digest** (D8), and only a run on `main` produces the digest the release callers pin.
5. **The real signing key reaches only tag runs, and never `npm ci`** (D7). A dispatch signs with a key pair made in the
   job, so the signing path is still exercised.
6. **The AppImage tools are seeded from pinned checksums** (WP-9) before `tauri build`, so it does not download them.
7. **A check fails the run if any of this did not hold** (§5), and the release job needs it.

Release mode is one mechanism, `conformance/ci/release-isolation.sh setup|verify` (with a thin action). The existing
actions read one variable, `PADDOCK_RELEASE_MODE=1`, and the overrides they already have (`PADDOCK_SCCACHE_MOUNT` in
`setup-rust`, `PADDOCK_CI_MOUNT` in `node-caches` and `./dev`). Branch and dispatch runs keep every cache: the speed of
ADR 0080 is untouched outside tags.

**Step order.** `setup` is the first step after the checkout (and Git bash on Windows), before `build-jobs` and
`job-isolation`, so the slot holder is already the run's own `paddock-dev`. `verify` runs after the build and its checks
and **before** `upload-artifact`, `prune-build-folder` and `host-cleanup`, so a failed check uploads nothing and the folders
it inspects still exist.

## 5. The check

`release-isolation verify` fails the job with one `::error::` per finding (the kind, the path, the next step) and writes a
summary line to the job summary.

1. **The ledger.** Every action that chooses a folder or a cache appends one line `kind<TAB>path<TAB>mode` to
   `$RUNNER_TEMP/release-isolation/ledger.tsv` (`setup-rust`: `sccache off`; `build-folder`: `target`; `kept-clone`:
   `workspace`; `node-caches`: `npm`, `angular`; `./dev`: `dev-tool`; Windows: `tauri-tools`; Debrief: `libduckdb`,
   `node-modules`). Verify fails when a mode is not `job`, `run` or `off`, or when a path does not match what `setup`
   recorded for its kind. A kind that the job's declared list expects and the ledger lacks is **check broken**, never a pass
   (rule 71).
2. **Freshness by identity, not by modification time.** Cargo unpacks registry sources with the times stored in the
   package (often years old), copies keep their times, and a writer can set any time it likes, so modification times prove
   nothing. Instead `setup` creates `CARGO_HOME`, the build folder, the npm cache, the clone and the `paddock-dev` folder
   with `mktemp -d` and records each one's device and inode (and, on Unix, its ctime), in a record under `$RUNNER_TEMP`.
   Verify checks that each path still resolves (canonical; `cygpath -u` on Windows) to the same device and inode, and that
   each is still under the folder `setup` made it in. It also checks every `.crate` archive in `CARGO_HOME/registry/cache`
   against the SHA-256 that the tagged `Cargo.lock` names: an archive with no lock entry or another hash fails.
3. **No compile cache.** `RUSTC_WRAPPER`, `CARGO_BUILD_RUSTC_WRAPPER` and `RUSTC_WORKSPACE_WRAPPER` are empty, and no
   sccache server answers on the job's port (or `--show-stats` shows 0 compile requests).
4. **The job's own pointers on Linux** (the gate's choice, D2): `SCCACHE_DIR` is unset or inside the job, `npm_config_cache`
   is under the job's record, `<app-dir>/.angular/cache` is absent or a real folder inside the checkout (never a link), and
   sccache served 0 requests. Verify does **not** look for files on `/sccache`: up to four containers share it (ADR 0074
   I7), so a file there cannot be traced to this job. D2 (b), a release runner without the mount, would let the probe show
   the mount is absent; it is the follow-up, not the gate.
5. **Configuration from the tagged tree only.** Every `.cargo/config` and `.cargo/config.toml` that cargo would read for
   the build (each parent folder of the Cargo workspace up to the root, and `CARGO_HOME`) is either a file of the tagged
   tree (`git ls-files` in the clone) or absent; `.cargo/ci.toml` holds only the `target-dir` that `build-folder` wrote. No
   `.npmrc` outside the tagged tree is read (`npm config list --json` shows no user or global file), and `git config
   --global --get core.hooksPath` is empty. On Windows the NSIS files Tauri used are the pinned ones (WP-9's hashes).
6. **Output.** `release isolation: 10 kinds made by this run, 0 shared; 512 crate archives match Cargo.lock; no compile
   cache; no cargo or npm configuration outside the tag` (numbers per job), or the `::error::` list. The `release` job needs
   the bundle jobs, so a red check stops the publish.
7. **Static check (conformance, next free C number).** Every job reachable from a tag (`app-release.yml` bundle, keychain
   and release; `debrief.yml` bundle, keychain and release; the demo release workflows' bundle and release; and the gate
   jobs that feed a published file) runs `setup` first and `verify` before its upload; none of them uses `actions/cache*`,
   `sccache-backend: auto|redis`, `stable-workspace`, or a kept-folder kind of `kept-folders.txt` outside release mode; no
   release workflow names an image by tag; no step before `tauri build` gets the signing key.

### 5.1 How it is shown to see the thing (rule 71 / ER-33)

- **Unit tests** (`conformance/test/release-isolation.test.mjs`, `node --test`, synthetic folders under the test scratch).
  For each kind, a ledger entry or a path that points into a fake CI home makes verify fail and name it. A folder replaced
  after `setup` (same path, another inode) fails. A `.crate` archive whose bytes differ from `Cargo.lock`'s hash fails. An
  Angular cache that is a link fails. A `.cargo/config.toml` in a parent folder outside the tree fails. An **old-mtime
  fixture** (registry sources unpacked with 2006 times, a copied file with its old time) **passes**, so an honest run is not
  red. An empty ledger reports check broken. A clean fixture passes. A mutation check (delete each probe in turn) makes a
  test fail.
- **A positive control in CI, once, before the first isolated tag.** The release-mode dispatch (WP-3) with
  `isolation-probe: leak` shares on purpose, on every OS: the Angular link and sccache; on the hosts also the kept build
  folder and the runner user's `CARGO_HOME`; on Windows also the kept Tauri tool cache. Verify must go red and name each
  one. The run number goes into §10; without it the check does not count.
- **The real proof.** A green release-mode dispatch on the release candidate (WP-3), then the tag run itself, each showing
  the summary line with non-zero counts.

## 6. Cost

From the CI records in [docs/benchmarks.md, 2026-11-10 release bundle jobs](../benchmarks.md) (rule 48). No fully cold
bundle exists in the records, so the cold side is an estimate, to be replaced by WP-3's measurement.

| Leg | Warm, tag 7695 | Cold, closest record | Estimate isolated |
|---|---|---|---|
| Linux | 5.5 min (821 sccache hits; crates already downloaded every run) | 7851: 80 misses, cargo 1 min 43 s + 3 min 11 s, job 8.2 min | 10 to 15 min (all 1,039 compiles on 16 cores) |
| macOS | 3.9 min (150 hits, kept build folder) | 7728 Pitwall: 520 misses, cargo 9.4 min, job 20.1 min; 7729 Dyno on a shared Mac: 31 min of cargo, 90.8 min job | 20 to 35 min on a quiet Mac, more when its three runners are busy |
| Windows | 8.3 min (210 hits) | 7729 Dyno bundle 28.4 min with 22 misses (load not recorded); 7810 cold-slot test build 21.3 min (debug, not a bundle) | 25 to 40 min |

So a tag run's bundle stage goes from about 8 min (the slowest warm leg) to an estimated 25 to 40 min wall, and from
about 18 to about 55 to 90 runner-minutes over the three OS. The last two app tags (Dyno 1.0.0 and 1.1.0) were a week
apart. At a few tags a month the cost is an estimated 1 to 6 runner-hours a month, at times the owner chooses. Branch and
`main` runs are not touched. Not measured: whether the Windows Defender exclusions cover a job folder as they cover the
CI home (a scan of every new file could make the cold Windows leg slower than the estimate); WP-3 measures it.

Two ways to cut it later: run the tag's bundle legs when the host runners are idle (the coordinator already chooses the
tag time), and D6 (a), which keeps a warm, release-only cache.

## 7. Decisions for the user

- **D1. Scope of release mode.** (a) tag runs and the pre-tag dispatch of rule 25; (b) every rule 54 dispatch too; (c)
  every release workflow run. *Recommended: (a).* Dispatch artifacts are never published, so (b) and (c) would slow every
  rule 54 run by 20 to 30 min for no gain. The rule 25 dispatch is the dress rehearsal, so it runs exactly what the tag
  runs.
- **D2. Linux.** (a) the tag job ignores the `/sccache` mount through the existing overrides, and verify checks the job's
  own pointers (§5 item 4); (b) a second Linux runner instance on the same host, label `ubuntu-release`, whose config has
  no mount, for release jobs, and a probe that the mount is absent. *Recommended: (a) in the gate, (b) as a follow-up host
  action (WP-6).* (a) closes the read. (b) also makes the mount invisible: any job may name the label, but nothing persists
  on a runner without a mount.
- **D3. Compile cache for tags.** (a) none; (b) a separate namespace (another folder, Redis database or key prefix) used
  only by tag runs; (c) the shared cache read-only. *Recommended: (a) with today's runners.* (b) holds only while the
  identity is shared: on a host every job can write every folder of its user, and Redis has no password, so a tag-only
  namespace is a naming convention, not a boundary. With a separate release identity (D6 (a)) it becomes a real boundary.
  (c) protects nothing, because the read is the attack (§2.6).
- **D4. Host runners.** (a) folders made by the run (§4 item 1), the toolchain and Node kept as installed; (b) (a) plus the
  toolchain installed into the job (`RUSTUP_HOME` in the job: about 1 to 2 min more, still the same OS user).
  *Recommended: (a).* (b) adds time without closing §2.3 item 2, which only D6 closes.
- **D5. The check.** (a) ledger, identity and configuration probes that fail the run (§5); (b) a warning annotation only.
  *Recommended: (a).* A warning on a tag nobody watches is not a gate (rule 71).
- **D6. A separate identity for tag builds on the hosts** (the code-execution path, §2.3). Options, to weigh against the
  cold-build design of §4:
  - (a) **a separate OS user per host with its own runner** (registered with a label such as `macos-release`,
    `windows-release`; the release callers pick it with a tag-dependent `runs-on`), its own toolchain, Node, `CARGO_HOME`,
    git and npm configuration, **and a warm release-only cache** (sccache, build folder, cargo home) that only that user
    writes. Branch jobs never run as that user, so T1 is closed on the hosts too, and tag builds stay warm (about
    today's 4 to 8 min instead of 25 to 40). Needs: a runner-side guard that refuses non-tag jobs on that runner (a branch
    could name the label; whether act_runner can refuse by ref is unverified), and per-user installs on each host.
  - (b) a throw-away VM per tag build (Tart on the Mac, already used by the VM smoke; a Windows VM snapshot): the strongest,
    the slowest, and the most work.
  - (c) accept the host residual risk and write it down (this ADR does).
  *Recommended: (c) for the next tag, then decide between (a) and (b) on WP-7's research note.* D6 does not gate the next
  tag.
- **D7. Signing secrets only on tags, never in `npm ci`.** (a) the release callers pass the updater key and its password
  only when the ref is a tag; a dispatch signs with a key pair made in the job (the signing and `latest.json` path is still
  exercised); `tauri-bundle.sh` runs `npm ci` and `prepare` without the key in the environment; (b) keep. *Recommended: (a),
  and it gates the next tag (WP-4).* Today `tauri-bundle.sh:55` runs `npm ci` with `TAURI_SIGNING_PRIVATE_KEY` set on every
  dispatch, so every npm install script of every branch sees it, and a stolen key signs updates that install without a
  prompt. WP-4 also researches signing in a separate step after the build (the build without the key, then `tauri signer
  sign` on the finished installers), which would keep the key away from `tauri build`'s build scripts too.
  **Key rotation (the user's decision):** the key has already been in the environment of many dispatch builds (T6).
  Rotating it means a new updater public key in every app and one manual update of every installed copy.
- **D8. Release images by digest.** (a) release jobs name `paddock-ci` and `garage` by `@sha256:` digest; only a
  `ci-image.yml` run on `main` produces the digest the callers pin (a branch dispatch may build and push for testing, and
  its registry `buildcache` is writable by any branch, so a digest from a branch is never pinned); (b) keep tags.
  *Recommended: (a), in the gate (WP-5).* With a mutable tag and `forcePull=false`, a tag run uses whatever image the host
  holds under that tag.
- **D9. No shared remote cache in a tag run, ever.** A conformance rule: no `actions/cache*` and no `sccache-backend` other
  than release mode's `off` in any job a tag reaches. The Debrief `licences` gate fetches the advisory database and crates
  fresh on a tag. ADR 0080 D4's shared cache service inherits this. *Recommended: yes.*
- **D10. A rule.** Add to `docs/rules.md` (next free number) and propose as an ER: "A tag run reads no state that a
  non-tag run can write: it builds from folders it creates, without a compile cache, with signing keys only after the
  install step, and a check fails it otherwise." *Recommended: yes,* with the lesson (ER-6).

## 8. Work packages

Estimates in agent-days, raw. `.gitea/` and the shared actions are integration points (AGENTS.md): one integrating branch.

**The gating list (the same in the Status line and the README).** The next tag of an app needs, all landed and proven:
**WP-1 for that app's workflow** (WP-1a for Dyno, Pitcrew, Pitwall and Spotter; WP-1b for Debrief; WP-1c for the shell
and workbench demos), **WP-2, WP-3, WP-4, WP-5 and WP-9.** That is about 6 agent-days for the first app on
`app-release.yml`, plus 1 for Debrief and 0.5 for the demos. WP-6, WP-7 and WP-8 do not gate.

| WP | What | Gates the next tag | Days | Depends on |
|---|---|---|---|---|
| **WP-1a** | `release-isolation.sh setup` and its action; release mode in `setup-rust` (no wrapper), `build-folder` (folder made by the run), `kept-clone` (fresh clone in the job folder), `node-caches` (the run's npm cache, no Angular link), `./dev` (§4 item 3; on Linux inside the container); `CARGO_HOME` and, on Windows, `LOCALAPPDATA` made by the run; wired into `app-release.yml` bundle, keychain and release in the §4 step order; ADR 0022 comments corrected on those jobs | yes (Dyno, Pitcrew, Pitwall, Spotter) | 1.5 | — |
| **WP-1b** | the same for `debrief.yml` bundle: `kept-clone` instead of `stable-workspace` in release mode (and the Windows clone's `clean -ffd` gap closed with `-x` and hooks off in every mode), `libduckdb` checked every time, no kept `node_modules`; the `licences` gate without `actions/cache` on tags (D9) | yes (Debrief) | 1 | WP-1a |
| **WP-1c** | the same for `shell-demo-release.yml` and `workbench-demo-release.yml` bundles (their own job-isolation and kept build folder on Windows, `actions/setup-node`, `dtolnay/rust-toolchain` by digest) | yes (the demos) | 0.5 | WP-1a |
| **WP-2** | `release-isolation.sh verify`: ledger, identity freshness, `.crate` hashes against `Cargo.lock`, the configuration probes, the job's own pointers on Linux, summary, `::error::` per finding; the unit tests of §5.1 with the old-mtime fixture and the mutation check; the static conformance check | yes | 2 | WP-1a |
| **WP-3** | `release-mode` and `isolation-probe` inputs on the release callers' dispatch; rule 25's pre-tag dispatch always in release mode; the positive-control run (red on every OS, naming each planted leak) and a green release-mode run on the release candidate; their timings, Windows Defender's effect included, as a results block in `docs/benchmarks.md`; the run numbers in §10 | yes (rule 71's proof) | 0.5 + runner time | WP-2 |
| **WP-4** | D7: the key only on tags and never in `npm ci` or `prepare`; a throw-away key on dispatch; a note on signing after the build as a separate step | yes | 1 | — |
| **WP-5** | D8: release jobs pin the images by digest; `ci-image.yml` prints the digest and only a `main` run's digest is pinned (a conformance check compares the pinned digest with `main`'s last image run) | yes | 0.5 | — |
| **WP-9** | the AppImage tools (`linuxdeploy`, `linuxdeploy-plugin-appimage`, `AppRun`) and the Windows NSIS tools fetched from pinned URLs with pinned SHA-256 values into the places Tauri looks, before `tauri build`; a test that the build downloads nothing else | yes | 0.5 | — |
| WP-6 | D2 (b): the `ubuntu-release` runner without the mount (a host action by the user, then the label change and the mount-absent probe) | no | 0.25 + host action | WP-1a |
| WP-7 | D6: research note on a release identity for the hosts (a separate user with a release-only cache; a Tart VM per tag; Windows options; act_runner job guards by ref; Gitea secret scoping by ref), then an ADR | no; after the next tag | 1 | — |
| WP-8 | D10: the rule, the lesson, the ADR 0022 amendment (`PADDOCK_DEV_FRESH`'s claim made true for tags) | no | 0.25 | WP-2 |

## 9. Consequences

- Tag runs are slower (§6) and download their crates, npm packages and bundler tools each time. A registry outage stops a
  tag (the tag waits; nothing is published from a partial build).
- A tag build is reproducible from the tag, the pinned image and the pinned tools alone: no leftover of an earlier branch
  can reach an installer (T5).
- Release mode is one more mode in five actions and three workflows. The ledger and the static check keep it from
  drifting, and the actions' own tests gain the release-mode case.
- The residual risk is written down: T2, the hosts' code-execution path (D6), the unpinned actions by tag
  (`actions/checkout@v4`, `actions/setup-node@v4`, `taiki-e/install-action@v2`) and act's action cache on each runner, and
  an already-exposed signing key (T6, D7).

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
