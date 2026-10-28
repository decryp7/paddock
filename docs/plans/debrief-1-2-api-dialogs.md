# Debrief 1.2 API design, dialogs section (package 0.5 for packages 1.1 and 1.2)

The design is [ADR 0026](../adr/0026-shared-dialog-api.md) (Proposed). This file holds two things:

- the per-app dialog lists with the kind each one maps to;
- the work packages that replace rows 1.1 and 1.2 of [debrief-1-2.md](debrief-1-2.md).

Inventories were read on 2026-10-28:

- Dyno on the local branch `dyno/monitoring-only` (it has not been pushed);
- Spotter and Pitwall on `origin/main`;
- Pitwall's old repository on branch `feat/shell`;
- Debrief on `apps/debrief/app/src/app/dialog-windows/dialog-registry.ts`.

## 1. Dialogs per app and their kind

Widths are the named steps `narrow` (400), `medium` (560) and `wide` (720). "Actions" lists the declared
`dialogActions`:

- **P**: primary
- **D**: destructive
- **C**: cancel or close
- **S**: secondary
- **lead**: placed at the leading edge

### Shared (`paddock-ui`, 13 today plus one new)

| Id | Kind | Modality | Width | Actions |
|---|---|---|---|---|
| `pd.confirm` (new, §5 of the ADR) | decision | modal | narrow | C; P or D (`confirm.destructive`) |
| `pd.about`, `pd.whats-new` | viewer | modeless | wide | C |
| `pd.updates`, `pd.settings`, `pd.setup`, `pd.wb.dock.layout-manage` | tool | modeless | 960 x 680 (setup 560 x 520) | C; status line |
| `pd.channels.secret`, `pd.mcp.key`, `pd.wb.dock.layout-save` | form | modal | narrow | C, P |
| `pd.channels.remove`, `pd.mcp.revoke`, `pd.storage.confirm` | decision | modal | narrow | C (Enter), D |
| `pd.report-problem` | decision → **form** (it has inputs) | modal | wide (680 today) | C, P |

### Pitwall 1.1 (5 own plus shared; from `feat/shell` and `origin/main`)

| Today | Becomes | Kind | Actions | Notes |
|---|---|---|---|---|
| `pw-confirm-dialog` / `pw.confirm`, 11 uses: revoke key, delete app, clear cache, remove or roll back an engine, delete models or presets, download update | `pd.confirm` | decision | C; D for deletions, P for roll back and download | The download starts a job; the dialog closes with the choice |
| `pw-create-app-dialog` / `pw.create-app` | `pw.create-app` | form, modal, narrow | C, P "Create app" (disabled until valid) | Focus goes to the name field (`feat/shell` 6148b83 removed autofocus: restore it through the frame) |
| `pw-key-secret-dialog` / `pw.key-secret` | `pw.key-secret` | decision, `requiredChoice` | S lead "Copy" (`run`), P "I've saved it" | No Cancel; Escape does nothing |
| `pw-quit-dialog` / `pw.quit` | `pw.quit` | decision | C, S "Quit and stop serving", P "Quit, keep serving"; checkbox in the body | A second request focuses the open window (single instance) |
| `pw-service-consent` / `pw.service-consent` | `pw.service-consent` | decision | C "Not now", P "Run in the background"; "Don't ask again" in the body | The install runs as a job after the choice |
| `pw-slots-panel` (anchored, `role=dialog`) | stays an in-app popover | not a dialog | Close, Use the default, Save N | Rule 17: anchored panels stay in-app |
| settings, about, What's new, updates | `pd.*` | tool / viewer | | Shared |

### Spotter 1.0

| Today | Becomes | Kind | Actions |
|---|---|---|---|
| `sp-confirm-dialog` (delete watch, 2 call sites) | `pd.confirm` | decision | C (focus), D "Delete watch" |
| Editor wizard (`features/editor`) | stays a page | not a dialog | |
| Folder and file pickers | stay native | not a dialog | |
| Tray popup | stays its own window | not a dialog | |
| settings, about, updates | `pd.*` | tool / viewer | |

### Dyno 1.0 (`dyno/monitoring-only`)

| Today | Becomes | Kind | Actions |
|---|---|---|---|
| `dy-confirm-dialog`, 9 uses (end process, delete alert rule, forget programs, remove helper, ...) | `pd.confirm` with `details` | decision | C, D or P |
| `alarms/rule-dialog` (alert rule) | `dy.alert-rule` | form, modal, medium | C, P "Add rule" / "Save" |
| `dy-lockers-dialog` (what is locking a file) | `dy.lockers` | **tool** (live search, a list, End per row through `pd.confirm`) | C; P "Find" in the body |
| `dy-web-search-dialog` | `dy.web-search` | form, modal, narrow | C, P "Search" |
| `dy-hash-search-dialog` | `dy.file-hash` | **viewer** (hashing streams in) | S lead "Copy", S lead "Try again", C, P "Open VirusTotal" (`run`) |
| `dy-helper-dialog` | `dy.helper-install` | decision | C "Not now", P "Install"; the OS prompt and install run as a job |
| `dy-coming-dialog` (planned features) | `dy.coming` | viewer | C (it has none today, only the X) |
| `dy-command-palette` | stays in-app | not a dialog | |
| `rules/rule-dialog`, `dy-temporary-dialog`, `dy-affinity-dialog`, `dy-cap-dialog` | **open question Q3**: delete with automation and process changes, or form modal (wide / narrow / narrow / narrow) | form | C, P |
| Orphans: `dy-cleanup-confirm-dialog`, `dy-fix-dialog`, `dy-tuning-dialog`, `dy-add-folder-dialog`, `dy-preview-dialog` | deleted with their removed backends | | |
| Tray popover, native Save and folder pickers | unchanged | not dialogs | |

### Debrief 1.2 (50, in package M2b)

These keep their ids. Kinds as registered today, with the five fixed-height forms decided here so that M2b does not
have to.

| Kind | Ids |
|---|---|
| decision (12) | parser-import-confirm, package-review, plugin-permissions, marker-override, encoding, open-live, unsaved-changes, recent-projects, open-ask, close-prompt (`requiredChoice`), import-range, save-layout |
| form, modal (9) | drop, mark, infer, mask, extract, paste, add-tile, redaction, **ai-setup** (fitted, medium; the download is a job) |
| form, modeless (18) | parser-editor, clock, collector, note-editor, search-web, folder-setup, knowledge-editor, profile-native, profile-etl, profile-dotnet, report-problem (→ `pd.report-problem`), alert-rules, export, manage-layouts, report-export, **format-wizard** (fitted, wide, stepper in the header slot), **ai-propose** (fitted, wide), **redacted-copy** (fitted, medium; preview capped and scrolling in the body) |
| viewer (7) | about (→ `pd.about`), updates (→ `pd.updates`, a tool), cheat-sheet, incident-summary, ai-narrative, ai-proposal-result, ai-regex |
| tool (4) | settings, research, ai-models, **ai-query** (from fixed-height form: user-sized, results stream) |

Notes:

- The table counts 51 rows across the kinds because `report-problem` and `about`/`updates` are listed with the shared
  dialogs that replace them. The registry has 50 ids.
- ADR 0016's "28 forms" includes ai-query, which moves to tool here.
- `unsaved-changes` and `close-prompt` use Save / Cancel / Don't save, with D lead.

## 2. Work packages (replace rows 1.1 and 1.2)

Every package starts with a failing test that is committed red, with the failure pasted into the PR. All of them use
heavy.sh, `CARGO_BUILD_JOBS=2` and their own target directory.

Order: 1.1a → (1.1b ∥ 1.2a) → 1.2b → 1.2c. After 1.2c, Pitwall, Spotter and Dyno can adopt in parallel with Debrief's
M2b.

| WP | What | Files (shared UI / crates) | Failing test first | Role | Depends | Days |
|---|---|---|---|---|---|---|
| **1.1a** | **Contract in Rust.** <ul><li>`dialogs!` macro: command function paths, const checks of kind/modality and of `DIALOG` in the command table.</li><li>`Contracts::from_tables`.</li><li>Generated `dialog-kinds.ts` and per-owner `dialog-contracts.ts`.</li><li>`defineDialog(contract, ui)`.</li><li>Move the 13 `pd.*` contracts and the demos' 7.</li><li>Delete `serializeDialogContracts`, `dialogContractFile`, the JSON file and its parse path.</li></ul> | `crates/paddock-dialogs/src/{contract,table,generated}.rs`, the `dialogs.rs` of `paddock-shell`/`-channels`/`-mcp`/`-storage`/`-workbench`, `ui/projects/paddock-ui/shell/{src/define.ts,dialogs/src/{kind,contracts}.ts,generated/}`, shell-demo and workbench-demo `src-tauri` | <ul><li>`kind.spec.ts`: for every kind x {height, no height}, TS `fitsContent` equals the generated `fits`. Fails today on the viewer.</li><li>`trybuild` case: a dialog naming a command without `DIALOG` fails to compile.</li></ul> | implementer (with this spec) | ADR 0026 accepted | 2 |
| **1.1b** | **Sizing and the window lifecycle.** <ul><li>Minimum content as `set_min_size`.</li><li>Remembered size clamped (decisions and forms keep offset and width only).</li><li>Cap never below the minimum.</li><li>Logical to physical rounded up.</li><li>Title bar = outer − inner.</li><li>Create hidden; `dialog_ready`/`dialog_fit {content, minContent}`; shown on ready.</li><li>Watchdog shows a dialog that is not ready.</li><li>`ScaleFactorChanged` refit.</li></ul> | `crates/paddock-dialogs/src/{placement,window,tauri}.rs`, `crates/paddock-startup/src/watchdog.rs` (dialog labels), `shell/dialogs/src/io.ts` | <ul><li>Property test (scale 1-3, work area 800x600 to 7680x4320, any remembered size): placed size in [min content, work area], and physical back to logical ≥ request. Fails today on rounding and on the remembered size below the content.</li><li>Mock-runtime test: a dialog is not visible before `dialog_ready`.</li></ul> | implementer | 1.1a | 2 |
| **1.2a** | **DialogFrame.** <ul><li>Header slot, body, `flex: none` footer.</li><li>`DialogAction`, `layoutActions` (pure).</li><li>Enter, Escape and focus moved from `keys.ts` into the frame; busy state; focus return in `DialogService`.</li><li>Measuring wrapper and refit triggers (one per frame).</li><li>`dialogStatus`, `dialogMinBodyHeight`.</li><li>`hasCloseAction`/`findDefaultAction` heuristics deleted.</li></ul> | `ui/projects/paddock-ui/shell/dialogs/src/{dialog-frame,actions,dialog-shell,keys,dialog.service}.ts` | <ul><li>`actions.spec.ts` table (order, counts, Enter never destructive, fallback Close).</li><li>Playwright on shell-demo `sd.rename` at deviceScaleFactor 1.5 in a 1366x768 window: the footer buttons are hit by `elementFromPoint` after the content grows by 400 px and shrinks back, gap ≤ 24 px. Fails today (grow-only, unpinned footer).</li></ul> | designer | 1.1a | 2.5 |
| **1.2b** | **Test kit and checks.** <ul><li>Harness additions (ADR §6), on top of `fix/dialog-boot-flake` (merged first).</li><li>`FakeDialogIo` ready and fit.</li><li>`expectDialogFits`, `openEveryDialog`, `measureDialogWindow` in `paddock-e2e`.</li><li>`dialog-commands-seen.json` and the `unexercised` reasons check.</li><li>Lint `paddock/dialog-frame-footer`.</li><li>Conformance: visited ids = contract entries.</li></ul> | `ui/projects/paddock-ui/shell/testing/src/`, `ui/projects/paddock-e2e/src/dialogs/`, `eslint/src/rules/dialog-frame.cjs`, `conformance/src/dialogs.mjs` (or the `paddock-dev` rule when phase 4 lands) | <ul><li>The lint fails on a planted `.foot`.</li><li>The harness reports "draws its own footer" on today's `channel-remove-dialog`.</li><li>`expectDialogFits` fails on a planted fixed-height form at 1.5.</li></ul> | implementer | 1.2a, 1.1b | 1.5 |
| **1.2c** | **Adoption by the shared and demo dialogs.** <ul><li>`pd.confirm` (new).</li><li>13 `pd.*` + 5 shell-demo + 2 workbench-demo declare actions and lose their footers.</li><li>`pd.report-problem` becomes a form.</li><li>Screenshots of every kind in both themes for the independent review (rule 22).</li></ul> | `ui/projects/paddock-ui/shell/{about,channels,mcp,storage,diagnostics,setup,settings,confirm}/`, `workbench/dock/src/layout-dialogs.ts`, `ui/projects/shell-demo/`, `ui/projects/workbench-demo/` | 1.2b's checks over all 20 (red until each dialog is moved); `expectDialogFits` over 20 at 3 scales x 2 work areas in Chromium | implementer | 1.2b | 1.5 |

In total that is 9.5 agent-days, against 4.5 in the plan. The critical path 1.1a → 1.2a → 1.2b → 1.2c is about
7.5 days.

### Adoption packages (after 1.2c, one per app, sized for its release)

| WP | App | Work | Days |
|---|---|---|---|
| PW-D | Pitwall 1.1 | `src-tauri/src/dialogs.rs` (4 own); `pd.confirm` for 11 call sites; quit, consent and key-secret actions; `expectDialogFits` over its contract | 1.5 |
| SP-D | Spotter 1.0 | `pd.confirm` for 2 call sites; delete `confirm-dialog.ts`; shared settings, about and updates; checks | 0.5 |
| DY-D | Dyno 1.0 | 6 dialogs + `pd.confirm` for 9 call sites; lockers as a tool; delete the orphans and the `Dialogs` service and `dy-dialog`; checks | 2 (+1 if Q3 keeps the four rule and process dialogs) |
| M2b | Debrief 1.2 | As planned, plus moving the 50 command lists to `src-tauri/src/dialogs.rs` and deleting the TS analysis | 3.5 + 0.5 |

## 3. Open questions for the user

- **Q1.** Is one button order for every OS (primary on the right, Cancel to its left: design guide §Dialogs) right?
  The alternative is the Windows order (OK, Cancel) on Windows.
- **Q2.** Should the widths be the design guide's three steps (400 / 560 / 720)? Pitwall's dialogs today are 420-540,
  and Debrief's viewer floor is 680.
- **Q3.** Dyno 1.0 is monitoring-only. Do the automation rule editor, the rule end time, CPU affinity and CPU cap go
  (with their dialogs), or do they stay?
- **Q4.** Should a command that no test reaches be allowed with an `unexercised` reason? This is the ADR 0022 trade-off
  made explicit. The alternative is to require a fixture that drives it.
