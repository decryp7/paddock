# 16. Dialog window sizing contract

- Status: Accepted (2026-10-27, by the user)
- Amended by ADR 0043 (decisions 1, 5 and 6: a declared size mode per kind, one measure loop, the user's resize).
- Date: 2026-10-27
- Related: rule 17 (every dialog is a real OS window), rule 39, rule 40 (recurring bugs mean a missing design rule),
  rule 43; ADR 0009 (dialog kinds and declared contracts, which this amends for sizing), ADR 0011 (layout by declared
  constraints), ADR 0012 (app shell); Debrief ADRs 0050 and 0054

## Context

ADR 0009 gave every dialog a kind and a contract, and Debrief ADR 0054 said "sizing is the shell's job". Debrief's
manual build 1.1.1 still shows dialogs whose Close button is cut off and dialogs with a large empty band below the
footer. Size and footer fixes have been made one dialog at a time at least fifteen times: `7990fef4e` and `d3ab6ca49`
(redacted copy and AI setup footers), `077374291` (About pinned footer), `398f2e009` (dialogs fill their window),
`bcdcd354c` and `37b14b7e8` (fit to content, exact-fit height), `3272b2441` (no scrollbar, no gap for three
decisions), `0c6f5347f` (streamed dialogs only grow), `a2197bf30` (tool width), `3580a5a50` (manifest minimum size),
`1b8b4e274` (settings footer). Debrief's `CHANGELOG.md` already promises "a Close or Cancel button that stays in view";
nothing enforces it in general.

There are two copies of the code. Debrief 1.1.1 has its own (`apps/debrief/app/src-tauri/src/dialog_windows.rs`,
`dialog_placement.rs`, `app/src/app/dialog-windows/`); the shared `crates/paddock-dialogs` and
`ui/projects/paddock-ui/shell/dialogs` were ported from it and are used by shell-demo, workbench-demo, `paddock-shell`,
`paddock-window` and `paddock-panels`. The logic is the same in both, so the causes below apply to both.

### How a size is chosen today

- **Width** is fixed per dialog in the registry or the kind default (640, viewer 680, tool 960); never measured.
- **Height**: fixed for tools, viewers and forms that declare one. For "fitted" dialogs (decisions, forms without a
  height) Rust opens the window at 520 (`DEFAULT_SIZE`/`FALLBACK_SIZE`), the shell measures the last child's bottom
  after `afterNextRender` and `document.fonts.ready` (`dialog-shell.ts` `neededHeight`, paddock 271-283, Debrief
  269-282) and calls `dialog_fit_height` (`crates/paddock-dialogs/src/tauri.rs:606-651`), which clamps between the
  floor and the work area minus a fixed 32 px title bar (`placement.rs:13`, `window.rs:359-368`).
- **Placement** without a remembered size caps both sides at 75 % of the logical work area and then raises them to the
  kind floor (`placement.rs:192-197`, `SCREEN_SHARE = 0.75`); a remembered size is only raised to the floor (200 px
  for most kinds, `contract.rs:32-53`), never to what the content needs.

### Root causes, with the code that shows them

1. **Fixed heights against the screen (DPI scaling).** At 1920x1080 and 150 % the logical work area is about 688 px
   high and the 75 % cap is about 516; at 125 % about 619; 1366x768 at 100 % gives about 540. Every fixed height of
   560-680 shrinks (settings 680, redacted-copy 680, format-wizard 640, ai-setup 640, ai-models 640, research 560,
   ai-query 560). These are never fitted again, the body scrolls, and a footer that is not pinned is below the fold:
   **the Close button is cut off**.
2. **The footer is not pinned and not shared.** The shell's body is a scrolling flex column; its own `.pd-foot`/`.foot`
   is an ordinary in-flow row. There is no shared footer component: each Debrief dialog draws its own `.foot`, and only
   redacted-copy, about, settings and four `position: sticky` wizards pin theirs. ADR 0054 promised a pinned footer for
   forms; it is not implemented.
3. **Grow-only fitting and floors leave gaps.** The UI asks only for a taller window (`if (needed <= last) return`,
   paddock `dialog-shell.ts:246`, Debrief `:237`), even for forms that Rust would let shrink. After a wizard step, a
   cleared error or a shorter list the window keeps its height; viewers have a 560 floor and grow only; remembered
   viewer, tool and fixed-form heights come back larger than the content: **the big gap after the footer**.
4. **Refits after growth do not fire.** The ResizeObserver watches the content element, which is a stretched flex item
   (`flex: 1 1 auto; min-height: 0`) whose box is always the space left in the window, so later content (a loaded
   list, a streamed answer) does not change it. The first fit often measures a loading state. Unit tests replace
   ResizeObserver with a hand-fired fake (Debrief `dialog-shell.spec.ts:224-251`, paddock `ui.spec.ts:569-611`).
5. **Fonts.** Heights were tuned on one font stack. The OS default (`theme-tokens.ts:157-158`: Segoe UI Variable … Cantarell,
   Noto) and the latin-only bundled faces (`styles.scss:6-23`) have different metrics, and density and theme can
   change after the first fit through UiSync (`popout/shared-state.ts:58-64`), with nothing refitting.
6. **Smaller items.** A fixed 32 px title bar is wrong for GNOME client-side decorations and macOS; rounding through
   `PhysicalSize`/`LogicalSize` at fractional scales can leave the inner height a pixel short, which adds a scrollbar
   and re-wraps text; the TypeScript `fitsContent` (viewer always fits) and Rust `is_fitted` (viewer fits only without
   a height) disagree; `hasCloseAction` checks that a Close exists, not that it is visible.

### Instances: 68 registered dialogs

| Owner | Count | Kinds | Source |
|---|---|---|---|
| Debrief | 50 | 12 decision, 28 form (23 fitted, 5 fixed height), 7 viewer, 3 tool | `apps/debrief/app/src/app/dialog-windows/dialog-registry.ts`, `src-tauri/dialog-contracts.json` |
| `paddock-ui` shared | 13 | `pd.about`, `pd.whats-new` (viewer); `pd.updates`, `pd.settings`, `pd.setup`, `pd.wb.dock.layout-manage` (tool); `pd.channels.secret`, `pd.mcp.key`, `pd.wb.dock.layout-save` (form); `pd.channels.remove`, `pd.report-problem`, `pd.mcp.revoke`, `pd.storage.confirm` (decision) | `*.contributions.ts` under `ui/projects/paddock-ui/`, `workbench/dock/src/layout-dialogs.ts` |
| shell-demo | 5 own | `sd.confirm-reset`, `sd.choose-style` (decision), `sd.rename` (form), `sd.about` (viewer), `sd.inspector` (tool) | `ui/projects/shell-demo/src/app/demo.contributions.ts` |

The contract files hold 50 (Debrief) + 16 (shell-demo: 5 own + 11 shared) + 2 (workbench-demo) = 68 entries, which are
68 distinct definitions. Pitwall, Dyno, Pitcrew and Spotter have no dialog windows yet (in-page confirms in Spotter
and Pitwall); they inherit this contract when they adopt ADR 0009. Sighter and Pacenotes have no code.

### What the tests check today, and what they miss

- Boot harnesses (Debrief `dialog-all.spec.ts`, paddock `shell/testing/src/dialog-harness.ts`) run in jsdom: no layout.
- Playwright (Debrief `e2e/dialog-window.e2e.ts`, paddock `ui/e2e/shell-demo.spec.ts`) runs at device scale 1, checks
  Close is `toBeVisible` (which does not mean inside the viewport) at 1440x900, no scrollbar for decisions, and a few
  named dialogs.
- The real-window smoke (`apps/debrief/app/e2e-tauri/`, WebDriver on Windows and Linux) opens four dialogs and clicks
  Close with a WebDriver click, which scrolls it into view first, so a cut-off Close passes.
- Nothing covers 125 % or 150 %, a remembered smaller size, the 75 % cap, the gap below the footer, growth after the
  first fit, or another font stack.

## Decision

1. **Declared size range, measured size.** Every dialog declares `minWidth`, `maxWidth`, and for kinds that do not fit
   content a `preferredHeight`; the kind supplies defaults. The window's **content size** is measured, not assumed:
   the shell measures the natural height of header + body content + footer (the body laid out with `height: auto` in
   an off-flow measuring pass, so a stretched flex item cannot hide growth) and asks for exactly that, clamped to
   `[minContentHeight, workArea − decorations]`.
   - `decision`: width declared, height = measured, always (shrinks and grows).
   - `form`: width declared, height = measured, shrinks and grows (no more grow-only); a fixed height is no longer
     allowed for forms.
   - `viewer`: width and preferred height declared; grows to fit up to the cap, never shrinks below its
     *content-or-floor*, whichever is smaller, so short content leaves no band.
   - `tool`: user-sized; its minimum is the measured minimum content size (below).
2. **One shared dialog frame with a fixed footer row.** `DialogFrame` (header, body, footer) in
   `@decryp7/paddock-ui/shell/dialogs` is the only way a dialog renders actions. The footer is a declared button list
   (`actions: [{ id, label, role: 'primary' | 'secondary' | 'cancel' | 'close' }]`) rendered by the frame as a
   `flex: none` row outside the scroll container; only the body scrolls. Dialog components no longer draw a `.foot`.
   The frame owns the Close/Cancel rule of ADR 0009.
3. **Minimum content size.** The frame computes `minContentHeight` = header + footer + the body's declared minimum
   (default: two lines of body text) and `minContentWidth` = the footer's buttons on one line plus padding. Both are
   reported to Rust with each fit and become the window's OS minimum (`set_min_size`), so the user cannot shrink a
   window below the point where the footer fits.
4. **Remembered size is clamped to the content.** A remembered size is raised to the minimum content size and, for
   decisions and forms, replaced by the measured height (they remember only width and offset). The 75 % cap applies
   to the preferred size only; it never pushes a dialog below its minimum content size.
5. **Refit triggers are explicit.** The frame refits on: first render after `document.fonts.ready`; a ResizeObserver on
   the *measuring* wrapper (natural height), not the stretched element; font, density or theme changes; a scale-factor
   change (moving to another monitor, Tauri `ScaleFactorChanged`); and a dialog's own `requestFit()` for streamed
   content. Refits are debounced and capped (at most one per animation frame).
6. **Logical pixels end to end, rounded up.** Requests and fits are logical px; Rust converts once with the window's
   current scale factor and rounds the physical size **up**. The title bar height comes from the platform (outer minus
   inner size of the parent window), not a constant.
7. **One implementation.** Debrief's copies (`dialog_windows.rs`, `dialog_placement.rs`, `dialog-windows/kit/`,
   `dialog-shell.ts`) are deleted when Debrief adopts `paddock-dialogs` and `paddock-ui` dialogs (rule 43). The
   TypeScript and Rust "is fitted" rules come from one generated table so they cannot disagree.

## The check

1. **Playwright over every registered dialog** (shared spec in `paddock-ui` testing, `expectDialogFits(contract)`, run by
   every app over its contract file): for each of the 68 dialogs, with its fixture input, at the default size and at
   its minimum content size, with `deviceScaleFactor` 1, 1.25 and 1.5 and a work area matching 1920x1080 and 1366x768:
   - every footer button's bounding box lies fully inside the window's viewport and is not covered (`elementFromPoint`
     at its centre returns the button);
   - the gap between the footer's bottom and the viewport's bottom is at most N px (proposed N = 24, the frame's own
     padding plus 8) for decisions, forms and viewers whose content is shorter than the window;
   - the body scrolls only when the content exceeds the cap, and then the footer is still fully visible;
   - after the dialog's content grows (fixture with a "long" variant: long error, long list, streamed answer) and
     shrinks, the same two assertions hold.
   With two font stacks (bundled Inter and a fallback-metrics font) in the full suite. Chromium in branches, WebKit in
   the full suite (rule 3).
2. **Real-window smoke over every registered dialog** (Debrief `e2e-tauri` and a shared `paddock-e2e` helper; Windows
   at 100/125/150 % via the runner's display settings, Linux WebKitGTK with `GDK_SCALE`/`GDK_DPI_SCALE`, macOS as soon
   as a driver exists): open each dialog, read the window's inner size and the footer buttons' rectangles through
   `executeScript` (no WebDriver click before measuring), assert the same two conditions, then close by the declared
   Close/Cancel.
3. **Rust property test** in `paddock-dialogs`: for any monitor (scale 1-3, work area 800x600 to 7680x4320), any
   remembered size and any declared range, the placed size is ≥ the minimum content size and ≤ the work area, and the
   physical size converted back is ≥ the logical request.
4. **Lint `paddock/dialog-frame-footer`**: a component registered as a dialog may not contain a `.foot`/`.actions`/
   footer element of its own or a button with a Close/Cancel/OK role outside `DialogFrame`'s `actions`; `defineDialog`
   rejects a fixed height for `form` and `decision`.
5. **Conformance**: the count of contract entries equals the number of dialogs the Playwright and real-window specs
   visited (the spec writes the visited ids; a missing id fails), so a new dialog cannot skip the check.

## Consequences

- "Close cut off" and "gap after the footer" become failing tests in every app the moment a dialog is added or a
  content change breaks it, at the scales users have.
- Dialog components lose their footers (about 40 Debrief components change, mostly deleting markup) and declare
  actions instead; the frame handles keyboard defaults (Enter = primary, Escape = cancel/close) in one place.
- Forms no longer declare heights; the five fixed-height Debrief forms (format-wizard, ai-setup, ai-query, ai-propose,
  redacted-copy) become fitted forms or tools, decided per dialog in the migration.
- The real-window smoke grows from 4 to 68 dialogs × 3 scales; it runs in the full suite before a release, not per
  branch.

## Migration plan

1. Failing regression tests first (rule 5): Playwright at 1.5 for `ai-query` (Close cut off) and `ai-narrative` with a
   short answer (gap), in Debrief.
2. `DialogFrame`, measuring wrapper, minimum content size and the property test in `paddock-ui` and `paddock-dialogs`;
   shell-demo and workbench-demo adopt.
3. Debrief adopts `paddock-dialogs` and the shared frame (this completes ADR 0009 migration step 2), deletes its copies,
   and moves its 50 dialogs to declared actions.
4. The Playwright check over all 68 dialogs and the conformance count; then the real-window smoke at three scales.
5. Pitwall, Dyno, Pitcrew and Spotter get the frame when they adopt ADR 0009.

## Alternatives considered

- **Raise the fixed heights and the cap.** Fixes 1080p at 100 % and breaks 768p at 150 %; the next content change
  breaks it again.
- **Make every body scroll and pin each footer by CSS per dialog.** That is what the 15 fixes did; nothing stops the
  51st dialog from forgetting.
- **Let the OS size the window from the webview's preferred size.** Neither WebView2, WKWebView nor WebKitGTK reports a
  preferred content size to Tauri; the measurement has to be ours.
