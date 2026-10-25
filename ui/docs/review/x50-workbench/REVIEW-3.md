# X50 workbench: independent design review 3 (rule 22)

- Reviewer: independent agent. I did not build X50 and did not write review 1 or 2. Date: 2026-10-25.
  Branch: `origin/shell/integrate` @ 6882e4c.
- Inputs: the 60 shots and `workbench-demo-icon.png` in this folder, REVIEW.md, REVIEW-2.md, RESPONSE-1.md,
  RESPONSE-2.md, `e2e/contact-sheet-x50.spec.ts`, the sources where a pixel needed explaining, `icons/` (the masters,
  the small frames and the tray template of `workbench-demo` and `shell-demo`), ADR 0013, `docs/design-guide.md`,
  Debrief ADR 0056 / 0061, and the suite UI rules (ui-review-2 patterns).
- Method: every verdict was checked in the pixels. I cropped at full resolution, sampled colours from the PNGs and
  computed contrast with the WCAG formula. Positions are CSS px (device px ÷ 2). I ran `icons/tools/cvd.mjs` for the
  demo tile, which nobody had run before.
- **Verdict: not approved. One P1 remains.** Both review-2 P1s are fixed and look right. Ten of the eleven P2s are
  fixed in the pixels, and the eleventh is fixed in code and unit tests. Most P3s are done. The workbench now reads as a
  professional IDE in both themes.
  - The P1 is the keyboard path into the Explorer's row menu. Shot 07b was meant to prove it, and it shows the
    opposite: focus stays on the tree row, and no menu item is focused.
  - The P2s are new, mostly from states shot for the first time: code blocks that vanish in light, the native blue
    checkbox, stopping and failure drawn like the wrong thing, a deleted conversation that stays open, and a demo icon
    that is almost the shell demo's.

Counts (new findings in this review): **P1: 1, P2: 6, P3: 12.**
Review 2 (2 P1, 11 P2, 16 P3):
- Fixed and seen in the pixels: 2 P1, 9 P2, 11 P3.
- Fixed in code and unit tests only: P2-C's focused Decline, and P3-16.
- Partly fixed, carried into new findings: P2-A, the P2-K 07b shot, P3-3, P3-7 and P3-14.

---

## 1. Review 2 findings, verified

### P1

| # | Verdict | Evidence |
|---|---|---|
| P1-A folded rows | **Fixed** | `*-22`: each folded row is about 44 CSS px tall. The problem line and `src/…/request_handler.rs:2` both sit inside the row with clear space above the divider, and nothing overlaps a divider in either theme. The Problem column is truncated with an ellipsis, not clipped. The e2e checks `scrollHeight ≤ clientHeight` and a 4 px gap. |
| P1-B Explorer at 320 | **Fixed** | `*-21`: below 480 px the Explorer is an overlay that covers the whole dock, and a scrim tints the Assistant edge strip. No dock control is sliced or half visible. Escape and outside click were not shot; I accept them from the e2e. |

### P2

| # | Verdict | Evidence |
|---|---|---|
| P2-A active tab hidden | **Partly fixed → P3-2** | `*-22`: `main.rs`, the active tab, is visible with `⌄ 3` beside it. But the strip is scrolled rather than reflowed: a 17 px sliver of the previous tab is left at the start, and in dark a scrollbar thumb shows under the tab. |
| P2-B pop-out title | **Fixed** | `*-15`: the title bar says "main.rs". Dock back, the path line and the code all start at the 12 px inset. Keep on top is hidden where it cannot work. |
| P2-C destructive approval | **Fixed** (focus from code) | `*-12`: the card is danger-tinted and says "This cannot be undone." with a glyph. "Allow erase" is danger text on a neutral button and names the consequence. Decline is a neutral button on the right, and nothing is the filled primary. Contrast: Allow text 6.0:1 (dark) and 5.0:1 (light); the warning line 5.35:1 and 4.72:1 on the tint. The focused default Decline is **not visible** in the shot, because the spec clicks Details with the mouse first. It is pinned by `assistant.spec.ts:510` (`activeElement` is Decline) and `assistant.ts:394`. The "No arguments" fix can't be seen either, because Details is closed in the shot (P3-1). |
| P2-D severity glyphs | **Fixed** in Problems | Error is circle-× in red, warning a triangle in amber, each in its own column (`*-02`, `*-22`). Other failure states in the workbench still use the triangle (P2-4). |
| P2-E one selected item | **Fixed** | `*-08`: Files has the 2×20 bar and the accent tint, and Layouts has only a neutral open tile (#202226 against #262b22 in dark). The pointer rests away, and no shot shows a leftover hover tint. |
| P2-F Outline follows document | **Fixed** | `*-10` to `*-19`: after a reload the tree reveals `src/main.rs`, and the Outline shows `ƒ main · fn · line 3`. |
| P2-G declined ≠ error | **Fixed** | `*-12b`: a neutral card with × and a muted "Declined". |
| P2-H failed turn | **Fixed** | `*-13`: icon, "The answer did not finish.", the likely cause with a next step, Retry (outline) and Show details (ghost). It is aligned with the answer column and keeps the partial text. The glyph is the warning triangle (P2-4). |
| P2-I composer and streaming | **Fixed** | `*-10`: the composer is empty with its placeholder, a three-dot cue follows the text, and Stop replaces Send. |
| P2-J match marks | **Fixed** | `*-03b`: `Show` is semibold with an accent underline in three rows. `*-04`: `handle` is marked in `handle_request`. |
| P2-K sheet gaps | **Mostly fixed → P1-1, P3-1** | `*-01` has a dock (Welcome and the assistant). Shot 18 shows a 2 px ring on the `role=tab` Welcome tab. Tool running (13b), tool failed (13c), the list with delete on focus (13d) and Undo focused (13e) are all there. The mock answer is real markdown. But **07b shows no focused menu item**, and 12 does not show what its caption promises. |

### P3

Fixed and seen:
- P3-1: the menu opens below its row, and the README label stays readable.
- P3-2: a check mark on Understand.
- P3-4: `⌃⌥O` is U+2303 (`keys.ts:67`).
- P3-5: the `ƒ` kind glyph and the file path as detail.
- P3-6: one row style.
- P3-8: the pop-out inset.
- P3-9: a 14 px spinner in the status bar (`*-14`).
- P3-10: a danger-ring badge.
- P3-11: the titles "Request handler" and "Explain the request pipeline".
- P3-12: chips dimmed while inert (#818386 against live #eceef1).
- P3-13: a 14 px rotating chevron.
- P3-15: the `pd:file` glyph and `⌄ 3`.

Accepted from code: P3-16, the first-frame test.

Partly fixed:
- **P3-3**: the column is declared at `width: '28px'`, but with cell padding the problem text starts 52 px into the table at 320 px (22 %) → P3-7.
- **P3-7**: the Jobs panel is sized to its content and the card has the inset. The header row is still empty (pin and × only), and the panel still spans the Assistant column and covers its composer (`*-14`) → P3-3.
- **P3-14**: one line at 320 px, and 358 px wide at 700 px. But the one line is truncated ("Layout adjusted to fit this …") → P3-8.

### Deviation follow-ups

1. **Undo after delete.** Verified. `*-13e` shows Undo with the 2 px ring, and the e2e asserts `toBeFocused`. The list
   hides the row. But the deleted conversation stays open as the current one (P2-5).
2. **Keep on top.** It stays deferred to the first desktop app. It is hidden in the browser pop-out, which is right.
3. **First frame of the menu.** Accepted from the unit test and the e2e.

---

## 2. New findings

### P1 (blocks approval)

**P1-1. Shift+F10 opens the Explorer menu, but keyboard focus stays on the tree row.**
`*-07b` (both themes) was shot after Shift+F10 and then ArrowDown, with the caption "keyboard focus on an item (2 px
ring)". No item has a ring or an active tint. The README row carries the 2 px focus ring, which is the tree's
`:focus-visible` style (`*-07`, opened by mouse, draws only the 1 px target outline). The ArrowDown went to the tree,
where README is the last node, so nothing moved. Also, "Copy path" has no icon in 07b, though it has one in 07.
- **Why it blocks:** the context menu is the only keyboard path to a row's actions, and the Explorer is shared by every
  workbench app (WCAG 2.1.1 and 2.4.3).
- **Cause:** `explorer.ts` `openMenu` calls `trigger.open(at)`. No test checks focus after the keyboard open: the unit
  test clicks the item, and both e2e files assert only visibility and position.
- **Fix:**
  1. Start with a failing e2e: Shift+F10, then `expect(menu.getByRole('menuitem').first()).toBeFocused()`; ArrowDown
     moves to the second item; Escape returns focus to the row.
  2. Then focus the first item after the overlay attaches. For example, call `focusFirstItem('keyboard')` on the CDK
     menu, so that the keyboard focus style applies.
  3. Find out why the icon is missing on the keyboard path. It uses the same `menuTarget(row)`, so the two paths should
     render the same items.
  4. Re-shoot 07b.

### P2 (fix before the workbench ships in an app)

**P2-1. Light theme: code blocks and inline code have no surface.** In `light-10`, `-11`, `-12`, `-13*` and `-14`, the
code block and `handle_request` render on plain white. Sampled #ffffff inside and outside the block. They read as
stray monospace text with odd indents. The cause is `rich-text.ts:49-50`: they use `--pd-surface-3`, the raised
(popover) surface, which is `#ffffff` in light (`_tokens.scss:174`), the same as the pane. Use a sunken or code token
that differs from the pane in both themes (dark is #22262c on #15171b today, which works). Add a token test: the code
surface differs from the pane in every scheme.

**P2-2. The "Allow actions" checkbox is the browser's blue in both themes.** A checked box is #99c8ff in dark with the
Pacenotes olive accent, and #0075ff in light (Chrome's default, not Debrief's #1d5fd1). See `*-12b`, `*-13`, `*-13c`
to `-14`. The design guide §6 says: "the fill of an on toggle, checked box or radio is `--pd-accent-control`". Only
`language-page.ts` sets `accent-color`. Set it once in the base stylesheet for every native checkbox and radio
(`accent-color: var(--pd-accent-control)`), or use the shared toggle. In dark olive this is the most off-brand pixel on
the sheet.

**P2-3. A tool stopped by the user is drawn as a failure.** `*-13c` and `*-13d`: after Stop, the running `find` card
gets the danger border, the warning glyph and "Did not finish", followed by a muted "Stopped.". This is the same
mistake as review 2 P2-G: the user's own choice is not an error. Draw it as a neutral card with a muted "Stopped" (as
for "Declined"), and keep danger for real failures like the "index is locked" card below it.

**P2-4. Failure states use the warning triangle (P2-D, not done everywhere).** The failed turn (`*-13`, in danger
red) and the failed tool card (`*-13c`) both use the triangle that the Problems pane now reserves for warnings. A
colour-blind user reading by shape sees "warning" for an error.
- Rule 40 applies here: this is the second time glyphs have disagreed across surfaces. Give the shared set one
  `status → glyph` map (error = circle-×, warning = triangle, info = circle-i), used by Problems, the assistant, toasts
  and job cards.
- Add a lint that forbids `alert`/triangle next to `--pd-danger`.
- The approval's "This cannot be undone." is a warning, so the triangle is right there.

**P2-5. Deleting the open conversation leaves it open.** `*-13e` and `*-14`: after "Deleted 'Request handler'", the
heading still says "Request handler", and its whole transcript is still shown under the Undo strip. The list has
already dropped the row, so the view and the list disagree. If the user types now, it is unclear where the message goes.
- On delete, switch to the next conversation, or to a new one, at once, and restore both on Undo.
- Add a unit test: delete the active conversation → the heading is no longer its title → Undo → it is active again.

**P2-6. The demo icon is nearly the shell demo's icon.**
- **Tile colour:** the warm stone #DAD5CB is ΔE 2.4 from the shell demo's silver #D1D4DA (OKLab ×100, normal vision;
  2.3 protan, 2.5 deutan, 1.9 tritan, from `cvd.mjs`). The family floor is 17.7 normal and 10.7 tritan, so the two tiles
  are the same colour to everyone.
- **Glyph:** both are a window with a title bar and a left sidebar of three pills. Only the right area differs (two
  panes against lines). At 1024 you can tell them apart side by side. At 32 and 16 px they reduce to "a window with a
  divider" on a pale tile.
- **The rule:** icons/README says "hue is never the only cue: each glyph has its own silhouette", and here neither the
  hue nor the silhouette separates the two demos.
- **What holds:** against the seven family tiles the stone keeps protan, deutan and tritan at their minimums. Normal
  vision drops from 17.7 to 16.3 (Spotter~wb). The ink #26231E is 10.7:1 on the base and 8.4:1 on the bottom stop. The
  tray template is a clean solid shape.
- **Fix:** keep the warm stone, which is a nice material. Give the workbench a silhouette the shell demo does not
  have: the dock's split with a tab strip on the panes (two tabs on the top pane), or the activity bar as a vertical
  strip of dots, with no sidebar pills. Or move the tile to near-black, the other neutral that `cvd.mjs` showed keeps
  the family minimums. Record the cvd numbers in icons/README (they say "not run").

### P3 (polish)

- **P3-1.** Shot 12 does not show the tool and arguments that its caption promises. The spec clicks the `<details>`
  element at its centre. The summary is `inline-flex` at the left, so the click lands on empty space and does not
  toggle. The mouse click also hides the focused-Decline default. Click the summary, then re-focus Decline (or press
  Tab back to it) before the shot, so that both are visible.
- **P3-2.** At 320 px the overflowed tab strip is scrolled: a sliver of the previous tab shows at the start, and the
  dark theme shows a scrollbar thumb under `main.rs`. Reflow instead: only the tabs that fit, the active one always
  among them, the rest in `⌄ n`, and the strip's scrollbar hidden.
- **P3-3.** Jobs slide-out (carried P3-7): drop the empty header row, or put "1 running" in it. Bound the panel to the
  dock group it belongs to, so it does not cover the Assistant's composer.
- **P3-4.** While the first turn of a conversation runs, the heading says "New conversation" (`*-10`, `*-13b`). It
  changes only when the turn ends. Set the title when the question is sent.
- **P3-5.** The same context chip is labelled in two ways: "Current file: src/main.rs" in the user bubble, and
  "Current file src/main.rs" (muted path, no colon) in the composer. Use one chip component.
- **P3-6.** A running tool card shows a static clock and "Working…" (`*-13b`). A clock reads as "queued". Use the 14 px
  spinner from the status bar (static under reduced motion), and keep the text.
- **P3-7.** The severity column (carried P3-3) is 28 px plus the cell's horizontal padding, about 52 px before the
  problem text at 320 px (22 % of the table). Give the glyph column 4–8 px padding, so the glyph sits about 12 px from
  the text.
- **P3-8.** At ≤ 480 px the one-line toast truncates its own sentence ("Layout adjusted to fit this …"). Use a short
  form that fits, such as "Layout fitted", with Undo, and put the full sentence in Notifications.
- **P3-9.** The mock gives the same pipeline answer to every prompt, including after a declined erase (`*-12b`) and a
  failed tool (`*-13c`). That reads as the assistant ignoring what just happened. Give `/write` (declined), `/fail` and
  `/tool-fail` short answers that fit what happened ("I did not erase anything."), so reviewers judge realistic content.
- **P3-10.** The palette footer (`> Commands  @ Go to symbol  # Search text`) does not mark the current mode. In
  `>show` or `@handle` mode, give the active one text colour and semibold, and leave the others muted.
- **P3-11.** In the conversation list (`*-13d`), the delete button's focus ring touches the window edge (about 1 CSS px
  away), and the list rows have no end inset. Give the rows the 12 px pane inset. The Undo button in 13e has room, so
  match that.
- **P3-12.** `*-17`: the Problems badge sits on top of the activity item's focus ring and hides part of its corner. Keep
  the badge, but draw it inside the ring, or lift the ring above the badge, so the 2 px ring stays continuous.

---

## 3. Checked and fine

- **Parity:** light and dark match in layout and hierarchy in all 30 pairs. The first screen now has the dock in both.
- **Real accents:** olive (dark) and blue (light) reach the selection bar, the active group line, primaries, the
  palette ring and the marks, citations, the toast's Undo, the job stripes and the focus rings. Danger and warning stay
  distinct from both.
- **Focus rings where shot:** tree row, activity item, tab (`role=tab`), palette input and row, fly-out item, composer,
  conversation delete and Undo. All are 2 px accent rings, and the earlier measurements (≥ 4.7:1) still hold. The
  Explorer menu items are the exception (P1-1).
- **Destructive approval:** the danger tint, "This cannot be undone.", danger-text Allow that names the action, and no
  filled primary in the card. This is a good pattern to reuse in every app.
- **Error state of a turn:** the icon, the plain-words heading, the cause with a next step, and Retry plus Show
  details. Apart from the glyph (P2-4), it matches the design guide §Error states.
- **Palette:** groups (Recently / Frequently used / Other commands), marks, the kind glyph with a path detail, a
  did-you-mean that quotes the words without the prefix, a no-match message, and macOS key glyphs.
- **Running job:** the phase label with a growing count, indeterminate stripes, Cancel, and the same phase with a
  14 px spinner in the status bar (rules 11 and 12).
- **Pop-out:** the document name in the title bar, Dock back with the dock-in glyph, and one shared inset.
- **320 px:** no horizontal page scroll. The status bar keeps its start item, Jobs and the bell with a count. Both 320 px
  states are free of clipped controls.

## 4. What approval needs

- **P1-1:** fixed with a failing e2e first, then 07b re-shot showing a focused item.
- **P2-1 to P2-6:** fixed, or the icon change (P2-6) taken to the user as a decision.
- **Re-shoot:** 07b, 12 (Details open, Decline focused), 13c, 13e and one light code block. Add the demo icon at 16,
  32 and 1024 beside the shell demo's.

A fourth review can then be limited to those shots.
