# X50 workbench: response to review 1

What changed for each finding of [REVIEW.md](REVIEW.md). The new contact sheet (`contact-sheet.png`, the shots beside it)
is for review 2; it is not self-approved.

## P1

- **P1-1 Explorer menu at the origin.** The tree's CDK context-menu trigger opens the menu for the right-click itself (it
  keeps the button's own mouseup/auxclick from closing it again); the row only names itself the menu's target and keeps a
  "menu target" outline while it is open. Shift+F10 and the context-menu key open it below the row. e2e: within 8 px of
  the click and of the row (`workbench-demo.spec.ts`, "P1-1"); shot 07.
- **P1-2 Problems at 320 px.** Severity is a 14 px glyph column (word as accessible name and tooltip; its header is for
  assistive technology only: `PdColumn.headerHidden`, an additive X21 field); Problem first, File drops below 360 px, Line
  below 440 px; below 360 px `file:line` folds under the problem as a muted second line; sorted by severity, then file.
  The dock now holds a group to a window smaller than its views' minimum instead of clipping it, and Problems declares its
  true minimum (240). e2e: the Problem column is ≥ 60 % of the table at 320 px; shot 22.
- **P1-3 "Layout adjusted" over the tab strip.** The dock's notices go to the shell's notifications (toast above the status
  bar, then the bell) with their button: `ShellNotice.action` (additive) keeps such a notice in its window and runs the
  window's `NOTICE_ACTION_HANDLERS` entry; the dock registers Undo/Retry there. Nothing is drawn over a group. e2e "P1-3";
  shot 22 shows the toast at 320 px.
- **P1-4 Missing states.** The sheet is @2x, dark with the Pacenotes accent and light with the Debrief accent, 22 shots each:
  palette with groups, provider, typo and no match; Explorer menu; activity fly-out with focus; assistant needs-AI,
  streaming with Stop, tool card with citation, write approval with details, failed turn; a running job (pane and status
  bar); a pop-out window; focus on a tree row, an activity item, a tab and the palette; 700 px; 320 px with the Explorer
  and with Problems. Problems shows all rows. Making the pop-out shot found a real bug: a pop-out window booted into the
  main window's root element and stayed empty (`mountPopoutWindowHost`, regression e2e).

## P2

- **P2-1** Welcome has one primary (`pd-btn--primary`), the rest are ghost buttons with their keys (`pd-kbd`).
- **P2-2** Explorer section headings are sentence case, 12 px semibold, muted.
- **P2-3** The assistant heading is the conversation's title ("New conversation" when none); an auto-hidden pane's
  slide-out no longer repeats the strip tab's title (Jobs).
- **P2-4** One Outline: the Outline view is gone from the demo; the Explorer's Outline section has one row format
  (`main`, "fn · line 3") and one empty text. Perspectives no longer show two outlines.
- **P2-5** One selected activity item (the one whose Explorer section owns the side bar, or `active`), drawn with a 2×20 px
  bar on the bar's inner edge; views open in the dock get a 4 px dot.
- **P2-6** Explorer, activity fly-outs and the dock menu use the shared `pd-popmenu` items (2 px focus ring, label, hint,
  icon column, danger); `aria-disabled` and `aria-expanded` are styled there.
- **P2-7** The tab × is a 14 px close glyph in a 20 px hit area; the tab ▾ is gone (group "···" and right-click remain).
- **P2-8** Keep on top uses the new `layers` glyph and a tinted on-state; Dock back uses `dock-in`; the dock's pin stays
  auto-hide. (The browser pop-out shows Dock back only: Keep on top needs a desktop window.)
- **P2-9** "Did you mean" and the empty list quote the words without the prefix and name the provider searched.
- **P2-10** Matches are text-coloured, semibold, underlined 2 px in the accent fill.
- **P2-11** Path details use `pd-path-text` (middle truncation).
- **P2-12** A disabled primary drops its fill (outline, subtle text) in `_controls.scss`.
- **P2-13** While the assistant cannot run, the composer is inert with the state's reason ("Start Pitwall to ask
  questions"), the draft is kept, and Open Pitwall is the one primary.
- **P2-14** The log follows the stream only within 24 px of the bottom; otherwise "Jump to latest".
- **P2-15** The log is `aria-live="off"`; one polite announcement when a turn ends.
- **P2-16** Delete is shown on hover or focus, in danger text, with an 8 s Undo.
- **P2-17** `--pd-font-mono` adds Menlo, DejaVu Sans Mono and Liberation Mono before `monospace`.

## P3

- P3-1 groups "Recently used", "Frequently used", "Other commands"; P3-2 the status line takes no room when empty;
  P3-3 key hints in the sans face, 12 px, tabular figures; P3-4 symbol rows show why they matched, no letter badge;
  P3-5 the icon column is always reserved and separators appear only in menus of three items or more; P3-6 the badge sits
  on the tile's corner with a ring; P3-7 `chat` glyph; P3-8 `history` glyph with a tooltip; P3-9 a chip's icon only when
  the glyph exists; P3-10 approval details (tool and arguments), full-size Allow/Decline; P3-11 severity glyph and sort;
  P3-12 overflow tooltip "1 more tab: …" with the chevron; P3-13 inactive tabs keep a 40 % × (no jump);
  P3-14 Explorer providers can `reveal` the active item (the demo's Files follows the file in use) and apps put their
  state at the start of the status bar (the demo's "245 files indexed").
