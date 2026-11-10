# Amendment to ADR 0073 (2026-11-11): Elevated Debrief: a risk warning, the user decides

- Status: **Proposed 2026-11-11, revision 6.**
  - **The user's decision (2026-11-11, final):** "if the user wants to run as admin, just prompt the risk and if the
    user accepts, it should be ok, like Visual Studio and VS Code."
  - "Restart as administrator" stays as it is. Before it runs, Debrief shows one warning with **Continue** and
    **Cancel**.
  - The approved gating of program-naming environment variables stays (DB9, below).
  - Nothing else changes: no helper, no refusals while elevated, no trusted plugin copies.
  - Nothing here is built yet. ER-23 does not apply (no drawings).
- Amends: ADR 0073 §1.3, the paragraphs on a relaunch through `runas` and on "No data-folder override is read by an
  elevated or service process"; revision 3's DB9 note; and the Debrief part of "Migration per app" (item 2). DB9's own
  fix is unchanged: release builds ignore the program-path variables (`9905ea48c`, plus `bf40474ec` for
  `CHROMIUM_PATH`, on `origin/fix/debrief-env-exe-override`, merged into `origin/train/2026-11-11-5`, not on main yet).
  This text is merged into ADR 0073 once it is accepted.
- **Scope: user-chosen elevation only.** That means Debrief relaunched through "Restart as administrator", or started
  elevated by the user. Privileged services and helpers that run on their own (Dyno's helper, Pitwall's service) are
  out of scope, and rule 75 (ADR 0092 §4) still binds them. ADR 0092 WP-7 (the W2 bootstrap of the per-plugin elevated
  process) is unchanged.
- Evidence: read on 2026-11-11 with `git show`, `git grep` and `git rev-parse` after `git fetch`. Paths are at
  `origin/main` `ea94d9f1e`, and every row below is also in `debrief-v1.1.1` (`33eba7841`). Nothing was built or run.
  The heads, re-read right before the commit:

```evidence-heads
read: 2026-11-11
origin/main ea94d9f1e
debrief-v1.1.1 33eba7841
origin/train/2026-11-11-5 55c056089
origin/fix/debrief-env-exe-override 82517897b
engineering-rules HEAD 35701f739
```

## The risk the user accepts

An elevated Debrief, and every program and plugin it starts, runs with administrator rights. Anything the user's
account can change can then run as administrator. The warning covers these, all found in the code:

| # | What an elevated Debrief starts or reads | Where |
| --- | --- | --- |
| S1 | The browser at `settings.browser_path` | `app/src-tauri/src/browser/runtime.rs:72,92` |
| S2 | The Chromium runtime downloaded into the data folder | `browser/runtime.rs:73,94` |
| S3 | A custom ffmpeg | `video/runtime_commands.rs:153` |
| S4 | The ffmpeg downloaded into `<data>/video/ffmpeg/<tag>`, with no hash check at use | `video/mod.rs:52,132`; `crates/debrief-video/src/runtime/store.rs:277-295` |
| S5 | Third-party plugins from `<data>/plugins` | `lib.rs:638`; `debrief-plugin-host/src/installed.rs:2,521` |
| S6 | VS Code found through `PATH`, `LOCALAPPDATA` and `ProgramFiles*` | `opener.rs:172-190` |
| S7 | A browser found from `ProgramFiles*` and `LOCALAPPDATA` | `crates/chromium-driver/src/discover.rs:44,94` |
| S8 | A browser named by `CHROMIUM_PATH` | `discover.rs:114`. **Fixed** by `bf40474ec` (train 5) |
| S9 | Programs by bare name (`logman`, `relog`, `wevtutil`, `reg`, `typeperf`, `cmd`, `explorer`, `taskkill`) | `perfmon_control.rs:115`, `blg.rs:54`, `tools.rs:755`, `debrief-collect/src/runner.rs:87`, `storage/walk.rs:202`, `opener.rs:74`, `chromium-driver/src/process.rs:47` |
| S10 | Sidecars next to `current_exe()`, in the per-user install folder (Tauri's default `currentUser`) | `tauri.conf.json` sets no NSIS `installMode`; per-machine installs are ADR 0053's separate decision |
| E1 | The elevated native recorder reads and downloads symbols in the user's AppData | `debrief-plugin-native/src/tool.rs:347,358`, `paths.rs:61-75` |
| E2 | The twelve built-in plugins, importers included, run elevated | `debrief-plugins/src/main.rs:12-27` |

The same holds for data: an elevated Debrief opens and parses the user's projects, imports and settings with
administrator rights. This is the shape of running Visual Studio or VS Code elevated. Microsoft does not treat UAC as
a security boundary (Microsoft Security Servicing Criteria for Windows,
<https://www.microsoft.com/en-us/msrc/windows-security-servicing-criteria>; read before, not re-read for this revision).

## Decision

**The warning.** Every entry point calls `LifecycleService.restartElevated()`
(`app/src/app/lifecycle/lifecycle.service.ts:90-93`): the error hint, the monitor, the event log view, the command
palette and the menu (`elevation-hint.ts:14-15`, `monitor-view.ts:45-47`, `eventlog-view.ts:109-110`,
`lifecycle-commands.ts:11`, `menu-model.ts:347`). So the warning goes there once, before the backend call, through the
app's existing dialog mechanism. The text, in sentence case:

> **Restart Debrief as administrator?**
> While elevated, Debrief and the programs and plugins it starts run with administrator rights. Anything your account
> can change could then run as administrator: the browser and FFmpeg you chose or downloaded, VS Code, third-party
> plugins, and the files you open. Only continue if you trust what is on this computer.
>
> **Continue** · **Cancel** (Cancel is the default button)

- **Continue** proceeds exactly as today: `restart_elevated` (`app/src-tauri/src/system.rs:120-136`) is unchanged.
- **Cancel** does nothing.
- One log line records the choice, with no user content (ER-27).
- A Debrief that is already elevated never offers the restart (`lifecycle.service.ts:35-36`), so it never shows the
  warning either.
- macOS and Linux do not offer the restart (`system.rs:65`), so nothing changes there.

## Work package

| WP | Package | Days |
| --- | --- | --- |
| WP-W | The warning dialog in `restartElevated()` with its i18n strings; a failing spec first (ER-11): Cancel never calls the backend; Continue calls `restart_elevated` with the session flag as today; every entry point goes through the warning; the mock backend records the choice. Release-notes and lessons lines. | 0.5–1 |

It is not a release-path change, so rule 54's installer build is not needed. It goes in Debrief 1.2.

## Decisions for the user

| # | Decision | Recommendation |
| --- | --- | --- |
| D-W1 | Offer "Don't ask again"? | **No.** The restart is rare, and the warning is the whole control: a remembered "yes" would leave nothing. If wanted later, a setting in Settings ▸ Advanced could turn the warning off, visible and reversible, for about 0.25 day. |

## Considered and set aside (2026-11-11)

Both were set aside by the user on 2026-11-11 as more than a personal tool needs. UAC is not a security boundary, and a
warning with the user's choice matches Visual Studio and VS Code.

- **A narrow elevated helper**, revisions 1 to 3 (`6f9d81618`, `78f3f4f2d`, `e32a612f8`), each independently
  reviewed. It removed the relaunch and added:
  - a separate `debrief-admin.exe` with a closed typed request list (ETW, collector sets, the Security log, dumps,
    process samples);
  - a verified pipe in `paddock-ipc`;
  - a per-request UAC prompt for sensitive requests.

  The estimate was 22.75 to 31.25 days. It removed working flows (elevated consoles, root on Unix, admin-only files),
  and a P1 was still open.
- **A middle path** (not committed): keep the restart, install per machine, and while elevated refuse programs from
  user-writable places, with UAC-trusted copies of third-party plugins. About 8 to 11 days.
