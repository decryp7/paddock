# 0111. No writable folder is shared between concurrent CI lanes

- Status: Accepted 2026-11-20 (user, relayed by the coordinator: "train B", the fix for shared mutable CI folders); the rule below is a draft
  until it is numbered in `docs/rules.md`.
- Date: 2026-11-21
- Related: ER-21 (a recurring class gets a redesign), ER-26 (pinned tools and actions), ER-27 (logs explain failures), ER-33 (positive
  controls), ER-37 (common practice and cost), rule 55 (one implementation); ADR 0025 (job isolation and slots).
- Number: 0111. On 2026-11-21 the highest ADR on any `origin` branch was 0110 (`git ls-tree docs/adr/` over every remote branch); there is no
  reservation tool for ADR numbers.

## Context: the class

**Concurrent CI jobs write one folder, and one of them changes what another reads.** On the Linux runner (d-gtr9, six lanes since
2026-11-20) the `/sccache` mount is shared by every lane and was used as sccache's own folder, as `./dev`'s home and build folder, and as
npm's and Angular's cache. Earlier instances:

| # | Where | Instance | Source |
|---|---|---|---|
| 1 | `./dev` build folder | two jobs of one host built in one fixed folder and broke each other's compiler outputs (E0514 in pitwall-mac) | run 8027, fixed by one build folder per compiler |
| 2 | the heavy tool binary | a binary in a shared folder vanished under a running job | 2026-11-14 (named in the brief for this change; no run id) |
| 3 | `./dev` on Windows | a build could not replace the tool another job was running; the launcher keeps "another run published it first" as a special case (`dev`, the `mv -f` fallback; `dev:604` in the brief, line 614 on the train 3d776b605) | no run id (`dev`, read on 3d776b605) |
| 4 | sccache's folder | sccache's LRU evicts any file below `SCCACHE_DIR`, cargo's rlibs among them: "can't find crate for `foldhash`" (E0463) | run 8373, job 42677 (debrief.yml); reproduction `tools/paddock-dev/tests/it/sccache_eviction.rs` |
| 5 | sccache servers | six servers on one local folder, which sccache documents as unsupported ("Multiple concurrent servers will race and cause spurious build failures", docs/Local.md) | no run id: the 6-lane change of 2026-11-20 and sccache's own documentation |

Each was fixed where it showed. The next one is the same class, so the rule is general.

## Decision

1. **The rule (draft, to be numbered):** *No writable folder is shared between concurrent CI lanes. State that must outlive a job goes
   through a keyed cache or an artifact; state that only speeds a job up lives in the job's own folder or in the lane's own slot folder.*
2. **Compile cache.** A Linux job takes a lane with the slot holder the host runners use (`dev ci slot`, root `<mount>/lanes`) and gets
   `<mount>/sccache-<lane>` with its share of the budget (`PADDOCK_CI_SCCACHE_GB`, default 60 GB over the host's lanes). Never the mount
   itself. A container without the mount, or a lane that cannot be taken, keeps the cache in the job (with a notice).
3. **`./dev`.** On a CI runner the tool and its build folder live in the job's own home (`~/.paddock-dev`, which dies with a Linux
   container). The binary is kept between jobs by `actions/cache`, keyed by the launcher's input hash, the OS, the architecture and a
   cache-version salt, with no restore-keys; a restored binary runs only when it answers `self hash` with the wrapper's hash (a flipped byte or a
   wrong hash is rebuilt); only a build that passed that check is saved; the summary says hit or miss and whether the save is there.
4. **npm and Angular caches.** In a Linux container they are the job's own folders under `RUNNER_TEMP`, filled and saved by `actions/cache`:
   npm keyed by OS, Node and the lockfile (restore-keys allowed), Angular by the lockfile (it holds the Angular version) and the app's
   sources, falling back to the same lockfile's older entry (content-addressed, so partial is safe). Host runners keep their slot's folders.
5. **Pins.** Every `actions/cache` use is pinned to the verified v4.3.0 digest (ER-26), checked by `conformance/test/ci-actions.test.mjs`.

## Common practice (ER-37)

GitHub's and Gitea's own guidance is the Actions cache for anything kept between jobs, with a key that names its inputs; GitLab's runner
cache and Bazel's remote cache are keyed stores as well, and none shares a mutable folder between concurrent jobs. sccache's own
documentation says its local disk cache supports one server at a time and recommends a remote backend (Redis, S3) for sharing. The
decision follows that; the cost is about 1.5 agent-days, inside common practice, so it needed no extra approval beyond the user's yes.

## Consequences and what is not measured

- A cold lane starts with an empty sccache folder; the Redis backend (`sccache-backend: auto`) stays the way to share compiles.
- A job that builds `./dev` from source (`PADDOCK_DEV_DOWNLOAD=0`, no read token, `PADDOCK_DEV_FRESH=1`) never uses the dev-tool cache: a downloaded binary must not run as source-built (ADR 0022 section 3.3).
- The slot holder checks that its lock file is still the same file (device and inode) and ends loudly if an old job's cleanup deleted it; the first lane taken also removes, once idle for a day, what older jobs left at the mount root (`paddock-dev/`, `npm/`, `angular/`, sccache's `0`..`f`), never `lanes/` or `sccache-*`.
- `ci_setup_rust::lane_sccache_dir` is the one function naming a lane's folder; another tier derives its own folder from it (`<mount>/sccache-<lane>-s3`).
- A `dev-tool` hit whose binary is rejected cannot be saved over (the key exists): bump `PADDOCK_DEV_TOOL_CACHE_VERSION`.
- Not measured, because the runners are reserved for the Dyno release and d-gtr9 is offline: that a slot holder started in a container
  step survives into the next step; the hit rates and times of the three caches on the runners' `cache.s3` (RustFS; buckets, expiry and the Windows zstd limit are in docs/dev-setup.md, "CI's Actions cache"); the per-lane 10 GB cap against
  d-gtr9's disk. Each needs one CI run there; until then the lane falls back to a job-private folder when the holder cannot be started.
- Angular's and npm's saves happen in `actions/cache`'s post step; a failed save is a warning in that step's log, not in the summary.
