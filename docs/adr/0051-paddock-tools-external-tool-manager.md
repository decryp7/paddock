# 51. `paddock-tools`: one manager for the external tools, runtimes and model files the apps find, download and install

- Status: Proposed (2026-11-04). Written on the user's go-ahead of 2026-11-04; awaits an independent review and the user's
  answers to the open questions (Q1 to Q8). Docs only: no crate exists yet.
- Date: 2026-11-04
- Related: ER-17 to ER-20 (long operations, progress, no UI timeouts, parallelism and cost), ER-21 / rule 40 (recurring bugs
  mean a missing design rule), ER-22 / rules 39, 42, 43 (share first, shared behaviour tested once), ER-27 (logs explain
  every failure), rule 21 (tunables are Settings, knowledge lists are TOML packs), rule 52 (no starvable QoS for waited
  work), rule 53 and ADR 0033 (never act on a reusable OS name), rule 55 and ADR 0034 (one implementation per mechanism),
  ADR 0028 (`paddock-about` and `paddock-deps`: licences and notices), ADR 0024 (background work), Dyno ADR 0027 §7 and its
  amendments (the disk-health tool), Pacenotes README tier 3 and `pacenotes-core/src/pack.rs` (language-server packs),
  Sighter stage-2 plan SW9, SW11, SW36 and ADR 0001 (weights, ONNX Runtime, DirectML, CUDA). Memory notes
  "avoid hard coding", "licence exceptions", "network on by default", "local test VMs", "estimate after design".
- Number: 0051. `origin/main` (`72a5bfd1b`) holds up to 0044 and 0049; 0045 to 0048 and 0050 exist on other branches
  (`git ls-tree --name-only <ref> docs/adr/` over all remote branches, 2026-11-04); no branch holds 0051.
- Crate name: `paddock-tools` as briefed. Q8 asks whether a less overloaded name is wanted ("tools" also means MCP tools,
  `tools/paddock-dev` and Debrief's prompted tools).

## Context

### Three apps need the same thing, and four apps already have copies of parts of it

All read in git on 2026-11-04 at the refs named.

| App | What it finds, downloads or installs | Where (ref) |
| --- | --- | --- |
| Dyno 1.1.0 | smartmontools 7.5: pins in `dyno-agent/tools/tool-pins.toml` (agent) **and** compiled into `dyno-platform/src/tool_install.rs` `PINS` (helper; "the agent's table is never trusted by root"); download job `dyno-agent/src/tool_download.rs` over `paddock-download` and `paddock-net` `Egress`; Windows install by the privileged helper (NSIS `/S /SO x64,smartctl,drivedb /D=`, `tool_install.rs`, `dyno-helper/src/tools.rs`); macOS own copy from the DMG (`dyno-agent/src/tool_own_copy.rs`: `hdiutil`, `pkgutil --expand-full`, quarantine flag, `--version` run once); Linux through PackageKit (ADR 0027 §7); the offer card (`dyno-core/src/tool_offer.rs`); user-tool search (`dyno-core/src/toolpath.rs`, `dyno-platform/src/toolpath.rs`: login-shell `PATH`, Homebrew and `~/.local/bin` fall-backs); pin evidence `dyno-helper/tools/smartmontools-7.5.pin.md` and `scripts/verify-tool-pin.sh` (system `gpg`) | `origin/steward/dyno-1.1.0` `1589c46bb` |
| Pacenotes | Language servers (tier 3): a declarative `ServerPack` with `detect` (a program on `PATH`, a rustup component), `install_offer` (a command shown, never run without a click) and a pinned `download` (https plus SHA-256, unpinned refused); servers are never bundled and start only for a trusted repository. Embedding models (ADR 0001: a file on disk, checked by hash). | `origin/steward/pacenotes-stage1` `c1bba3f9f`: `apps/pacenotes/README.md` lines 32-34, `crates/pacenotes-core/src/pack.rs`, `docs/adr/0001-embeddings.md` |
| Sighter | RF-DETR weights (about 1.29 GB, four single files mirrored as the Gitea generic package `sighter-weights` 1.11.2, catalogue `apps/sighter/catalogue/model-weights.toml`, "the parser for single-file entries is the shared asset kind of SW11"); its own ONNX Runtime build as a `paddock-runtime` catalogue (`scripts/ort/catalogue/onnxruntime.toml`, placeholder pins until the build publishes); DirectML (fetched from nuget.org at build time, ships only if the S2/P6 check passes); the opt-in CUDA training runtime (licence exception, downloaded from upstream on request, never mirrored); SW36 managed training runtime as a `paddock-runtime` entry | `origin/sighter/sw9-weights` `ef7bcc8d8`; `origin/docs/sighter-stage-2` `apps/sighter/docs/plans/stage-2.md` rows SW11, SW36; memory note "Sighter decisions 2026-10-29" |
| Debrief (shipped) | llama.cpp builds (`debrief-ai/src/runtimes/store.rs`), GGUF models (`debrief-ai/src/models/`), FFmpeg (`debrief-video/src/runtime/store.rs`, and the user's own ffmpeg found by `runtime/detect.rs`: Homebrew, package-manager and Windows folders plus `PATH`), Chrome for Testing (`app/src-tauri/src/browser/runtime.rs`), a custom `llama-server` path | `origin/main` |
| Pitwall (shipped) | llama.cpp installs "adapted from Debrief's runtime manager" (`pitwall-server/src/engines.rs`), GGUF downloads from Hugging Face (`pitwall-server/src/library.rs`) | `origin/main` |
| Shared | `paddock-runtime` (catalogue, versioned store, unpack, self-test, rollback, quarantine; **no app depends on it yet**: `git grep paddock-runtime -- apps` finds only docs), `paddock-download`, `paddock-net`, `paddock-archive`, `paddock-packs`, and `paddock-chromium/src/discover.rs` (finding a user-installed Chromium) | `origin/main` |

Two mechanisms therefore already exist several times:

1. **The pinned own-copy store** (download, verify, unpack, self-test, versioned folder, rollback): `paddock-runtime`,
   Debrief's FFmpeg store, Debrief's llama.cpp store, Pitwall's engines (a port of Debrief's), Dyno's own copy. Only the
   Debrief model downloader is in `docs/known-copies.toml` (`model-download`); the stores are not registered (rule 55 (c)).
   Dyno ADR 0027 §7 said "a `paddock-runtime` catalogue entry per OS"; the code built its own `tool-pins.toml` instead.
2. **Finding the user's own install** of a program: Dyno's `toolpath` and the Windows `smartctl_search` (below), Debrief's
   `ffmpeg` `detect.rs`, `paddock-chromium`'s `discover.rs`, Pacenotes' `Detect`. Each has its own list of folders.

### The Dyno bugs of 2026-11-04 are one pattern (ER-21)

Found by the user while testing Dyno 1.1.0 on Windows. The fix is on `fix/dyno-windows-smart` (worktree
`dyno-smart`, uncommitted on 2026-11-04; its draft amendment to Dyno ADR 0027 is the source of this list):

| # | Bug | What was missing |
| --- | --- | --- |
| B1 | The helper installed `smartctl`, but nothing read SMART through it (`HelperLink::smart` had no caller) | a consumer that obtains the program from the manager rather than from its own lookup |
| B2 | The helper's `Info` listed no tools, so Settings said "Not installed" after a successful install | one state report derived from the same layout the installer writes |
| B3 | Discovery missed the user's own install (`%ProgramFiles%`, the installer's `InstallLocation` under `WOW6432Node`, `PATH`) | a locate step with per-platform sources, each reported |
| B4 | No re-read after the install | a confirm phase at the end of every install, and a change event |
| B5 | Errors were silent | typed outcomes with a next step, and phase logs (ER-27) |
| B6 | The install folder was defined in two places | one path definition used by app, agent and helper |

Each is the kind of bug the next app will repeat with its own copy: Pacenotes needs B3 for every language server, Sighter
needs B2/B4 for its runtime and weights, and Debrief already wrote B3 twice (FFmpeg, Chromium). This ADR is the ER-21
redesign: one mechanism that makes B1 to B6 impossible or checked, and the copies above registered and retired through it.

## Decision

### D1. One crate, `paddock-tools`, built on the existing crates; core plus `cfg`-gated adapters, not separate crates

`paddock-tools` owns the **lifecycle of an external thing an app uses but does not ship**: a program (smartctl, a
language server, llama-server, ffmpeg), a runtime (ONNX Runtime, a Python training runtime) or a data file (model weights).
It composes the shared crates instead of copying them (rule 42):

| Concern | Owner |
| --- | --- |
| Tool packs (parse, limits, built-in plus user, reload) | `paddock-packs` (a new `PackKind`) |
| HTTP policy: https, host allow-list, reach guard, offline switch | `paddock-net` (`Policy`, `Egress`) |
| Bytes on disk: ranges, resume, hash while writing, free-space check, per-host caps | `paddock-download` |
| The own-copy store: versioned folders, staging, self-test, rollback, quarantine, cross-process lock | `paddock-runtime` (extended, D5) |
| Unpacking archives | `paddock-archive` |
| Jobs, phases, Cancel, clock | `paddock-jobs` |
| Running a probe or an installer, by identity | `paddock-proc` (rule 53) |
| Licences and notices of downloaded tools | `paddock-deps` / `paddock-about` (ADR 0028) |
| **Locate, probe, report, plan install and uninstall, the privileged port, the shared layout, update checks** | **`paddock-tools` (new)** |

Layout of the crate (one crate, modules):

```text
crates/paddock-tools/
  src/pack.rs        ToolPack: the PackKind, typed and validated (D2)
  src/layout.rs      ToolLayout: the one path definition (D7)
  src/locate/        Source enum, the search plan (pure), the Locator over ports; native/{windows,macos,linux}.rs
  src/probe.rs       version probe over a Runner port, bounded, cached by file identity
  src/state.rs       ToolState and ToolReport (D6), generated TypeScript
  src/install/       the plan (pure), OwnCopy over paddock-runtime, PackageManager, Privileged port (D4)
  src/privileged.rs  the helper-side verified install (feature `privileged`, no network code)
  src/update.rs      pin-driven update check
  src/tauri.rs       feature `tauri`: commands, event, health entry, settings section
  src/testing.rs     feature `testing`: fakes and the contract tests apps run (rule 43)
```

Features: `fetch` (default: `paddock-net` and `paddock-download` with `http`), `privileged` (the helper's side: pack parse,
verify, run the installer; **no** network or Tauri code so a helper binary stays small), `tauri`, `testing`.

**Why not core plus platform adapter crates:** the adapters are small (a registry read, a few folder lists, `hdiutil` and
`pkgutil` calls) and `cfg`-gated; extra crates cost a publish entry, a hakari line and a coverage gate each for no
substitution benefit (the ports already make them replaceable in tests). Native glue that tests cannot reach goes in one
`native.rs` per platform, kept free of logic and listed in the coverage ignore regex (new-crate skill step 6).

**Why not just grow `paddock-runtime`:** it has one job (a versioned store of releases Paddock installed) and does it well.
Locating a user's install, probing a version found anywhere, privileged installs, package managers and the report are a
different reason to change. `paddock-tools` uses the store as one install route (D5).

### D2. Per-tool knowledge is data: a `tool` pack kind, built-in packs pinned, user packs only for finding

A tool pack is a TOML file read through `paddock-packs` (rule 21: parsed once with limits, unknown keys reported, one bad
pack refused whole, reloaded without a restart). What is declared as data versus code:

- **Data (the pack):** identity, licence, how it is used, pinned artifacts per platform with evidence, locate hints, the
  version probe (arguments and a capped regex), install recipes chosen from a closed set, uninstall notes.
- **Code (closed enums in the crate):** the kinds of locate source (D3), the kinds of install recipe (D4), the kinds of
  evidence. A new kind is a new variant with its adapter and tests (open/closed); a new tool is a new pack.

Format (`format = 1`), Dyno's smartmontools as the worked example:

```toml
format = 1
id = "smartmontools"                       # folder-name safe; unique across the app's packs
name = "smartmontools"
licence = "GPL-2.0-or-later"               # SPDX; checked against the paddock-deps policy (D9)
use = "separate-program"                   # separate-program | library | data; copyleft only as separate-program
source_url = "https://www.smartmontools.org"
programs = ["smartctl"]                    # executables this tool provides; the first is the main one

[probe]                                    # how a found copy proves what it is (bounded run, D3)
args = ["--version"]
version = '^smartctl (\d+\.\d+)'           # user_regex limits of paddock-packs
timeout_ms = 5000
min_version = "7.0"                        # below this: Found, but "too old: <v>, needs 7.0"

[locate]                                   # hints only; the sources themselves are code (D3)
windows.dirs = ["%ProgramFiles%\\smartmontools\\bin", "%ProgramFiles(x86)%\\smartmontools\\bin"]
windows.registry = [
  { key = 'HKLM\SOFTWARE\smartmontools', value = "InstallLocation", subdir = "bin" },
  { key = 'HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\smartmontools', value = "InstallLocation", subdir = "bin" },
]                                          # both registry views are always read (D3)
macos.dirs = ["/opt/homebrew/sbin", "/usr/local/sbin"]
linux.dirs = ["/usr/sbin", "/usr/bin"]
linux.package = { deb = "smartmontools", rpm = "smartmontools" }

[[artifact]]                               # one per platform key; same fields as paddock-runtime's catalogue entries
platform = "windows-x86_64"
version = "7.5"
file = "smartmontools-7.5.win32-setup.exe"
url = "https://github.com/smartmontools/smartmontools/releases/download/RELEASE_7_5/smartmontools-7.5.win32-setup.exe"
size = 1508408
sha256 = "896337fcc253220614cf8cdbd5cf2321c5aa326a37a04160a672a281e6104c70"
hosts = ["github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com"]
install = { recipe = "nsis", privileged = true, args = ["/S", "/SO", "x64,smartctl,drivedb"], dest_arg = "/D=" }
layout = { executable = "bin/smartctl.exe" }
evidence = [
  { kind = "openpgp", fingerprint = "0C9577FD2C4CFCB4B9A599640A30812EFF3AEFF5", verified = "2026-11-03", record = "smartmontools-7.5.pin.md" },
  { kind = "authenticode", subject = "CN=SignPath Foundation", verified = "2026-11-03" },
]

[[artifact]]
platform = "macos-aarch64"                 # macos-x86_64 too if the DMG is universal (WP-11 checks)
version = "7.5"
file = "smartmontools-7.5.dmg"
url = "https://github.com/smartmontools/smartmontools/releases/download/RELEASE_7_5/smartmontools-7.5.dmg"
size = 1619514
sha256 = "4aa176c161612488f3db053fdf3eee47bbc5178c8fe46afb2002d8dbdf563cc5"
hosts = ["github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com"]
install = { recipe = "dmg-pkg", privileged = false, take = ["usr/local/sbin/smartctl", "usr/local/share/smartmontools/drivedb.h"] }
layout = { executable = "smartctl" }
evidence = [{ kind = "openpgp", fingerprint = "0C9577FD2C4CFCB4B9A599640A30812EFF3AEFF5", verified = "2026-11-03", record = "smartmontools-7.5.pin.md" }]

[[artifact]]
platform = "linux-*"
install = { recipe = "package-manager" }   # PackageKit InstallPackages, or the distro command shown (Dyno ADR 0027 §7)
```

The `take` paths above are illustrative; WP-11 copies the real ones from `tool_own_copy.rs`.

Rules the parser enforces (each a failing test first):

- An artifact without `size`, a 64-hex `sha256`, an `https` `url` whose host is in its own `hosts`, and at least one
  `evidence` entry is refused. `hosts` becomes the `paddock-net` `Policy` allow-list of that download, and `size` its body cap.
- `use = "library"` with a copyleft licence is refused unless `paddock-deps` lists a scoped exception (D9).
- **Origin matters** (`paddock_packs::Origin`): a **user** pack may add `locate` hints and a `probe` for a tool, and may
  declare a new find-only tool; it may **not** declare artifacts or install recipes. Downloads and installs come only from
  built-in packs compiled into the app. A privileged install comes only from the copy compiled into the helper (D4).
- Weights and other data files use `use = "data"`, no `probe`, `programs = []` and the single-file recipe (D5); the
  Sighter catalogue's extra fields (`lineage`, `resolution`, `upstream_md5`) go into an `[artifact.meta]` table the crate
  carries through without judging.

**Avoid hard coding (memory note):** the folder hints are a fall-back. The sources that need no list come first: the
registry's own `InstallLocation`, the package manager's own answer (`brew --prefix <formula>`, `dpkg -L`/`rpm -ql` are
**not** run by default, see Q4), the login-shell `PATH` and the user's per-tool path in Settings. A pack's `dirs` list
covers only measured gaps, and the user corrects a miss with Settings, not a new release.

### D3. Locate: Paddock's copy first, then the user's install, every source reported

`Locator::locate(tool) -> Located` runs a **search plan** (pure: built from the pack, the layout, the platform and the
Settings override) over ports, in this order:

1. **Settings override** (`tools.<id>.path`): the user's explicit choice wins; validated as a plain absolute path (the
   rules of Dyno's `toolpath::validate_dirs`).
2. **Paddock's own copy**: the app's `paddock-runtime` store for the tool (`ToolLayout::own`), then the helper's protected
   copy (`ToolLayout::privileged`) when the app has a helper.
3. **The user's install**, by platform:
   - Windows: `%ProgramFiles%`, `%ProgramFiles(x86)%` and the pack's `windows.dirs` (environment variables expanded, nothing
     else); the registry values the pack names, **both views** (`KEY_WOW64_64KEY` and `KEY_WOW64_32KEY`) because 32-bit
     installers such as smartmontools' NSIS write under `WOW6432Node`; then the `PATH` folders.
   - macOS: Homebrew (`/opt/homebrew`, `/usr/local`), the pack's `macos.dirs`, then the login-shell `PATH` (Dyno's
     resolver: asked once, bounded, cached) and the process `PATH`.
   - Linux: the pack's `linux.dirs` (distro paths), `/usr/local`, `/snap/bin`, then the login-shell and process `PATH`.
4. Every candidate that exists is **probed** (D6); the first that passes wins, the others are listed as alternatives.

Every attempt is recorded as `Attempt { source: Source, path, outcome }` (`NotThere`, `NotExecutable`, `ProbeFailed(why)`,
`TooOld(v)`, `Ok(v)`), returned in the report and logged (paths only at debug level: ER-27, no user content by default).
B3 becomes a test per source with a fake environment.

Sources are a closed enum: `Override`, `OwnCopy`, `PrivilegedCopy`, `KnownDir`, `Registry { view }`, `LoginPath`,
`ProcessPath`, `PackageManager`. Ports: `Fs` (exists, is-executable, file identity), `Registry` (read one value in one view),
`Env` (variables, `PATH`, login-shell `PATH`), `Runner` (the probe). The Windows registry adapter uses the `windows-registry`
crate of windows-rs (MIT OR Apache-2.0; not in any lockfile yet, so WP-2 checks its licence and size) instead of spawning
`reg.exe` as the Dyno fix does: one API call per value, no output parsing.

**Trust of a located file.** `Located` carries `trust: Protected | UserWritable`. A program in a folder only an
administrator can write (Program Files, `/usr/sbin`, the helper's folder, root-owned and not group- or other-writable along
the chain) is `Protected`. A privileged process (Dyno's helper) runs only `Protected` copies; it never executes a file from
a user-writable place. The check runs on the open file (rule 53): the helper opens the executable, checks its owner and
the folder chain, and runs it by that held identity or re-checks immediately before the spawn.

### D4. Install routes: own copy, package manager, or the app's privileged port; the crate never escalates

`ToolManager::install(tool, ctx) -> Result<ToolReport, ToolError>` is one job (ER-17, ER-18) with phases on their own
scales: `plan` → `download` (bytes) → `verify` → `install` (route-specific) → `confirm` (locate and probe again; the job fails
if the result is not `Confirmed`, B4) → `report` (event `paddock://tools`, B1/B2). Cancel works up to the uncancellable swap.

Routes, chosen by the artifact's `install.recipe` and `privileged` flag:

| Route | Recipes | Who runs it |
| --- | --- | --- |
| Own copy | `archive` (zip/tar/tar.gz/tar.xz via `paddock-archive`), `single-file`, `dmg-pkg` (macOS: `hdiutil attach -readonly -nobrowse -mountrandom`, `pkgutil --expand-full`, take the listed files, detach; quarantine flag removed only from the taken files and logged, as Dyno does), `nsis`/`msi` with `privileged = false` (per-user installers into the store) | the app or agent, as the user |
| Package manager | `package-manager` (Linux: PackageKit `InstallPackages` over D-Bus, the user authenticates through polkit; macOS: the `brew install` command is **shown**, never run) | the system, after the user's own authentication |
| Privileged | any recipe with `privileged = true` | the app's helper, through the port below |

The privileged port is a trait the app implements over its own helper IPC:

```rust
/// What the app's privileged helper offers. The crate calls it; it never elevates itself.
pub trait PrivilegedInstaller: Send + Sync {
    /// Whether the helper is there and answering; the text says how to turn it on when it is not.
    fn availability(&self) -> Availability;
    /// Installs `artifact` of `tool` from `staged` (a file the job verified). The helper verifies again against ITS
    /// compiled-in pack and returns what it holds afterwards.
    fn install(&self, req: &PrivilegedRequest, staged: &StagedFile) -> Result<HeldTool, PrivilegedError>;
    fn uninstall(&self, tool: &ToolId) -> Result<(), PrivilegedError>;
    /// What the helper holds, from the shared layout (B2).
    fn held(&self) -> Result<Vec<HeldTool>, PrivilegedError>;
}
```

The helper side is **shared code too** (`paddock_tools::privileged`, feature `privileged`), lifted from Dyno's
`tool_install.rs`: given the helper's compiled-in packs, a tool id, the bytes or a handle to the staged file, and the
helper's `ToolLayout::privileged` roots, it writes create-new into the helper's staging folder while hashing, refuses a wrong
size or hash, runs the recipe with `TEMP`/`TMP` inside staging, empties the destination first (the Dyno fix), confirms the
`layout.executable` exists, clears staging on every outcome, and logs each phase with its duration. The helper's packs are
the **same files** as the app's, compiled in with `include_str!` from one path; a test in each app asserts the helper
and the app embed byte-identical packs (so B6's twin for pins cannot recur). The agent's catalogue stays untrusted by root.

Transfer: Dyno today streams the bytes in the IPC frame (about 1.5 MB, under its 4 MB frame). The shared port takes a
`StagedFile` instead: the helper opens the agent's staged file once, copies it with hashing into its own protected staging,
and never executes from the unprivileged location; small transports may still send bytes (`StagedFile::Bytes`). Large
artifacts (runtimes, hundreds of MB) need this; Q5 asks whether any privileged artifact will ever be large.

### D5. `paddock-runtime` gains what the routes need, so the store stays one implementation

- **Single-file artifacts** (Sighter SW11 / P3): a catalogue entry that is one file, no unpack, no executable, a
  `self_test` that only re-checks the hash. This is the same work package as SW11; it lands once, here.
- **An install step besides unpack**: `RuntimeKind` gets an optional `prepare(staged, staging_dir)` hook so `dmg-pkg` and
  per-user installers fill the staging folder; the store keeps its swap, rollback and quarantine.
- **`runtime.json` records the executable's SHA-256** at install; `paddock-tools` re-checks it before a privileged run.
- `paddock-runtime`'s catalogue format stays the per-runtime form for existing users (Sighter's ORT catalogue); a tool
  pack's `[[artifact]]` uses the same field names so a catalogue converts one-to-one (no shim: pre-v1).

### D6. State is reported honestly, including "found but not confirmed: why"

```rust
pub enum ToolState {
    /// Nothing usable was found; every place looked at is in `searched`.
    NotFound { searched: Vec<Attempt>, offer: Option<Offer> },
    /// A copy is there and its probe passed.
    Confirmed { source: Source, path: PathBuf, version: String, trust: Trust, alternatives: Vec<Attempt> },
    /// A copy is there but could not be confirmed. `why` names the cause and the next step: the probe timed out, macOS
    /// refused to run it (Gatekeeper), the version is below `min_version`, it needs administrator rights to read, the
    /// helper holds it but is off.
    Unconfirmed { source: Source, path: PathBuf, why: Why, next_step: String },
    /// An install or uninstall job is running.
    Busy { job: JobId, phase: String },
    /// Not available on this system; `instead` says what to do.
    Unsupported { instead: String },
}
pub struct ToolReport { pub id: ToolId, pub name: String, pub licence: String, pub source_url: String,
                        pub state: ToolState, pub in_use: Option<Source>, pub update: Option<UpdateOffer> }
```

`ToolReport` and `Offer` are wire types with generated TypeScript (`paddock_commands::ts`, guarded by
`testing::sync_generated`). **Consumers never look a tool up themselves:** they call `ToolManager::resolve(id)`, which
returns the `Confirmed` path or the report explaining why there is none, and they subscribe to changes; a lint (WP-12)
flags app code that builds a tool path by hand. This is the structural answer to B1.

Probes run through `paddock-proc` with a timeout and an output cap, and are cached by file identity (path, size, modified
time, and on Unix the inode), so a status refresh re-probes only what changed. A probe is live work (a person waits for
Settings to fill): normal priority on Windows, `QOS_CLASS_UTILITY` allowed on macOS (rule 52).

### D7. One path definition: `ToolLayout`

`ToolLayout` is a pure function from `(root kind, tool id, version, platform)` to every folder and file the crate uses:
the own-copy store root and release folder, the helper's protected `tools\<id>-<version>` folder and its staging folder,
and the executable inside each (`layout.executable` from the pack). The installer, the locator, the helper's `held()` and
its readers all call it; nothing else formats a tool path (B6). The roots come from `paddock_env::AppDirs` for the app and
from the helper's own protected root, passed in.

### D8. Downloads, caching and dedup

- Downloads go through `paddock-net` `Egress` with a `Policy` built from the artifact (https, its `hosts` only, public
  addresses, body cap = `size`, the offline switch), then `paddock-download` (resumable `.part`, hash while writing,
  free-space check, per-host caps, 429 back-off). Network features are on by default with a per-feature switch (memory
  note, 2026-11-03): each app's tool downloads sit under one switch, and nothing is fetched without the user's click.
- **v1: per app, content-addressed.** Downloads land in `<app cache>/downloads/sha256/<hex>.part`, renamed to `<hex>` when
  verified, and are deleted after a successful install (kept on failure for resume; swept at start-up after 7 days).
- **Shared across apps: later, and only if measured** (Q3). The likely overlaps are models (Model2Vec-class embedding models are candidates in
  Pacenotes ADR 0001 and in Debrief; Debrief and Pitwall both use llama.cpp and GGUF files). A family folder
  (`<Paddock data>/shared/blobs/sha256/<hex>` with a reference file per app and a sweep for unreferenced blobs) makes the
  move a path change because the layout is already content-addressed. A shared blob is re-hashed before use: another
  app's process is the same user, but a truncated or swapped file must still be refused.

### D9. Licences and evidence

- Every pack names an SPDX licence and `use`. `paddock-deps` (ADR 0028) gets a "downloaded on request" class: the tool, its
  licence and source link appear in About and the notices; copyleft tools are allowed only as `separate-program` (never
  linked, never bundled, never mirrored unless a decision says so; Sighter's Apache-2.0 weights are mirrored by decision).
  Named exceptions (DirectML, the CUDA runtime) are `paddock-deps` exceptions, not pack flags.
- Evidence (`openpgp`, `authenticode`, `minisign`, `upstream-digest`, `mirror-of`) is checked **when a pin is added**, by a
  developer tool, never by the app: the app's gate is the size and SHA-256 pin. `apps/dyno/scripts/verify-tool-pin.sh`
  becomes `./dev tools pin <pack> <artifact>` (ADR 0022: scripts move to Rust), which downloads, runs the system `gpg
  --verify` against the pinned fingerprint (or `signtool verify`/`osslsigncode` for Authenticode), writes the record file
  and prints the artifact lines. A pin without a record file is refused by a conformance check (WP-9).
- Optional runtime defence (Q6): on Windows, `WinVerifyTrust` on an Authenticode-signed installer before the helper runs it.

### D10. Update checks are pin-driven

A newer version reaches users only with a new pin, which ships with an app release (the helper's packs are compiled in).
`update::check` therefore compares what is installed (own copy, helper copy) with the pin of this build and offers
"Update to 7.6" when the pin is newer; for a user's own install it reports "your 7.0 is older than the 7.5 Dyno knows",
without replacing it. No network request is needed. An upstream "a newer release exists" check is Q2.

### D11. Uninstall

Own copies: `paddock-runtime` removes the release folders (and the previous one) under its lock. Privileged copies: the
port's `uninstall`, which uses `ToolLayout` and refuses anything outside the helper's tools folder. Package-manager and
user installs are never removed by Paddock; the report says how ("installed by Homebrew: `brew uninstall smartmontools`").
After any uninstall the confirm phase runs again and the event fires, so `in_use` falls back to the next source.

### D12. Logging (ER-27)

Per job: one correlation id (the job id); a line per phase with its duration and outcome (`plan`, `download` with bytes
and resumes, `verify` with size and hash outcome, `install` with the recipe, the exit code and stderr tail of an installer,
`confirm` with the winning source); every failure with its cause chain and the next step shown to the user. Per locate:
one summary line (`sources=7 found=2 confirmed=1 winner=registry32 ms=14`) at info, the attempts at debug. No URLs with
query strings, no user paths at info level, no file contents.

### D13. Tauri adapter and UI

Feature `tauri`: managed `ToolManager`; commands `tools_status`, `tools_install`, `tools_uninstall`, `tools_set_path`,
`tools_recheck`; the event `paddock://tools`; a `paddock-health` entry per tool whose Repair is "Check again" (or "Install"
when an offer exists); a settings section with the per-tool path override (rule 44: the section has a setting). The shared
UI is a `pd-tool-card` in `@decryp7/paddock-ui` (rule 39), built from Dyno's `tool-card.ts`, showing the state, the source
in words ("the copy Dyno installed", "your own install (Program Files)", "Homebrew"), licence and size before the click, the
job's progress, and the searched places when nothing was found.

### Parallelism and cost (ER-20, rule 51)

- **Locate** is cheap: a few dozen `stat` calls and at most a handful of registry reads per tool; run in sequence per tool,
  tools in parallel on the shared executor (I/O bound, a few at a time). Measured target for WP-2: under 50 ms for 10 tools
  with a warm cache, recorded in `docs/benchmarks.md` (rule 48).
- **Probes** are the expensive part (a process each): cached by file identity, at most 2 at a time, never repeated within
  one status call. A changed file or an install invalidates its entry.
- **Downloads**: `paddock-download`'s per-host caps and segments; bulk work at `QOS_CLASS_UTILITY` / nice 10 /
  `THREAD_PRIORITY_BELOW_NORMAL` (rule 52), with progress and Cancel.
- **Installs**: one at a time per tool (the store's lock, the helper's `InstallGuard`); different tools may install at once.
- Thread counts are not new Settings: the executor's existing pools and `paddock-download`'s caps apply.

### Testing (rule 43, ER-14)

- **Pure core**: pack parsing (every refusal, never-panics fuzz for check C36), the search plan, the state machine, the
  layout, the update comparison, the install plan.
- **Fakes** (`paddock_tools::testing`): `FakeFs`, `FakeRegistry` (two views, so the `WOW6432Node` case is a unit test),
  `FakeEnv` (`PATH`, login `PATH`, variables), `FakeRunner` (scripted probe answers, timeouts, Gatekeeper refusal),
  `FakePrivileged` (records requests, can refuse or fail), `paddock_net::FakeWire` for downloads, `FakeClock`.
- **Contract tests** that each adopting app runs on its wiring: "after a successful install, `resolve` returns the
  installed path and an event fired" (B1, B4), "`held()` lists what the installer wrote" (B2), "the helper and app embed the
  same packs" (B6).
- **Real OS**: the registry adapter and NSIS route on Windows, `dmg-pkg` on macOS, PackageKit on Linux run only in the
  **local test VMs** (memory note: VMs local only, never CI), as a `vm-smoke` scenario that installs smartmontools 7.5 for
  real (own copy and through a test helper), uninstalls it and checks nothing is left. CI runs the fakes only; tests never
  read the real registry, `PATH` or install folders of the machine they run on.

## Migration and adoption

### Dyno (first adopter, after the 1.1.0 tag)

Dyno 1.1.0 ships with its own code and the `fix/dyno-windows-smart` fix. After the tag (WP-11):

| Dyno file (`origin/steward/dyno-1.1.0`) | Goes to |
| --- | --- |
| `dyno-agent/tools/tool-pins.toml`, `dyno-agent/src/tool_pins.rs`, `PINS` in `dyno-platform/src/tool_install.rs` | `apps/dyno/tools/smartmontools.toml`, a tool pack compiled into the agent and the helper from one path |
| `dyno-platform/src/tool_install.rs` (check, stage, NSIS command) and `dyno-helper/src/tools.rs` (one at a time, staging) | `paddock_tools::privileged`; the helper keeps only its IPC dispatch and the `InstallGuard` wiring |
| `dyno-agent/src/tool_download.rs` (download job, `ToolSink`) | `ToolManager::install`; `HelperLink` implements `PrivilegedInstaller` |
| `dyno-agent/src/tool_own_copy.rs` (DMG, pkgutil, quarantine) | the `dmg-pkg` recipe over `paddock-runtime` |
| `dyno-platform/src/smartctl_search.rs` (the fix) and the smartctl part of `toolpath.rs` | `paddock_tools::locate`; `toolpath.rs` keeps restic and borg until they are tool packs too (find-only) |
| `dyno-core/src/tool_offer.rs` | `ToolReport` / `Offer` |
| `app/.../settings/tool-card.ts`, `tool-model.ts`, `state/tool.store.ts` | `pd-tool-card` and the generated types |
| `scripts/verify-tool-pin.sh` | `./dev tools pin` |

The helper wire changes (`InstallTool` takes a staged-file handle, `Info` lists `HeldTool`s): a helper protocol bump, so
it lands with the next helper version. Per rule 43 Dyno deletes its copies and the tests that duplicated them and records the
test count and time before and after. Until then the copies are registered (WP-0).

### Pacenotes (adopts before its first release; it has no shipped copy)

- A `ServerPack` keeps what is about the language (globs, features, init options, trust rules) and names its server by
  tool id: `server.tool = "rust-analyzer"`. The tool pack holds `detect` (as `locate` and `probe`), `install_offer` (as the
  `package-manager` recipe's shown command or a `rustup` source) and `download` (as an artifact). `Detect::RustupComponent`
  becomes a locate source variant (`Rustup { component }`), added in the crate with its test.
- Pacenotes' rule that a server starts only for a trusted repository stays in Pacenotes: `paddock-tools` finds and installs,
  it never decides to run.
- Embedding model files (ADR 0001) use `use = "data"` single-file artifacts.

### Sighter (adopts with SW11)

- SW11 (single-file asset kind) is WP-5 here. The weights catalogue becomes a tool pack of four data artifacts with the
  Gitea URLs and our SHA-256s; `upstream_url`, `upstream_md5`, `lineage`, `resolution` move to `[artifact.meta]`.
- ONNX Runtime stays a `paddock-runtime` catalogue (an archive runtime) and is reported through `paddock-tools` as a tool
  with `use = "library"`, licence MIT.
- DirectML: the build-time fetch is not this crate's job; if the "system DLL too old" case ships, it is an own-copy
  `single-file` artifact under the DirectML licence exception. The S2/P6 kit stays a manual kit.
- The CUDA training runtime is an artifact from upstream (never mirrored) under its licence exception, behind its opt-in.

### Debrief and Pitwall (later, after their next releases)

Their stores (FFmpeg, llama.cpp, Chrome for Testing, GGUF) and finders (`detect.rs`, `paddock-chromium` `discover.rs`) are
registered now (WP-0) with delete-by tags chosen by the user (Q7), and move onto `paddock-runtime` plus `paddock-tools` in
their following release lines. GGUF model **libraries** (search, fit check, many user-chosen files) stay app domain; only the
download and verify of a chosen file uses the crate.

## Work packages

Sizes in agent-days (S ≤ 1, M 2-3, L 4-6); firm once the review and Q1-Q8 are settled (memory note "estimate after
design"). Order: WP-0 now (docs only); WP-1 to WP-10 after the Dyno 1.1.0 and Spotter 1.0 tags (current release order);
WP-11 in Dyno's next line; WP-12, WP-13 with Pacenotes and Sighter.

| WP | Content | Size | Depends on |
| --- | --- | --- | --- |
| WP-0 | Register the copies in `docs/known-copies.toml`: `runtime-store` (canonical `paddock-runtime`; Debrief FFmpeg and llama.cpp stores, Pitwall engines, Dyno own copy) and `tool-locate` (canonical `paddock-tools`, once it exists; Dyno `toolpath`/`smartctl_search`, Debrief `detect.rs`, `paddock-chromium` `discover.rs`), delete-by tags per Q7 | S | user Q7 |
| WP-1 | Crate scaffold (new-crate skill), `ToolPack` `PackKind` with all refusals, `Origin` rules, limits, never-panics test, `ToolLayout` | M | - |
| WP-2 | Locate: search plan, sources, ports, Windows (`windows-registry`, both views), macOS, Linux adapters, Settings override, attempts and trust; locate benchmark | L | WP-1 |
| WP-3 | Probe (bounded, cached by identity), `ToolState`/`ToolReport`, `resolve`, change events, generated TypeScript | M | WP-2 |
| WP-4 | Download and verify: `Policy` from the artifact, `paddock-download`, content-addressed downloads folder, sweep | S | WP-1 |
| WP-5 | `paddock-runtime`: single-file artifacts (= Sighter SW11), the `prepare` hook, executable SHA-256 in `runtime.json`; the `archive`, `single-file`, `dmg-pkg`, per-user installer recipes | L | WP-4 |
| WP-6 | `PrivilegedInstaller` port, `paddock_tools::privileged` (from Dyno's `tool_install.rs`), staged-file transfer, trust check of located files | M | WP-1, WP-3 |
| WP-7 | Package-manager route (PackageKit, shown commands), uninstall, pin-driven update check | M | WP-3 |
| WP-8 | Tauri adapter (commands, event, health entry, settings section) and `pd-tool-card` in `paddock-ui` | M | WP-3, WP-7 |
| WP-9 | `./dev tools pin` (Rust port of `verify-tool-pin.sh`, OpenPGP and Authenticode evidence, record file) and the conformance check "every artifact has a record" | M | WP-1 |
| WP-10 | Real-OS `vm-smoke` scenarios: Windows (NSIS own copy and test helper, registry both views), macOS (`dmg-pkg`, Gatekeeper refusal path), Linux (PackageKit) | M | WP-5, WP-6, WP-7 |
| WP-11 | Dyno migration (table above), helper protocol bump, delete copies, test counts before and after | L | WP-1..10, Dyno 1.1.0 tag |
| WP-12 | Pacenotes adoption (`server.tool`, `Rustup` source, data artifacts for embeddings); lint against hand-built tool paths | M | WP-8 |
| WP-13 | Sighter adoption (weights pack, ORT reported, DirectML and CUDA artifacts behind their exceptions) | M | WP-5, WP-8 |
| WP-14 | Debrief and Pitwall onto `paddock-runtime` and `paddock-tools` (each app its own WP in its next line) | L each | Q7 |

Crate total (WP-1 to WP-10): about 27 to 38 agent-days; adopters WP-11 to WP-13 about 8 to 12 more.

## Consequences

- B1 to B6 become tested properties of one crate instead of fixes in one app; Pacenotes and Sighter start on the mechanism
  instead of writing the fourth and fifth copies.
- `paddock-runtime` finally gets users; its store stops being copied.
- The helper's trust boundary is shared code with shared tests, which raises the stakes of a bug in it: WP-6 gets an
  independent security review before Dyno adopts it.
- Dyno pays a migration (WP-11) and a helper protocol bump after 1.1.0, for no user-visible feature.
- One more crate in the publish and coverage gates.

## Rejected

- **Copy Dyno's code into Pacenotes and Sighter**: rule 42 and rule 55; the bugs of 2026-11-04 would recur per copy.
- **A tool manager per platform crate (`paddock-tools-windows`, ...)**: more publish and gate overhead for adapters of a few
  hundred lines each (D1).
- **Tool knowledge as Rust code per tool** (a `trait Tool` implemented for smartmontools, rust-analyzer, ...): needs a
  release for every new tool or path and cannot take user corrections; packs are rule 21.
- **Runtime OpenPGP verification in the app**: Dyno decided against carrying OpenPGP code; the pin (size plus SHA-256) is
  the gate and the signature is evidence checked when pinning. Revisit only if pins are ever fetched at run time (they are not).
- **Letting user packs declare downloads or privileged installs**: a dropped file would then choose what root runs.

## Open questions for the user

- **Q1. Crate shape.** One crate `paddock-tools` on top of `paddock-runtime` (recommended, D1), or fold everything into
  `paddock-runtime` and rename it?
- **Q2. Upstream update checks.** Pin-driven only (recommended, D10: no network, an update needs a new pin anyway), or also
  a per-tool "a newer upstream release exists" check (GitHub releases API, under the network switch)?
- **Q3. Shared download cache across apps.** Per app in v1 with a content-addressed layout (recommended, D8), or a family
  blob store now (saves about 1 GB per shared model, needs a reference and sweep scheme)?
- **Q4. Package-manager queries in locate.** May locate run `brew --prefix <formula>`, `dpkg -S`/`rpm -qf` (a process each,
  slower, but finds non-standard prefixes), or only read folders, registry and `PATH` (recommended for v1, with the Settings
  override for the rest)?
- **Q5. Privileged artifact size.** Will any privileged install be larger than a few MB (a driver, a runtime)? If not, keep
  Dyno's bytes-over-IPC as the only transfer and drop the staged-file handle from v1.
- **Q6. Authenticode at run time.** Should the Windows helper also check the installer's Authenticode signature
  (`WinVerifyTrust`, expected subject from the pin) before running it, as defence in depth beyond the SHA-256 pin?
- **Q7. Delete-by tags** for the registered copies: Dyno (its copy after 1.1.0: suggest `dyno-v1.2.0`), Debrief
  (FFmpeg, llama.cpp, Chromium stores, `detect.rs`: suggest the release after 1.2), Pitwall (engines: suggest after 1.1).
- **Q8. Name.** Keep `paddock-tools`, or a name that does not collide with MCP tools and `tools/` (for example
  `paddock-external` or `paddock-toolbox`)?
