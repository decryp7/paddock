# X50 workbench: independent design review (rule 22)

- Reviewer: independent agent (did not build X50). Date: 2026-10-25. Branch: `origin/shell/integrate` @ 65788c7.
- Inputs: the 20 shots in this folder (both themes), `ui/projects/paddock-ui/workbench/*`, `ui/projects/workbench-demo`,
  ADR 0013, `docs/plans/workbench.md`, `docs/design-guide.md`, Debrief ADR 0056 / ADR 0061, suite UI rules (ui-review-2 patterns).
- Verdict: **not approved yet.** The bones are good: calm surfaces, one family with the shell and the X21 table (`pd-table`
  filter / row count / Columns reads identically), real light/dark parity in almost every pixel, good middle truncation in the
  table, a proper needs-AI state with a working "Open Pitwall" step, a designed empty state for Jobs, "did you mean" works.
  Four P1s block approval: a context menu drawn at the window origin, two 320 px / narrow-window failures, and a contact
  sheet that does not show half of what has to be reviewed.

Counts: **P1: 4, P2: 17, P3: 14.**

---

## P1 (blocks approval)

**P1-1. Explorer context menu opens at the window's top-left corner, not at the row.**
`*-explorer-menu.png`: right-clicking README.md (y≈253) draws the menu at (0,0), over the app title and the File/View menu.
The keyboard path (`openMenuFor`, Shift+F10) computes a position from the row rect; the pointer path (`(contextmenu)` on the
row + `cdkContextMenuTriggerFor` on the tree) ends at the origin. Fix: open the menu yourself in the row's `(contextmenu)`
handler with `trigger.open({ x: e.clientX, y: e.clientY })` and `preventDefault()` (one code path for mouse and keyboard),
and mark the target row with the selection/"menu target" style while the menu is open. Add a Playwright assertion that the
menu's bounding box is within 8 px of the click point (and of the row for Shift+F10).

**P1-2. At 320 px the Problems table is unreadable: Level keeps its width, Problem is cut to "pr…".**
`*-320.png`: Level ("warning") takes ~90 px, Problem gets two characters, File and Line are gone. At 600 px (`*-narrow.png`)
File and Line are dropped, which are the columns you act on. Fix in the table's column priorities (X21 contract, not the
demo): Level becomes a 14 px severity icon/dot column (fixed 24 px, label in the tooltip and aria), Problem is the flex
column with priority 1, File (middle-truncated) priority 2, Line priority 3; below ~360 px collapse to one cell
"icon · problem" with "file:line" as a muted second line or in the tooltip. Add a 320 px test asserting the Problem
column is ≥ 60 % of the table width.

**P1-3. The "Layout adjusted to fit this window" notice covers the tab strip and group actions.**
`*-narrow.png` hides the active tab (main.rs), the overflow chevron and part of the actions; `*-320.png` hides the whole tab
strip and the path line. This is exactly when the user needs to see what got tabbed. ADR 0056 §"layout.fit" says delivery
belongs to the notification system, not to the dock, and ADR 0061's spirit (controls stay where they are) is broken by an
overlay on the controls. Fix: deliver it through the shared notification surface (bottom-end toast above the status bar,
or the bell list), never over a group header; keep Undo + Dismiss, auto-collapse into the bell after ~8 s. At ≤ 480 px use
a one-line status-bar message "Layout fitted · Undo".

**P1-4. The contact sheet does not show the states the review must judge.**
Missing pixels for: assistant streaming text, citations, tool cards (running/failed), Stop while busy, the write-tool
approval prompt, a failed turn; any pop-out window (title bar, inactive dimming, Dock back, Keep on top); a running job in
the Jobs pane and in the status bar; the palette with no match and no suggestion; the Explorer opened at 320 px; one
keyboard-focus shot per surface (activity bar, tree row, tab, palette row, menu item) to check rings ≥ 3:1; a shot with a
real app accent (everything here is `graphite`, so selection, marks and the primary all look grey). Rule 20 also asks for
deviceScaleFactor 2 (these are 1440×900 @1x) and nothing clipped mid-element (the last Problems row is cut in every wide
shot). Fix: extend `contact-sheet-x50.spec.ts` with these states (the mock backend can script a streamed answer with a
citation, a write tool and a 2-step job), shoot at @2x, raise the Problems pane or use 3 rows that fit. Until then the
items below marked "(code)" are judged from source only.

---

## P2 (fix before the workbench ships in an app)

**Hierarchy, primary actions, naming**

- **P2-1. Welcome has four equal outline buttons, no primary** (`*-first.png`; rule 1). Make "Open src/main.rs" (or the
  app's real first step) the filled primary, the rest ghost links or a short "Start" list with key hints (⌘P, ⇧⌘P). The demo
  buttons are hand-styled (`orientation-view.ts`): use `pd-btn` so the demo shows the family controls.
- **P2-2. Uppercase, letter-spaced section headings "FILES", "OUTLINE"** (rule 11). Sentence case, 12/600 muted: "Files",
  "Outline".
- **P2-3. Panel headings repeat their tab title**: the Assistant pane header says "Assistant" under the "Assistant" tab; the
  Jobs auto-hide panel says "Jobs" with a "Jobs" strip tab under it and "Jobs" in the status bar (rule 11). Use the header
  for the conversation title (falling back to "New conversation") and drop the Jobs title row's text when the strip tab
  already names it.
- **P2-4. Two Outlines that disagree.** The Explorer's Outline section shows `main … fn` (kind right, no line); the Outline
  pane shows `fn  main  3` (kind left, line right); empty copy differs ("Open a file to see its symbols." vs "No symbols in
  the selected file."). One row component, one empty text. Also decide whether a perspective shows both at once (it
  should not by default).
- **P2-5. Activity bar: three items look "on" at once** (Files, Problems, Outline in `*-files.png`), and the 2 px inset bar on
  a 36 px rounded tile renders as a bracket outline rather than an indicator. Keep `pd-on` for the item that owns the side
  bar only; views merely visible in the dock get a 4 px dot under the icon. Draw the indicator as a 2×20 px bar on the bar's
  inner edge (outside the tile), not an inset shadow on the rounded tile.

**Focus, icon sizes, glyphs**

- **P2-6. Menu item focus is not visible enough** (code). Explorer menu: `.pd-ex-mi:focus { background: var(--pd-hover);
  outline: none }` is a 5 % tint, far below 3:1. Activity fly-out: a 1 px inset ring. Use the dock menu's rule everywhere:
  `box-shadow: inset 0 0 0 2px var(--pd-focus-ring)` on `:focus-visible`, hover stays a tint. One shared menu-item style in
  paddock-ui instead of three copies (dock-menu, explorer, activity-bar).
- **P2-7. Tab "▾" and "×" are text glyphs below 14 px** (`.pd-dock-tab-menu` at label size renders the triangle ~6 px). Use
  `pd-icon` chevron-down / close at 14 px in 20 px hit areas. Also two menus per tab (tab ▾ and group "···") is one too many:
  keep the group "···" and the tab's right-click menu; drop the ▾.
- **P2-8. Pin means two things.** The dock header pin = auto-hide; the pop-out title bar pin = keep on top (code). Use a
  "layers/on-top" glyph for Keep on top, and "Dock back" should not use the generic `window` glyph (use "dock-in"/arrow-into-
  frame). Keep on top's on-state is accent text only: add `background: var(--pd-accent-tint)` so it survives graphite and
  colour-blind users.

**Palette**

- **P2-9. "Did you mean" echoes the mode prefix:** `Nothing matches ">indx repositry"`. Quote `commandQuery()` (prefix
  stripped), and for an `@`/`#` search say what was searched ("No symbols match "handel"").
- **P2-10. Match highlighting is invisible with graphite** (`mark { color: var(--pd-accent) }` ≈ text colour; `@handle` shows
  no visible mark). Mark with weight 600 + `--pd-text` and a 2 px underline in the accent, or a `--pd-accent-tint`
  background, so it works in any accent and in both themes.
- **P2-11. Detail paths truncate at the start via `direction: rtl`**, not in the middle (rule 18), and rtl reorders trailing
  punctuation. Use `pd-path-text` (already used by the Explorer) for `row.detail` when it is a path.

**Assistant (code, plus the needs-AI shots)**

- **P2-12. Primary and disabled look the same in dark graphite**: "Open Pitwall" (enabled primary, light grey fill) and the
  disabled "Send" are both grey pills (`dark-assistant.png`). Disabled primary must drop its fill (outline + subtle text),
  and graphite's dark primary needs a fill clearly distinct from `--pd-surface-3`.
- **P2-13. Composer stays live while the assistant cannot run.** With Pitwall not running the textarea, the context chip and
  "Allow actions" are interactive and the hint says "Ask a question…". Disable the textarea (keep the draft), replace the
  hint with "Start Pitwall to ask questions", and keep one primary in the pane (Open Pitwall).
- **P2-14. Forced auto-scroll on every streamed token** (`effect` sets `scrollTop = scrollHeight` on any `turns()` change). A
  user who scrolls up to read a citation is yanked down. Stick to bottom only when the user was within ~24 px of it; show a
  "Jump to latest" pill otherwise.
- **P2-15. The whole log is `aria-live="polite"` while text streams**: screen readers re-announce partial tokens. Set
  `aria-live="off"` on the streaming answer, and announce once on completion ("Answer ready", or the stopped/failed note).
- **P2-16. Delete conversation is one click on a trash icon, at rest, with no confirmation or undo** (rule 2). Show it on
  hover/focus only, move it into the row's "···"/context menu in danger text, and confirm naming the conversation (or offer
  Undo for 8 s).

**Type**

- **P2-17. Mono renders as a Courier-like fallback in the shots** (code view, path line, palette shortcuts, Outline kind), which
  rule 10 forbids. `ui-monospace`/`'SF Mono'` are not resolved by Chromium here and the stack falls to the generic. Add
  `Menlo` (macOS), `'DejaVu Sans Mono'`/`'Liberation Mono'` (Linux) before `monospace` in `--pd-font-mono` and re-shoot.

---

## P3 (polish)

- P3-1. Palette list with an empty query mixes categories (View, Perspective, View, Window) with no "Recently used" /
  "Common" headings, so the order looks random. Add two small group headers or a muted "recent" clock glyph.
- P3-2. Palette status row reserves 20 px even when empty (the gap under the input in `*-palette.png`). Collapse it unless it
  has content (it is `role=status`, so keep the element and set `min-height: 0`).
- P3-3. Shortcut hints are ~11 px mono muted and use `^` for Control next to ⌘/⇧/⌥: use ⌃ consistently on macOS, the sans
  font with `font-variant-numeric: tabular-nums`, 12 px.
- P3-4. Symbol results show a letter "S" badge; show the kind glyph (fn/struct) and drop "fn" from the detail.
- P3-5. Explorer menu: "Open" has no icon, "Copy path" has one, so labels misalign; a separator between two items is noise.
  Reserve the icon column always; group only with ≥ 3 items.
- P3-6. Activity badge "3" sits on the warning glyph's stroke; offset it to the tile corner (top-end, 2 px outside the
  glyph box) and give it a 1.5 px ring in `--pd-surface-1`.
- P3-7. Assistant activity/tab icon is a pulse/heartbeat line; that reads as "health". Use a chat/sparkle glyph.
- P3-8. Conversation-list toggle uses the hamburger `menu` glyph; use "history" and a tooltip.
- P3-9. Context chip has an empty 16 px gap before "Current file" (`*-assistant.png`): the chip icon name does not resolve;
  render the icon only when the glyph exists, or fix the registered name.
- P3-10. Approval prompt shows only `summary` (code); add a disclosure with the tool name and its arguments/target so "Allow"
  is an informed choice, and make Allow/Decline full-size buttons like Send/Stop.
- P3-11. Problems: Level as plain words, rows unsorted (warning, error, warning). Severity dot + word (rule 3) and default sort
  by severity then file.
- P3-12. Group overflow "⌄ 1" next to visible tabs does not say what is hidden; add a tooltip "1 more tab: Jobs" and use the
  same chevron the tab ▾ would have used.
- P3-13. Inactive tabs keep the hidden close button's space (wide gap after "mod.rs"); use `display: none` + hover reveal with
  a reserved 4 px, or keep close visible on all tabs at 40 % opacity.
- P3-14. Explorer selection does not follow the active document (README.md selected while main.rs is active); reveal and
  select the active document's node (with a setting to turn it off). Also: status bar right side has only "Jobs" and a bare
  bell; give the bell an aria-label/count and let apps put their state on the left so the bar does not look empty.

---

## Checked and fine

- Light/dark parity: same layout, same hierarchy, contrast holds in both (apart from P2-12).
- One family with the shell and X21: the table chrome, filter field, Columns button, surfaces and hairlines match.
- No horizontal page scroll at 320 px; the side bar collapses into the activity bar; Outline docks to a rotated edge tab.
- Activity bar icons render at 18 px; dock action icons and pop-out glyphs are 14 px (≥ 14 px rule met except P2-7).
- Focus rings on tabs, dock actions, palette input, tree rows, chips and pop-out buttons are 2 px in `--pd-focus-ring`
  (graphite: #a6a8ad on #0e0f12 and #4f5764 on #fff, both well above 3:1).
- Pitwall-not-running: clear title, one-sentence cause, one working step (rule 19); Stop replaces Send in the same place
  (ADR 0061); citations are buttons with text bindings only (no `innerHTML`).
- Jobs empty state is designed (icon, title, one sentence).
