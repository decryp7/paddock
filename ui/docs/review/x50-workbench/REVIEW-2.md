# X50 workbench: independent design re-review (rule 22, review 2)

- Reviewer: independent agent (did not build X50 and did not write review 1). Date: 2026-10-25.
  Branch: `origin/shell/integrate` @ 7095554.
- Inputs: the 44 shots in this folder (@2x; dark with the Pacenotes accent, light with the Debrief accent), REVIEW.md,
  RESPONSE-1.md, `e2e/contact-sheet-x50.spec.ts`, the workbench sources where a pixel needed explaining, ADR 0013,
  `docs/design-guide.md`, Debrief ADR 0056 / 0061, the suite UI rules (ui-review-2 patterns).
- Method: every claim was checked in the pixels. Crops at full resolution; colours sampled from the PNGs; contrast
  computed with the WCAG formula; positions converted from device pixels (÷2) to CSS pixels.
- **Verdict: not approved.** A large step forward. The menu is at the pointer, the "Layout adjusted" notice moved into
  the notification toast, the Problems table now keeps the Problem column at 320 px, focus rings pass 3:1 everywhere they
  were shot, the real accents work, and the assistant's needs-AI state is good. Two P1s remain, both at 320 px:
  two-line Problems rows that spill over their dividers, and a dock cut off mid-control when the Explorer is open.
  There are also eleven P2s, most of them in the assistant states that were shot for the first time.

Counts (new findings in this review): **P1: 2, P2: 11, P3: 16.**
Review 1 (4 P1, 17 P2, 14 P3): **fixed and seen in the pixels: 2 P1, 10 P2, 9 P3**; accepted from code or as a stated
deviation: 5 P2; partly fixed or reopened: 2 P1, 2 P2, 5 P3 (each one is carried into a new finding below).

---

## 1. Review 1 findings, verified

### P1

| # | Verdict | Evidence |
|---|---|---|
| P1-1 menu at origin | **Fixed** | `*-07`: the menu's top-left is at the pointer (CSS ≈ (98, 255), click at row x+60, row middle), well within 8 px. The target row is outlined in the accent. The e2e polls the same 8 px. |
| P1-2 Problems at 320 | **Partly fixed → new P1-A** | `*-22`: the table is 230 CSS px and Problem is ≈171 px (**74 %**, ≥ 60 % ✓). Severity is a glyph ✓, `file:line` folds under the problem ✓ with middle truncation (`src/…request_handler.rs:2`) ✓, rows are sorted by severity ✓. But the rows keep their single-line height, so the second line sits on the row divider (see P1-A). |
| P1-3 notice over header | **Fixed** | `*-20`, `*-21`, `*-22`: a bottom-end toast above the status bar with Undo, "Show in Notifications", close, and the bell count "1". Nothing covers a group header or the tab strip. |
| P1-4 missing states | **Mostly fixed → new P2-K** | @2x ✓, real accents ✓, every row of Problems in view ✓, palette groups / provider / typo / no match ✓, Explorer menu ✓, fly-out with focus ✓, needs-AI / streaming + Stop / tool card + citation / approval with details / failed turn ✓, running job in the pane and the status bar ✓, pop-out ✓, 700 px ✓, 320 px ×2 ✓. Still missing or wrong: `dark-01` has an **empty dock**; `*-18-focus-tab` shows **no focus ring** on any tab; there is no running or failed tool card, no keyboard focus in the Explorer menu, no conversation list with delete and Undo, and no inactive pop-out. |

### P2

| # | Verdict | Evidence |
|---|---|---|
| P2-1 one primary on Welcome | **Fixed** (light) | `light-01`: one filled "Open src/main.rs" and ghost rows with `pd-kbd` keys. Open Pitwall is in another view region (design guide: one primary per view region). This can't be judged in dark because the dock is empty there (P2-K). |
| P2-2 uppercase headings | **Fixed** | "Files" and "Outline" are sentence case, semibold and muted. |
| P2-3 repeated titles | **Fixed** | The assistant heading is the conversation title. The Jobs slide-out no longer repeats "Jobs", but its header row is now empty (P3-7). |
| P2-4 two Outlines | **Fixed** | One Outline section with one row format (`main` / `fn · line 3`) and one empty text. Its source is wrong after a reload, though (P2-F). |
| P2-5 three items "on" | **Reopened → P2-E** | The 2×20 bar on the inner edge and the 4 px dots are right. But while a fly-out is open, two items are drawn selected, and hover tints left in the shots make a third look on. |
| P2-6 menu focus | **Fixed** (fly-out) | `*-08`: a 2 px ring in the accent on surface-3. Dark: #8da74e, ≥ 5.3:1. Light: #1d5fd1, 4.7:1 on the tinted row. The Explorer menu's keyboard focus is not shot. |
| P2-7 tab glyphs | **Fixed** | The × is a 14 px glyph, inactive tabs show it at 40 %, and the ▾ is gone. |
| P2-8 pin means two things | **Partly verified** | Dock back uses the dock-in glyph ✓ (`*-15`). Keep on top is a stated deviation (§2). |
| P2-9 did-you-mean prefix | **Fixed** | `Nothing matches "indx repositry"` and `No command matches "zzzzzz"`. |
| P2-10 match highlight | **Reopened → P2-J** | The style exists in code (`mark`: semibold + 2 px accent underline), but no shot shows a mark. The `@handle` row has none, in either theme. |
| P2-11 rtl truncation | Accepted from code | `pd-path-text` is used for details. Nothing in the shots is long enough to truncate. |
| P2-12 disabled = primary | **Fixed** | A disabled Send is an outline with subtle text. The enabled Open Pitwall is a filled olive/blue button. |
| P2-13 composer live | **Fixed** | The composer is inert with "Start Pitwall to ask questions", and Allow actions is dimmed. The context chip's × still looks live (P3-12). |
| P2-14 forced scroll | Accepted from code | `SCROLL_STICK_PX = 24` and a "Jump to latest" pill. |
| P2-15 aria-live | Accepted from code | The log has `aria-live="off"` and there is one polite `assistant-announce`. |
| P2-16 one-click delete | Deviation accepted (§2) | Not in any shot. |
| P2-17 mono fallback | **Fixed** | The code, path line and pop-out render in a real monospace face (Menlo/SF Mono), not Courier. |

### P3

Fixed and seen in the shots:
- P3-1: palette groups.
- P3-2: no empty status gap.
- P3-5: icon column reserved; no separator in a two-item menu.
- P3-7: chat glyph.
- P3-8: history glyph.
- P3-9: the chip has no gap.
- P3-10: details and full-size buttons.
- P3-11: glyph and sort, though the glyphs are wrong (P2-D).
- P3-13: 40 % ×.

Partly fixed:
- **P3-3**: the hints are sans and tabular, but Control is still a caret (`^⌥O`) → P3-4.
- **P3-6**: the badge sits on the corner with a ring, but it is accent-coloured → P3-10.
- **P3-12**: the overflow tooltip can't be checked in a still, and the overflow chevron points left (`< 3`) → P3-15.
- **P3-14**: there is a status item at the start, but the active file is not revealed after a reload → P2-F.

Not fixed:
- **P3-4**: the symbol row still has the letter badge "S" and the detail "fn in …" → P3-5.

---

## 2. The three stated deviations

1. **Conversation delete = 8 s Undo on a hover/focus button.** **Accepted.** Review 1 offered Undo as an alternative to a
   confirmation, and the code backs it: `UNDO_MS = 8000`, an Undo strip with `role=status`, and the list hides the
   pending row. Two conditions apply. The Undo button must be reachable by keyboard from where focus was (Delete removes
   the focused row, so focus has to move to Undo, not to `body`). And the next sheet must include one shot of the list
   with the danger-text delete visible on focus, plus one of the Undo strip.
2. **Keep on top not shootable in the browser pop-out.** **Accepted, deferred to the first app.** It is a desktop-window
   feature. Condition: the first app that adopts the workbench (Pacenotes) shoots its Tauri pop-out with Keep on top off
   and on, and inactive. Until then a unit test should pin the `layers` glyph and the tinted on-state. The inactive
   dimming has not been seen at all yet.
3. **CDK positions the menu one frame after attach.** **Accepted, with a P3 (P3-16).** One frame is invisible in
   practice. But the e2e uses `expect.poll`, which would also pass if the first painted frame were at (0, 0). Keep the
   pane `visibility: hidden` until the position strategy has applied, or assert on the first animation frame, so that
   a regression to the origin cannot slip through.

---

## 3. New findings

### P1 (blocks approval)

**P1-A. At 320 px the two-line Problems rows spill over their row dividers.**
`*-22-320-problems`: the row height stays at the one-line height (≈32 CSS px) while the cell holds two lines. The problem
text starts against the top divider, and `src/server/mod.rs:11` sits on the next divider (its descenders touch it). This
is the state P1-2 was about, and it reads as broken. Fix this in the X21 table: when a column folds a second line
(`where`), the row gets `min-height: auto` with 6 px vertical padding (≈44 px for two lines), and the virtual scroller
uses that height in folded mode. Add the assertion to the 320 px e2e: no cell's `scrollHeight` exceeds its
`clientHeight`, and the second line's bottom is ≥ 4 px above the row's bottom border.

**P1-B. At 320 px with the Explorer opened, the dock is cut off mid-control (rule 18).**
`*-21-320-explorer`: the side bar takes ≈150 CSS px, and the dock gets ≈88 px but is drawn wider than that and clipped
under the Assistant edge strip. The path line reads `s…main.ı` (a glyph sliced), the maximise buttons are half visible,
the Filter field is cut, the Columns button can't be reached, and the table shows only "Pr" / "th". RESPONSE-1 says the dock
"holds a group to a window smaller than its views' minimum instead of clipping it", but in this state it clips. Fix:
below the width where side bar + dock minimum + edge strips no longer fit (≈ 480 px), open the Explorer as an
overlay fly-out above the dock, dismissed on Escape or outside click, like the auto-hide panels. Never squeeze the dock
under its minimum. Add the e2e: at 320 px with the Explorer open, every visible control in the dock is fully
inside the viewport, or the dock is covered.

### P2 (fix before the workbench ships in an app)

**P2-A. The overflowed tab strip hides the active tab.** `*-22`: the strip shows "Welcome" (inactive styling) with
`< 2`, while the group shows `src/main.rs`. The tab you are looking at must always stay visible. When the strip
overflows, keep the active tab and move the others into the overflow.

**P2-B. Pop-out title bar names the view type, not the document.** `*-15`: "Code", then the path line again. The main
window's tab says "main.rs". Use the panel's title ("main.rs", with the path in the tooltip) for both the title bar
and the OS window title. A user with three pop-outs otherwise has three windows called "Code".

**P2-C. Approval of a destructive write makes "Allow" the accent primary.** `*-12`: "Erase everything" gets a
filled accent Allow next to an outline Decline. For a write tool the safe choice should not look like the default. Make
Decline the secondary on the right and Allow neutral, or danger text when the tool is marked destructive. Name the
consequence ("Allow erase: removes everything in …"). "Arguments None" leaks a null: show "No arguments", or hide the
row.

**P2-D. Severity glyphs are wrong.** Errors use the triangle (the warning shape), and warnings use a circled "i"
(the info shape) in amber (`*-02` and every Problems shot). Use the suite's status glyphs: error = circle-×,
warning = triangle, info = circle-i, each in its status colour with the word as its name. This matters for colour-blind
users, who rely on the shape.

**P2-E. Activity bar: more than one item looks selected.** `*-08`: with the perspectives fly-out open, Files and Layouts
both have the tinted tile and the 2×20 bar. `*-02`, `*-09` and `*-22` also show a neutral tile behind Problems or Assistant
left over from the click, so two tiles look "on". An open fly-out should mark its trigger with `aria-expanded` and a
hover-level tint, never the selected bar. Also move the pointer away before each shot (P2-K).

**P2-F. The Outline and Files do not follow the active document after a reload.** In `*-10` to `*-19`, main.rs is the
active document, yet the Explorer is collapsed and the Outline says "No symbols in the selected file". The Outline is
keyed to the tree selection instead of the active document. Drive the Outline from the active document, and have
`reveal` run on restore as well as on open.

**P2-G. A declined tool is drawn as an error.** `*-13`, `*-14`: the "erase … Declined by the user" card has the danger
border and a warning glyph. The user's own choice is not a failure. Use a neutral card with a muted "Declined", and keep
danger for real tool failures.

**P2-H. A failed turn is not a designed error state.** `*-13`: a single red line, "The model stopped answering.",
indented 16 px further than the answer column, with no icon, no cause and no next step. Design guide §Error states:
"icon in danger, what failed in plain words, likely cause, Retry + secondary". Add Retry (resend the question) and a
secondary "Open Pitwall" or "Show details", and align it with the answer text. Keep the partial text, as the
caption promises. The shot has no partial text to show that.

**P2-I. While streaming, the composer keeps the sent question, and the answer has no progress cue.** `*-10` in both
themes: "/slow Explain the request pipeline" is in the user bubble and also still in the composer. The store sets
`draft: ''` on send, but the textarea is bound with `[value]="store.draft()"`. With zoneless change detection, `fill` +
Enter happen with no render in between, so the bound value never leaves `''`, and the textarea keeps its text. Start
with a failing test (fill then Enter in one task, then the textarea's value is ''). Then write the DOM value on send, or
bind through a `model`/Signal Forms control. Separately, the streaming answer looks exactly like a finished one: add a
subtle progress cue (a caret or three-dot pulse at the end of the text, static in reduced motion).

**P2-J. Match highlighting is still invisible in every shot (review 1 P2-10).** The provider rows (`@handle` →
`handle_request`) carry no marks, and no shot shows a command query with marks. Have providers return match indices (the
names provider can use the same fuzzy matcher as commands), and add a shot of `>show` with marks.

**P2-K. Contact-sheet gaps that hide real states.**
- `dark-01-first`: the dock is empty (no Welcome, no Assistant). The spec waits only for the Explorer's `src` node. If
  the dock really mounts later than the Explorer, the user sees an empty main area on start: render a dock skeleton or
  hold the first paint until the layout is restored (see the webview ready-handshake lesson). In any case, the spec must
  wait for `pd-dock [role=tab]`.
- `*-18-focus-tab`: no tab has a ring. The spec focuses `[data-testid="pd-dock-tab"]` (the inner label), but the
  `:focus-visible` ring is on `.dv-tab[role=tab]`, so focus never lands. Focus the `role=tab` element by keyboard (Tab
  into the strip) and re-shoot. Until then, tab focus is verified from code only.
- Missing: a running and a failed tool card, keyboard focus on an Explorer menu item, the conversation list with delete
  on focus plus the Undo strip, and an inactive pop-out.
- Move the mouse to a neutral spot before each shot, so hover tints don't read as states (P2-E, and the second
  palette row highlighted in `*-19`).
- Replace the mock's "You asked: …" echo with a short realistic answer: a paragraph, a list, inline code and a
  code block. That is what the markdown renderer is meant to show.

### P3 (polish)

- **P3-1.** The Explorer menu opens at the pointer's y in the row's middle, so it covers the lower half of the row's own
  label (`README.md` is cut in `*-07`). Anchor the menu's top to `max(pointer.y, row.bottom + 2)` for pointer opens.
- **P3-2.** The perspectives fly-out doesn't show which perspective is current, and it reserves an icon column it never
  uses, so the labels start 40 px in. Use a check mark in that column for the current item (`menuitemradio`).
- **P3-3.** The severity column is ≈60 CSS px wide for a 14 px glyph in every width (25 % of the table at 320 px).
  Fix it at 28 px, as asked in review 1.
- **P3-4.** Control is still drawn as `^` (`^⌥O`). Use ⌃ on macOS (review 1 P3-3).
- **P3-5.** Symbol rows keep the letter badge "S" and the detail "fn in src/…" (review 1 P3-4, not done). Show the kind
  glyph and "src/server/request_handler.rs".
- **P3-6.** Palette rows mix category styles: "View:  Show problems" has a muted category and a double gap, while
  "Perspective: Understand" is in full text colour with one space. Use one row component.
- **P3-7.** The Jobs slide-out has an empty 40 px header (only pin and ×). The job card sits flush with the panel's left
  and right edges and has no inset. The panel is ≈440 px tall for one card, and it spans the Assistant column, covering
  its composer. Size it to its content (max 50 %), give the card the 12 px pane inset, and either put a muted
  "1 running" in the header or drop the row.
- **P3-8.** In the pop-out, "Dock back" ends 4 px from the window edge, and the path line (24 px inset) and the code
  (16 px) don't share a left edge. Use the 12 px pane inset for all three.
- **P3-9.** The status-bar spinner is 10 px (`.pd-busy .pd-dot`), below the 14 px icon floor. Make it 14 px with a 2 px
  stroke.
- **P3-10.** The activity badge uses the accent fill. Design guide §Badges: navigation counts use a neutral surface-3
  pill, or a status tint when the count means a problem (3 problems → warning or danger tint).
- **P3-11.** The conversation title is the raw first prompt, slash command included ("/slow Explain the request
  pipeline"). Strip leading commands, use sentence case, and truncate with a tooltip.
- **P3-12.** While the assistant is inert (`*-02`, `*-09`), the "Current file" chip's × still looks interactive. Dim it
  like Allow actions, or keep it live on purpose and say so.
- **P3-13.** The approval's "Details" disclosure uses the native ▼ text marker (≈8 px). Use the 14 px chevron `pd-icon`,
  rotating, as the Explorer does.
- **P3-14.** At ≤ 480 px the toast covers about 20 % of the window height (`*-21`, `*-22`) and covers the Problems rows it
  just rearranged. Review 1 asked for a one-line status-bar message ("Layout fitted · Undo") at this width. At 700 px the
  toast is ≈378 px wide, over the guide's 360 px maximum.
- **P3-15.** File nodes and tabs use a letter badge "T" for .rs and .md alike. It carries no information, and it
  contradicts the "no letter badges" direction of P3-5. Use a generic file glyph, or one per type. The overflow
  control `< 3` points left; use a chevron-down with the count, plus the tooltip from review 1 P3-12.
- **P3-16.** The menu-position e2e uses `expect.poll`, so a first frame at the origin would still pass (deviation 3).

---

## 4. Checked and fine

- **Focus rings ≥ 3:1 where shot:**
  - Tree row: #8da74e on #262b22 = 5.35:1 (dark); #1d5fd1 on #dfe8f9 = 4.72:1 (light).
  - Activity item: 6.64:1 dark, 5.82:1 light.
  - Fly-out item: accent ring on surface-3.
  - Palette input and active row: accent ring and accent row bar.
  - Composer: 2 px accent.
- **Icons ≥ 14 px:** activity 18 px, tab and dock actions 14 px, tab × 14 px, chevrons and refresh 14 px. Exceptions are
  the spinner (P3-9) and the disclosure marker (P3-13).
- **Real accents:**
  - Pacenotes olive and Debrief blue reach selection, the active-group top line, primaries, citations, the
    palette ring, the toast's Undo and the progress stripes, and read as the app's colour in each theme.
  - Danger and warning stay distinct from olive.
- **Running job:** the phase label "Finding files" with a growing count ("12 so far" → "24 so far"), never a static 0.
  Indeterminate stripes, Cancel, and the same phase in the status bar with a spinner (rule 11).
- **Stop** replaces Send in the same place (ADR 0061). Disabled "+" while busy. "Stopped." note after Stop.
- **Approval card:** a warning-tinted panel, a plain-language summary, a tool and arguments disclosure, and full-size
  buttons.
- **Palette:** "Recently used / Frequently used / Other commands", footer mode hints, a no-match message without the
  prefix, and a did-you-mean suggestion.
- **Explorer menu:** the shared `pd-popmenu` look, a reserved icon column, and a target-row outline.
- **Parity:** light and dark have the same layout and hierarchy in every shot except `dark-01` (P2-K).
- **No horizontal page scroll** at 320 px. The status bar keeps its start item, Jobs and the bell with a count.
