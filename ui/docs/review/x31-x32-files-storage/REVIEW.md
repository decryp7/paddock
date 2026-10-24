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
