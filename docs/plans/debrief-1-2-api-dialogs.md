# Debrief 1.2 API design, dialogs section (package 0.5 for packages 1.1 and 1.2)

The design is [ADR 0026](../adr/0026-shared-dialog-api.md) (Proposed). This file holds two things:

- the per-app dialog lists with the kind each one maps to;
- the work packages that replace rows 1.1 and 1.2 of [debrief-1-2.md](debrief-1-2.md).

Inventories were read on 2026-10-28:

- Dyno on the local branch `dyno/monitoring-only` (it has not been pushed);
- Spotter and Pitwall on `origin/main`;
- Pitwall's old repository on branch `feat/shell`;
- Debrief on `apps/debrief/app/src/app/dialog-windows/dialog-registry.ts`;
- Pitcrew on `origin/main`.

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
| `pd.settings` | tool | modeless | own size 880 x 620 (user, 2026-10-28) | C; status line |
| `pd.updates`, `pd.setup`, `pd.wb.dock.layout-manage` | form (after the visual review, 2026-10-28: their content is short and varies, so the frame measures it up to the cap) | modeless | medium | C (Close), P by phase |
| `pd.channels.secret`, `pd.mcp.key`, `pd.wb.dock.layout-save` | form | modal | narrow | C, P |
| `pd.channels.remove`, `pd.mcp.revoke`, `pd.storage.confirm` | decision | modal | narrow | C (Enter), D |
| `pd.report-problem` | decision → **form**, **modeless** (a text area, a report that loads and saves; the user may look at the app while writing) | modeless | wide (680 today) | C, P "Save report"; after saving, P "Done" |

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

### Dyno 1.0 (`dyno/monitoring-only`, at fabb1fb35)

Dyno is monitoring-only (Dyno ADR 0021, D3). The branch has removed the backends of rules, process actions, undo,
tuning, cleanup and watched folders. Its UI still holds their dialogs and confirms; DY-D deletes them with the move.
Counted on the branch on 2026-10-28.

| Today | Becomes | Kind | Actions |
|---|---|---|---|
| `dy-confirm-dialog`: the 3 call sites that stay (delete an alert rule, `alarms/custom-rules.ts`; remove the helper, `helper/helper-card.ts`; forget the known programs, `issues/unusual-view.ts`) | `pd.confirm` (with `details` for the alert rule) | decision | C, D |
| `alarms/rule-dialog` (alert rule) | `dy.alert-rule` | form, modal, medium | C, P "Add rule" / "Save" |
| `dy-lockers-dialog` (what is locking a file) | `dy.lockers` | **tool**, read-only: a path field and Find in the body, a live list; no End per row (process actions are gone) | C |
| `dy-web-search-dialog` | `dy.web-search` | form, modal, narrow | C, P "Search" |
| `dy-hash-search-dialog` | `dy.file-hash` | **viewer** (hashing streams in) | S lead "Copy", C, P "Open VirusTotal" (`run`); a failed hash is the rejected state with Try again |
| `dy-helper-dialog` | `dy.helper-install` | decision | C "Not now", P "Install"; the OS prompt and the install run as a job |
| `dy-coming-dialog` (planned features) | `dy.coming` | viewer | C (it has none today, only the X) |
| `dy-command-palette` | stays in-app | not a dialog | |
| Tray popover, native Save and folder pickers | unchanged | not dialogs | |

Deleted in DY-D, with their call sites:

- the 6 confirm call sites of removed features: two in `rules/rules-page.ts`, end process in `processes/processes-page.ts`
  and in the lockers list (`processes/process-dialogs.ts`), the act preview in `why-slow/why-slow-page.ts`, and stop
  watching in `watched/watched-page.ts`;
- the dialogs `rules/rule-dialog`, `dy-temporary-dialog`, `dy-affinity-dialog`, `dy-cap-dialog`,
  `dy-cleanup-confirm-dialog` (also used by `extras/extras-files.ts`), `dy-fix-dialog`, `dy-tuning-dialog`,
  `dy-add-folder-dialog` and `dy-preview-dialog`.

### Pitcrew (its next minor on the shell; a fourth copy, ER-22)

| Today (`origin/main`) | Becomes | Kind | Actions |
|---|---|---|---|
| `pc-confirm-dialog` (`shared/ui/confirm-dialog.ts`, 440), 6 call sites: runner actions, service, cache, engine, updates, remove channel | `pd.confirm`; remove channel → `pd.channels.remove` once Pitcrew uses the shared channels | decision | C, P or D |
| `pc-stop-process-dialog` (460): stop a runner or its process | `pd.confirm` with `details` | decision | C (focus), D "End the runner process" / "Stop now" |
| `pc-config-diff-dialog` (760): review config changes before applying | `pc.config-diff` | decision, wide; the diff scrolls in the body | C, P "Save and apply" |

### Debrief 1.2 (50, in package M2b)

These keep their ids. Kinds as registered today, with the five fixed-height forms decided here so that M2b does not
have to.

| Kind | Ids |
|---|---|
| decision (12) | parser-import-confirm, package-review, plugin-permissions, marker-override, encoding, open-live, unsaved-changes, recent-projects, open-ask, close-prompt (`requiredChoice`), import-range, save-layout |
| form, modal (9) | drop, mark, infer, mask, extract, paste, add-tile, redaction, **ai-setup** (fitted, medium; the download is a job) |
| form, modeless (18) | parser-editor, clock, collector, note-editor, search-web, folder-setup, knowledge-editor, profile-native, profile-etl, profile-dotnet, report-problem (→ `pd.report-problem`), alert-rules, export, manage-layouts, report-export, **format-wizard** (fitted, wide, stepper in the header slot), **ai-propose** (fitted, wide), **redacted-copy** (fitted, medium; preview capped and scrolling in the body) |
| viewer (7) | about (→ `pd.about`), updates (→ `pd.updates`, a modeless form), cheat-sheet, incident-summary, ai-narrative, ai-proposal-result, ai-regex |
| tool (4) | settings, research, ai-models, **ai-query** (from fixed-height form: user-sized, results stream) |

Notes:

- The table counts 51 rows across the kinds because `report-problem` and `about`/`updates` are listed with the shared
  dialogs that replace them. The registry has 50 ids.
- ADR 0016's "28 forms" includes ai-query, which moves to tool here.
- `unsaved-changes` and `close-prompt` use Save, Cancel and Don't save. Don't save is a plain outlined button: lead on macOS and Linux; on Windows the order is Save, Don't save, Cancel.

#### Debrief's widths on the three steps (D2)

The registry declares 13 different widths, and 640 is the default for those that declare none, so 14 in all. The rule:
480 and below → `narrow` 400; 481-679 → `medium` 560; 680 and above → `wide` 720. A tool keeps a number.

| Today | Step | Ids |
|---|---|---|
| 420-480 | narrow 400 | save-layout, unsaved-changes, open-ask, close-prompt, marker-override, open-live, import-range, export |
| 520-640, and the 640 default | medium 560 | parser-import-confirm, encoding, recent-projects, manage-layouts, add-tile, report-export, plugin-permissions, clock, search-web, package-review, profile-etl, note-editor, drop, mask, extract, paste, redaction |
| 680 | wide 720 | report-problem (→ `pd.report-problem`) |
| 720-760 | wide 720 | folder-setup, knowledge-editor, profile-native, profile-dotnet, about, incident-summary, collector, mark, infer, format-wizard, ai-setup, ai-proposal-result, ai-propose, alert-rules, redacted-copy, and the viewers without a width (cheat-sheet, ai-narrative, ai-regex) |
| tools, own number | 960 x 680 default | settings, research (720 x 560), ai-models (820 x 640), ai-query (720 x 560), **parser-editor** (820: from form to tool, an editor beside the work) |

Narrowing a 480 form to 400 and a 760 one to 720 reflows them. `expectDialogFits` over the 50 in M2b catches a
footer or field that no longer fits.

## 2. Work packages (replace rows 1.1 and 1.2)

Every package starts with a failing test that is committed red, with the failure pasted into the PR. All of them use
heavy.sh, `CARGO_BUILD_JOBS=2` and their own target directory.

Order: 1.1-0 ∥ 1.1a → (1.1b ∥ 1.2a) → 1.2b → 1.2c. 1.1b needs both 1.1-0 and 1.1a. After 1.2c, Pitwall, Spotter,
Dyno and Pitcrew can adopt in parallel with Debrief's M2b.

| WP | What | Files (shared UI / crates) | Failing test first | Role | Depends | Days |
|---|---|---|---|---|---|---|
| **1.1-0** | **Spike: hidden-until-ready on three webviews** (ADR §4.4). <ul><li>A probe dialog on WKWebView (macOS), WebView2 (Windows) and WebKitGTK (Linux), at scales 1, 1.25 and 1.5.</li><li>Measure: does a hidden window run layout and resolve `fonts.ready`; the time to `dialog_ready`; the measured height against the height after show.</li><li>Compare: (a) hidden, then show; (b) visible at alpha 0 or off-screen, then show; (c) a synchronous layout after `fonts.ready`.</li><li>Pick the mechanism per OS and check the ~1 s show deadline against the measured times.</li></ul> | `crates/paddock-dialogs/examples/show-probe.rs`, `docs/research/2026-10-dialog-show-spike.md` | The probe's assertion "measured height = height after show, and ready before the deadline" is the test; it is kept as an `#[ignore]` real-window test | researcher | ADR 0026 accepted | 1 |
| **1.1a** | **Contract in Rust.** <ul><li>`command_table!` emits `dialog_callable::<name>` markers and `DIALOG_CALLABLE`.</li><li>The `dialogs!` macro: marker references, `prefix*` const assertion, plugin-qualified names checked by a unit test, typed kind/modality.</li><li>`Contracts::from_tables`.</li><li>Generated `dialog-kinds.ts` and per-owner `dialog-contracts.ts`.</li><li>`defineDialog(contract, ui)`.</li><li>Move the 13 `pd.*` contracts and the demos' 7.</li><li>Delete `serializeDialogContracts`, `dialogContractFile`, the JSON file and its parse path.</li></ul> | `crates/paddock-commands/src/table.rs` (markers), `crates/paddock-dialogs/src/{contract,table,generated}.rs`, the `dialogs.rs` of `paddock-shell`/`-channels`/`-mcp`/`-storage`/`-workbench`, `ui/projects/paddock-ui/shell/{src/define.ts,dialogs/src/{kind,contracts}.ts,generated/}`, shell-demo and workbench-demo `src-tauri` | <ul><li>`kind.spec.ts`: for every kind x {height, no height}, TS `fitsContent` equals the generated `fits`. Fails today on the viewer.</li><li>`trybuild` cases: a misspelt command, a command without `DIALOG`, a prefix matching nothing, `Kind::Viewer` with `Modal`; each fails to compile.</li></ul> | implementer (with this spec) | ADR 0026 accepted | 2 |
| **1.1b** | **Sizing and the window lifecycle** (ADR §4). <ul><li>Minimum content as `set_min_size`.</li><li>Remembered size clamped (decisions and forms keep offset and width only).</li><li>Cap never below the minimum.</li><li>The dialog's own inner logical size, rounded up once; decorations from the dialog's own outer − inner.</li><li>Create hidden with the theme background; `dialog_ready`/`dialog_fit {content, minContent}`; show on ready or at the show deadline, with the mechanism from 1.1-0.</li><li>Parent disabled (or scrim) at show; a second open while pending focuses on show.</li><li>`ScaleFactorChanged` refit.</li></ul> | `crates/paddock-dialogs/src/{placement,window,tauri}.rs`, `crates/paddock-startup/src/{config,watchdog}.rs` (show deadline, dialog labels), `shell/dialogs/src/io.ts` | <ul><li>Property test (scale 1-3, work area 800x600 to 7680x4320, any remembered size): placed size in [min content, work area], and physical back to logical ≥ request. Fails today on rounding and on the remembered size below the content.</li><li>Mock-runtime tests: not visible before `dialog_ready`; shown at the deadline with no ready; the parent stays enabled until show; a second open while pending creates no window.</li></ul> | implementer | 1.1-0, 1.1a | 2 |
| **1.2a** | **DialogFrame** (ADR §2, §4.8). <ul><li>Header slot, body, `flex: none` footer.</li><li>`DialogAction` union, `DialogOutcome`, `layoutActions` (pure).</li><li>Button order native per OS (D1).</li><li>Enter, Escape and focus moved from `keys.ts` into the frame.</li><li>Busy (abortable and not), rejected with Try again.</li><li>Focus return in `DialogService`.</li><li>Measuring wrapper and refit triggers (one per frame).</li><li>`dialogStatus`, `dialogMinBodyHeight`.</li><li>`in-page.ts` on the same frame: hidden until ready or the deadline, focus trap.</li><li>`hasCloseAction`/`findDefaultAction` heuristics deleted.</li></ul> | `ui/projects/paddock-ui/shell/dialogs/src/{dialog-frame,actions,dialog-shell,keys,dialog.service,in-page,dialog-host}.ts` | <ul><li>`actions.spec.ts` table: order on macOS, Linux and Windows; counts; Enter never destructive; fallback Close; each action state, with an Escape while busy for an abortable and a non-abortable answer.</li><li>Playwright on shell-demo `sd.rename` at deviceScaleFactor 1.5 in a 1366x768 window: the footer buttons are hit by `elementFromPoint` after the content grows by 400 px and shrinks back, gap ≤ 24 px. Fails today (grow-only, unpinned footer).</li></ul> | designer | 1.1a | 2.5 |
| **1.2b** | **Test kit and checks.** <ul><li>Harness additions (ADR §6), on top of `fix/dialog-boot-flake` (merged first).</li><li>`FakeDialogIo` ready, fit and withheld ready.</li><li>The command recorder at the TS invoke layer, tagging windows and in-page dialogs.</li><li>`expectDialogFits`, `openEveryDialog`, `measureDialogWindow` in `paddock-e2e`.</li><li>`dialog-commands-seen.json`; the "declared but never sent" check in nightly and full runs only (never in affected-only CI, ER-7); the `unexercised` reasons and their ratchet (D4).</li><li>Lint `paddock/dialog-frame-footer`.</li><li>Conformance: visited ids = contract entries.</li></ul> | `ui/projects/paddock-ui/shell/{ipc,testing}/src/`, `ui/projects/paddock-e2e/src/dialogs/`, `eslint/src/rules/dialog-frame.cjs`, `conformance/src/dialogs.mjs` (or the `paddock-dev` rule when phase 4 lands), `.gitea/workflows/nightly.yml` (one step, by the integrating agent) | <ul><li>The lint fails on a planted `.foot`.</li><li>The harness reports "draws its own footer" on today's `channel-remove-dialog`.</li><li>The recorder fails an in-page dialog that sends an undeclared command.</li><li>`expectDialogFits` fails on a planted fixed-height form at 1.5.</li></ul> | implementer | 1.2a, 1.1b | 1.5 |
| **1.2c** | **Adoption by the shared and demo dialogs.** <ul><li>`pd.confirm` (new).</li><li>13 `pd.*` + 5 shell-demo + 2 workbench-demo declare actions and lose their footers.</li><li>`pd.report-problem` becomes a modeless form.</li><li>Screenshots of every kind and state in both themes and both button orders for the independent review (rule 22).</li></ul> | `ui/projects/paddock-ui/shell/{about,channels,mcp,storage,diagnostics,setup,settings,confirm}/`, `workbench/dock/src/layout-dialogs.ts`, `ui/projects/shell-demo/`, `ui/projects/workbench-demo/` | 1.2b's checks over all 20 (red until each dialog is moved); `expectDialogFits` over 20 at 3 scales x 2 work areas in Chromium | implementer | 1.2b | 1.5 |

**Effort: 10.5 agent-days** (1 + 2 + 2 + 2.5 + 1.5 + 1.5), against 4.5 in the plan. The critical path,
1.1a → 1.2a → 1.2b → 1.2c, is 7.5 days. The spike runs beside 1.1a and is on the critical path only if it takes
longer than 2 days.

### Adoption packages (after 1.2c, one per app, sized for its release)

| WP | App | Work | Days |
|---|---|---|---|
| PW-D | Pitwall 1.1 | `src-tauri/src/dialogs.rs` (4 own); `pd.confirm` for 11 call sites; quit, consent and key-secret actions; `expectDialogFits` over its contract | 1.5 |
| SP-D | Spotter 1.0 | `pd.confirm` for 2 call sites; delete `confirm-dialog.ts`; shared settings, about and updates; checks | 0.5 |
| DY-D | Dyno 1.0 | 6 own dialogs and `pd.confirm` for the 3 call sites that stay; lockers as a read-only tool; delete the dialogs and the 6 confirm call sites of removed features, the `Dialogs` service and `dy-dialog`; checks | 2 |
| PC-D | Pitcrew (next minor) | `pd.confirm` for 6 + 1 call sites; `pc.config-diff`; delete `confirm-dialog.ts` and `stop-process-dialog.ts`; checks | 1 |
| M2b | Debrief 1.2 | As planned, plus moving the 50 command lists to `src-tauri/src/dialogs.rs` and deleting the TS analysis | 3.5 + 0.5 |

## 3. Decisions taken (user, 2026-10-28)

- **D1. Button order is native per OS.** macOS and Linux: Cancel, then the primary on the right. Windows: the primary,
  then Cancel. The frame applies it (`layoutActions(…, platform)`); apps declare roles only. 1.2a's table test covers
  both orders.
- **D2. Widths are three steps,** 400 / 560 / 720, defaulted by kind; only a tool declares a number.
- **D3. Dyno is monitoring-only** (Dyno ADR 0021). The rule editor, rule end time, CPU affinity and CPU cap dialogs are
  removed; DY-D is 2 days.
- **D4. `unexercised` commands are allowed** with a written reason that is reviewed and counted. The per-app count may
  only fall: `dev check dialog-commands` fails when it rises above the committed baseline (1.2b).

## Checklist line (ADR 0043)

Adopting the shared dialogs means adopting their size model once: decisions, forms and viewers follow their content
(`dialog_measure { epoch, seq, measure }`), a tool keeps its size, only a tool remembers its size. Debrief's five fixed-height
forms become `Height` forms or tools. Run `expectLoopSettles` over the whole contract on chromium and webkit, and keep
conformance C78 clean.
