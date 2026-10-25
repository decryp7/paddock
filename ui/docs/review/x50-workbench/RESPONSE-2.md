# X50 workbench: response to review 2

This file lists what changed for each finding of [REVIEW-2.md](REVIEW-2.md). The regenerated contact sheet
(`contact-sheet.png`, with the shots beside it) is for review 3. It is not self-approved.

## P1

- **P1-A Folded Problems rows.** The X21 table has a new additive field, `PdColumn.foldBelow`. Below that width the row
  gets the `pd-folded` class and grows by `PD_FOLD_EXTRA` (12 px) in the virtual scroller, and the cell template receives
  `folded`. Problems folds `file:line` below 360 px. The 320 px e2e checks two things for every cell:
  `scrollHeight ≤ clientHeight`, and the second line ends ≥ 4 px above the row border. See shot 22.
- **P1-B The Explorer at 320 px.** Below 480 px (`EXPLORER_OVERLAY_BELOW`) the Explorer is an overlay fly-out above the dock,
  with a scrim. Escape or an outside click dismisses it. The dock is never squeezed under its minimum. The e2e checks that
  every visible dock control is inside the viewport, or that the dock is covered. See shot 21.

## P2

- **P2-A** The active tab stays visible in an overflowed strip (`keepActiveTabVisible`). The others go to the overflow.
- **P2-B** The pop-out title bar and the OS window title use the panel's title ("main.rs"), with the path in the tooltip.
- **P2-C** Partly done. Allow is now neutral and Decline is on the right. A missing argument list reads "No arguments".
  Danger styling for destructive tools is still open: tools do not yet declare whether they are destructive (`ToolInfo`
  has no such flag), so every write approval gets the same neutral Allow.
- **P2-D** Each severity has its own glyph: error is a circle with ×, the new `pd:error`; warning is a triangle; info is a
  circle with i. Each is drawn in its status colour, and its accessible name is the word.
- **P2-E** An open fly-out marks its trigger with `aria-expanded` and a hover-level tint (`pd-open`). It never gets the
  selected bar. The pointer now rests at a neutral spot before every shot.
- **P2-F** The Outline follows the active document. In the demo, `selectedPath` is the active Code panel, else the tree
  selection. `reveal` runs on restore as well as on open. After a restore the dock's front panel is the active one, which
  is how the e2e catches this regression.
- **P2-G** A declined tool shows a neutral card with a muted "Declined". Danger styling is kept for real failures
  (shots 12b and 13c).
- **P2-H** A failed turn has a designed error state: an icon, what failed, the likely cause, Retry (which resends the
  question) and Show details. It is aligned with the answer text, and any partial text is kept (shot 13).
- **P2-I** The composer clears on send. A failing test came first: fill and Enter in one task must leave the textarea
  empty. While an answer streams, a three-dot pulse shows; it is static under reduced motion. The log also follows the
  user's own question, scrolling after the new turn renders.
- **P2-J** Provider results carry match marks (the names provider uses the same fuzzy matcher as commands). Shot 03b shows
  `>show` with marks, and shot 04 shows `@handle`.
- **P2-K** The sheet now waits for `pd-dock [role=tab]` before the first shot, and the dock shows a skeleton until its layout
  is restored. Tab focus comes from the keyboard on `role=tab` (shot 18). New shots:
  - a running tool (13b) and a failed tool (13c);
  - the conversation list (13d) and Undo after a delete (13e);
  - the Explorer menu from the keyboard (07b).

  The mock answers with a paragraph, a list, inline code and a code block.

## P3

- **P3-1** A pointer-opened menu has its top at `max(pointer.y, row.bottom + 2)`.
- **P3-2** The perspectives fly-out uses checkbox items with a check mark on the current perspective.
- **P3-3** The severity column is 28 px, unsortable and `headerHidden`.
- **P3-4** Control is drawn as ⌃ (U+2303) on macOS. The `^` in shot 03 was the old build.
- **P3-5** Symbol rows have no letter badge. The detail is the file path, for example `src/server/request_handler.rs`.
  There is no per-kind glyph yet.
- **P3-6** Palette rows are one component, with the category in muted text and a single space. Perspective commands now
  have the category "Perspective".
- **P3-7** The Jobs slide-out is sized to its content (at most 50 %) and the card has the 12 px pane inset.
- **P3-8** The pop-out's Dock back, path line and code all use the 12 px pane inset.
- **P3-9** The status-bar spinner is 14 px with a 2 px stroke.
- **P3-10** Activity badges use a neutral pill by default, or a status tint (`badgeTone`). Problems is drawn in danger.
- **P3-11** Conversation titles drop a leading slash command, use sentence case, and truncate with a tooltip.
- **P3-12** While the assistant is inert, the context chips are dimmed and disabled.
- **P3-13** Details uses the 14 px chevron glyph and rotates it.
- **P3-14** A toast is at most 360 px wide. At 480 px and below it is one line above the status bar.
- **P3-15** File nodes and tabs use the `pd:file` glyph. The overflow control is a chevron-down with the count.
- **P3-16** A unit test checks the menu's first frame: at the moment it is inserted it is hidden and unplaced. The e2e
  checks the first frame too, not just a poll.

## Deviation follow-ups

1. **Undo after Delete.** Focus moves to Undo after a delete (unit test), and shot 13e shows it.
2. **Keep on top.** A unit test covers the glyph, the tint when on, and hiding it where it cannot work. It still cannot be
   shot in a browser pop-out. The first desktop app shows it.
3. **First frame of the menu.** Covered by P3-16.

## Gaps closed

- **Pop-outs in the layout.** Popped-out panels are saved with the layout, including their slot. On restore they reopen in
  their windows. A panel whose window failed is docked back.
- **Layout dialogs.** Save layout and Manage layouts are real dialog windows with declared kinds. The demo's dialog window
  serves them over `layout_get`/`layout_set`, which are open to dialogs through `dialog-contracts.json`. The app embeds
  that file, and the plugin refuses `layout_get` from a dialog without a contract.
- **Desktop paths.** `workbench-demo-desktop.spec.ts` plays the main, pop-out and dialog windows over a faked Tauri IPC.
- **Release scripts (rule 42).** There is one `scripts/demo-release.mjs` for both demos, and `scripts/demo/` takes the app
  as an argument.
- **`new-app.mjs --template workbench-demo`** starts an IDE-style app. The copied app points at its own dialog contracts.
- **`cargo deny`** runs for the demo app (`apps/workbench-demo/src-tauri/deny.toml`, as a CI job and in the release
  workflow).
- **Demo icon.** It is on the contact sheet.

## Also fixed: order-dependent dock specs (rule 5)

The Angular builder runs Vitest with `isolate: false`, so spec files reused in one worker share a window and a module
registry. Which files share a worker depends on timing, which is why the failures showed up only in full runs.

- **dock-attach.** It expected dockview not to be loaded yet. An earlier file in the worker had already imported it, so the
  cached import resolved during the mount. The spec now holds the import (`forgetDockviewForTest(load)`) and waits on the
  dock's own load promise. The fix was confirmed against a full run in which this spec failed.
- **Leftovers from a failing test.** Fake timers, spies on DOM prototypes, stubbed globals, page nodes and storage leaked
  into the next file. `installTestKit` now resets all of them before and after every test. A regression spec in
  `testing.spec.ts` failed before this fix, and its leftovers also broke the next describe block.
- **Sleeps and dead state.** The two 5 ms sleeps in `dock.spec.ts` became `DockPersistence.flush()`. The unused
  module-level `liveDock` was removed.

After the fix, two full `ng test paddock-ui` runs passed: 172 files, 2036 tests each.

## Known gaps

- P2-C: danger styling for destructive tools needs a `destructive` flag on tools (paddock-assistant `ToolInfo`).
- P3-5: symbol rows have no per-kind glyph.
- Keep on top cannot be shot in the browser.
- A newly generated app's build has not been run end to end in CI.
