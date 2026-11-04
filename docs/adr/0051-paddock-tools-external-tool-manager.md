# 51. `paddock-tools`: one manager for the external tools, runtimes and model files the apps find, download and install

- Status: Proposed, revision 3 (2026-11-04). The user accepted Q1 to Q4 and Q6 to Q8 of revision 1 as recommended
  (2026-11-04, "User decisions" below). Revision 2 answered the independent review of revision 1 (`838aad1ea`: 4 P1,
  11 P2, 4 P3); revision 3 answers the re-review of revision 2 (`ae487ceac`: N1 to N5 and five P3). Open: Q5, Q9, Q10 (listed
  together at the end). Docs only: no crate exists yet.
- Date: 2026-11-04
- Related: ER-17 to ER-20 (long operations, progress, no UI timeouts, parallelism and cost), ER-21 / rule 40 (recurring bugs
  mean a missing design rule), ER-22 / rules 39, 42, 43 (share first, shared behaviour tested once), ER-27 (logs explain
  every failure), rule 21 (tunables are Settings, knowledge lists are TOML packs), rule 52 (no starvable QoS for waited
  work), rule 53 and ADR 0033 (never act on a reusable OS name), rule 55 and ADR 0034 (one implementation per mechanism),
  ADR 0028 (`paddock-about` and `paddock-deps`), ADR 0030 ("Every unexplained browser abort names the causes it ruled out, in one driver"), Dyno ADR 0027 §7 and its amendments,
  Pacenotes README tier 3 and `pacenotes-core/src/pack.rs`, Sighter stage-2 plan SW9, SW11, SW36 and ADR 0001. Memory
  notes "avoid hard coding", "licence exceptions", "network on by default", "local test VMs", "estimate after design".
- Number: 0051. `origin/main` (`72a5bfd1b`) holds up to 0044 and 0049; 0045 to 0048 and 0050 exist on other branches
  (`git ls-tree --name-only <ref> docs/adr/` over all remote branches, 2026-11-04); no branch holds 0051.

## User decisions (2026-11-04)

| # | Question (revision 1) | Decision |
| --- | --- | --- |
| U1 | Q1 crate shape | A new crate `paddock-tools` on top of `paddock-runtime` (D1) |
| U2 | Q2 update checks | Against the pins of the build only; no upstream request (D10) |
| U3 | Q3 download cache | Per app, content-addressed (D8); no family blob store |
| U4 | Q4 package-manager queries in locate | None: locate only reads folders, the registry and `PATH` (D3) |
| U5 | Q6 Authenticode | Checked too: the Windows helper verifies the installer's Authenticode signature against the pin before running it (D5, D9) |
| U6 | Q8 name | `paddock-tools` |
| U7 | Q7 delete-by tags | As proposed: Dyno's copies `dyno-v1.2.0`, Debrief's the release after 1.2 (`debrief-v1.3.0`), Pitwall's the release after 1.1 (`pitwall-v1.2.0`) (WP-0). Pitcrew, Pacenotes and Debrief's browser runtime were not covered by that question: Q10 |

## Context

### Three apps need the same thing, and five already hold copies of parts of it

Read in git on 2026-11-04 at the refs named.

| App | What it finds, downloads or installs | Where (ref) |
| --- | --- | --- |
| Dyno 1.1.0 | smartmontools 7.5: pins in `dyno-agent/tools/tool-pins.toml` (agent) **and** compiled into `dyno-platform/src/tool_install.rs` `PINS` (helper; "the agent's table is never trusted by root"); download job `dyno-agent/src/tool_download.rs` over `paddock-download` and `paddock-net` `Egress`; Windows install by the privileged helper (NSIS `/S /SO x64,smartctl,drivedb /D=`, `tool_install.rs`, `dyno-helper/src/tools.rs`); macOS own copy from the DMG (`dyno-agent/src/tool_own_copy.rs`); Linux through PackageKit (ADR 0027 §7); the offer card (`dyno-core/src/tool_offer.rs`); user-tool search (`dyno-core/src/toolpath.rs`, `dyno-platform/src/toolpath.rs`); pin evidence `dyno-helper/tools/smartmontools-7.5.pin.md`, `scripts/verify-tool-pin.sh` | `origin/steward/dyno-1.1.0` `1589c46bb` |
| Dyno, the 2026-11-04 fix | Helper-first SMART reads (`dyno-agent/src/helper_wear.rs`), the Windows search for a user's `smartctl` (`dyno-platform/src/smartctl_search.rs`: Program Files from environment variables, the helper's folder, the registry's `InstallLocation` in both views through `reg.exe`, `PATH`), one install-folder definition | `origin/fix/dyno-windows-smart` `4377d9362` (not on `steward/dyno-1.1.0` yet) |
| Pacenotes | Language servers: a declarative `ServerPack` with `detect` (a program on `PATH`, a rustup component), `install_offer` (a command shown, never run without a click) and a pinned `download` (https plus SHA-256); servers start only for a trusted repository. Embedding models as hashed files (ADR 0001). | `origin/steward/pacenotes-stage1` `c1bba3f9f`: `apps/pacenotes/README.md` lines 32-34, `crates/pacenotes-core/src/pack.rs`, `docs/adr/0001-embeddings.md` |
| Sighter | RF-DETR weights (about 1.29 GB, four single files, Gitea package `sighter-weights` 1.11.2, `apps/sighter/catalogue/model-weights.toml`, "the parser for single-file entries is the shared asset kind of SW11"); its ONNX Runtime build as a `paddock-runtime` catalogue (`scripts/ort/catalogue/onnxruntime.toml`); DirectML (build-time fetch; ships only if S2/P6 passes); the opt-in CUDA training runtime; SW36 training runtime | `origin/sighter/sw9-weights` `ef7bcc8d8`; `origin/docs/sighter-stage-2` `apps/sighter/docs/plans/stage-2.md` |
| Debrief | llama.cpp builds (`debrief-ai/src/runtimes/store.rs`), GGUF models (`debrief-ai/src/models/`), FFmpeg (`debrief-video/src/runtime/store.rs`; the user's own ffmpeg by `runtime/detect.rs`), Chrome for Testing (`app/src-tauri/src/browser/runtime.rs`; the driver copy `crates/chromium-driver/src/{discover,install}.rs`), a custom `llama-server` path | `origin/main` |
| Pitwall | llama.cpp installs "adapted from Debrief's runtime manager" (`pitwall-server/src/engines.rs`), GGUF downloads (`pitwall-server/src/library.rs`), the login-shell `PATH` (`pitwall-server/src/service/login_path.rs`) | `origin/main` |
| Spotter | Chrome for Testing: `spotter-render/src/{discover,install,manifest}.rs` and `spotter-render/src/chromium-manifest.toml` | `origin/main` |
| Pitcrew | The login-shell `PATH` for its runners (`pitcrew-service/src/login_env.rs`, `pitcrew-core/src/runner_env.rs`) | `origin/main` |
| Shared | `paddock-runtime` (catalogue, versioned store, unpack, self-test, rollback, quarantine), used today only by `paddock-chromium` (its Chrome for Testing runtime, `crates/paddock-chromium/Cargo.toml`), which no app depends on yet; `paddock-download`, `paddock-net`, `paddock-archive`, `paddock-packs`; `paddock-chromium/src/discover.rs` | `origin/main` |

Three mechanisms exist several times:

1. **The pinned own-copy store** (download, verify, unpack, self-test, versioned folder, rollback): canonical
   `paddock-runtime`; copies in Debrief (FFmpeg, llama.cpp), Pitwall (engines) and Dyno (own copy); the Chromium installs of
   Spotter and Debrief.
2. **Finding the user's own install**: Dyno (`toolpath`, `smartctl_search`), Debrief (`detect.rs`), the Chromium discovery of
   `paddock-chromium`, Spotter and Debrief, Pacenotes (`Detect`).
3. **The login-shell `PATH`** (a service started by launchd or systemd gets a minimal `PATH`): Dyno `toolpath.rs`, Pitwall
   `login_path.rs`, Pitcrew `login_env.rs` and `runner_env.rs`. They differ: Pitcrew runs `$SHELL -l -i -c` (interactive),
   Dyno runs `-l -c` and says why not `-i` (a prompt framework can hang it), and each has its own fall-back folder list.

Only Debrief's model downloader (`model-download`) and the browser driver (`browser-driver`, ADR 0030) are in
`docs/known-copies.toml`; the rest are unregistered (rule 55 (c)). Dyno ADR 0027 §7 said "a `paddock-runtime` catalogue
entry per OS"; the code built its own `tool-pins.toml`.

### The Dyno bugs of 2026-11-04 are one pattern (ER-21)

Found by the user testing Dyno 1.1.0 on Windows; fixed in `4377d9362`, whose amendment to Dyno ADR 0027 is the source:

| # | Bug | What was missing |
| --- | --- | --- |
| B1 | The helper installed `smartctl`, but nothing read SMART through it (`HelperLink::smart` had no caller) | a consumer told **how** to use a tool (directly or through the helper), not just where it is |
| B2 | The helper's `Info` listed no tools, so Settings said "Not installed" after a successful install | one state report derived from the layout the installer writes |
| B3 | Discovery missed the user's own install (`%ProgramFiles%`, the installer's `InstallLocation` under `WOW6432Node`, `PATH`) | a locate step with per-platform sources, each reported |
| B4 | No re-read after the install | a confirm phase at the end of every install, and a change event |
| B5 | Errors were silent | typed outcomes with a next step, phase logs (ER-27) |
| B6 | The install folder was defined in two places | one path definition used by app, agent and helper |

### A trust gap the review found (revision 2)

The Dyno helper runs `smartctl` as SYSTEM from `<helper data dir>\tools` (`dyno-helper/src/serve.rs:117` at `4377d9362`),
under `%ProgramData%\Dyno\Helper` (`dyno-core/src/helper_paths.rs`). The reviewer reports that this folder inherits the
`%ProgramData%` ACL, which lets ordinary users create files and folders. A search of `apps/dyno` at `4377d9362` finds no
code that sets a DACL on the helper's folders (`icacls` is only named in comments). **Not yet verified on a Windows VM**;
if confirmed it is a local privilege escalation in Dyno 1.1.0 (a user plants or swaps `smartctl.exe` before the helper
runs it) and must be fixed in Dyno before or with 1.1.0, independently of this ADR ("Dyno now" below).

## Decision

### D1. One crate, `paddock-tools`, on the existing crates; core plus `cfg`-gated adapters (U1)

`paddock-tools` owns the **lifecycle of an external thing an app uses but does not ship**: a program (smartctl, a language
server, llama-server, ffmpeg), a runtime (ONNX Runtime, a training runtime) or a data file (model weights).

| Concern | Owner |
| --- | --- |
| Tool packs (parse, limits, built-in plus user, reload) | `paddock-packs` (a new `PackKind`) |
| HTTP policy: https, host allow-list, reach guard, offline switch | `paddock-net` (`Policy`, `Egress`) |
| Bytes on disk: ranges, resume, hash while writing, free space, per-host caps | `paddock-download` |
| The own-copy store: versioned folders, staging, self-test, rollback, quarantine, cross-process lock, **its downloads** | `paddock-runtime` (extended, D5a) |
| Unpacking archives | `paddock-archive` |
| Jobs, phases, Cancel, clock | `paddock-jobs` |
| Running a probe or an installer, by identity; kills | `paddock-proc` (rule 53) |
| The login-shell `PATH` (new canonical home, D3) | `paddock-proc` (`login_env` module) |
| SPDX expressions, licence policy, notices, the "downloaded on request" class | `paddock-deps` / `paddock-about` (ADR 0028, D9). `paddock-deps` is not on `main` yet (`crates/` at `72a5bfd1b`): until it lands `paddock-tools` carries a local `Spdx` newtype (syntax-checked expression text), which WP-12 replaces with `paddock-deps`' type |
| **Locate, trust, probe, report, resolve, plan install and uninstall, the privileged port and its helper side, the layout, update checks** | **`paddock-tools`** |

```text
crates/paddock-tools/
  src/pack.rs        ToolPack: the PackKind, typed and validated; the user-pack merge (D2)
  src/layout.rs      ToolLayout: every path the crate uses (D7)
  src/locate/        Source, the search plan (pure), the Locator over ports; native/{windows,macos,linux}.rs
  src/trust.rs       protected or not, per OS (D4); native per OS
  src/probe.rs       the version probe, bounded, cached by file identity
  src/state.rs       ToolState, ToolReport, Route and resolve (D6); generated TypeScript
  src/install/       the plan (pure), the own-copy route over paddock-runtime, the package-manager route, the privileged port
  src/privileged.rs  feature `privileged`: the helper's side (verified install, protected roots, held-image run)
  src/update.rs      pin-driven update check
  src/tauri.rs       feature `tauri`
  src/testing.rs     feature `testing`: fakes and the contract tests apps run (rule 43)
```

Features: `fetch` (default: `paddock-net`, `paddock-download` with `http`), `privileged` (no network or Tauri code, so a
helper binary stays small), `tauri`, `testing`. Native glue unreachable by tests sits in one `native.rs` per platform, free
of logic, in the coverage ignore regex. Separate adapter crates were rejected (publish, hakari and coverage overhead for
adapters of a few hundred lines; the ports already make them replaceable). `paddock-runtime` is not grown into the manager:
its one job is the store; locating, trust, privileged installs and the report are different reasons to change.

### D2. Per-tool knowledge is data: the `tool` pack kind

A tool pack is a TOML file read through `paddock-packs` (rule 21). Data: identity, licence, uses and what each needs, the
version probe, locate hints, pinned platform builds with evidence, system packages, install recipes chosen from a closed
set. Code: the closed enums (source kinds, folder tokens, registry hives, recipes, evidence kinds, needs). A new kind is a
new variant with its adapter and tests; a new tool is a new pack.

**Field names follow `paddock-runtime`'s catalogue** (`crates/paddock-runtime/src/catalogue.rs`: top level `format`,
`name`, `version`, `source`, `license`; `[[platform]]` with `key`, `url`, `size`, `sha256`; the file name is the URL's last
part). A pack is a catalogue plus extra tables, so the mapping is mechanical:

| Tool pack | `paddock-runtime` catalogue |
| --- | --- |
| `name` (folder-name safe, the tool id) | `name` |
| `version` (one per pack, as in the catalogue) | `version` |
| `source`, `license` | `source`, `license` |
| `[[platform]]` `key`, `url`, `size`, `sha256` | `[[platform]]` `key`, `url`, `size`, `sha256` |
| `[[platform]]` `hosts`, `install`, `layout`, `evidence`, `meta` | not in the catalogue: `paddock-tools` keeps them; the conversion drops them |
| `[[system_package]]`, `[[use]]`, `[probe]`, `[locate]`, `display_name`, `use_as`, `source_url`, `programs` | not in the catalogue |

Smartmontools as the worked example (values from Dyno's `tool-pins.toml`; the `take` paths are illustrative until WP-13
copies them from `tool_own_copy.rs`):

```toml
format = 1
name = "smartmontools"                      # the tool id; unique across the app's packs
display_name = "smartmontools"
version = "7.5"
source = "smartmontools RELEASE_7_5 (GitHub releases)"
license = "GPL-2.0-or-later"                # an SPDX expression, parsed by paddock-deps (D9)
use_as = "separate-program"                 # separate-program | library | data
source_url = "https://www.smartmontools.org"
programs = ["smartctl"]                     # the first is the main program

[[use]]                                     # what each use of the tool needs (D6)
id = "smart-read"
needs = "admin-preferred"                   # user | admin | admin-preferred
platforms = ["windows", "linux"]
[[use]]
id = "smart-read"
needs = "user"
platforms = ["macos"]

[probe]
args = ["--version"]
version = '^smartctl (\d+\.\d+)'            # paddock-packs user_regex limits
timeout_ms = 5000
min_version = "7.0"

[locate]                                    # hints only; folder tokens are a closed set (D3)
windows.dirs = ["{ProgramFiles}/smartmontools/bin", "{ProgramFilesX86}/smartmontools/bin"]
windows.registry = [
  { hive = "local-machine", key = 'SOFTWARE\smartmontools', value = "InstallLocation", subdir = "bin" },
  { hive = "local-machine", key = 'SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\smartmontools', value = "InstallLocation", subdir = "bin" },
]                                           # both views are always read; a key naming WOW6432Node is refused
macos.dirs = ["/opt/homebrew/sbin", "/usr/local/sbin"]
linux.dirs = ["/usr/sbin", "/usr/bin"]

[[platform]]                                # a pinned, downloadable build (a paddock-runtime entry plus extras)
key = "windows-x86_64"
url = "https://github.com/smartmontools/smartmontools/releases/download/RELEASE_7_5/smartmontools-7.5.win32-setup.exe"
size = 1508408
sha256 = "896337fcc253220614cf8cdbd5cf2321c5aa326a37a04160a672a281e6104c70"
hosts = ["github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com"]
install = { recipe = "nsis", privileged = true, args = ["/S", "/SO", "x64,smartctl,drivedb"], dest_arg = "/D=", timeout_s = 120 }
layout = { executable = "bin/smartctl.exe" }
evidence = [
  { kind = "openpgp", fingerprint = "0C9577FD2C4CFCB4B9A599640A30812EFF3AEFF5", verified = "2026-11-03", record = "smartmontools-7.5.pin.md" },
  # The helper matches the signer and root certificates by SHA-256 thumbprint; `subject` is for people only.
  # The thumbprints are written by `./dev tools pin` (WP-10); the placeholders below are not real values.
  { kind = "authenticode", signer_sha256 = "<leaf thumbprint>", root_sha256 = "<root thumbprint>", timestamp = "required", subject = "CN=SignPath Foundation", verified = "2026-11-03" },
]

[[platform]]
key = "macos-aarch64"                       # macos-x86_64 as a second entry if the DMG is universal (WP-13 checks)
url = "https://github.com/smartmontools/smartmontools/releases/download/RELEASE_7_5/smartmontools-7.5.dmg"
size = 1619514
sha256 = "4aa176c161612488f3db053fdf3eee47bbc5178c8fe46afb2002d8dbdf563cc5"
hosts = ["github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com"]
install = { recipe = "dmg-pkg", privileged = false, take = ["usr/local/sbin/smartctl", "usr/local/share/smartmontools/drivedb.h"], timeout_s = 120 }
layout = { executable = "smartctl" }
evidence = [{ kind = "openpgp", fingerprint = "0C9577FD2C4CFCB4B9A599640A30812EFF3AEFF5", verified = "2026-11-03", record = "smartmontools-7.5.pin.md" }]

[[system_package]]                          # not a download: installed by the system's package manager
os = "linux"
packagekit = "smartmontools"                # PackageKit InstallPackages; without PackageKit the command is shown
```

`[[platform]]` (a pinned download) and `[[system_package]]` (an install the system does) are separate variants: a
`[[platform]]` without `url`, `size`, `sha256`, `hosts` and at least one `evidence` entry is refused, and a system package
has none of those fields.

Validation (each a failing test first):

- `url` is https and its host is in `hosts`; `hosts` becomes the download's `paddock-net` `Policy` allow-list and `size` its
  body cap. `key` is a `paddock-runtime` platform key (`<windows|macos|linux>-<x86_64|aarch64>`).
- **Unknown keys inside `[[platform]]`, `install`, `layout`, `evidence`, `[[use]]` and `[[system_package]]` refuse the
  pack** (their serde types deny unknown fields; `paddock-packs` turns that into a refusal of the whole pack). Unknown
  top-level keys stay `paddock-packs` warnings. A test loads every built-in pack of every app and asserts **zero warnings**.
- `use_as = "library"` with a copyleft licence, and any licence `paddock-deps` refuses (also for `use_as = "data"`: weights
  carry licences too), are refused unless `paddock-deps` lists a scoped exception (D9).
- Sighter's extra weight fields (`upstream_url`, `upstream_md5`, `lineage`, `resolution`) go in a `meta` table per platform
  entry, carried through and never judged.
- `use_as`, `license` and `programs` are **required** with no default (a missing key refuses the pack); `programs` may be
  empty only with `use_as = "data"`.
- Two `[[use]]` entries with the same `id` whose `platforms` overlap refuse the pack (one platform, one `needs` per use).

**User packs and built-in packs merge append-only** (`paddock_packs::Origin`):

- A user pack may **add** `[locate]` hints (folders and registry values) to a built-in tool of the same `name`; they are
  searched after the built-in hints. It may declare a **find-only** tool of its own (`[probe]`, `[locate]`, `[[use]]` with
  `needs = "user"` only).
- A user pack may **not** set or override: `version`, `license`, `use_as`, `programs`, `[probe]` of a built-in tool,
  `[[use]]` of a built-in tool, any `[[platform]]`, `install`, `layout`, `evidence` or `[[system_package]]`. Such a key
  refuses that user pack with a message naming the key; the built-in pack is untouched.
- Registry hints in user packs are read by the app (unprivileged) only, never by a helper.
- **Pacenotes consequence:** a user language pack can no longer carry a pinned `download` (today `ServerPack::download`
  allows it); downloads of language servers come only from Pacenotes' built-in tool packs. Q9 asks the user.

**Avoid hard coding (memory note):** the sources that need no list come first (the user's Settings path, the registry's own
`InstallLocation`, the login-shell `PATH`); a pack's folder hints cover measured gaps only, and the user corrects a miss in
Settings without a release.

### D3. Locate: Paddock's copy first, then the user's install, every source reported; reading only (U4)

`Locator::locate(tool)` runs a **search plan** (pure, from the pack, `ToolLayout`, the platform and Settings) over ports:

1. **Settings override** (`tools.<name>.path`), validated as a plain absolute path. If it fails (missing, not executable,
   probe failed, too old), the result is `Unusable { why: OverrideFailed(..) }`, shown as "your chosen path failed: <why>";
   there is **no silent fall-through** to another copy. The report lists what else was found so the user can clear the
   override.
2. **Paddock's own copy** (`ToolLayout::own`), then the helper's protected copy (`ToolLayout::privileged`, read through the
   port's `held()`).
3. **The user's install:**
   - Windows: the pack's folder hints with **folder tokens** (a closed enum: `{ProgramFiles}`, `{ProgramFilesX86}`,
     `{ProgramData}`, and for the app only `{LocalAppData}`, `{Home}`) resolved with `SHGetKnownFolderPath`
     (`FOLDERID_ProgramFiles`, `FOLDERID_ProgramFilesX86`, ...), never from environment variables; the registry values the
     pack names, in **both views** (`KEY_WOW64_64KEY`, `KEY_WOW64_32KEY`; 32-bit installers such as smartmontools' NSIS
     write under `WOW6432Node`). The hive is a closed enum (`LocalMachine`, `CurrentUser`); the app also reads the
     `CurrentUser` uninstall key of the tool (per-user installs); the helper reads `LocalMachine` only; a pack key that
     contains `WOW6432Node` is refused (views are chosen by the flag, not the path). Then the `PATH` folders.
   - macOS: the pack's folders (Homebrew's `/opt/homebrew`, `/usr/local`), then the login-shell `PATH` and the process `PATH`.
   - Linux: the pack's distro folders, `/usr/local`, `/snap/bin`, then the login-shell and process `PATH`.
   - No package-manager queries (`brew --prefix`, `dpkg -S`, `rpm -qf`) run (U4).
4. Each existing candidate is classified for trust (D4) and probed (D6) in the order the context allows; the first that
   passes wins.

Every attempt is an `Attempt { source: Source, path, trust, outcome }` (`NotThere`, `NotExecutable`, `NotProtected`,
`ProbeFailed(why)`, `TooOld(version)`, `Ok(version)`) in the report. Sources are a closed enum: `Override`, `OwnCopy`,
`PrivilegedCopy`, `KnownDir(token)`, `Registry { hive, view }`, `LoginPath`, `ProcessPath`. Ports: `Fs`, `Registry`,
`KnownFolders`, `Env`, `LoginShell`, `Runner`. The Windows registry adapter uses the `windows-registry` crate of windows-rs,
already in five app lockfiles (Debrief, Dyno, Pitcrew, Pitwall, Spotter), instead of spawning `reg.exe` as `4377d9362` does.

**The login-shell `PATH` gets one canonical home** in `paddock-proc` (`login_env`: one bounded, timed-out, cached ask of
the login shell, the marker parsing and the allow-listed variables of Dyno's `toolpath.rs`, the fall-back list). WP-14
builds it from the three copies, deciding `-i` (Pitcrew) against no `-i` (Dyno) with a test per shell; `paddock-tools`
uses it, and Pitcrew, Pitwall and Dyno adopt it in their next lines (`login-shell-path` in the register, WP-0).

### D4. Trust in a privileged context

A privileged process (Dyno's helper: SYSTEM on Windows, root elsewhere) uses `paddock_tools::privileged` and follows these
rules; the unprivileged app classifies trust only to report it.

1. **Trust before probe.** In a privileged context a candidate is checked for trust **before** it is probed: a probe runs
   the file as SYSTEM or root. A candidate that is not protected is reported `NotProtected` and never run.
2. **The helper never accepts a path from the caller, from Settings or from a user pack.** It runs only what it found
   through `ToolLayout::privileged` or the built-in pack's hints, and only when protected.
3. **Protected**, per OS (implemented in `trust.rs`, each rule with a test over a fake security descriptor or stat, and a
   real-OS test in the VMs):
   - **Windows.** "Trusted SIDs" are SYSTEM, Administrators and TrustedInstaller; a `CREATOR OWNER` entry counts as
     granting its rights to any user who can create there. The rule differs by position, because stock ancestors let users
     add entries (the reviewer reports `C:\` granting Authenticated Users "create folders" and `C:\ProgramData` granting
     Users write data, append data and write extended attributes), and adding a sibling never changes what a path
     resolves to:
     - **Every ancestor** (from the volume root down to the containing folder's parent): the owner is a trusted SID, and no
       untrusted SID has `DELETE` on that component, `FILE_DELETE_CHILD` on its parent, or `WRITE_DAC` / `WRITE_OWNER` on
       it (any of these lets a user rename or re-permission the component and swap the path below it).
     - **The containing folder and the file itself:** all of the above, and also no untrusted SID has `FILE_ADD_FILE`,
       `FILE_ADD_SUBDIRECTORY` (folder), `FILE_WRITE_DATA` or `FILE_APPEND_DATA` (file).
     - **No component is a reparse point** (junction, symlink, mount point): checked from `FILE_ATTRIBUTE_REPARSE_POINT`
       on each component opened with `FILE_FLAG_OPEN_REPARSE_POINT`. The check walks the chain of the path returned by
       `GetFinalPathNameByHandle` on the **held** image handle (D4.5), so the checked chain is the one the handle refers to.
     - Tests (WP-3) run the rule over fake security descriptors that are the **real default SDDL** of `C:\`,
       `C:\ProgramData`, `C:\Program Files` and `C:\Program Files\<app>` on a stock Windows 11 VM (captured once with
       `Get-Acl | Select Sddl` in the local test VM, stored as fixtures; no user machine is read): `C:\Program Files\x\y.exe`
       passes, a file directly in `C:\ProgramData\Dyno\Helper\tools` with the inherited ACL fails on the containing-folder
       rule, the same folder with the protected DACL of D4.4 passes, and a junction anywhere in the chain fails.
   - **macOS:** the path is resolved (symlinks followed to the real file, each component checked); the file and every
     ancestor are owned by root, not group- or other-writable, and carry no ACL entry that allows write, append, delete or
     change-permissions to anyone but root.
   - **Linux:** the same rule after canonicalising the path, checked on each component.
4. **Protected roots.** `ToolLayout::privileged` creates the helper's tools and staging roots itself with a protected DACL
   (Windows: inheritance off; SYSTEM and Administrators full control; Users read and execute; no `CREATOR OWNER`) or root
   ownership and mode `0755` (Unix), and **verifies** the rule above before every install and every run. A root that fails
   the check is reported as `Unusable { why: HelperFolderNotProtected }` and repaired at the helper's next start, never used.
5. **No time-of-check gap (rule 53):**
   - **Windows:** the helper opens the image with `FILE_SHARE_READ` only (no write or delete sharing), keeps the handle open
     through `CreateProcess`, and hashes through that handle when a hash is needed; nobody can swap or delete the file while
     it is held.
   - **Linux:** `open(O_RDONLY | O_NOFOLLOW | O_CLOEXEC)`, check trust and hash on the descriptor, then
     `execveat(fd, "", argv, envp, AT_EMPTY_PATH)` with `argv[0]` kept as the program's name (tools that read their own
     `argv[0]`). Only for **native binaries** (the descriptor starts with the ELF magic): a `#!` script would be run by an
     interpreter that reopens it by path (and fails on a descriptor opened `O_CLOEXEC`), and a snap launcher is a symlink
     that `O_NOFOLLOW` refuses. In a privileged context such candidates are reported `Why::NotNativeExecutable` and not
     run; in the unprivileged app they are run by path as today.
   - **macOS:** there is no exec-by-descriptor; the protected folder chain (rule 3) is what prevents a swap.
   - Revision 1's "re-check `runtime.json` before a privileged run" is **dropped**: that file is in a user-writable store
     and proves nothing to root. A privileged run trusts only the protected chain and the held image.
6. **Staged-file transfer** (agent to helper): the helper opens the staged file with no-follow, requires a regular file,
   caps reading at the pin's `size`, and copies it while hashing into its protected staging (create-new). Preferred: the
   agent **passes a handle** (Windows: the helper duplicates it from the client process, identified through the pipe's
   client process and checked as in ADR 0033; Unix: `SCM_RIGHTS` over the socket), so no path is opened at all. Small
   artifacts may still travel as bytes in the frame (Dyno today). The helper never executes from the unprivileged location.

### D5. Install routes; the crate never escalates

`ToolManager::install(tool, ctx)` is one job (ER-17, ER-18) with phases `Plan`, `Download`, `Verify`, `Install`, `Confirm`
(locate and probe again; the job fails unless the result is usable for each declared `[[use]]`, B4), `Report` (event
`paddock://tools`, B1, B2).

| Route | Recipes | Who downloads | Who installs |
| --- | --- | --- | --- |
| Own copy | `archive`, `single-file`, `dmg-pkg` (macOS: `hdiutil attach -readonly -nobrowse -mountrandom`, `pkgutil --expand-full`, take the listed files, detach; a quarantine flag removed only from the taken files, logged), per-user `nsis`/`msi` with `privileged = false` | **the `paddock-runtime` store** (`RuntimeStore::install`, through `paddock-download`, into the downloads folder `ToolLayout` gives it) | the store (unpack or `prepare`, self-test, swap) |
| Package manager | `[[system_package]]`: PackageKit `InstallPackages` over D-Bus (the user authenticates through polkit); otherwise the command is shown, never run | the system | the system |
| Privileged | any `[[platform]]` with `privileged = true` | `paddock-tools` (`fetch`), into the same downloads folder | the app's helper through `PrivilegedInstaller` |

```rust
/// What the app's privileged helper offers. The crate calls it; it never elevates itself.
pub trait PrivilegedInstaller: Send + Sync {
    /// Whether the helper is there, answering, and which pack versions it carries (D6, version skew).
    fn availability(&self) -> Availability;
    /// Installs `platform` of `tool` from `staged`. The helper verifies again against ITS compiled-in pack and returns what it holds.
    fn install(&self, req: &PrivilegedRequest, staged: StagedFile) -> Result<HeldTool, PrivilegedError>;
    fn uninstall(&self, tool: &ToolName) -> Result<(), PrivilegedError>;
    /// What the helper holds, from ToolLayout::privileged (B2).
    fn held(&self) -> Result<Vec<HeldTool>, PrivilegedError>;
}
```

The helper side (`paddock_tools::privileged`, lifted from Dyno's `tool_install.rs`): verify the protected roots (D4.4),
receive the staged file (D4.6) into protected staging while hashing, refuse a wrong size or hash, **check the Authenticode
signature** of a Windows installer (U5, below), empty the destination, run the recipe with `TEMP`/`TMP` inside staging from the held image (D4.5), confirm
`layout.executable`, clear staging on every outcome, log each phase with its duration. The helper's packs are the same
files as the app's, compiled in with `include_str!` from one path; a contract test asserts both embed byte-identical packs.

**The Authenticode check (U5).** Run on the held staged copy, after the SHA-256 check passed:

- `WinVerifyTrust` with `WINTRUST_ACTION_GENERIC_VERIFY_V2`, `WTD_UI_NONE`, `fdwRevocationChecks = WTD_REVOKE_NONE` and
  `WTD_CACHE_ONLY_URL_RETRIEVAL`: the helper makes no network request (the SHA-256 pin already binds the exact bytes, so
  revocation adds only a network dependency to a SYSTEM process). `WTD_LIFETIME_SIGNING_FLAG` is not set.
- **A timestamp is required** when the evidence says `timestamp = "required"`: the signature must carry an RFC 3161 (or
  Authenticode) countersignature that falls within the signing certificate's validity, so a signature made before the
  certificate expired stays valid (SignPath certificates are short-lived). A signature without one is refused.
- **Match by thumbprint, not by name:** from the trust provider's state (`WTHelperProvDataFromStateData`, the signer's
  chain) the helper compares the SHA-256 thumbprint of the leaf certificate with `signer_sha256` and of the chain's root with
  `root_sha256`. The subject CN is logged and shown only.
- Outcomes are typed (D6): `Why::SignatureRejected(SignatureFault)` with `SignatureFault` one of `NotSigned`, `BadDigest`,
  `UntrustedRoot`, `WrongSigner`, `NoTimestamp`, `Expired`, next step `NextStep::DownloadAgain` (a second failure after a
  fresh download becomes `NextStep::UpdateApp`: the pin is wrong); and `Why::SignatureUncheckable(UncheckableCause)` (the
  trust provider failed or is unavailable, a policy error code), next step `NextStep::TryAgainLater`. Both refuse the install
  (fail closed); the log carries the `WinVerifyTrust` HRESULT.
- Evidence is written by `./dev tools pin` (WP-10), which reads the thumbprints and timestamp of the published file.

**Bounds and Cancel (ER-17 to ER-19).** The installer runs under `paddock_proc::Supervised` with the recipe's `timeout_s`
(default 120, at most 600); past it, the tree is ended through `paddock_proc::kill_tree` by identity, the destination is
emptied and the job fails with the cause. That bound is on an external program, not a UI timeout: the UI shows "Still
working" for as long as the job runs. Cancel is honoured in `Plan`, `Download` (the partial download is kept for resume),
`Verify` and `Confirm`. Two steps are **not cancellable** once started and say so in the job: the installer run (Cancel is
recorded and applied when it returns, by removing what it wrote) and the store's final rename.

### D5a. `paddock-runtime` gains what the routes need, so the store stays one implementation

- **Single-file entries** (Sighter SW11 / P3): no unpack, no executable, a self-test that re-checks the hash. Same work as
  SW11; it lands once, here.
- **A `prepare(staged, staging_dir)` hook** on `RuntimeKind` so `dmg-pkg` and per-user installers fill the staging folder;
  the store keeps its swap, rollback and quarantine.
- **`RuntimeStore::with_downloads(dir)`** so the store downloads into `ToolLayout`'s downloads folder (one layout, D8).
- Files that are not zip or tar (`.exe`, `.dmg`) are accepted only with a `prepare` hook.

### D6. State, routes and `resolve`

```rust
/// How a consumer uses a tool for one declared use (closed; the structural answer to B1).
pub enum Route {
    /// Run this file yourself.
    Direct { path: PathBuf },
    /// Ask the helper; it runs its protected copy.
    ThroughHelper { tool: ToolName },
    /// Not usable now; the report says why and what to do.
    Unusable { why: Why, next_step: NextStep },
}

pub enum ToolState {
    NotFound { searched: Vec<Attempt>, offer: Option<Offer> },
    Confirmed { source: Source, path: PathBuf, version: Version, trust: Trust, alternatives: Vec<Attempt> },
    Unconfirmed { source: Source, path: PathBuf, why: Why, next_step: NextStep },
    Busy { job: JobId, phase: InstallPhase },
    Unsupported { instead: Instead },
}

pub struct ToolReport {
    pub name: ToolName, pub display_name: String, pub license: Spdx, pub source_url: Url,
    pub state: ToolState, pub routes: Vec<(UseId, Route)>, pub update: Option<UpdateOffer>,
    pub notes: Vec<Note>,
}
impl ToolReport {
    /// Derived from `state` and `routes`, never stored.
    pub fn in_use(&self) -> Option<&Source>;
}
```

- `InstallPhase`, `Why`, `NextStep`, `Instead` and `Note` are closed enums; `NextStep` holds variants such as
  `TurnOnHelper`, `UpdateHelper`, `UpdateApp`, `InstallAgain`, `DownloadAgain`, `TryAgainLater`, `ClearOverride`,
  `AllowInSecuritySettings`, `InstallWith { manager, command }`, `UninstallWith { manager, command }`; the UI words them. `Spdx` is `paddock-deps`'
  parsed SPDX expression; `Version`, `ToolName` and `UseId` are newtypes.
- `Why` includes `ProbeTimedOut`, `RefusedByOs` (Gatekeeper), `TooOld { found, needs }`, `NeedsAdmin`, `HelperOff`,
  `NotProtected`, `NotNativeExecutable`, `HelperFolderNotProtected`, `SignatureRejected(SignatureFault)`,
  `SignatureUncheckable(UncheckableCause)`, `OverrideFailed(Box<Why>)`, `NotCheckedYet`, and the version skew:
  `HelperTooOld { helper_pin, app_pin }` (the helper's compiled-in pack is older than the app's, so it refuses the app's
  bytes; next step `UpdateHelper`) and `AppTooOld { helper_pin, app_pin }` (next step `UpdateApp`). A test installs with a
  fake helper one pin behind and asserts that state, not a hash error.
- **Needs and routes:** for each `[[use]]`, `needs = "user"` resolves to `Direct`; `needs = "admin"` to `ThroughHelper`
  when the helper is available and holds a protected copy, else `Unusable { NeedsAdmin, TurnOnHelper }`;
  `needs = "admin-preferred"` (Dyno's amended O2: helper first, unelevated fallback) to `ThroughHelper` when available,
  else `Direct` with `Note::ReadingWithoutAdmin`. A Settings override applies only to the `Direct` route: when the route
  is `ThroughHelper`, the helper runs its own protected copy and ignores the override (D4.2), and the report carries
  `Note::OverrideNotUsedByHelper { use_id }` so the user sees why their chosen path is not the one reading.
- **`resolve(tool, use)` never blocks.** It answers from the cached report at once; when the cache is empty or stale it
  returns the last answer (or `Unusable { NotCheckedYet, .. }`) and starts a **refresh job** (keyed per tool,
  deduplicated, phases `Locate` and `Probe`). Consumers subscribe to `paddock://tools` (or the Rust channel) and resolve
  again on change.
- When a newer copy than the one in use was passed over (a user's 7.6 while the helper's 7.5 serves an admin use), the
  report carries `Note::NewerCopyNotUsed { source, version, why }`.
- **B1 is checked twice:** a contract test (`paddock_tools::testing`) that, after a privileged install, the consumer's read
  goes through the route `resolve` returned (a fake helper records the read); and a **lint** (WP-4) that refuses, in app
  crates outside tests, `std::process::Command::new`, `paddock_proc` spawns or path joins whose program is a literal equal
  to (or ending in) a `programs` entry of any pack the app ships. It runs as a `./dev` conformance check reading the packs.

Probes run through `paddock-proc` with a timeout and an output cap, cached by file identity (path, size, modified time,
inode on Unix). A probe is live work (a person waits for Settings): normal priority on Windows, `QOS_CLASS_UTILITY`
allowed on macOS (rule 52).

### D7. One path definition: `ToolLayout`

A pure function from `(root kind, tool name, version, platform)` to every folder and file the crate uses: the own-copy
store root and release folder, **the downloads folder** (`<app cache>/tools/downloads/sha256/<hex>[.part]`, used by both
the store and the privileged route), the helper's protected `tools/<name>-<version>` folder and its staging folder, and the
executable in each. The installer, the locator, the helper's `held()` and its readers call it; nothing else formats a tool
path (B6). App roots come from `paddock_env::AppDirs`; the helper's protected root is passed in by the helper.

### D8. Downloads and cache (U3)

- `paddock-net` `Egress` with a `Policy` from the platform entry (https, its `hosts`, public addresses, body cap = `size`,
  the offline switch), then `paddock-download` (resume, hash while writing, free space, per-host caps). Network features
  are on by default with a per-feature switch (memory note 2026-11-03); nothing is fetched without the user's click.
- Per app, content-addressed (`ToolLayout`'s downloads folder): `<hex>.part` while downloading, `<hex>` once verified,
  deleted after a successful install, kept on failure for resume, swept at start-up after 7 days. No family blob store.

### D9. Licences and evidence

- `license` is an SPDX expression parsed by `paddock-deps` and checked against its policy for every `use_as`, `data`
  included. `paddock-deps` (ADR 0028) gains a **"downloaded on request" class** (WP-12): each tool with its licence, source
  link and how it is used appears in About and the notices; copyleft is allowed only as `separate-program` (never linked or
  bundled). **Mirroring** a copyleft binary (none planned) also mirrors its corresponding source at the same version.
  Named exceptions (DirectML, the CUDA runtime) are `paddock-deps` exceptions, not pack flags.
- Evidence (`openpgp`, `authenticode`, `minisign`, `upstream-digest`, `mirror-of`) is checked when a pin is added, by
  `./dev tools pin <pack> <platform>` (the Rust successor of `verify-tool-pin.sh`; system `gpg --verify` against the
  fingerprint; `signtool verify` or `osslsigncode` for Authenticode), which writes the record file and prints the entry. A
  conformance check refuses a `[[platform]]` without its record file. At run time the gate is size plus SHA-256, and on
  Windows also the Authenticode check in the helper (U5).

### D10. Update checks are pin-driven (U2)

`update::check` compares installed copies (own, helper) with the pin of this build and offers "Update to 7.6" when the pin
is newer; for a user's own install it notes "your 7.0 is older than the 7.5 Dyno knows" without replacing it. No network.

### D11. Uninstall

Own copies: the store removes its releases under its lock. Privileged copies: the port's `uninstall`, using `ToolLayout`
and refusing anything outside the helper's tools folder. Package-manager and user installs are never removed; the report
says how (`NextStep::UninstallWith`). Afterwards `Confirm` runs again and the event fires.

### D12. Logging (ER-27)

Per job: the job id as correlation id; a line per phase with duration and outcome (`Download` with bytes and resumes,
`Verify` with the size and hash outcome, `Install` with the recipe, the Authenticode outcome, the installer's exit code and
stderr tail, `Confirm` with the winning source). Per locate, at info: one summary (`sources=7 found=2 confirmed=1 ms=14`)
and the winning **source kind and path**, with the home folder redacted (`~/...`, `%USERPROFILE%\...`); every attempt at
debug. Failures carry their cause chain and the next step. No URL query strings, no file contents.

### D13. Tauri adapter and UI

Feature `tauri`: managed `ToolManager`; commands `tools_status`, `tools_install`, `tools_uninstall`, `tools_set_path`,
`tools_recheck` (starts the refresh job); the event `paddock://tools`; a `paddock-health` entry per tool (Repair: "Check
again", or "Install" with an offer); a settings section with the per-tool path (rule 44). UI: `pd-tool-card` in
`@decryp7/paddock-ui` (rule 39), from Dyno's `tool-card.ts`: state, source in words, licence and size before the click,
the job's phases, the places searched, "your chosen path failed" and newer-copy notes.

### Parallelism and cost (ER-20, rule 51)

- **Locate**: a few dozen `stat` calls and a handful of registry reads per tool; tools in parallel on the shared executor,
  a few at a time (I/O). Target for WP-2: under 50 ms for 10 tools, recorded in `docs/benchmarks.md` (rule 48).
- **Trust checks**: one security-descriptor read per path component, cached with the probe cache by file identity.
- **Probes**: a process each; cached by identity, at most 2 at a time, run only in the refresh job.
- **Downloads**: `paddock-download`'s caps; bulk work at `QOS_CLASS_UTILITY` / nice 10 / `THREAD_PRIORITY_BELOW_NORMAL`.
- **Installs**: one at a time per tool (the store's lock, the helper's install guard).
- No new thread-count Settings: the executor's pools and `paddock-download`'s caps apply.

### Testing (rule 43, ER-14)

- **Pure core**: pack parsing (every refusal, the user-pack merge, never-panics fuzz for C36), the search plan, the trust
  rules over fake descriptors and stats, the state machine and routes, the layout, the update comparison, the install plan.
- **Fakes** (`paddock_tools::testing`): `FakeFs`, `FakeRegistry` (hives and both views), `FakeKnownFolders`, `FakeEnv`,
  `FakeLoginShell`, `FakeRunner` (probe answers, timeouts, Gatekeeper refusal), `FakeSecurity` (owners and DACL entries),
  `FakePrivileged` (records requests and reads; can refuse, fail, or carry an older pin), `paddock_net::FakeWire`,
  `FakeClock`.
- **Contract tests** each adopting app runs on its wiring: the route after a privileged install (B1, B4), `held()` lists
  what the installer wrote (B2), the helper and app embed the same packs (B6), every built-in pack loads with zero warnings.
- **Real OS**, only in the local test VMs (never CI, memory note): Windows (NSIS own copy and a test helper, both registry
  views, the protected-root DACL, a planted file in an unprotected folder refused, the held image not replaceable during the
  run), macOS (`dmg-pkg`, Gatekeeper refusal, root-owned chain), Linux (PackageKit, `execveat`). CI runs the fakes; tests
  never read the real registry, `PATH` or install folders of the machine they run on.

## Copies register (WP-0)

WP-0 adds these to `docs/known-copies.toml` (docs only; the steward runs `copies --check-register` before it lands),
delete-by tags per U7 and Q10. **An entry lands with, or after, the branch that brings its files to `main`** (a register
path must name a tracked file). On `origin/main` (`72a5bfd1b`) today: Debrief's `debrief-video/src/runtime/{store,detect}.rs`
and `debrief-ai/src/runtimes/store.rs`, Pitwall's `engines.rs` and `service/login_path.rs`, Pitcrew's `login_env.rs` and
`runner_env.rs`, Dyno's `dyno-core/src/toolpath.rs` and `dyno-platform/src/toolpath.rs`; these entries land now (WP-0a).
Not on `main`: Dyno's `tool_own_copy.rs`, `tool_download.rs`, `tool_pins.rs`, `tool_install.rs`, `dyno-helper/src/tools.rs`
(they land with the Dyno 1.1.0 batch from `steward/dyno-1.1.0`), `smartctl_search.rs` (with `fix/dyno-windows-smart`), and
Pacenotes' `pack.rs` (with the Pacenotes stage-1 landing); their lines are added in the same change that lands the files
(WP-0b, one line per landing, by the landing's steward).

| Mechanism id | Canonical | Copies (owner, delete-by) |
| --- | --- | --- |
| `runtime-store` | `paddock-runtime` (`crates/paddock-runtime/src/**`) | `apps/debrief/crates/debrief-video/src/runtime/store.rs`, `apps/debrief/crates/debrief-ai/src/runtimes/**` (debrief, `debrief-v1.3.0`); `apps/pitwall/crates/pitwall-server/src/engines.rs` (pitwall, `pitwall-v1.2.0`); `apps/dyno/crates/dyno-agent/src/{tool_own_copy,tool_download,tool_pins}.rs`, `apps/dyno/crates/dyno-platform/src/tool_install.rs`, `apps/dyno/crates/dyno-helper/src/tools.rs` (dyno, `dyno-v1.2.0`) |
| `tool-locate` | `paddock-tools` (status "planned" until WP-2 lands) | `apps/dyno/crates/dyno-platform/src/smartctl_search.rs` once `4377d9362` lands (dyno, `dyno-v1.2.0`); `apps/debrief/crates/debrief-video/src/runtime/detect.rs` (debrief, `debrief-v1.3.0`); `apps/pacenotes/crates/pacenotes-core/src/pack.rs` symbol `Detect` (pacenotes, `pacenotes-v1.0.0` proposed, Q10) |
| `login-shell-path` | `paddock-proc` `login_env` (planned, WP-14) | `apps/dyno/crates/dyno-core/src/toolpath.rs`, `apps/dyno/crates/dyno-platform/src/toolpath.rs` (dyno, `dyno-v1.2.0`); `apps/pitwall/crates/pitwall-server/src/service/login_path.rs` (pitwall, `pitwall-v1.2.0`); `apps/pitcrew/crates/pitcrew-service/src/login_env.rs`, `apps/pitcrew/crates/pitcrew-core/src/runner_env.rs` (pitcrew, `pitcrew-v1.2.0` proposed, Q10: not `pitcrew-v1.1.0`, which is in the current release order and would be refused while the copy is there) |

**Files the browser-driver mechanism already covers stay there only.** `browser-driver` (ADR 0030) registers
`apps/spotter/crates/spotter-render/src/**` and `apps/debrief/crates/chromium-driver/src/**` whole, which includes
Spotter's `discover.rs`, `install.rs`, `manifest.rs`, `chromium-manifest.toml` and Debrief's `discover.rs`, `install.rs`.
They are not listed again under `runtime-store` or `tool-locate` (one file, one mechanism, so a change has one set of copies
to follow); instead the `runtime-store` and `tool-locate` entries carry a note "Chromium discovery and install: see
`browser-driver`; its canonical `paddock-chromium` already installs through `paddock-runtime`", and the browser-driver
consolidation deletes them with the rest of those trees. Debrief's `app/src-tauri/src/browser/runtime.rs` (373 lines: the
pinned release, the download job and the store; not a few lines of hook-up) goes under `browser-driver` as a **copy**
(`kind = "copy"`), owner debrief, delete-by proposed `debrief-v1.2.0` to match that mechanism's existing Debrief copy
(`chromium-driver`), or `debrief-v1.3.0` with the other Debrief copies of this ADR (Q10).

## Migration and adoption

### Dyno now (before or with 1.1.0, independent of this ADR)

Verify the reviewer's finding on a Windows VM (an ordinary user creating a file under `%ProgramData%\Dyno\Helper\tools`).
If confirmed, the helper sets the protected DACL of D4.4 on its data and tools folders at start and refuses to run a
`smartctl.exe` from a folder that fails the check, starting from a failing regression test (rule 11). This is a Dyno fix
now, not a wait for the crate.

### Dyno (first adopter of the crate, after the 1.1.0 tag; WP-13)

| Dyno file | Goes to |
| --- | --- |
| `dyno-agent/tools/tool-pins.toml`, `dyno-agent/src/tool_pins.rs`, `PINS` in `dyno-platform/src/tool_install.rs` | `apps/dyno/tools/smartmontools.toml`, compiled into the agent and the helper from one path |
| `dyno-platform/src/tool_install.rs`, `dyno-helper/src/tools.rs` | `paddock_tools::privileged`; the helper keeps its IPC dispatch and install guard wiring |
| `dyno-agent/src/tool_download.rs` | `ToolManager::install`; `HelperLink` implements `PrivilegedInstaller` |
| `dyno-agent/src/tool_own_copy.rs` | the `dmg-pkg` recipe over `paddock-runtime` |
| `dyno-platform/src/smartctl_search.rs` and the smartctl part of `toolpath.rs` | `paddock_tools::locate` |
| `dyno-agent/src/helper_wear.rs` (helper first, local fallback) | the `Route` from `resolve(smartmontools, smart-read)` with `needs = "admin-preferred"` |
| the login-shell part of `toolpath.rs` | `paddock_proc::login_env` |
| `dyno-core/src/tool_offer.rs`; `tool-card.ts`, `tool-model.ts`, `tool.store.ts` | `ToolReport` / `Offer`; `pd-tool-card` and the generated types |
| `scripts/verify-tool-pin.sh` | `./dev tools pin` |

The helper wire changes (`InstallTool` takes a `StagedFile`, `Info` lists `HeldTool`s and pack versions): a helper protocol
bump. Per rule 43 Dyno deletes its copies and their duplicated tests and records test count and time before and after.

### Pacenotes (before its first release; no shipped copy)

- A `ServerPack` keeps the language part (globs, features, init options, trust rules) and names its server by tool name
  (`server.tool = "rust-analyzer"`); the tool pack holds locate, probe, downloads and system packages.
  `Detect::RustupComponent` becomes a source variant `Rustup { component }` (reads rustup's toolchain folders; runs nothing).
- User language packs lose pinned downloads (D2; Q9).
- Pacenotes' rule that a server starts only for a trusted repository stays in Pacenotes: the crate finds and installs, it
  never decides to run. Embedding model files are `use_as = "data"` single-file entries.

### Sighter (with SW11)

SW11 is WP-7's single-file entries. The weights catalogue becomes a pack of four data entries with the Gitea URLs and our
SHA-256s, the extra fields in `meta`. ONNX Runtime stays a `paddock-runtime` catalogue reported as `use_as = "library"`
(MIT). DirectML, if the "system DLL too old" case ships, is an own-copy single-file entry under its exception; the S2/P6 kit
stays manual. The CUDA training runtime is an upstream entry (never mirrored) under its exception, behind its opt-in.

### Debrief and Pitwall (their next lines)

Registered now (WP-0); they move onto `paddock-runtime` and `paddock-tools` by their delete-by tags. GGUF model libraries
(search, fit check, user-chosen files) stay app domain; only the download and verify of a chosen file use the crates.

## Work packages

Sizes in agent-days (S ≤ 1, M 2-3, L 4-8); firm once the review closes and Q5, Q9, Q10 are answered. WP-0a now (docs); the Dyno
trust fix now (above, outside these WPs); WP-1 to WP-12 and WP-14 after the current releases; WP-13 in Dyno's next line.

| WP | Content | Days | Depends on |
| --- | --- | --- | --- |
| WP-0a | Register the entries whose files are on `main` and the browser-driver notes (table above) | 1 | Q10 |
| WP-0b | One register line per landing for the files not yet on `main` (Dyno 1.1.0 batch, `fix/dyno-windows-smart`, Pacenotes stage 1), in the landing change | - (part of each landing) | those landings |
| WP-1 | Crate scaffold (new-crate skill); `ToolPack` with all refusals, the user-pack merge, the zero-warnings test; `ToolLayout`; the local `Spdx` newtype until `paddock-deps` lands | 3 | - (`paddock-deps` optional: if ADR 0028's crate WPs have landed, use its `Spdx` instead of the local one) |
| WP-2 | Locate: plan, sources, folder tokens through `SHGetKnownFolderPath`, registry hives and views (`windows-registry`), HKCU uninstall keys, macOS and Linux adapters, the override and its failure state, attempts; locate benchmark | 5-6 | WP-1, WP-14 |
| WP-3 | Trust: the protected rule per OS (owner and DACL walk on Windows, owner, mode and ACL on macOS, canonical walk on Linux), protected root creation and verification, held-image run (share mode, `execveat`), staged-file and handle transfer | 6-8 | WP-1 |
| WP-4 | Probe, `ToolState`, `Route`, `resolve` from cache with the refresh job, change events, notes, generated TypeScript (`license` as the local `Spdx` or `paddock-deps`' type, whichever WP-1 used); **the lint** against hand-run pack programs (conformance check) | 4-5 | WP-2, WP-3 |
| WP-5 | Download and verify into `ToolLayout`'s downloads folder, `Policy` from the entry, sweep | 1-2 | WP-1 |
| WP-6 | Privileged port and helper side: verified install, Authenticode (`WinVerifyTrust` and subject), installer bounds and kill through `paddock_proc`, version-skew states and test | 4-6 | WP-3, WP-5 |
| WP-7 | `paddock-runtime`: single-file entries (= SW11), `prepare` hook, `with_downloads`; recipes `archive`, `single-file`, `dmg-pkg`, per-user installers | 5-7 | WP-5 |
| WP-8 | Package-manager route (PackageKit, shown commands), uninstall, pin-driven update check | 2-3 | WP-4 |
| WP-9 | Tauri adapter (commands, event, health entry, settings section) and `pd-tool-card` | 3-4 | WP-4, WP-8 |
| WP-10 | `./dev tools pin` and the "every platform entry has a record" conformance check | 2-3 | WP-1 |
| WP-11 | Real-OS VM scenarios on Windows, macOS, Linux (Testing above) | 3-4 | WP-3, WP-6, WP-7, WP-8 |
| WP-12 | `paddock-deps`: the "downloaded on request" class, licence checks for every `use_as`, the mirrored-source rule, About and notices; replace the local `Spdx` with `paddock-deps`' type | 2-3 | WP-1, **the `paddock-deps` crate** (ADR 0028's work packages; not on `main` yet) |
| WP-13 | Dyno migration (table above), helper protocol bump, deletions, test counts | 5-7 | WP-1 to WP-12, Dyno 1.1.0 tag |
| WP-14 | `paddock_proc::login_env` from the three copies (shell flags decided by tests per shell) | 2-3 | - |
| WP-15 | Pacenotes adoption (`server.tool`, `Rustup` source, data entries, user packs without downloads) | 2-3 | WP-9, Q9 |
| WP-16 | Sighter adoption (weights pack, ORT reported, DirectML and CUDA entries behind their exceptions) | 2-3 | WP-7, WP-9 |

Sums: the crate and its shared parts, WP-1 to WP-12 plus WP-14: 3 + 5 + 6 + 4 + 1 + 4 + 5 + 2 + 3 + 2 + 3 + 2 + 2 = **42**
to 3 + 6 + 8 + 5 + 2 + 6 + 7 + 3 + 4 + 3 + 4 + 3 + 3 = **57** agent-days (the reviewer estimated 30 to 45 for the crate
alone; WP-12 and WP-14 add 4 to 6). Adopters WP-13, WP-15, WP-16: 5 + 2 + 2 = **9** to 7 + 3 + 3 = **13**. WP-0a: 1. The
Debrief and Pitwall moves are sized in their own plans.

## Consequences

- B1 to B6 become tested properties of one crate (the route enum, the contract tests and the lint for B1); Pacenotes and
  Sighter start on the mechanism instead of writing more copies.
- The privileged trust boundary becomes shared, specified per OS and tested on real systems; WP-3 and WP-6 get an
  independent security review before Dyno adopts them.
- The reviewer's ACL finding may need a Dyno fix now, before the crate exists.
- `paddock-runtime` gets app users through `paddock-tools` (today only `paddock-chromium` uses it, and no app uses that).
- Dyno pays a migration and a helper protocol bump after 1.1.0; Pacenotes' user packs lose downloads (Q9).
- One more crate in the publish and coverage gates.

## Rejected

- **Copy Dyno's code into Pacenotes and Sighter**: rules 42 and 55.
- **Adapter crates per platform**: overhead without substitution benefit (D1).
- **Tool knowledge as Rust code per tool**: a release per tool or path, no user corrections (rule 21).
- **Runtime OpenPGP verification**: the pin is the gate; the signature evidence is checked when pinning.
- **User packs declaring downloads, installs, probes or uses of built-in tools**: a dropped file would choose what runs.
- **Package-manager queries during locate** (U4).
- **Re-checking a user-writable `runtime.json` before a privileged run** (revision 1): it proves nothing to root (D4.5).

## Open questions for the user

All three open questions, together (Q1 to Q4 and Q6 to Q8 are decided: U1 to U7).

- **Q5. Privileged artifact size.** Will any privileged install be larger than a few MB (a driver, a runtime)? If not, the
  bytes-in-frame transfer stays the default and the handle transfer (D4.6) is built only when needed.
- **Q9. User language packs and downloads (Pacenotes).** Under D2 a user pack may add locate hints but not downloads, so a
  user language pack can no longer offer a pinned server download (Pacenotes' `ServerPack::download` allows it today).
  Accept (recommended: downloads only from built-in packs), or allow user-pack downloads that run only unprivileged, after a
  confirmation naming the URL and hash?
- **Q10. Delete-by tags not covered by U7.** Proposed: Pitcrew's `login-shell-path` copies `pitcrew-v1.2.0` (its next
  minor, 1.1.0, is in the current release order and its tag would be refused while the copies exist); Pacenotes' `Detect`
  `pacenotes-v1.0.0` (its first release; it has no shipped copy, so adoption before 1.0 is planned in WP-15); Debrief's
  `browser/runtime.rs` under `browser-driver` either `debrief-v1.2.0` (matching that mechanism's existing Debrief copy,
  recommended, since the browser-driver consolidation deletes them together) or `debrief-v1.3.0` (matching U7's Debrief tag).
