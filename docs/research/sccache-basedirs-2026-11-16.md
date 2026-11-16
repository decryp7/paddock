# sccache across worktrees: basedirs for Rust (2026-11-16)

Short research note. Question: can a shared compiler cache now give cache hits across agents' worktrees and CI checkouts,
and would it pay off? Background: ADR 0014 §1 measured about 6.5% hits across worktrees on 2026-10-25 and the user decided
against a shared compiler-cache backend on 2026-10-28.

**Heads read** (ADR 0091): Paddock `origin/train/2026-11-15-13` 3585bcab12d5, `origin/main` 23213a35eb51; mozilla/sccache
tag `v0.19.0` and `main` 433cf4c5510 (2026-11-16 08:00 UTC); bazelbuild/rules_rust `main` 5762903139ba. All read on
2026-11-16.

## 1. What sccache has shipped

**Verified (release metadata, source at the tag):**

- The newest release is **v0.19.0, published 2026-10-24** (GitHub releases API, `published_at` 2026-10-24T10:25:17Z;
  <https://github.com/mozilla/sccache/releases/tag/v0.19.0>). Paddock CI already pins it
  (`SCCACHE_VERSION = "0.19.0"`, `tools/paddock-dev/src/ci_setup_rust.rs:38`, on origin/main).
- The setting is **`SCCACHE_BASEDIRS`** (environment, `:`-separated on Unix, `;` on Windows) or `basedirs = [...]` in the
  config file. It is read once, when the **server** starts (`src/config.rs`), absolute paths only; the longest matching
  prefix is stripped (sccache `docs/Configuration.md`).
- **In v0.19.0 it does nothing for rustc.** `src/compiler/rust.rs` at the tag has no reference to basedirs. The basedirs
  items in the 0.16 to 0.19 changelogs (#2736, #2840 "also strips base directories from the compiler arguments", #2883)
  are for the C/C++ preprocessor path.
- Rust support is **merged but unreleased**: PR #2918 "rust: add support for SCCACHE_BASEDIRS when hashing", merged
  2026-11-02 as cfb169c2e, 17 commits after v0.19.0 (GitHub compare API, `ahead_by` 17, `behind_by` 0;
  <https://github.com/mozilla/sccache/pull/2918>, fixes #2870). The earlier #2678 was closed unmerged. On `main` the Rust
  key strips the basedirs from the arguments, every `CARGO_*` variable, the `env!` dependency values (such as `OUT_DIR`)
  and the working folder (`generate_hash_key`, rust.rs about lines 2147 to 2241). A stripped value is prefixed with NUL so it
  never collides with an unstripped one. `SCCACHE_BASEDIRS_VERBATIM_ENV_DEPS` (read per compile) lists `env!` variables that
  must keep their full path because a crate bakes them into its output.

**How the v0.19.0 Rust key is built** (rust.rs at the tag, read): the arguments without `-L`, `--extern`, `--out-dir`,
`--check-cfg`; the content hashes of sources, externs and static libraries; the `env!` dependencies; **every `CARGO_*`
environment variable** except `CARGO_MAKEFLAGS`, `CARGO_REGISTRIES_*`, `CARGO_BUILD_JOBS` and `CARGO_ENCODED_RUSTFLAGS`; the
working folder; the rustc version. sccache never caches incremental compiles, binaries, proc-macros, cdylibs or build
scripts (sccache `docs/Rust.md`).

**rustc's `--remap-path-prefix`** does not help the key in 0.19.0: the flag is hashed as an argument, and its value names
the checkout (`--remap-path-prefix=/wt/a=.`), so two worktrees still differ. It only changes what lands in the output
(debug info, panic paths; <https://doc.rust-lang.org/rustc/remap-source-paths.html>). Cargo's `trim-paths` is still
unstable (cargo#12137, <https://doc.rust-lang.org/cargo/reference/unstable.html>, read 2026-11-16). On sccache `main` the
basedir strip also normalises the remap argument.

**Comparable practice:**

| Tool | How it gets cross-checkout hits | Source |
|---|---|---|
| Bazel rules_rust | Keys actions on command lines with `${pwd}`, `${exec_root}`, `${output_base}` placeholders that the process wrapper substitutes at run time, and passes `--remap-path-prefix` for each (rustc.bzl, about line 1369) | <https://github.com/bazelbuild/rules_rust/blob/main/rust/private/rustc.bzl> (verified) |
| ccache | `CCACHE_BASEDIR`, the model sccache's basedirs copies | sccache Configuration.md (verified wording) |
| Firefox CI | Builds from one fixed checkout path on its workers, so sccache keys match | not verified (secondary) |
| Buck2 | Relative paths from the project root plus a remote action cache | not verified (secondary) |
| Paddock CI today | Fixed path per runner slot (`stable-workspace` action), target folder via `--config build.target-dir`, not `CARGO_TARGET_DIR` (`.gitea/actions/README.md`, `setup-rust` row) | read on origin/main |

So the common answer is either a fixed build path (what Paddock CI already does) or relative command lines (Bazel, Buck2).

## 2. Measurement (sccache 0.19.0, local disk cache)

Results block: [docs/benchmarks.md, "sccache 0.19.0 across worktrees"](../benchmarks.md#sccache-0190-across-worktrees-local-disk-cache-2026-11-16-docssccache-basedirs).
Unreleased `main` was not built: the brief asks for a pinned release binary only.

`tools/paddock-dev`, dev profile, `CARGO_BUILD_JOBS=2`, one shared `CARGO_HOME`, single runs on a loaded Mac:

| Run | Wall | Rust hits |
|---|---|---|
| No sccache, cold | 64 to 66 s | n/a |
| Same worktree, warm cache | 23 s | 100% |
| Other worktree, target in the worktree or via `--config build.target-dir` | 28 to 32 s | 82% |
| Other worktree, `CARGO_TARGET_DIR` in the environment | 58 s | 0% |

What this shows:

- **ADR 0014 §1's 6.5% came from `CARGO_TARGET_DIR`, not from the checkout path.** Cargo passes the variable down to rustc,
  and 0.19.0 hashes every `CARGO_*` variable, so a per-worktree target folder in the environment misses on every crate. The
  memory note "sccache hashes CARGO_TARGET_DIR" is right, for 0.19.0 too.
- With the target folder given through `--config build.target-dir` (or left in the worktree), **0.19.0 already hits 82% of
  the third-party crates across worktrees**, saving about 35 s of 65 s here, without basedirs. Registry crates compile from
  the shared `CARGO_HOME`, so their paths match.
- The 18% that miss are crates whose build script sets `OUT_DIR` (proc-macro2, serde, serde_json, thiserror,
  libsqlite3-sys, ...) and crates built on them. These are exactly what #2918 fixes.
- The workspace's own crates never hit in a dev build (`CannotCache(incremental)`), with or without basedirs. For them the
  kept target folder per app per worktree (ADR 0014 §9) stays the only saving.
- Not measured: an app workspace (Debrief has about 600 lockfile packages), Linux container, Windows, release profile.
  The Linux container check was skipped for time.

## 3. Recommendation

**Ranked:**

1. **Agents: one local sccache 0.19.0 on disk, no service** (common practice, the sccache README's own setup). One server
   for all agents (sccache's local storage takes one server at a time, `docs/Local.md`), `SCCACHE_DIR` on the vault build
   volume, size cap 30 GiB to start, `RUSTC_WRAPPER` set by the agents' build environment, and **the target folder passed as
   `--config build.target-dir=...`, never `CARGO_TARGET_DIR`**. Gain: a new worktree's first build of an app compiles
   about 80% fewer third-party crates (here 64 s to 28 s); later builds already reuse their kept target folder. It
   only pays on the first build of each worktree and app, which agents do several times a day. Invalidation: the key
   carries the rustc version, so a toolchain bump starts cold; sccache evicts by LRU at the size cap.
2. **CI: no change now.** It already uses 0.19.0, fixed paths and `--config build.target-dir`. Once a release contains
   #2918, `SCCACHE_BASEDIRS` could replace the `stable-workspace` move (simpler, not faster).
3. **basedirs locally: wait for a release with #2918, then decide.** Its limit for worktrees: the list is fixed when the
   server starts and must name every worktree root and every target folder, so each new worktree means a server restart,
   which cuts off running builds. A per-agent server with its own list would need its own cache folder (no sharing).
   It would also put worktree A's paths into debug info and panic messages of crates rebuilt from cache in B (cosmetic,
   and A may already be deleted).

**Fallback:** keep today's practice (no wrapper locally, kept target folders); nothing is lost.

**Risks:** one shared server is a single point: a crashed server can fail `RUSTC_WRAPPER` builds until it restarts
(how sccache falls back is not measured here); `sccache` must come from the pinned, checksum-checked download that
`ci_setup_rust.rs` already does, not from `~/.cargo`; cache poisoning is limited to one user's own machine. Nothing here
contradicts the 2026-10-28 decision: no shared backend or server is proposed.

**Cost (ER-37), against common practice (local sccache, which this is):**

| Package | Size |
|---|---|
| WP-1 Agent build environment: wrapper, one server on a declared port, cache folder and cap; `--config build.target-dir` instead of `CARGO_TARGET_DIR` where Paddock tools set it (for example `pre_push/heavy_tool.rs:116`) and in the AGENTS.md wording | 0.5 agent-day |
| WP-2 Measure an app workspace (Debrief, Pitwall) cold new worktree with and without; record in benchmarks.md | 0.25 agent-day |
| WP-3 After an sccache release with #2918: bump `SCCACHE_VERSION`, try basedirs in CI in place of `stable-workspace` | 0.5 agent-day, later |

Total about 0.75 agent-day now, at common practice; nothing beyond it.

## Decisions for the user

- **D1.** Turn on a local sccache 0.19.0 (one server, disk cache, no service) for agents' builds, with the target folder
  passed by `--config` (WP-1, WP-2)? Recommended: yes. The cheaper option is to keep today's practice.
- **D2.** Revisit basedirs only when an sccache release ships #2918 (WP-3)? Recommended: yes, wait.

Recurring: none.
