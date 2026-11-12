# Independent design review: file-type glyphs, Files page, Settings > Storage (plan X31/X32)

Reviewer: independent agent (rule 22), 2026-10-25. I did not build these parts. This review is read-only; I changed no code.

Inputs: `contact-sheet.png` and the 8 PNGs in this folder (2x device pixels, light and dark), on `origin/shell/integrate` at
`b566bdb`. Code read for causes: `projects/paddock-ui/icons/src/paths.ts`, `shell/storage/page/src/{storage-page,styles}.ts`,
`shell/storage/src/tree-rows.ts`, `shell/storage/dialog/src/storage-confirm-dialog.ts`, `shell/files/src/recent-files.ts`.
References: `docs/design-guide.md` (§6 icons, controls, tables, dialogs), the UI review 2 patterns (rules 1-20 below), `../REVIEW-2.md`
and `../x21-data/REVIEW.md`. Measurements come from the PNG pixels: 1 CSS px = 2 device px.

**Counts: 2 P1, 11 P2, 9 P3.**

**Verdict: not approved.** The four glyphs are good. They sit on the grid, use the family stroke and caps and take
`currentColor`, and at 16 px `file`, `file-text` and `archive` read clearly. They need only small geometry fixes, so I
approve them once P2-1 and P3-1 are fixed. The recent-files list is close to the bar: it has hover actions, a middle
ellipsis and a tooltip. Storage has the right content and safe behaviour. Clearing asks first in a modal window, Keep is
the default and Clear is the danger action. Lower bounds are honest, and jobs show progress and offer Cancel. But the
Storage page does not look professional yet. Its rows carry walls of bordered text buttons, one of them destructive, and
the buttons wrap. One row's divider is broken. The size tree shows a folder's total as a sibling of its own children, so
the Share column adds up to more than 100 %. The evidence is also incomplete: the brief asks for 320 px and the confirm
dialog, and neither was captured.

---

## P1

### P1-1: The size tree double-counts. A folder's total is listed as a sibling of its children (`*-storage-tree.png`)
This is the implementer's known item, and it is worse than the name repeating. Under the group "thumbnails 3", the rows
are `thumbnails 29 MB 62.5 %`, `data 14 MB 31.3 %` and `index 14 MB 31.3 %`, all at the same indent. Within the group
the shares add up to 125 %, and on screen the cache's shares add up to about 187 %. A reader cannot tell that `data` and
`index` are inside `thumbnails`. The group row has a count (3) but no size, which breaks pattern 6 (the group row carries
the aggregate). The cause is in `tree-rows.ts:43`: the code pushes "its own line first" for every top-level folder, then
flattens the children with no depth.
**Fix:** drop the self row. Put the folder's size and share on the group row (right-aligned in the Size and Share
columns, 600) and count only the children. Deeper levels already get a `parent/child` prefix, so keep that, or indent
16 px per level with the folder glyph. The shares of the children of one group must add up to the group's share. Add a
test that asserts the sum.

### P1-2: Storage rows use bordered text buttons at rest, including a destructive one, and they wrap (`*-storage.png`)
Every category row shows `Folders` and `Show` as bordered 32 px text buttons. Logs adds `Clear…`, which wraps onto a
second line under the other two, so the action column is ragged and rows are 95 px, 76 px and more tall. This breaks
pattern 2 (no bordered or text button at rest; destructive actions only in the menu, in danger text) and pattern 8
(uniform rows). The Logs row also has a broken divider: under Kept it sits at y=993, under the other cells at y=1003. The
cause is `.pd-storage__name { display: flex }` on a `<td>`, which takes the cell out of table layout. A `<div>` inside
the cell avoids it. `Folders` and `Show` are also unclear labels. They mean "What is inside (size tree)" and "Show in
Finder/Explorer", but side by side they read as synonyms.
**Fix:** give each row one line. The Kept cell shows the name, plus the note as a muted 12 px line only if it fits;
otherwise put the note in a tooltip. On hover or focus, show two icon buttons: `chart`/`layers` "What is inside" and
`folder`/`external` "Show in Finder". Add a `…` menu with the same items plus `Clear…` in danger text, and make the same
list the context menu. Better still, render this table with the shared `pd-table`, which already has hover actions,
menus, sorting, column priorities and a 320 px layout (rules 39/42: one table, not a second hand-rolled one).

---

## P2

### P2-1: `file-code` cannot be read at 14 px and barely at 16 (`*-glyphs.png`)
The brackets are 2 × 4 grid units (about 1.2 × 2.3 px at 14 px). With a 1.5 px stroke they merge into a blob, so at
14/16 px `file-code` reads as "file with a smudge", not as code. **Fix:** make them 3 units wide and 6 tall, and lower
them into the page's body: `M10 11.5 7.5 14l2.5 2.5M14 11.5l2.5 2.5-2.5 2.5`. That gives wider bracket arms and keeps
the 1.5 px gap from the outline. Check again at 14 px.

### P2-2: `folder` means two things in one recent-files row
`recent-files.ts` draws a folder entry's kind with `pd:folder`, and draws the Reveal action button with the same glyph.
On a folder row, the kind and the action look identical. The guide says same metaphor, same icon, so one metaphor needs
one glyph. **Fix:** Reveal uses `external`, or add a `reveal` glyph (a folder with an arrow), and keep `folder` for the
kind.

### P2-3: Recent files lead with the path, not the name (`*-files.png`)
Each entry is one muted mono line, `/demo/projects/a-very-long-project-folder-n… service-…log`, so there is no anchor
to scan by. The component supports `name` (600), but the entries leave it empty. **Fix:** when no name is given, use the
base name as the title (14/500) and show the parent path as a muted 12 px mono line. That is pattern 14 hierarchy, and
it matches Finder and VS Code. Paths in mono are 13 px; pattern 10 says 12 px.

### P2-4: The middle ellipsis loses the separator
The ellipsis reads `…-folder-n… service-2026-…`: a space after the ellipsis, and no `/` before the file name. Readers
then take `service-…` for a continuation of the folder name. **Fix:** keep the tail's leading separator
(`…/service-2026-10-24-11-30.log`) and draw no space. Add a test in `path-text`.

### P2-5: The page's one primary is the destructive "Clear cache (46 MB)…"
The summary line fills `Clear cache` with the accent, next to a secondary `Measure again`. Clearing the cache is safe,
but the accent primary should be the thing people come to do, and a removal styled as the accent call to action is the
wrong emphasis (guide: destructive actions are danger-styled and confirmed, never the default emphasis). Cache also has
no row-level Clear, while Logs does, so the page has two ways to clear and they look different. **Fix:** make
`Clear cache…` a secondary button in the summary row, or put it in the Cache row's menu like the others. The page then
has no filled primary; that is correct for a settings section.

### P2-6: Redundant kind label: "Cache / Cache"
The Cache row shows the title `Cache`, then a kind line `Cache`, then the note. "Your data" under Logs carries
information, but repeating the title breaks pattern 7. **Fix:** show the kind only when it is not the title. Better,
group the rows "Cache (rebuilt by the app)" and "Your data (cannot be rebuilt)" as group rows (pattern 6). That also
explains which rows can be cleared together.

### P2-7: The Size header is left-aligned over right-aligned numbers
`Size` starts at x=1194 while the values end at x=1353. `.pd-storage__table th { text-align: start }` (0,1,1) overrides
`.pd-storage__size { text-align: end }` (0,1,0). **Fix:** use `th.pd-storage__size { text-align: end }`. The tree's
`Size` header is also 5 CSS px off its values (it ends at 1539/2 and the values at 1529/2), probably space reserved for
the sort icon. Align the values to the header text.

### P2-8: Lower-bound markers in prose
"The app keeps ≥ 4 GB on this computer" puts a math symbol in a sentence. Rounding also has to go down: a lower bound
made of 46 MB + 6.2 MB + ≥ 3.9 GB is not proven to be 4 GB. **Fix:** write "at least 4.0 GB" in the sentence (round
down, `floor` to one decimal). Keep `≥ 3.9 GB` in the cell, and explain the marker in the cell's tooltip ("At least: the
walk stopped early"). The tooltip already exists (`exact(c)`); check that it says why.

### P2-9: The tree has no file or folder glyphs, and its context is off-screen (`*-storage-tree.png`)
The new glyphs this change adds are not used in the one table that lists files and folders. The tree's `h3` ("What is in
Cache") is scrolled out of the shot, so the image shows a bare "7 rows" table with no title. Also, "7 rows" counts data
rows while 3 group rows are visible. **Fix:** give the Item column a 16 px kind glyph (folder, file-text, archive, file),
like the recent list. Keep the title, the Close/Collapse link and the capped note in view above the table. Say "7 items".

### P2-10: Missing evidence: 320 px, the confirm dialog, hover/focus states
The brief and rule 18 ask for 320 px, but no narrow capture exists. The container queries drop the Folder column at
560 px. At 320 px, a 13 em action column with three bordered buttons plus a 6 em size column leaves almost nothing for
the name. I expect the x21 P1-1 failure again, and fixing P1-2 through `pd-table` removes the risk. The confirm dialog
(`storage-confirm-dialog.ts`) reads well in code: it names what and how much, Keep is the default, and Clear is danger.
But it was not captured, and it is the most important destructive surface. There is also no shot of the recent row's
hover actions or the path tooltip. **Fix:** add `*-storage-320.png`, `*-files-320.png`, `*-storage-confirm.png`
(cache and data versions) and `*-files-hover.png` to the contact sheet.

### P2-11: Screenshots break pattern 20
The shots are 900 × 640 CSS px, not 1440 × 900. The Storage table is cut mid-row (the Models row's buttons are clipped
at the dialog's scroll edge), and the tree is cut at the "Smaller items" group. The tree shot has a row hovered at rest
(`index` in the first group is tinted). **Fix:** capture at 1440 × 900 @2x, scroll each subject fully into view (or
raise the dialog's height for the capture), and move the pointer away.

---

## P3

1. **Fold geometry of `file`, `file-text` and `file-code`.** The outline's corner runs from (14, 3.5) to (18.5, 8), but
   the fold is drawn at x=13.5 and y=8.5. The fold misses the diagonal by 0.5 units, which leaves 1 px stubs at 48 px.
   Use `M13.5 3.5V8.5H18.5` with an outline of `M6 3.5h7.5l5 5v12H6z`, or move the fold to 14/8. The page also spans
   x 6-18.5 (centre 12.25); centre it at 5.75-18.25.
2. **Corner style differs across the file family.** `folder` has 1-unit rounded corners, while `file`, `archive` and
   the file kinds are square, and they sit side by side in one list. Round joins already soften the corners; pick one.
   Square is fine if `folder`'s radius drops to 0.5 or below.
3. **`archive` lid at 14 px.** The 4-unit lid becomes a solid 2 px bar. A 3.5-unit lid (`M4 5h16v3.5H4z`, body from 8.5)
   reads lighter beside the page glyphs. As a file kind for .zip, the box is the common metaphor (Lucide, Phosphor), so
   keep it.
4. **The contact-sheet glyph labels use a Courier fallback.** Use the suite mono stack (pattern 10). This is review
   tooling only.
5. **"Clear the list" is an underlined link 16 px in from the heading.** It is fine as a quiet action. Align its text
   with the row icons, or make it a ghost button in the section header ("Recent files · Clear"), as pattern 14 says
   section actions go.
6. **The Share column uses "25 %" with a space.** Match the family's number format (format package); other Paddock
   tables write `25%`.
7. **Group names are raw folder names** (`thumbnails`, `previews`) next to the sentence-case `Smaller items`. That is
   correct for real names, but render real names in mono (they are paths) so they read as names, not labels.
8. **The Files demo page has five equal bordered buttons that wrap to two rows**, and demo controls ("Simulate a drop",
   "Try an unchosen path") sit beside real ones. This is demo-only, but it is the reference page: make `Open a file…` the
   primary, and put the demo affordances in a "Demo" row or a `…` menu.
9. **The Settings sheet is in-app and does not dim what is behind it.** This is a browser-build stand-in for the OS
   window (rule 17); the tests should confirm that the desktop build opens Settings as a window. Out of scope here; noted
   only.

---

## What is good (keep)

- Glyphs: one 24 px grid, a non-scaling 1.5 px stroke, round caps and joins, `currentColor`, the same weight in both
  themes, and a clear silhouette for `file`, `file-text` and `archive` at 14/16 px.
- Recent files: icon actions appear on hover and focus (they are always visible on touch), with 28 px targets, a tooltip
  with the full path, and refusals shown as inline text, not a dialog.
- Storage: it says plainly what it keeps and where, and that nothing is removed outside the app's own folders. Sizes are
  tabular and lower bounds honest. It shows job progress, "Still working" and Cancel, and an error with Retry. The
  confirm window names the thing and the size, defaults to Keep, and says when data cannot be rebuilt.

## To re-review

Fix P1-1 and P1-2, then P2-1 to P2-9, and recapture per P2-10 and P2-11 (including 320 px and the confirm dialog in
both themes). Then I would expect to approve.

---

# Review 2 (2026-10-25, `origin/shell/integrate` at `3bfeffc`)

Same reviewer, read-only. Inputs: the 22 recaptured PNGs (1440 × 900 @2x, and 320 × 640 @2x for the narrow shots), the new
contact sheet, and the diff `b566bdb..3bfeffc` of `icons/src/paths.ts`, `shell/storage/**`, `shell/files/src/recent-files.ts`,
`src/lib/path-text.ts`, `table/src/{table,model}.ts` and `e2e/contact-sheet-x3x.spec.ts`.

**Counts now: 0 P1, 2 P2, 6 P3 (new or left over).** Of the first review, both P1s, 10 of the 11 P2s and 6 of the 9 P3s are
fixed. Three items were left as agreed deviations (below). P2-10 is still open because the narrow shots show the wrong
thing.

**Verdict: not approved yet, blocked on evidence only.** The glyphs, the Files list, the Storage table, the size tree and
both confirm windows now meet the bar at 1440. They read calm and tidy, and the information is right. What still
blocks approval is rule 18: neither 320 px shot shows the part it is named after (P2-R1). Recapture them; if the Storage
table and the Files list are clean at 320, this is approved without another full pass. P2-R2 can ship as a follow-up.

## First-review items, checked in the pixels

| Item | State | Evidence |
| --- | --- | --- |
| P1-1 tree double-counts | **Fixed** | `*-storage-tree.png`: groups `thumbnails` 29 MB 62.5% = 31.3 + 31.3 (rounding); `previews` 25% = 12.5 + 12.5; groups add up to 62.5 + 25 + 12.5 = 100%. The self row is gone, each group row carries its size and share, and the count counts children (2). |
| P1-2 row buttons at rest | **Fixed** | `*-storage.png`: shared `pd-table`, one 44 px line per row (name 600 + muted note on the same line), only a faint `…` at rest. `*-storage-hover.png`: layers + reveal icons on hover. `*-storage-menu.png`: "What is inside", "Show in the file manager", a divider, then "Clear Logs" last in danger. The broken divider is gone with the hand-rolled table. |
| P2-1 `file-code` at 14/16 | **Fixed** | Brackets are now 2.5 × 5 units, lower in the page; at 14 px they read as `<>`, and at 16 px clearly. |
| P2-2 folder = kind and action | **Fixed** | New `pd:reveal` (folder + arrow) for the action; `folder` stays the kind. `*-files-hover.png` and the menu use it. |
| P2-3 name first | **Fixed** | `*-files*.png`: file name 14/500, parent path below in muted 12 px mono. |
| P2-4 ellipsis drops `/` | **Fixed (by test)** | `path-text.ts` keeps the separator in its own non-shrinking span; `parts.spec.ts` asserts `/` and `\`. Not visible in the pixels, because the path fits at 1440 and the 320 shot does not show the page (P2-R1). |
| P2-5 destructive primary | **Fixed** | "Clear cache (46 MB)…" is secondary; the section has no filled button. The Files demo's one primary is "Open a file…". |
| P2-6 "Cache / Cache" | **Fixed, one echo left** | Kinds became group rows ("Cache: rebuilt by the app", "Your data: cannot be rebuilt"). See P3-R1. |
| P2-7 Size header alignment | **Fixed on Storage**, 5 px off on the tree | Storage: header and values share the right edge (x 1786). Tree: "Size" ends at 1819, values at 1812 (display px), about 5 CSS px. See P3-R2. |
| P2-8 lower bounds | **Fixed** | "The app keeps at least 3.9 GB on this computer" (rounded down); cells keep `≥ 3.9 GB`. |
| P2-9 tree glyphs and context | **Fixed** | "What is in Cache" title with a Close link, "5 items", a `file` glyph per row, and group names in mono. |
| P2-10 missing evidence | **Partly**: hover, menu and confirm shots are present and good; **320 is not** | See P2-R1. |
| P2-11 capture rules | **Fixed** | 1440 × 900 @2x, subjects scrolled into view, nothing hovered by accident. |
| P3-1 fold geometry, centring | **Fixed** | Outline `M5.75 3.5h7.75l4.75 5v12H5.75z` and fold `M13.5 3.5v5h4.75` meet exactly; the page is centred on 12. No stubs at 48 px. |
| P3-2 folder corners | **Deviation accepted** | `folder` keeps its 1-unit radius. At 14–24 px, round joins make the difference invisible; at 48 px it is a mild mismatch, but folder shapes are rounded in every family I compared, so it is fine. |
| P3-3 archive lid | **Fixed** | 3.5-unit lid, lighter at 14 px. |
| P3-4 contact-sheet mono | **Fixed** | Labels in the suite mono stack. |
| P3-5 "Clear the list" | **Fixed** | Now aligned with the name column. |
| P3-6 "25 %" | **Fixed** | `25%`, `62.5%`. |
| P3-7 mono for real names | **Done** through `groupMono`. The note says it was dropped, but the pixels show mono group names. See P3-R3 for a side effect. | |
| P3-8 Files demo buttons | **Fixed** | One primary, and a labelled "Demo controls" row. |
| P3-9 Settings as an OS window | **Out of scope (agreed)** | |
| Tooltip shot | **Deviation accepted** | A native `title` cannot be screenshotted; the unit tests that assert `title` = the full path are enough. |

## New shared `pd-table` inputs

- **`groupSummary`** (rows → cells by column key): this is the right shape, an aggregate drawn in the group row's own
  cells. It only covers shown columns, leaves the first column for the name and adds the empty action cell, so the grid
  lines up (Storage: 46 MB / ≥ 3.9 GB under Size; tree: Size and Share). Muted tabular figures for the aggregate are
  correct: they read as a total without competing with the rows. One limit: the summary is text, so sorting groups by
  size is not possible. That is fine until someone asks for it.
- **`groupMono`**: it does what it should for path-like groups, but it is all or nothing (P3-R3).
- **`countLabel`**: correct, and the tree uses it ("5 items"). The Storage categories table does not, and still says
  "3 rows" (P3-R4).

## P2

### P2-R1: Neither 320 px shot shows its subject
`*-files-320.png` shows the Settings sheet over the Files page, not the Files page. In the spec, `page.goto('#/files')`
only changes the hash, so Settings, opened earlier for the confirm shots, stays open; `Escape` closed only the confirm
window. `*-storage-320.png` shows Settings stacked (nav above content), with the visible part ending at "Clear
cache (46 MB)…". The table, which is what rule 18 asks about, is below the fold, and "Measure again" is not visible
either. Both shots also show the shell's menu bar wrapping at 320: "Help" drops onto a second line and overlaps the page
(y≈70). That is shell, not X31/X32, but it is in these pictures and it breaks rule 18. **Fix:** close Settings (or
reload) before `files-320`, and scroll the Storage table (and the summary row) into view for `storage-320`, plus one shot
of the row menu open at 320. File the menu-bar wrap as its own shell item (collapse the menus into one "…" or a
hamburger under about 400 px).

### P2-R2: The confirm window says its title twice
`*-storage-confirm-*.png`: the title "Clear Logs?" is followed by the body "Clear Logs? Up to 4.8 MB is removed."
(pattern 7: one value per fact). **Fix:** the body starts with the facts: "Up to 4.8 MB is removed. This is your own
data. It cannot be rebuilt, and clearing it cannot be undone." Also, the label is used mid-sentence as written ("Clear
Cache?"); in sentence case the title is "Clear cache?". Lower-case the category label in prose, or write "Clear the
cache?". The rest of the window is right. "Up to 4.8 MB" of 6.2 MB is honest (today's log is kept), Clear is danger,
Keep is the default, and the dark scrim works.

## P3

1. **P3-R1, a group of one that repeats itself.** "Cache: rebuilt by the app · 1" holds a single row "Cache — Rebuilt
   when the app needs it." with the same 46 MB, so the group title and the note say the same thing. The tree's
   "Smaller items · 1" group holding "… smaller items" is the same pattern. Drop the note when the group says it, and
   draw a fold row as a plain row, not a group of one.
2. **P3-R2, tree "Size" header 5 CSS px right of its values.** The Share column lines up and Storage's Size column lines
   up, so this is specific to the tree's Size column (probably a sort-indicator gap). Same fix as P2-7.
3. **P3-R3, `groupMono` sets "Smaller items" in mono.** A synthetic label is not a path. Make it a predicate
   (`groupMono: (key) => boolean`), or let the row model mark literal groups.
4. **P3-R4, "3 rows" on the Storage table.** Use `countLabel` ("3 kinds", or no count at all). For a 3-row settings table,
   the "3 rows · Columns" toolbar is chrome that buys nothing, so consider hiding the toolbar below about 6 rows.
5. **P3-R5, the "… smaller items" row uses the same `···` glyph as the row-actions button** in the same column band, so
   it looks like a menu. Use no glyph, or a muted `minus`/stack glyph.
6. **P3-R6, the `reveal` arrow reaches x = 21.5,** past the set's 2.5 px safe margin (it is not one of the listed wide
   glyphs). At 14–16 px it still reads well. Either pull the arrow in by 1 unit or add `reveal` to the wide-glyph list
   in the `paths.ts` doc comment.

## To approve

Recapture `*-files-320.png` and `*-storage-320.png` per P2-R1. If the Storage table is still one line per row with
nothing scrolling sideways and the Files list keeps the name and the `…/name` ellipsis, this is approved. P2-R2 and the
P3s can follow in the next shell batch.

---

# Review 2 verification (2026-10-25, `origin/shell/integrate` at `7f39670`)

Same reviewer, read-only. I checked the six 320 px shots (`*-files-320`, `*-storage-320`, `*-storage-320-menu`), the
recaptured confirm and tree shots, and the diff `3bfeffc..7f39670` of `table/`, `shell/storage/`, `shell/files/`,
`icons/src/paths.ts` and `e2e/contact-sheet-x3x.spec.ts`. As asked, I left out the shell menu-bar wrap at 320.

**Verdict: not approved. Two small 320 px defects remain (V-1, V-2).** Everything else from review 2 is fixed in the
pixels. Both defects are narrow fixes, and I expect to approve once both are re-shot.

## Fixed (checked)

- **P2-R1, the 320 shots show their subjects.** `about:blank` before each shot means Files is no longer covered by Settings,
  and the Storage table is scrolled into view, with a separate row-menu shot.
- **P2-R2, the confirm window.** The title is "Clear cache?" / "Clear logs?" in sentence case, and the body starts with
  the facts ("Up to 46 MB is removed. The app rebuilds it when it needs it."). The menu item reads "Clear logs".
- **P3-R1.** The Cache note now says "Thumbnails and previews." instead of echoing the group. The fold row is
  "Other items · 1 / 7 smaller items" with no glyph. It is still a group of one, but its two lines now say different
  things, so I accept it.
- **P3-R2.** The tree's Size header and values share one right edge (x 1802 in the dark tree shot). The new measured
  scrollbar gutter (`--pd-gutter`) is the right general fix.
- **P3-R3.** `groupMono` takes a predicate; "Other items" is in the sans face and folder names are in mono.
- **P3-R4.** "3 kinds of data".
- **P3-R5.** No `···` glyph on the fold row.
- **P3-R6.** The `reveal` arrow now ends at x = 20, inside the safe margin.
- **Files at 320:** the name leads, the folder line keeps its separator (`/demo/pro… /a-narrow-pane`), and nothing
  scrolls sideways at page level.

## V-1 (P2, blocks): at 320 the Storage table's rows are wider than the table
- **At rest** (`*-storage-320.png`, both themes), the row `…` button is cut off by the table's right border: only two
  and a half dots show at x≈528-548 (device px).
- **When the row menu opens** (`*-storage-320-menu.png`), focusing the button scrolls the body sideways by about 8 CSS px.
  The rows move left ("Cache" starts at x=61 and the group chevrons at 74), but the header stays where it was ("Kept"
  at 77). Size values now end at 467 under a header that ends at 483. The body is `overflow-x: hidden`, so a user
  cannot scroll it, but focus can. That is sideways scroll in effect, and header and columns fall out of line.

**Fix.** The row grid has to fit the body's client width, not the grid's. At 320 the body shows a vertical scrollbar,
and the rows appear to keep the full width. Lay the rows out at `width: 100%` of `.pd-body`, or apply the measured
`--pd-gutter` to the rows as well as the header. Add a unit or e2e assertion that, at 320 with a scrolling body,
`body.scrollWidth <= body.clientWidth` and the last action button lies inside the grid's box. This is in the shared
`pd-table`, so every app gets the fix.

## V-2 (P2, blocks): file names truncate at the end
At 320, the recent entry reads `service-2026-10-24-11-3…`, which drops the extension, the part that says what the file
is. Rule 18 (binding) says paths **and file names** truncate in the middle. `.pd-name` in `recent-files.ts` is a plain
`text-overflow: ellipsis`. **Fix:** render the name with `pd-path-text` (or its middle-ellipsis part) so it reads
`service-2026-…-11-30.log`, with the full path still as the title. Add a test at a narrow width.

## P3

- The folder line shows a visible gap between the ellipsis and the kept separator (`pro… /a-narrow-pane`). The `/` is
  kept, which was the point, but there should be no space: it is either whitespace between the `pd-head` and `pd-sep`
  spans in the template, or a flex gap. Join them.

## To approve

Fix V-1 and V-2, then re-shoot `*-storage-320`, `*-storage-320-menu` and `*-files-320` in both themes. If the `…` sits
fully inside the table, the header and rows keep one edge with the menu open, and the file name keeps `.log`, this is
approved.

---

# Review 2 verification, round 2 (2026-10-25, `origin/shell/integrate` at `0ac5baa`, which contains `1eff1b1`)

Same reviewer, read-only. I checked the re-taken `*-files-320`, `*-storage-320` and `*-storage-320-menu` in both themes,
and the diff `7f39670..0ac5baa` of `table/src/model.ts` and `shell/files/src/recent-files.ts`.

- **V-1 fixed.** At rest the row `…` sits fully inside the table, with about 25 CSS px to spare before the border. With
  the menu open, header and rows keep one edge: "Kept" and the row names both start at x=77, and "Size" and its values
  both end at x=443 (device px). Nothing shifts sideways. The menu reads "What is inside", "Show in the file manager",
  a divider, then "Clear logs" in danger, last.
- **V-2 fixed.** The name keeps its extension with a middle ellipsis (`ser… 2026-10-24-11-30.log`), and the path line
  keeps its separator (`/demo/pro… /a-narrow-pane`).
- **Gap after "…": accepted as is.** The visible slack after the ellipsis is the coordinator's agreed deviation (text-overflow slack at character
  granularity). It shows in both lines but stays readable. P3 for a later pass: let the head take the slack (or
  measure the cut at sub-character width), and give the name's head a larger share than 3 characters
  (`service-20…11-30.log` reads better than `ser… 2026-…`).

**Verdict: approved.** The glyphs (`file`, `file-text`, `file-code`, `archive`, `reveal`), the Files list, Settings >
Storage, the size tree, both confirm windows and the shared `pd-table` additions (`groupSummary`, `groupMono` with a
predicate, `countLabel`, the measured scrollbar gutter) meet the bar at 1440 and at 320 in both themes. Open, non-blocking:
the P3 above, and the shell menu-bar wrap at 320, which another agent owns.
