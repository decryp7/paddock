# 54. OS integration proved on the real OS: read-back registrations, a lifecycle suite, no discarded errors at the edges, fakes under contract, recorded red runs and layout matrices

- Status: Proposed (2026-11-04), revision 4. Written on the user's go-ahead of 2026-11-04 (ER-21 / rule 40 redesign after
  the Dyno 1.1.0 work of that day). Docs only: nothing here is built yet. Patterns 1 and 2 go into Dyno 1.1.0, **the full
  set** (the user's choice of D5, 2026-11-04); patterns 3 to 5 apply family-wide after that release.
- Revision 4 (2026-11-04): the user approved folding a **VM broker** into the lifecycle gate (§1.5, packages V-1 to
  V-3 in the Dyno 1.1.0 set, check C97), another ER-21 pattern of the same day. The user also **froze Dyno 1.1.0's scope**
  (2026-11-04): only bug fixes for what is already in scope; new ideas go to 1.1.1 or 1.2. The broker is in the set
  because it is part of the gate this ADR already put in scope.
- Revision 3 (2026-11-04) answers the re-review of `707a1c1ec`, which approves the design with one P1 in the text:
  - P1: option (a). L-1b depends only on ADR 0053's S5 (the `login.rs` API, mostly already in `91d703d81`), not on Dyno's
    whole 0053 adoption. Only Dyno's `apps/dyno/app/src-tauri/src/login.rs` is sequenced between 0053's package D and
    L-3 (§1.1, §7.3). RC-1's estimate is named as tracked on its branch.
  - P2s: `native.rs` line numbers on main; the `crates/` cap is measured by E-3's first report run; one `// silent:`
    counter for C93 and C87, owned by A1-s; the lesson says "traced to" until RC-2 confirms; the register entry's `adr`;
    `repair.rs` in L-1a; the RC-1 citation.
  - The open decisions are recorded as the reviewer's recommendations, as coordinator defaults the user may revise. D6
    gates only Spotter's lifecycle run, not C92 and C93 enforcement.
- Revision 2 (2026-11-04) answers the independent review of `df802bb54` ("not yet"):
  - P1-1: no new trait; `paddock_service::backend::Backend` and `Inspection` are extended (§1.1).
  - P1-2: the slices of ADR 0052 that 1.1.0 needs are chosen and sized (§7.1).
  - P1-3: L3 judges crash counters at the end of the run (§1.3).
  - P1-4: C93 and E-4 cover the shared crates on the release path, with a cap (§2.2).
  - P1-5: `service_main` replaces both service entry points, with a register entry (§2.3).
  - P1-6: root-cause packages before the gate (§7.2).
  - P1-7: one owner for `login.rs` (§1.1).
  - Corrected: the login evidence, the Linux turn-off cause, ADR 0053's case list, the E-2 file. The P2s are answered
    where they apply. On D1 the user's side chose to fold pattern 2 into rule 67 (b) and ER-27, so the proposed rules are
    now 68 to 70.
- Date: 2026-11-04
- Related:
  - Engineering rules and family rules: ER-11 / rule 5 (a bug fix starts with a failing test), ER-14 / rule 7 (tests never
    touch the user's machine; VMs only), ER-21 / rule 40, ER-22 / rules 42 and 43 (share first; shared behaviour tested
    once), ER-27 (logs explain every failure; rule 56 on its branch), ER-28 (a cause is confirmed by a reproduction), rule 24
    (services survive updates), rule 54 (the release path builds installers; (c) proposed by ADR 0040), rule 55 / ADR 0034
    (one implementation per mechanism; `docs/known-copies.toml`).
  - Family ADRs: ADR 0021 (OS-aware test paths in `paddock-testkit`), ADR 0022 (checks in `./dev check`; G.7: larger shared
    changes stay out of another app's release window), ADR 0024 (no polling), ADR 0031 (patience, not wall clocks), ADR 0037
    (the app composition contract), ADR 0039 (release gates), ADR 0040 (the Windows GUI session in `vm-smoke`), ADR 0042 (one
    owner per harness), ADR 0049 (`wire.toml`, the push check).
  - **ADR 0052** on `origin/docs/adr-wiring-outcomes-ui` (accepted): its `wiring.toml` with `[caps]`, the `// silent: <why>`
    hatch, C87 (silent drops, family-wide, report-only first), `ReadBack` and `NextStep` (WP-B1), `features.toml`, and the
    shared layout checks `expectLayoutClean` (WP-C2). This ADR extends those and does not repeat them.
  - **ADR 0053** on `origin/docs/dyno-adr-0027-per-machine` (`868641b1c`, proposed): its Windows per-machine VM suite (§9,
    PM-1 to PM-19 with PM-5b and PM-12b) is the base of §1.3, and its S5 settles the `login.rs` API that L-1b
    builds on, and its package D wires Dyno's `login.rs` before L-3 (§1.1, P1-7).
  - The commit-lint trailer on `origin/ci/failing-first-trailer` (`263d49d0b`, `5c4358e64`, not on main): §4 extends it.
  - The branch `fix/dyno-helper-after-restart` (local, worktree `/Volumes/d7sst7/dev-targets/wt/dyno-helper-boot`, head
    `3fd9797ee`, not pushed when read) owns the Windows helper's restart failure (RC-1).
- Numbers (scanned with `git ls-tree -r --name-only origin/<branch> docs/adr` and `git grep` over every remote branch after
  `git fetch`, 2026-11-04):
  - **ADR 0054.** `origin/main` has up to 0049; 0052 is on `origin/docs/adr-wiring-outcomes-ui`, 0053 on
    `origin/docs/dyno-adr-0027-per-machine`; no branch has 0054.
  - **Checks C92 to C97.** The highest check named on any branch is C91 (ADR 0053).
  - **Rules 68 to 70.** Rules 56 to 59 are on branches; 60, 61 and 65 are proposed by ADRs 0041, 0042 and 0049; 64 by the
    0045 draft; 66 is on `origin/docs/rules-2026-11-04`; 67 is ADR 0052's. ADR 0052 notes that the drafts 0046 and 0048
    still name rules 65 and 66 provisionally: **they take 71 and up** when written out.
  - **ER-34 to ER-36.** ER-31 and ER-32 are on `engineering-rules` main; ER-33 is proposed by ADR 0052.

## Evidence

Read in git on 2026-11-04 at the commits named, and in the VM result files of that day. The labels in the "How checked"
columns mean:
- **verified:** read in the code or the result file;
- **measured:** a `git grep` count whose command is given;
- **reported:** the coordinator's or the reviewer's account, not found in git.

### Pattern 1: OS integration verified only against fakes, never across the real OS lifecycle

| Instance | Source | How checked |
|---|---|---|
| **Windows helper service.** Installed at 08:48:53 (event 7045); 21 s later it "terminated unexpectedly" (event 7034). After a real restart the service did not exist (`sc` error 1060), before and after Dyno started; the helper card said "Off". The root cause is owned by `fix/dyno-helper-after-restart` (RC-1; its first commits change the service's start type and let the agent start a stopped service). The crash itself had no logged cause because the helper's error went nowhere (pattern 2) | `/Volumes/d7sst7/dev-targets/scratch/dyno-vm-checks/out/2026-11-04T08-59-25-186Z-dyno-win.md` steps 5 and 8 (installer from CI run 7521, `vm-smoke` at `f2698d9cc`); `25eea905e`, `b47f8ba4d`, `3fd9797ee` on that branch | verified |
| **Start at login on Windows.** The UI said on ("Launch at login after switching it on: true"). After the restart nothing started Dyno, the `Run` key had no Dyno value and the tray had 0 icons. This build still used **`tauri-plugin-autostart`**: CI run 7521 predates `91d703d81`. The UI showed the plugin's intended state, not the OS's | same file, step 9 | verified |
| **Start at login on Linux (GNOME).** Switched on, then after the reboot Dyno did not start. This build also still used the plugin: CI run 7528 predates `91d703d81` | `.../2026-11-04T09-22-13-585Z-dyno-linux.md` step 6 (CI run 7528, `vm-smoke` at `6463a4843`) | verified |
| **The new login writers** of `91d703d81` (`feat/dyno-launch-at-login-default`) replace the plugin with `paddock_service::login`'s `UserRunEntry`, `XdgAutostart` and `AppServiceAutostart`, which read their write back. They are tested against a fake registry that keeps what `reg add` is given (`login.rs`, `registry(sys, drop_writes)`). **They have not run on a real VM yet** (RC-3) | `crates/paddock-service/src/login.rs` at `91d703d81` | verified |
| **Linux turn-off.** "Turn off administrator readings" reported done. 75 s later `dyno-helper.service` was still `active` and `enabled`, through the packaged unit `/usr/lib/systemd/system/dyno-helper.service` and its `multi-user.target.wants` link; `/var/lib/dyno-helper` stayed. The journal shows "restart counter is at 1". The review traced the cause to `Systemd::unregister` (to be confirmed by RC-2's reproduction, ER-28): it drops the result of `systemctl disable --now <unit>` (`let _ = self.ctl(..)`), and for a packaged unit it then returns `Ok(())` without looking. Why the installer's `Step::ConfirmGone` did not catch it is part of RC-2 | same file, step 15; `crates/paddock-service/src/backend/systemd.rs:188-190` at `afc0106b2` (`origin/dyno/wp-int`; `:173` on main) | verified (file and code); the causal link is reported |
| **Windows tray tooltip.** The `NIM_MODIFY` lacked `NIF_GUID`, so the shell refused it and every icon kept its first tip. The unit tests passed because nothing asked the shell | fix `ae396138c`; `apps/dyno/vendor/PATCHES.md`; the Windows VM run recorded in `18106f8e7` | verified (commits) |
| **Earlier: the TCC work folder.** macOS TCC denied self-responsible processes on the external work volume; only a real run on the runner showed it | lessons.md, "TCC blocked self-responsible processes on the external work volume" (2026-11-04) | read |

The family already has the read-back pattern in part, so this record extends it rather than adding a parallel one:
- `paddock_service::backend::Backend` has `inspect`, `register`, `unregister` and `evict`. Its backends are `LaunchdBundled`,
  `LaunchdFile`, `Systemd`, `WindowsRun`, `WindowsService` and `ElevatedWindowsService`.
- The privileged installer ends with `Step::Confirm` and `Step::ConfirmGone` (`privileged/install.rs:56`, `:66`, `:549`).
- The mechanism is registered as `service-registration` in `docs/known-copies.toml`.

What the existing pattern misses:
- `register` and `unregister` return no inspection. One drops its error (`systemd.rs`).
- `Inspection` has no failure state, no exit code and no unit file.
- The login writers sit beside the port instead of behind it.
- The UI shows switch values, not inspections.

ADR 0052 §1.4 requires a VM step per platform feature; it does not say what such a step must cover.

### Pattern 1b: the VM gate's own runs were not trustworthy (revision 4)

| Instance | Source | How checked |
|---|---|---|
| Two Windows VM runs collided: two agents started or reverted the same VMware Fusion guest at once | the coordinator's account, 2026-11-04 | reported |
| An old `.vmx` was restored over the current one, so later reverts went to the wrong snapshot set | the coordinator's account; the runners edit the `.vmx` through `scripts/vm-smoke/vmx-edit.mjs` (`origin/test/dyno-release-vm-checks-v2`, `9dd03e16e`) | reported; the file exists |
| A stale-lock dialog from VMware Fusion appeared on the user's desktop: a run started the GUI over a guest whose `.lck` folders were left behind (ER-14) | the coordinator's account | reported |
| Many step "failures" were bugs in the checks, not the product: a label's text matched too strictly, the Settings window was read as the page, the page handle was lost across a reboot, and a positive control itself failed | the coordinator's account; the runners `scripts/vm-smoke/run-win.mjs` and `run-linux.mjs` (`9dd03e16e`) and `scripts/vm-win.sh` (`origin/tools/win-test-vm`) | reported |
| Each runner starts, reverts and stops VMs itself (`vmrun`, `tart`), and the only guard is an ad hoc `plock` lock named `vm-lock`, which a runner can skip | the same files | reported; the files exist |

A gate whose VM can collide, revert to the wrong state or report its own bugs as product failures cannot gate a release:
its FAILs are not believed and its PASSes are not proof.

### Pattern 2: errors swallowed at OS edges

| Instance | Source | How checked |
|---|---|---|
| **Dyno's service entry.** `fn service_main(..) { let _ = serve_until_stopped(); }` drops every error of the SCM registration, the status reports and the serve loop. `serve()`'s error becomes exit code 1 (`ServiceExitCode::Win32(1)`), so the event log says only "terminated unexpectedly" | `apps/dyno/crates/dyno-helper/src/winsvc.rs:22`, `:53-54` at `afc0106b2` | verified |
| **The shared service entry has the same shape.** `native::scm::run_as_service` drops `set_service_status` three times (`let _ = handle.set_service_status(..)` at `:392`, `:402`, `:403`), and `:340` drops a `OnceLock::set` | `crates/paddock-service/src/native.rs:338-403` on `origin/main` (`4b6d99f57`; the same lines are `:351-416` at `afc0106b2`) | verified |
| **`Systemd::unregister`** drops `disable --now` (above) | `crates/paddock-service/src/backend/systemd.rs:188` at `afc0106b2` | verified |
| **The tray tooltip.** `let _ = icon.set_tooltip(..)` in Dyno's tray hid the refused modify ("app log 'tooltip sent' lines: 0") | `apps/dyno/app/src-tauri/src/tray.rs` before `ae396138c` (now `if let Err(e) = ..`, line 379) | verified |
| **The tray's visibility call.** `unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };` in `set_tray_visible` still drops the `BOOL` | `apps/dyno/vendor/tray-icon/src/platform_impl/windows/mod.rs:84` at `afc0106b2` | verified |
| **The autostart plugin** reported success with nothing registered, and nothing was logged | VM step 9; `91d703d81`'s message | verified |
| **The deferred helper log.** The file log waits until the helper's folders are verified (`fc140fc6f`, P1.2: "standard error until ..."). A Windows service's standard error is not kept, so a refusal at start was invisible | `fc140fc6f`; `apps/dyno/crates/dyno-helper/src/logfile.rs` | verified |
| **Footprint, shared crates on Dyno's release path,** non-test Rust at `afc0106b2`. `let _ = `: `paddock-service` 65, `paddock-proc` 26, `paddock-ipc` 27. `.ok();`: 1, 3 and 1. (The review gave its own counts; this revision uses these, measured as shown) | `git grep -cE 'let _ = ' origin/dyno/wp-int -- 'crates/<crate>/src/*.rs' ':!*tests*' ':!*test.rs' ':!*/testing*'` | measured |
| **Footprint, Dyno,** non-test Rust at `afc0106b2`. `let _ = `: `dyno-platform/src` 79, `dyno-helper/src` 17, `app/src-tauri/src` 44, `dyno-agent/src` 128. `.ok();`: 3, 0, 2 and 5 | the same command per folder | measured; not every site is an OS edge or a defect |
| **Footprint, family-wide,** non-test Rust on `origin/main` (`4b6d99f57`): 1,816 `let _ = ` and 139 `.ok();`, most of them `writeln!` into a `String` and other harmless drops | `git grep -cE 'let _ = ' origin/main -- 'crates/*/src/*.rs' 'apps/*/src/*.rs' ':!*tests*' ':!*test.rs' ':!*/testing*'` | measured |

ADR 0052's C87 covers `let Ok(..) else`, `if let Ok` without `else`, and `.ok()` feeding `?` or `unwrap_or*`, all
family-wide. It is report-only until its baseline is cut, and it adds Clippy `let_underscore_must_use` to the preset with a
baseline. None of the instances above is a `let Ok(..) else`: they are `let _ =`, dropped FFI returns and fatal errors with
nowhere to go. The family-wide footprint says a blanket ban would be mostly waivers; the instances say the OS edge is where a
drop hides a failure the user sees.

### Pattern 3: test fakes drift from the real system

| Instance | Source | How checked |
|---|---|---|
| A fake trust port rooted at `/` was used by two staging tests that ran on Windows. The fix gated the tests to Unix instead of making the fake OS-native | `74803654f` | verified (commit) |
| The helper test rig re-decided the restriction table itself: a second implementation of auth (rule 55). Fixed to call `dyno_helper::auth::authorize` and `auth::scope` | `deb7db7d6` | verified (commit) |
| Dyno's mock duplicated the engine's number-support strings. Fixed by a golden that the engine's own test writes (`number_support.rs:166-185`) and a mock spec that compares against it (`number-support-golden.spec.ts`) | at `afc0106b2` | verified |
| The new login writers are proven only against a fake registry that keeps what it is given (pattern 1) | `91d703d81` | verified |
| Scripted doubles such as `privileged/install_tests.rs`'s `Scripted` backend and the recording `System` fake ("`launchctl`, `sc`, `systemctl` are never run") answer from canned outputs. Nothing ties those outputs to what the tools really print | `crates/paddock-service/src/privileged/install_tests.rs:1-2`, `:60` | verified |

### Pattern 4: failing-first claims not backed by a run

| Instance | Source | How checked |
|---|---|---|
| `74e57f519` (WP-LNX) claimed red runs for two UI specs, and `b8628bc21` for a Rust test; none had been run. Corrected by `77bf0b5ae` | `77bf0b5ae` | verified (commit) |
| `13bf96710` claimed red against `Live` without a run. Corrected by `357b6704c` | `357b6704c` | verified (commit) |
| The trailer on `origin/ci/failing-first-trailer` (`263d49d0b`) is required on `fix:` commits only, and its parenthesis is free text, so both claims would have passed it | `conformance/src/commit-lint.mjs` on that branch | verified |

### Pattern 5: shared UI layout fixed one platform at a time

| Instance | Source | How checked |
|---|---|---|
| The tray "label over value" row of the shared widget chooser took four rounds in one afternoon: `5670221aa` (13:36), `4d56051c3` (15:22), `c896717b9` (15:31), `c927c9add` (15:37), then `78b230db2` on `fix/dyno-tray-label-layout`. In that last round the container query had measured the whole chooser, so with a preview column the rows used the wide layout. A Windows fix regressing the Mac row is the coordinator's account | commits; `ui/projects/paddock-ui/widgets/src/chooser.ts`, `apps/dyno/app/src/app/features/settings/menu-bar-items.ts` | verified (commits); the regression order is reported |
| The row's layout depends on three things, and no test ran their combinations: a platform capability (the number-support golden's keys `mac` 18 px, `win16`, `win20`, `win24`), a container width, and a variant | `number-support-golden.spec.ts` at `afc0106b2` | verified |

## Context: the five patterns and their invariants

1. **Real OS lifecycle.** An OS registration lives through restarts, other users' sign-ins, updates and removal, which only
   the real OS performs. *Invariant:* every registration is read back from the OS by the same port that wrote it; the UI
   shows what the OS reports; and before a release every registration has passed install, first run, restart, second user,
   update, turn-off and uninstall on a VM of each OS it supports, with no crash counted over the whole run.
2. **Errors at the edges.** *Invariant:* at an OS edge no error is dropped, and a service's fatal error reaches the OS's own
   log from its first line.
3. **Fakes under contract.** *Invariant:* every double passes the contract suite that the real adapter also passes, on each
   OS. A double never decides; a scripted double replays outputs recorded from the real tool.
4. **Recorded red runs.** *Invariant:* every claim names a red state that can be checked out again, and the log of its run.
5. **Layout matrices.** *Invariant:* every combination of platform capability, width and variant runs the shared layout
   checks on every change.

## Decision

### 1. Write, then verify, through the existing service port (pattern 1)

**1.1 `Backend` and `Inspection` are extended.** This changes the canonical implementation of the registered mechanism
`service-registration`, so every copy in the register is checked in the same change or waived (rule 55 (a)).

- **`register` and `unregister` return the post-inspection:**
  `fn register(&mut self) -> Result<Inspection, RegisterError>` and `fn unregister(&mut self) -> Result<Inspection, UnregisterError>`.
  - A write whose read-back disagrees is an error that carries the inspection: `RegisterError::NotAsWritten(Inspection)`,
    `UnregisterError::StillPresent(Inspection)`. It is never `Ok`.
  - `UnregisterError` is new and typed, replacing `String`.
  - The installer's `Step::Confirm` and `Step::ConfirmGone` judge the returned inspection.
  - A contract case (§3) checks for every backend that the returned inspection equals a fresh `inspect()`.
- **`Inspection` gets typed fields** (P2: no free text):

  ```rust
  pub enum Job { Absent, Running, NotRunning, Refused, Unknown,
                 Failed(LastExit) }                     // new: stopped with an error
  pub enum LastExit { Code(i32), Signal(i32), Win32(u32), ServiceSpecific(u32) }
  pub struct Inspection {
      // ... the existing fields ...
      pub last_exit: Option<LastExit>,                   // SERVICE_STATUS_PROCESS, systemd ExecMainStatus, launchd "last exit code"
      pub restarts: Option<u32>,                         // systemd NRestarts, launchd runs - 1; None on Windows (§1.3 reads the event log)
      pub unit: Option<UnitFile>,                        // which definition is in force
  }
  pub struct UnitFile { pub path: PathBuf, pub origin: UnitOrigin, pub enabled: UnitFileState }
  pub enum UnitOrigin { Packaged, Admin, Runtime, Bundle, ServiceManager }  // /usr/lib, /etc, /run, the app bundle, the SCM
  ```

  `Systemd::inspect` reads `systemctl show -p FragmentPath,UnitFileState,ActiveState,Result,NRestarts,ExecMainStatus`.
- **`Systemd::unregister` is fixed.** It checks `disable --now` and fails with the tool's output unless the post-inspection
  shows the unit disabled and inactive. A packaged unit is not removed from disk, but it must read back as disabled and
  not running.
- **Waits are event-driven.** Waiting for a state change uses `NotifyServiceStatusChangeW` and systemd's D-Bus
  `PropertiesChanged`, bounded by ADR 0031's patience, never a poll (ADR 0024).
- **The login writers move behind the port, in this order (P1-7).** Each file has one owner at a time:
  1. ADR 0053's S5 settles `crates/paddock-service/src/login.rs`'s `UserRunEntry` API (most of it is already in
     `91d703d81`). L-1b waits for S5 only, not for Dyno's whole 0053 adoption.
  2. Then L-1b moves the writers, with no API change before that:
     - `UserRunEntry` merges into the `WindowsRun` backend, the same `Run`-value mechanism. `WindowsRun` gains the
       `TokenElevationTypeFull` rule and `StartupApproved`.
     - `XdgAutostart` becomes a `Mechanism::XdgAutostart` backend.
     - `AppServiceAutostart` becomes `LaunchdBundled` for the main app (`SMAppService.mainApp`).
  3. `Autostart` and `Autostart::set` go.

  `login.rs` is the canonical of the registered mechanism `launch-at-login`, whose copies are Dyno's `login.rs` and
  `login_item.rs` (delete by `dyno-v1.1.0`) and Spotter's `tauri_plugin_autostart` wiring. L-1b therefore touches those
  copies or waives them in the same change (rule 55 (a)), and Dyno's copies go with L-3.

  Dyno's `apps/dyno/app/src-tauri/src/login.rs` is the one file both ADRs change. ADR 0053's package D edits it first (it
  wires `UserRunEntry` and the start check of its §6.2); L-3 edits it after D's commit has landed, on the API of L-1b. No
  other part of D gates this ADR.
- **Registrations that are not services return their own read-back.** Each is declared like a service (§1.2):
  - tray icons: Dyno's tray adapter returns a `TrayPlacement` from `Shell_NotifyIconGetRect`, `NSStatusItem.isVisible` or
    the StatusNotifierWatcher;
  - ACLs: `paddock_service::privileged::trust` returns the SDDL or owner and mode read by handle;
  - pipes: `paddock_ipc` returns the security descriptor or mode read by handle after creation.
- **The UI shows inspections, never the switch's own value.**
  - The Start-at-login switch and the helper card are bound to the last `Inspection`.
  - When it differs from the stored choice, the row says so with the next step, for example "Windows has no start entry for
    Dyno. Turn on again, or see the log."
  - The helper card shows the service manager's state and `last_exit`.
  - The diagnostics bundle lists every registration's inspection (no user content, ER-27).
- **No dependency on ADR 0052's WP-B1.** 1.1.0 uses the extended `Inspection` as its evidence. When WP-B1 lands, its
  `ReadBack` wraps `Inspection` (§7.1).

**1.2 Every registration is declared** in `apps/<app>/wiring.toml` (the file of ADR 0052, from the slice of §7.1):

```toml
[[os_registration]]
id = "helper-service"
kind = "service"                 # service | login-entry | tray-icon | scheduled-task | acl | pipe | firewall-rule
os = ["windows", "linux", "macos"]
scope = "machine"                # machine | user
adapter = "paddock_service::backend::WindowsService"   # or the non-service adapter of §1.1
made_by = ["code"]               # code | installer | package  (NSIS hooks, the .deb's packaged unit, the app bundle)
lifecycle = "helper-service"     # the vm-smoke lifecycle row (§1.3)
leftovers = []                   # what may stay after uninstall, each with why
```

**1.3 The lifecycle suite in `scripts/vm-smoke`.** One reusable suite per OS, on these bases:
- Windows: ADR 0053 §9's set-up (the ADR 0040 GUI session, `paddock-admin`, `paddock-test`, a signed-out account, UAC
  automation).
- Linux: the Tart GNOME image with a second account.
- macOS: the Tart image.

For each `[[os_registration]]` and each OS it lists, the suite runs the steps below in order on one install. At each step it
judges three things:
- the OS state, from a probe per kind: `sc qc`/`sc query`, `reg query`, `schtasks`, `icacls`, `systemctl show`, the
  autostart folder, `launchctl print`, `sfltool dumpbtm` (read-only, in the VM);
- the app's reported `Inspection`, from the diagnostics command;
- the declaration.

| Step | Passes when |
|---|---|
| L1 Install | the installer's and the package's registrations are present as declared, and the app's inspection agrees with the probe |
| L2 First run | the first-run defaults (Dyno: start at login on) are present by the probe and shown as such; turning each switch off and on changes the probe within the patience bound, and the UI follows the probe |
| L3 Settle (judged at the end, P1-3) | see the counters below |
| L4 Restart | after a real reboot and sign-in: services running, login entries started the app once, tray icons present, nothing repaired by hand |
| L5 Second user | a second account signs in (Fast User Switching on Windows, a second GNOME session, a second macOS account): its per-user registrations are its own, machine ones are shared, and neither user's change alters the other's |
| L6 Update (P2) | the VM starts from the **last released tag** and updates to the candidate by the path that release's users take: the updater on macOS, `dpkg -i` of the new `.deb` on Linux. Every registration survives or is re-registered once (rule 24). Where that path is a refusal by design (Dyno 1.0 per user to 1.1.0 per machine on Windows), L6 is ADR 0053's PM-4 and PM-5, and the per-machine update path is proved by PM-6 and PM-12 with the candidate over a second build of itself |
| L7 Turn off | every switch that removes a registration (the helper's Turn off, start at login off) leaves the probe showing it gone, including packaged units and their `.wants` links; only declared leftovers remain |
| L8 Uninstall | every registration, and the per-user ones of signed-in accounts, are gone; only declared leftovers remain; no process is left |

L3 counts crashes over the whole run, including both boots, and judges each service by its OS's counters:

| OS | Counter | When it is read | Passes when |
|---|---|---|---|
| Windows | System event log entries 7031, 7034 and 7024 for the service | once, at the end of the run (the log outlives L8) | none since the L1 install time |
| Linux | `systemctl show -p NRestarts,Result` | at the end of each boot segment, before L7 removes the unit | `NRestarts=0` and `Result=success` |
| Linux | the unit's journal, whole run | at the end of the run | no "Scheduled restart job" and no failing "Main process exited" line |
| macOS | `launchctl print`'s `runs` and `last exit code` | at the end of each boot segment | `runs = 1` and `last exit code = 0` |

The Windows 21 s crash (event 7034) and the Linux journal's "restart counter is at 1" would each fail L3, whatever the
state at any one moment.

ADR 0053's cases become the Windows per-machine rows of the same suite, so there is one harness:
- all of PM-1 to PM-19, with PM-5b and PM-12b;
- PM-10 is L4, PM-6 is L5 with an update, PM-9 is L8;
- PM-4, PM-5, PM-6 and PM-12 serve L6 as above.

Each step writes the probe's raw output and the app's inspection into the result file. VMs stay local (ER-14, the
2026-10-31 decision).

**1.4 Checks.**
- **C92 (OS registrations declared and covered),** a Rust check in `./dev check` (ADR 0022):
  - **Source.** A call that makes an OS registration must be inside a declared adapter. The check's list of such calls:
    `ServiceManager::create_service`, `CreateServiceW`, `ChangeServiceConfig*`, `Shell_NotifyIconW`, `SetSecurityInfo`,
    `SetNamedSecurityInfoW`, `CreateNamedPipeW`, `RegSetValueExW`, `SMAppService`; `reg add`, `schtasks`,
    `systemctl enable`, `launchctl bootstrap` and `icacls` run through `paddock_proc`; and writes under an `autostart`
    folder.
  - **Packaging.** The packaging inputs are enumerated too: a unit or plist in `bundle.linux.deb.files` and the bundle
    resources, and the service and `Run` commands in NSIS hooks. Each must be declared with `made_by = ["package"]` or
    `["installer"]`, which is how the packaged systemd unit gets a row.
  - **Declarations.** Every declared entry names an existing type and a lifecycle row for each OS it lists.
  - **Blind spots (P2), stated in its fix text.** It is name-level, like C83: a call through a macro or a trait object, a
    tool name built at run time, and registrations made by third-party code (a plugin crate, the WebView2 installer) are not
    seen. The lifecycle run is the backstop: its L1 probes list every service, `Run` value, unit and login item that names
    the app's folder or identifier, and an undeclared one fails L1.
- **Rule 54 (d) (proposed).** A tag of an app with any `[[os_registration]]` needs a lifecycle result with no FAIL for the
  tagged head on each OS its registrations list. `./dev release tag` (ADR 0039's gate) reads the result files. A row that
  cannot run in a VM (macOS Login Items approval) is MANUAL with a dated waiver (§8).

**1.5 The VM broker (revision 4).** A single broker, `./dev vm` (in `tools/paddock-dev`, ADR 0022), is the **only** thing
that starts, reverts or stops a VM for this suite, for ADR 0053's PM cases and for ADR 0040's GUI session.
- **One lease per VM.** `./dev vm lease <vm> --for <purpose>` returns a lease held by the caller (a process handle and
  start time, rule 53), bounded by ADR 0031's patience and released on exit. It replaces the ad hoc `plock` `vm-lock`. A
  second caller waits or is refused with the holder's purpose and age; it never shares the VM.
- **Preflight before every start or revert:** the `.vmx` (or the Tart config) parses; every disk it names exists; leftover
  `.lck` folders are reported with their age and owner, and a run does not start over them; no other process runs the
  VM (`vmrun list`, `tart list`). A failed preflight is a "could not run" result with the reason, never a FAIL.
- **Headless only.** The broker starts guests with `nogui` (VMware Fusion) or `--no-graphics` (Tart), so no dialog can
  appear on the user's desktop (ER-14). ADR 0040's interactive Windows session runs inside the guest and is reached over
  its own channel, not through a host window.
- **The `.vmx` is edited only by the broker,** with one validated function (a parsed edit, written to a temporary file
  and renamed), and a backup `.vmx` is **never** restored. A broken `.vmx` is a preflight failure for a person to fix.
- **Results carry the lease.** Each result file records the lease id, the VM, the snapshot and the runner's command.
  `./dev release tag` (rule 54 (d)) accepts only results stamped by a broker lease.
- **Runs outside the broker are detected and refused.** The broker refuses to lease a VM that is running without a lease
  and names the process. C97 (below) refuses scripts that call the VM tools directly.

**Every lifecycle check proves it can pass (positive control).** Each check of L1 to L8 declares a positive control: the
same probe and judge run against a state known to pass, before or beside the real step. Examples: the `Run`-value probe on
a value the runner wrote itself; the page reader on a known label; the page handle re-acquired after a reboot of a guest
whose app is known to be up. A step whose positive control fails is reported as **check broken**, never as a product
FAIL, and the gate does not pass on it.

**Every failing step classifies itself** as `product` or `check`, with the evidence for the choice:
- `check`: the positive control failed, the probe could not run, or the reader found no element of the expected kind;
- `product`: the positive control passed and the probe read a state that differs from the declaration.

The result file shows the class, the evidence (the probe's raw output and the control's result) and, for `check`, which
check to fix. Only `product` FAILs and `check` errors that hide a step both block a tag, and they are reported apart.

**C97 (VMs only through the broker),** a source check in `./dev check` over `scripts/` and `tools/`, fails on any call to
`vmrun`, `tart` (`run`, `clone`, `delete`, `stop`, `set`), `prlctl` or `VBoxManage`, or any write to a `*.vmx`, outside
`tools/paddock-dev/src/vm/`. It ships with a fixture of today's `run-win.mjs` call.

### 2. Errors at OS edges are never discarded (pattern 2; rule 67 (b) and ER-27 amended)

**2.1 OS-edge modules.** An OS-edge module is a Rust module that holds a `Backend` or another adapter of §1.1, calls an OS API
through FFI (`windows`, `windows-sys`, `libc`, `nix`, `objc2`, `core-foundation`), or runs an OS tool through `paddock_proc`.
- Each app lists its edge modules in `apps/<app>/wiring.toml`, and the shared crates list theirs in `crates/wiring.toml`
  (`[edges] modules = [..]`).
- C93 also finds unlisted edge modules by those imports.
- Each edge module starts with
  `#![deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)]`.

**2.2 C93 (edges never discard),** in `./dev check`, over the non-test code of every edge module of each app **and of the
shared crates on its release path**. For Dyno 1.1.0 those crates are `paddock-service`, `paddock-proc` and `paddock-ipc`,
taken from Dyno's Cargo graph. C93 fails on:
- `let _ = <expr>` and `_ = <expr>`;
- `.ok();` as a statement, and `.ok()` whose value is unused;
- an expression statement whose value is an FFI call's return (`unsafe { Shell_NotifyIconW(..) };`), unless the function
  is on the check's list of calls that return nothing;
- `if let Ok(..)` or `if let Some(..)` on a fallible OS call without `else` (C87's shape, here with no baseline);
- a `define_windows_service!`, a `main` of a binary declared as a service, or a launchd or systemd entry point that does not
  go through `paddock_service::service_main` (§2.3).

The escape hatch is ADR 0052's `// silent: <why>`:
- It is counted against `[caps] silent`.
- Dyno's cap is 20 (ADR 0052's starting value).
- `crates/wiring.toml` has its own cap. It is set from E-3's first report run over the shared crates, after E-4's fixes:
  the number of `// silent:` lines that remain is measured, recorded in the commit that sets it, and becomes the cap. No
  number is guessed here.
- There is one `// silent:` counter per app and one for `crates/`, shared by C93 and ADR 0052's C87: a hatch is counted
  once whichever check reads it. A1-s owns the counter (in the `wiring.toml` reader), and both checks call it.
- Adding a hatch or raising a cap needs ADR 0052's `Escape-hatch:` trailer.

Typical legitimate uses are `CloseHandle` during clean-up after an error was already reported, and a `send` to a receiver
that is known to have gone at shutdown.

**2.3 One service entry point.** `paddock_service::service_main(spec, |stop| -> Result<(), ServiceError>)` replaces both
`native::scm::run_as_service` and Dyno's `winsvc.rs` (P1-5). It does the following:
- **Opens an OS-log sink before anything else:**
  - Windows: the Application event log under the service's own event source, registered by `paddock_service::render`'s
    install plan;
  - Linux: the systemd journal (stderr, with `StandardError=journal` in rendered units);
  - macOS: the unified log (`os_log` under the service's identifier).

  A file log deferred for trust reasons (`fc140fc6f`) joins later; every line before it, and every refusal, goes to the OS
  log.
- **Checks every status report** (`set_service_status`) and logs start-up phases with their durations.
- **On `Err`:**
  - writes the error with its cause chain and next step to the OS log and to the file log when it is open;
  - stops with `ServiceExitCode::ServiceSpecific(code)` on Windows, so the event log's 7024 entry carries the code;
  - exits with a distinct status on Unix;
  - takes the code from one table per service.
- **Catches panics at the boundary,** logs them with their location, and turns them into an exit code.

`docs/known-copies.toml` gets the entry below, in this ADR's change, because the copies exist today. A register-only change
is docs-only, so rule 55 (e) asks the steward to run `copies --check-register` before it lands. That subcommand is not on
this tree yet (C58 is a follow-up), so the entry was checked by hand: the file parses as TOML, its 47 ids are unique, and
the format follows the file's header.

```toml
[[mechanism]]
id = "service-main"
title = "Running a service or daemon body under the OS service manager (status reports, exit codes, fatal error to the OS log)"
canonical = "paddock-service"
paths = ["crates/paddock-service/src/native.rs"]
symbols = ["item:run_as_service"]
status = "built"
adr = "0054"

  [[mechanism.copy]]
  paths = ["apps/dyno/crates/dyno-helper/src/winsvc.rs"]
  owner = "dyno"
  delete_by = "dyno-v1.1.0"
  note = "Dyno's helper wraps the SCM itself; it drops serve's error. Both go when service_main lands (ADR 0054 E-1, E-2)."
```

When E-1 lands, the canonical moves to `service_main` (`crates/paddock-service/src/service_main.rs`); the entry's `paths`
change with it, and the entry leaves the register with `winsvc.rs`.

**2.4 TypeScript edges.** These rules apply to the backend adapters of each app (the `invoke` wrappers behind the DI token)
and to `paddock-ui`'s shell adapters:
- `@typescript-eslint/no-floating-promises`;
- `@typescript-eslint/no-misused-promises`;
- a family rule `paddock/no-silent-catch`. It fails a `catch` or `.catch` that neither rethrows, logs through the app's
  logger, nor returns a typed error value, unless it carries `// silent: <why>`.

### 3. Fakes under contract (pattern 3)

**3.1 Contract suites.** `paddock-testkit` gets `contract`. A port's crate writes its contract once, as generic cases over a
factory, and every implementation instantiates it:

```rust
paddock_testkit::contract_tests!(backend_contract(), fake = |root| FakeBackend::new(root));
paddock_testkit::contract_tests!(backend_contract(), real = |root| Systemd::user(root), os = ["linux"]);
```

- **OS-native roots.** Each case gets a `TestRoot`, an OS-native temporary root (ADR 0021). A fake that hard-codes `/`
  fails its contract on the Windows runner (the `74803654f` instance) instead of being gated off.
- **Where the real side runs.** The real instantiation runs on each OS runner for ports over files, processes and sockets
  in that root. For ports that change OS registration state, the real side runs only inside the lifecycle VM (ER-14), and
  CI runs the fakes.

**3.2 A double stores, replays or delegates; it never decides.** A decision the real code makes (authorisation, limits,
formatting, number support, parsing) either comes from the real code, as `deb7db7d6` did for the rig, or from a golden that
the real engine writes in its own test. Goldens use the existing `paddock_commands::testing::sync_generated` (P2), not a
new writer.

**3.3 Scripted doubles (P2).** Backends tested over the recording `System` fake, and the installer's `Scripted` backend,
cannot run a behavioural contract: they answer from canned tool output. Their limit is stated, and they are tied to the real
tool another way:
- every canned output a scripted double answers with comes from a **recording**, made by the lifecycle suite's
  `--record` mode in the VM (`sc qc`, `systemctl show`, `launchctl print` output for each state of L1 to L8);
- the recordings are stored as fixtures with the OS version and the date;
- a scripted answer that matches no recording is reported.

This still does not prove the parser against tool versions that were never recorded; the lifecycle run remains the proof.

**3.4 Registers and checks.**
- **The registers.** `apps/<app>/fakes.toml` and `crates/fakes.toml` list every double: its port, its kind (`contract`,
  `script`, `golden`), and its contract, recordings or golden.
- **C94 (doubles registered and under contract).** It enumerates:
  - Rust impls of port traits (ADR 0052's `paddock_shell::Port` marker plus the traits in `fakes.toml`) whose type is
    under `#[cfg(test)]`, in a `testing` module, or named `Fake*`, `Mock*`, `Stub*`, `Null*` or `Scripted*`;
  - TypeScript classes in `mock/` folders that implement an app's backend token interface.

  Each must be in the register with an existing instantiation, recordings or golden, and each contract must also be
  instantiated for the real adapter.
- **C95 (mock imitations have engine goldens).** A TypeScript mock function marked `/** @imitates <crate>::<fn> */` has a
  spec that compares it with a generated golden, and the Rust side has a "golden is current" test.

### 4. A failing-first claim names a run that can be found again (pattern 4)

This extends `origin/ci/failing-first-trailer`; it does not add a second trailer.

```
Failing-first: <test id> red at <sha>[+patch] (log: notes | ci:<run>/<job>)
Failing-first: none (<reason: docs-only, build-only, revert, test-only>)
```

- **The red state and its log.** `./dev red <test id> [--patch <file>]` runs the test on the current tree (in a scratch
  worktree with the patch applied, when one is given) and requires it to fail. It then writes a git note under
  `refs/notes/failing-first` with:
  - the test id;
  - the red state: the base commit's patch-id, plus the patch;
  - the log's last 200 lines, masked, with no user content.
- **Attaching the note.** The note goes on the claiming commit after the commit is made: `./dev red --attach` after
  `git commit`, or the `commit-msg` hook of ADR 0042.
- **Notes survive a rebase (P2).** `./dev` sets `notes.rewriteRef = refs/notes/failing-first` and
  `notes.rewrite.rebase = true` and `notes.rewrite.amend = true` in the worktree's config, so git copies the note when it
  rewrites the commit. The red state is stored as a patch-id plus a patch, not as a sha, so it still applies after the
  rebase. The `<sha>` in the trailer is informative; the pre-push check accepts it when a commit with the same patch-id is
  in the range.
- **Corrections.** A wrong claim is corrected by a later `Failing-first correction: <claiming sha>: <new trailer>` line,
  checked in the same format.
- **Checks.**
  - commit-lint checks the format on every commit that carries the trailer, and still requires it on `fix:` commits.
  - `./dev check --pre-push` (ADR 0038) checks that each `log: notes` claim has its note, and `./dev` pushes and fetches
    the notes ref with the branch.
  - Nightly, `./dev red --verify-sample 5` re-runs five claims from the last seven days: red at their state, green at the
    claiming commit. The sample is drawn with a printed seed (ER-29), and a mismatch is a job-summary FAIL that names the
    commit.
  - **Measured before it is scheduled (P2).** R-3 records the sample's run time in `docs/benchmarks.md` (rule 48). Its
    budget is 15 minutes of one Linux slot; over budget, the sample shrinks to three.

### 5. Shared layout runs every platform, width and variant (pattern 5)

- **A platform difference is a capability value.** A shared component never detects the OS. What differs per platform
  reaches it as a typed input; for the chooser that is the tray's `NumberSupport`, generated from the engine's golden.
- **`defineLayoutMatrix`.** Each component below has a `<name>.layout.ts` beside it, listing its capability values, widths,
  variants and data profiles:

  ```ts
  export default defineLayoutMatrix(WidgetChooser, {
    capabilities: { tray: [GOLDEN.mac, GOLDEN.win16, GOLDEN.win20, GOLDEN.win24] },
    widths: [320, 520, 900, 1280],
    variants: { layout: ['fixed', 'editable'], items: ['one', 'limit'] },
    data: ['default', 'long-names'],
  });
  ```

- **The gallery and the runner.** The gallery generates one route per row. `runLayoutMatrix` in `@decryp7/paddock-e2e`
  visits every row and runs the shared layout checks: today `expectNoHorizontalScroll` and `expectNoSilentClipping`, and
  `expectLayoutClean` once ADR 0052's WP-C2 lands.
- **The affected plan.** A change to a component runs its whole matrix.
- **Large matrices.** A matrix over 200 rows runs pairwise (every pair of values covered) instead of the full product.
- **C96 (layout matrices),** narrowed (P2):
  - It covers `paddock-ui` components whose styles have `@container` or `@media` rules, or that have an input whose type
    name ends in `Capability` or `Support`.
  - It fails when such a component has no `.layout.ts`, or when its matrix omits a key of the capability's golden.

### 6. Rules and engineering rules (proposed)

> 68. **OS registrations are proved on the real OS, across their lifecycle, and the UI shows what the OS reports.** Every
> service, login entry, tray icon, scheduled task, ACL, pipe or firewall rule an app registers goes through an adapter that
> reads back what it wrote: for services and login entries, `paddock_service::backend::Backend`, whose `register` and
> `unregister` return the post-inspection. Settings, status and logs show that inspection, never the intended state. Each
> registration, including those made by an installer or package, is declared in `apps/<app>/wiring.toml` and has a row in
> the `vm-smoke` lifecycle suite for each OS it supports: install, first run, restart, second user, update from the last
> release, turn off, uninstall, with no crash or restart counted by the OS over the run. A tag needs that suite without
> FAIL for the tagged head (rule 54 (d)). The suite's VMs are started, reverted and stopped only by the `./dev vm` broker
> (one lease per VM, preflight, headless, no restored backups), every check has a positive control, and every failure says
> whether the product or the check failed, with evidence. Checked by C92, C97 and the lifecycle suite. *Why:* on 2026-11-04 Dyno 1.1.0's helper
> service crashed 21 s after install and was gone after a restart, start at login registered nothing on Windows or Linux
> while the switch said on, a Linux turn-off left the service running because its error was dropped, and a tray modify was
> refused, all with every test green against fakes.

**Rule 67 (b), amended (D1, decided):** after "an error is logged or carried, never dropped" it adds:

> ...; in an OS-edge module (an adapter of rule 68, a module that calls an OS API through FFI or runs an OS tool, in an app
> or in a shared crate on its release path) no `let _ =`, `_ =`, `.ok();`, dropped FFI return or `if let Ok` without `else`,
> unless the line says `// silent: <why>`, within the cap; a service runs through `paddock_service::service_main`, which
> writes its fatal error to the OS log from its first line and exits with a code that names it (C93; ADR 0054).

> 69. **A test double passes the same contract as the real adapter, and never decides.** Every fake, mock or scripted double
> of a port is registered in `fakes.toml`. A behavioural double passes the port's contract suite
> (`paddock_testkit::contract_tests!`), which also runs against the real adapter on each OS it supports: in CI for file,
> process and socket ports in an OS-native temporary root, and in the VM for registration ports (ER-14). A scripted double
> answers only with outputs recorded from the real tool. Decisions come from the real code or a golden the real engine
> writes. Checked by C94 and C95. *Why:* a fake rooted at `/` ran on Windows, a test rig re-implemented the helper's auth,
> the mock copied the engine's strings, and the login writers were proven against a registry fake that keeps whatever it is
> given.

**Rule 5, amended** (pattern 4; D4), replacing the branch's sentence:

> ... Every `fix:` commit, and every commit that claims a failing-first run, carries `Failing-first: <test id> red at
> <sha>[+patch] (log: notes | ci:<run>/<job>)`, made with `./dev red`, which keeps the red state and log in a git note that
> survives a rebase, or `Failing-first: none (<reason>)`. A wrong claim is corrected by a later `Failing-first correction:`
> line. commit-lint checks the format, the pre-push check that the note exists, and a nightly sample re-runs claims.
> *Why:* two claims of 2026-11-04 had not been run.

> 70. **A shared component with layout variants ships a layout matrix.** A `paddock-ui` component with `@container` or
> `@media` rules, or a `*Capability` or `*Support` input, declares in `<name>.layout.ts` every capability value, width,
> variant and data profile. Platform differences reach it as a capability value, never by detecting the OS. The gallery
> renders every row, and `paddock-e2e` runs the shared layout checks on each, so a fix for one platform is judged on all of
> them. Checked by C96. *Why:* the tray "label over value" row took four rounds in one afternoon, each fixing the platform
> being looked at.

For the engineering rules (coordinator-owned sync, as for ER-33):

> ER-34. **OS integration is proved on the real OS across its lifecycle.** Every registration with the OS reads back what it
> wrote through the same port, and the UI shows what the OS reports. Before a release each registration passes install,
> first run, restart, a second user, update from the last release, turn-off and uninstall on a VM of each OS it supports,
> with the OS's crash and restart counters at zero over the run. *Why:* fakes keep whatever they are given; the OS does not.

> ER-27, amended: "... Where code meets the OS, no error is discarded: it is returned, carried into the state the user
> sees, or logged with its cause; a deliberate drop says why. A service or daemon writes its fatal error to the OS's own log
> from its first line."

> ER-35. **A test double passes the same contract tests as the real thing, on each OS, and never decides;** scripted
> answers are recorded from the real tool. *Why:* fakes drift, and a fake that decides is a second implementation.

> ER-11, amended: "... The claim names the red state (a commit, or a commit plus a patch) and where its log is kept, in a
> form that survives a rebase, and a tool can re-run it."

> ER-36. **Shared UI is tested over every platform capability, width and variant it supports, in one run.** *Why:* a fix on
> the platform in front of you regresses the one that is not.

## Checks that make the classes impossible or detected

| Check | Kind | Would have failed on |
|---|---|---|
| `register`/`unregister` return the post-inspection; typed `Inspection`; the UI binds it | type plus the existing shared port | the Linux turn-off (`StillPresent`); a switch that is on with no `Run` value |
| C92: declared registrations, including packaged ones, each with a lifecycle row | source and packaging check | every Dyno registration today, none of which is declared; the packaged unit |
| Lifecycle suite L1 to L8 with crash counters; rule 54 (d) | real-OS release gate, local VM | the 21 s crash (L3: 7034); the service gone after a restart (L4); login (L4); the turn-off (L7) and its restart counter (L3) |
| `./dev vm` broker (lease, preflight, headless, no restored `.vmx`); positive controls; failure classes; C97 | one mechanism, release-gate rule, source check | the two Windows VM collisions, the restored `.vmx`, the stale-lock dialog, and check bugs reported as product failures |
| C93 over app and shared-crate edges, with caps; `service_main` | source check without baseline; one mechanism | `winsvc.rs:22`; `native.rs:392`, `:402`, `:403` (main); `systemd.rs:188`; `tray-icon/.../windows/mod.rs:84` |
| ESLint `no-floating-promises`, `paddock/no-silent-catch` | lint | an adapter that drops an `invoke` failure |
| `contract_tests!` with `TestRoot`; recordings for scripted doubles; C94 and C95 | shared kit plus enumerating checks | the fake rooted at `/`; the rig's copy of auth; canned `systemctl` output |
| commit-lint format, `./dev red` notes, pre-push check, nightly sample | lint, tool, re-run | `74e57f519`, `b8628bc21`, `13bf96710` |
| `defineLayoutMatrix`, `runLayoutMatrix`, C96 | shared kit plus enumerating check | the chooser at 900 px with its preview column, on each tray capability |

Each new check ships with a fixture it fails on (the 0046 draft's demand). The fixtures are synthetic reductions of:
- `winsvc.rs` and `systemd.rs:188` at `afc0106b2` (C93);
- an undeclared `Shell_NotifyIconW` call and an undeclared packaged unit (C92);
- a fake rooted at `/` (C94);
- a `feat:` commit with a free-text parenthesis (commit-lint);
- the chooser without a matrix (C96).

## 7. Work packages and order

Sizes are agent-days (estimates, not measurements). Owned paths are the files a package changes. Shared integration points
(`crates/*/src/lib.rs`, `crates/Cargo.toml`, `.gitea/`) take one declared line each, from the integrator. `scripts/vm-smoke`
is a harness (ADR 0042): L-4 lands from its home, coordinated with ADR 0053's S9, which uses the same files.

### 7.1 What 1.1.0 needs from ADR 0052, and the release window (P1-2)

**Pulled into 1.1.0: a slice of ADR 0052's WP-A1, as package A1-s.** It is the `wiring.toml` reader in `./dev`, limited to:
- `[[os_registration]]` and `[edges]` (this ADR's sections);
- `[caps] silent`;
- the `Escape-hatch:` trailer count.

It is sized at 1–1.5 days and done by the agent of L-5/E-3, which needs it first. WP-A1's owner reviews it, and WP-A1 later
adds `[[unwired]]` and C83 to the same reader and file. Why pull it rather than keep the declarations in `vm-smoke`'s Dyno
profile:
- a profile would be a second register, migrated later (rule 55's spirit);
- C92 and C93 must read the declarations from the source tree;
- the reader is tooling (`tools/paddock-dev`) and not shipped code, so ADR 0022 G.7's hold on shared changes in a release
  window does not reach it.

**Not pulled: ADR 0052's WP-B1** (`ReadBack`, `NextStep`, the `JobState` change).
- It is a breaking change to `paddock-jobs` across 91 non-test files in 29 crates (ADR 0052's count), which G.7 keeps out of
  a release window.
- 1.1.0 does not need it: the extended `Inspection` is the read-back evidence, and `RegisterError` and `UnregisterError`
  carry the reason.
- Cost moved to WP-B1: about 0.5 day, for `ReadBack` to wrap `Inspection` and `NextStep` to replace the error texts.

**The release window (G.7, rule 43).** The 1.1.0 `crates/` changes are:
- `paddock-service`: the `Backend` change, `service_main`, the `systemd.rs` fix, the login move;
- `paddock-proc` and `paddock-ipc`: drop fixes only, with no API change.

They are Dyno's own release path (rule 54 (a)), and they land before Spotter's window opens. `Manager`'s public API stays
as it is: the inspection flows into its existing result types. Its callers (`paddock-shell`'s setup items, and the
`service_repair.rs` of Dyno, Pitcrew and Pitwall) are therefore checked, not rewritten. Any caller that must change is
changed in the same commit (rule 43), and the register's copies of `service-registration` are checked or waived (rule 55).

### 7.2 Root causes first (P1-6; ER-27, ER-28)

Each package below adds the logging that shows the flow, reproduces on a VM, lists the plausible causes with the observation
that rules each out, and fixes with a failing test first. All three land before the L-4 gate run.

| WP | What | Owner | Owned paths | Days |
|---|---|---|---|---|
| RC-1 | The Windows helper crash 21 s after install, and the service gone after a restart | **the agent on `fix/dyno-helper-after-restart`** (worktree `/Volumes/d7sst7/dev-targets/wt/dyno-helper-boot`). When read on 2026-11-04 the branch was **not pushed**: local commits `25eea905e`, `b47f8ba4d` and `3fd9797ee`, plus uncommitted changes to `winsvc.rs`, `logfile.rs`, `serve.rs` and `setup.rs`. Its pushed commits are cited here once they exist. This ADR adds only its acceptance: an ER-28 cause list in the commit, a failing test, and a green L3/L4 on the Windows VM. **E-2 starts after it lands and rebases on it** | that branch's files | tracked on `fix/dyno-helper-after-restart` |
| RC-2 | The Linux turn-off: a failing test for `Systemd::unregister` with `disable --now` failing on a packaged unit, the fix of §1.1, and the reason `Step::ConfirmGone` passed (which path Turn off took, and what `inspect` returned), confirmed in the VM's journal | the L-1a agent | `crates/paddock-service/src/backend/systemd.rs`, `crates/paddock-service/src/privileged/install.rs` and their tests | 1–1.5 |
| RC-3 | The login writers of `91d703d81` on real VMs: Windows `Run` plus `StartupApproved`, GNOME autostart, macOS `SMAppService.mainApp`, with L2 and L4 rows run before the full suite; each failure fixed with a failing test | the L-3 agent, after ADR 0053's S5 | Dyno's login adoption files (L-3) and `login.rs` only after P1-7's hand-over | 1.5–2 |

### 7.3 Into Dyno 1.1.0 (patterns 1 and 2, the full set)

| WP | What | Owned paths | Depends on | Days |
|---|---|---|---|---|
| A1-s | The `wiring.toml` reader slice (§7.1), and the one `// silent:` counter that C93 and C87 share | `tools/paddock-dev/src/wiring.rs` | – | 1–1.5 |
| L-1a | `Backend`'s `register`/`unregister` return the inspection; `UnregisterError`; typed `Job::Failed`, `LastExit`, `restarts`, `UnitFile` for SCM, systemd and launchd; event-driven waits; `Step::Confirm`/`ConfirmGone` on the returned inspection; checks of `Manager`'s callers and the register's copies | `crates/paddock-service/src/backend/{mod,systemd,launchd,windows}.rs`, `crates/paddock-service/src/{manager.rs,repair.rs,privileged/install.rs}` | RC-2 in the same branch | 3–4 |
| L-1b | The login writers behind `Backend` (`WindowsRun` with the elevation rule, `XdgAutostart`, main-app `LaunchdBundled`); `Autostart` removed | `crates/paddock-service/src/login.rs`, `crates/paddock-service/src/backend/` (new XDG backend) | ADR 0053 S5 landed (P1-7); L-1a | 1.5–2 |
| L-2 | Tray read-back: every `Shell_NotifyIconW` result checked and logged, `Shell_NotifyIconGetRect` after add, macOS and Linux visibility read-back, the tray's placement in diagnostics | `apps/dyno/app/src-tauri/src/tray.rs`, `apps/dyno/vendor/tray-icon/src/platform_impl/windows/mod.rs`, `apps/dyno/vendor/PATCHES.md` | – | 1–1.5 |
| L-3 | Dyno adopts: the helper service, login entries, helper folder ACLs and the helper pipe declared; the switch and the helper card bound to inspections with the mismatch sentence; the diagnostics list | `apps/dyno/wiring.toml`; `apps/dyno/app/src-tauri/src/{login.rs,login_item.rs,service_repair.rs,shell.rs}`; `apps/dyno/crates/dyno-helper/src/setup.rs`; `apps/dyno/app/src/app/features/settings/{helper-card.ts,helper-model.ts}`; `apps/dyno/app/src/app/state/helper.store.ts`; `apps/dyno/app/src/app/core/mock/helper.ts` (P2: owned paths) | A1-s, L-1a, L-1b; ADR 0053 package D's commit to Dyno's `login.rs` | 2.5–3.5 |
| L-4 | The lifecycle suite: L1 to L8 driven by `[[os_registration]]`; probes per kind and OS; the crash counters of L3; L6 from the last released tag; `--record` for §3.3's recordings; PM-1 to PM-19, PM-5b and PM-12b as the Windows per-machine rows; README and testing-manual page | `scripts/vm-smoke/{lifecycle.mjs,lifecycle.test.mjs,probes/}`, `scripts/vm-smoke/README.md`, the Linux image script | ADR 0053 S9's accounts and UAC automation (shared; whichever lands first builds them) | 4.5–5.5 |
| L-5 | C92 (source and packaging), with its fixtures | `tools/paddock-dev/src/checks/c92_os_registrations.rs`, fixtures | A1-s | 1.5–2 |
| L-6 | Rule 54 (d): `./dev release tag` reads the lifecycle results; the tag checklist text | `tools/paddock-dev/src/release/` | L-4 | 0.5 |
| E-1 | `service_main` with OS-log sinks (the event source in the install plan, `StandardError=journal`, `os_log`), checked status reports, the exit-code table, the panic boundary; `native::scm::run_as_service` replaced and its callers moved (rule 43); the `service-main` register entry updated | `crates/paddock-service/src/{service_main.rs,os_log/,native.rs,render.rs}`, `docs/known-copies.toml` | – | 2.5–3 |
| E-2 | Dyno's helper adopts `service_main` on all three OS; `winsvc.rs` deleted; the deferred file log sends its pre-trust lines to the OS log | `apps/dyno/crates/dyno-helper/src/{winsvc.rs,main.rs,cli.rs,logfile.rs}` | E-1, RC-1 landed | 1 |
| E-3 | C93 over app and shared-crate edges, the caps, the inner `deny` attribute; `crates/wiring.toml` with `[edges]`, and `[caps] silent` set from its first report run after E-4 | `tools/paddock-dev/src/checks/c93_edges.rs`, fixtures, `crates/wiring.toml` | A1-s | 1.5–2 |
| E-4 | Fix every C93 finding: in `paddock-service` (65 `let _ =`, 1 `.ok();` as upper bounds), `paddock-proc` (26, 3), `paddock-ipc` (27, 1), and in Dyno's edge modules (upper bounds 79, 17, 44, 128 in its four Rust folders; most of `dyno-agent`'s are expected outside the edges) | the edge modules C93 lists | E-3, E-1 (so `native.rs` is fixed once) | 3–5 |
| E-5 | ESLint `no-floating-promises`, `no-misused-promises`, `paddock/no-silent-catch` for Dyno's backend adapters and the shell adapters | `eslint/src/rules/no-silent-catch.mjs`, the preset's adapter globs, findings in `apps/dyno/app/src/app/core/` | – | 0.5–1 |
| V-1 | The `./dev vm` broker: leases (replacing the `plock` `vm-lock`), preflight, headless start, revert and stop for VMware Fusion and Tart, the one `.vmx` edit, refusal of unleased running VMs, lease ids in results; C97 with its fixture | `tools/paddock-dev/src/vm/{mod,lease,preflight,fusion,tart,vmx}.rs`, `tools/paddock-dev/src/checks/c97_vm_broker.rs` (+ the command and check list lines) | – | 1.5–2 |
| V-2 | Positive controls for every lifecycle check and the `product`/`check` classification with evidence in the judge and the result file | `scripts/vm-smoke/{judge.mjs,checks.mjs,lifecycle.mjs}` and their tests | V-1; with L-4 | 1–1.5 |
| V-3 | Migration onto the broker: `scripts/vm-smoke/run-win.mjs`, `run-linux.mjs`, `vmx-edit.mjs` (removed), `vm-smoke.sh`, `scripts/vm-win.sh` (on `origin/tools/win-test-vm`), and ADR 0053 S0's spike scripts; README | those files; `scripts/vm-smoke/README.md` | V-1 | 0.5–1.5 |
| **Dyno 1.1.0 total** | (with RC-2, RC-3 and V-1 to V-3) | | | **29.5–41** |

**The 1.1.0 estimate.** Revision 1 said 19 to 25 days; revision 3 said 26.5 to 36. The 29.5 to 41 here adds:
- A1-s;
- RC-2 and RC-3;
- L-1b, now its own package after ADR 0053's S5;
- the shared crates in C93 and E-4 (118 `let _ =` and 5 `.ok();` more to judge);
- E-1's second entry point;
- the crash counters, the recordings and L6's start from the released tag in L-4;
- the VM broker, positive controls and the runners' migration (V-1 to V-3, 3 to 5 days; revision 4).

It does not include RC-1, whose estimate is tracked on `fix/dyno-helper-after-restart`.

**The order and the critical path.**
- Start in parallel: RC-2 with L-1a, E-1, A1-s, L-2, L-4 and V-1.
- V-2 with L-4; V-3 after V-1. The broker is ready by about day 2 and the runners moved by about day 4, before the first
  lifecycle run, so V-1 to V-3 are off the critical path.
- Then E-3 and L-5 (after A1-s); E-4 (after E-3 and E-1); L-1b (after ADR 0053's S5 and L-1a); L-3 with RC-3 (after
  L-1b and package D's `login.rs` commit); E-2 (after RC-1); then L-6.
- The critical path is L-1a (3–4) → L-1b (1.5–2) → L-3 with RC-3 (4–5.5) → the lifecycle runs on three OS (1.5–2.5): about
  **10 to 14 working days with four agents.** It holds on two conditions:
  - ADR 0053's S5 (1 to 1.5 days, most of it in `91d703d81`) lands before L-1a ends, about day 3 to 4. This is likely,
    since S5 has no dependency.
  - Package D's commit to Dyno's `login.rs` lands before L-1b ends, about day 5 to 6. D is 7 to 10 days, so its login
    step must come early in D's order. If it comes later, L-3 starts later and the path slips by the same number of days.
    The coordinator orders D's login step first.
- This is on top of ADR 0053's Dyno work (its own 3 to 4 weeks). The two share the VM set-up and runs, so the calendar time
  of both is less than the sum.

### 7.4 After the Dyno 1.1.0 tag (family-wide)

| WP | What | Owned paths | Depends on | Days |
|---|---|---|---|---|
| A-1 | Patterns 1 and 2 in Spotter, Pitwall, Pitcrew: registrations, edges, lifecycle rows, `service_main` for their services | each app's `wiring.toml`, edge modules, service entry points | 7.3 | 2–3 per app |
| F-1 | `paddock_testkit::contract` (`Contract`, `contract_tests!`, `TestRoot`) | `crates/paddock-testkit/src/contract.rs` | – | 1.5 |
| F-2 | `fakes.toml` registers and C94 (Rust and TypeScript, the `script` kind and recordings) | `tools/paddock-dev/src/checks/c94_doubles.rs`, `apps/*/fakes.toml`, `crates/fakes.toml` | F-1, L-4 `--record` | 2–2.5 |
| F-3 | `@imitates` and C95 over `sync_generated` goldens | `tools/paddock-dev/src/checks/c95_goldens.rs` | – | 1 |
| F-4 | Contracts for `Backend` (fakes in CI; real in the VM), Dyno's trust ports (real on each CI OS in a `TestRoot`), the helper rig, Dyno's mock; then the other apps | per crate and app | F-1 to F-3 | Dyno 2–3; others 1 each (4) |
| R-1 | commit-lint: the trailer format, every commit that carries it, `Failing-first correction:` | `conformance/src/commit-lint.mjs`, `conformance/test/c23-commit-lint.test.mjs` | `origin/ci/failing-first-trailer` landed | 0.5 |
| R-2 | `./dev red` (run, require red, note with patch-id and patch, `--attach`, `notes.rewrite*`), notes pushed and fetched | `tools/paddock-dev/src/red.rs` | R-1 | 2 |
| R-3 | The pre-push note check; nightly `--verify-sample 5` with a printed seed, measured and recorded in `docs/benchmarks.md` | `tools/paddock-dev/src/check/pre_push.rs`, a `.gitea/workflows/nightly.yml` step (integrator) | R-2 | 1–1.5 |
| M-1 | `defineLayoutMatrix`, the gallery routes, `runLayoutMatrix` with pairwise reduction | `ui/projects/paddock-ui/testing/layout-matrix.ts`, `ui/projects/paddock-e2e/src/layout-matrix.ts`, the gallery's route generator | – | 2 |
| M-2 | C96 (narrowed), and the affected-plan mapping from a component to its matrix | `tools/paddock-dev/src/checks/c96_layout_matrix.rs`, `scripts/affected.mjs` (and its Rust port) | M-1 | 1.5 |
| M-3 | Matrices: the widget chooser first, then each component C96 lists | beside each component | M-1, M-2 | 2–4 |
| **Family total** | | | | **about 25.5–32.5** (with three apps under A-1) |

R-1 to R-3 touch no product and no release path, so the coordinator may land them before the Dyno tag if an agent is free.
M-1 sits in a harness home in `ui/` (ADR 0042 §1) and waits for the release window to close.

**Rollback.** Each check is its own module and can be reverted alone. L-1a changes a port with one adopter of the new
fields in 1.1.0, so reverting it reverts L-3's bindings. `service_main` replaces two entry points; reverting E-1 restores
both, and with them the register entry.

## 8. Waivers (bounded by ADR 0052's rules: at most the next release-line tag or 60 days, a dated trailer to move)

| Waiver | Until (delete by) | Why |
|---|---|---|
| macOS lifecycle L2's Login Items approval and L5 (second user) are MANUAL rows for Dyno | `dyno-v1.2.0` or **2027-01-03**, whichever is first | the Tart macOS VM runs `--no-graphics`; approval needs a click in System Settings, and a second macOS GUI session is not automated yet |
| The lifecycle gate does not block apps other than Dyno and Spotter | each app's next release-line tag after `dyno-v1.1.0`, at latest **2027-01-03** | A-1 adopts it per app; Spotter is gated from `spotter-v1.0.0` (D6) |
| C92 and C93 (app edges) do not block apps other than Dyno, Spotter included | each app's next release-line tag after `dyno-v1.1.0`, and for Spotter its next after `spotter-v1.0.0`; at latest **2027-01-03** | A-1 adopts them per app; D6 gates only Spotter's lifecycle run |
| ESLint `paddock/no-silent-catch` in apps other than Dyno | the same | the same |
| C94, C95, C96 are report-only | **2027-01-03** | F-* and M-* land after the tag; each enforces when its adoption WP lands |

C93 over the shared crates on Dyno's release path has no waiver: E-4 fixes them in 1.1.0.

## Parallelism and cost (rule 51)

- **The checks.** C92 to C96 and commit-lint are source checks with no build. They run in ADR 0049's no-build push check
  and in `./dev check --pre-push`, each only when its inputs change. C92 and C93 share one `syn` parse per app and crate set
  with ADR 0052's C83. Their cost is measured in L-5 and E-3 and recorded in `docs/benchmarks.md` before anyone quotes it
  (rule 48).
- **The lifecycle suite** is the expensive part: it reboots each VM and signs in a second user. It runs once per release
  per OS, locally, and never in CI.
  - More parallelism: the three OS's VMs can run at once, one broker lease each, within `./dev ci slot`'s VM slots and the load cap (rule 41); a lease never shares a VM; L4
    and L5 are serial within one VM.
  - Cheaper: one install per OS for L1 to L8; L3's counters are read rather than waited for; L6 reuses the released
    installer from the release feed.
  - Its run time is not measured yet; L-4 records it.
- **`./dev red`** runs one named test. The nightly sample is five tests within a 15-minute budget (§4).
- **The layout matrices** run in Chromium in the affected plan for the components a change reaches, and the full set runs
  nightly. The chooser's matrix is 4 × 4 × 4 × 2 = 128 rows, under the pairwise threshold.

## Consequences

- Services and login entries keep one port (`Backend`), and its write calls now say what the OS did. A user sees "Windows
  has no start entry for Dyno" instead of a switch that is on and does nothing.
- A release of an app with a service or login entry needs a VM pass with reboots, a second user and an update from the
  last release on each OS. That costs hours of a person's or an agent's time per release, which is the price of finding
  these before the user does.
- Edge modules in apps and in the shared crates lose `let _ =`. Deliberate drops say why, and count against a cap.
- A service's failure is readable in Event Viewer, in `journalctl -u` or in Console, without the app's own log.
- Fakes become a small, enumerated set with contracts or recordings.
- Writing a failing-first claim costs one `./dev red` run, and a false claim becomes detectable.
- Shared components with variants carry a matrix file each.
- **Breaking before v1 (no shims):**
  - `Backend::register` and `unregister` change their return types;
  - `Inspection` gains fields;
  - `paddock_service::login::Autostart` goes;
  - `native::scm::run_as_service` and Dyno's `winsvc.rs` go;
  - the `Failing-first:` format of the branch changes before it lands.

## Alternatives considered

- **A new `os_reg::Registration` trait (revision 1).** It would be a second implementation of the registered
  `service-registration` mechanism (rule 55). Rejected: the existing port is extended instead.
- **Keep the declarations in `vm-smoke`'s Dyno profile for 1.1.0.** It is a second register to migrate, and the source
  checks cannot read it. Rejected for the WP-A1 slice (§7.1).
- **Pull ADR 0052's WP-B1 into 1.1.0.** A breaking change across 29 crates inside a release window (G.7), which 1.1.0 does
  not need. Rejected.
- **Unit tests against richer fakes only.** The fakes were the problem. Rejected.
- **Run the lifecycle on CI runners.** It changes a shared machine's services and login entries (ER-14), and a runner
  cannot be rebooted. Rejected.
- **A time-based settle.** A wall clock (ADR 0031 rejects them), and it misses a crash after the window. L3 reads the OS's
  counters over the whole run instead.
- **Clippy `let_underscore_must_use` family-wide now.** 1,816 sites on main, most harmless. ADR 0052 adds it with a
  baseline; this ADR enforces it at the edges only.
- **A new `// discard: <why>` hatch.** Rejected for ADR 0052's `// silent: <why>`: one hatch, one count, one cap (D2).
- **Keep the failing-first evidence in the message only, or in CI artifacts.** A message cannot be re-run, and artifacts
  expire and do not exist for local runs. Rejected for git notes, with rewrite rules (D3).
- **Pixel diffs of the layout matrix.** Brittle across fonts and OS builds. Rejected, as in ADR 0052.
- **Fix the instances only.** Each shape came back within one day. Rejected as the whole answer (ER-21).

## Decisions

| # | Question | State |
|---|---|---|
| D1 | Pattern 2 as its own rule or a clause of rule 67 (b) | **Decided: fold into rule 67 (b) and amend ER-27** (coordinator, 2026-11-04, following the review) |
| D2 | The escape hatch for a deliberate drop | **Coordinator default (the reviewer's recommendation), user may revise:** ADR 0052's `// silent: <why>`, one counter with C87 |
| D3 | Where red-run logs live | **Coordinator default (the reviewer's recommendation), user may revise:** git notes under `refs/notes/failing-first`, rewritten on rebase and amend, with CI run references also allowed |
| D4 | Pattern 4 as a new rule or an amendment | **Coordinator default (the reviewer's recommendation), user may revise:** amend rule 5 and ER-11 |
| D5 | Dyno 1.1.0 scope | **Decided: the full set** (the user, 2026-11-04), with the VM broker (revision 4): 29.5 to 41 days of packages in §7.3, critical path about 10 to 14 working days with four agents. **The scope is frozen** (the user, 2026-11-04): only bug fixes for what is already in scope; new ideas go to 1.1.1 or 1.2 |
| D6 | Spotter 1.0 under the lifecycle gate | **Coordinator default (the reviewer's recommendation), user may revise:** yes, the lifecycle run gates `spotter-v1.0.0`; C92 and C93 are not enforced for Spotter before that tag (§8) |
| D7 | Where registrations are declared | **Coordinator default (the reviewer's recommendation), user may revise:** `wiring.toml`, through the WP-A1 slice A1-s pulled into 1.1.0 |
| D8 | Real-adapter contract runs for registration ports | **Coordinator default (the reviewer's recommendation), user may revise:** in the lifecycle VM only (ER-14) |
