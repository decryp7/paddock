# Independent design review: `<pd-view>` (plan X51, ADR 0002)

- Reviewer: independent (did not build it), rule 22. Date 2026-10-25.
- Material: `origin/shell/integrate` at 597cd78. Part images were read at 100% in this folder, light and dark, with the 320 px
  strips (`*-narrow.png`) cut into sections. I also read the code in `ui/projects/paddock-ui/view/src` (node, view, tone,
  button, form, review, scene, layout, table, tree, matrix, chart, host, runtime) and `shell/pitwall/src/needs-ai.ts`.
- Bar: `docs/design-guide.md`, ADR 0002, the family look (shared `pd-field`, `pd-btn`, `pd-chart`, X21 tables and charts, the
  workbench) and "professional and beautiful".

## Verdict: NOT APPROVED

3 P1, 13 P2, 12 P3.

Much of the work is good. Tables, the tone mapping through tokens, badges, the form built on the shared `pd-field` (14 px
alert icon, error under the field, required marker), the review list with reasons, the paged table, charts through `pd-chart`,
text-only binding, and an inline error with Retry all match the family. Both themes hold up. The three P1s are a layout bug
that hides data, a status shown by colour alone, and a second, different "needs local AI" state.

---

## P1 (must fix before approval)

### P1-1 Radial graph stacks every root-layer node on the centre, so data is hidden
`light/dark-graph-radial.png`: the sample has 4 nodes (`paddock-ipc` plus `paddock-view`, `paddock-shell`, `paddock-env`, which
all point to it). Only 2 boxes show. `paddock-env` has a double outline because the three spokes are drawn on top of each other.
Cause (`layout.ts` `layoutGraph`, radial branch): layer 0 always gets radius 0 (`r` starts at 0 and only grows for `l > 0`).
Every layer-0 node lands on the centre whatever its angle. The one-root case is tested (`layout.spec.ts` "puts the one root …
in the middle"); the several-roots case is not. In this sample the hub also ends up on ring 1 at the top, and not in the
middle where a reader expects it.
Also: the canvas is about 1,050 × 525 px for 4 small boxes (`extent = last ring + 140 + PAD`). At 320 px the first screen of the
scroll region is empty (`dark-narrow.png`, below the layered graph).
Fix: when layer 0 has more than one node, put them on a ring, or (better for hub-and-spoke) centre the node with the highest
degree and put the others on rings by distance from it. Fit the extent to the real bounding box of the boxes. Add tests for
several roots, hub-and-spoke, and "no two boxes overlap" over the sample.

### P1-2 Graph and diagram status is colour alone; the list twin drops it
`*-graph.png`: Build and Unit tests are green, Integration tests amber, Publish red. The only cue is the border colour of the
box: no glyph and no word. This breaks design-guide principle 6 and checklist item 8 ("Status = icon + word + colour"). Green and
red outlines are the classic red/green pair. `scene.ts` sets only `stroke: var(--pd-view-tone)`. The SVG `<title>` and the
"Show as a list" `<li>` carry only the label. The text twin also leaves out the tone and the diagram `detail` lines (class
members), so it is not an equal alternative.
Fix: draw the tone glyph (the same `TONE_GLYPH` shape, 14 px) inside the box before the label. Put the tone word in the
accessible name and in the list (`Publish — failing`). List each node's caption and detail. Keep the coloured stroke as the third
cue.

### P1-3 `NeedsAi` is a second, different "needs local AI" state
ADR 0002 calls `NeedsAi` "the standard 'Needs local AI → Set up Pitwall' state (ADR 0004)". The view draws its own markup
(`node.ts` `needsAi` case) instead of `pd-needs-ai`, and the two differ in substance:
- **Source of truth.** `pd-needs-ai` shows what the host's `PitwallStore` knows: its kind, message and steps, pairing code,
  "Use the built-in engine" and "Open AI settings". The view shows the plugin's `reason` and one plugin action. One window can
  then show "Connect to Pitwall…" in a pane and "Pitwall is not running." in a view at the same moment.
- **Rule 19.** The repair button is a plugin `ActionRef`, so it goes to the plugin. Only the app can open or install Pitwall
  (that is why `pd-needs-ai` emits `(step)`). The renderer cannot promise that "Set up Pitwall" works.
- **Look and copy.** The title is the feature alone ("Explain the failing check"), where the family says "… needs local AI".
  The icon is `chat` instead of info or alert by kind. The border is `--pd-hairline` instead of `--pd-border-subtle`. There is no
  built-in fallback and no settings link.

Answer to the brief's question: **yes, split it.** Make `pd-needs-ai` a thin container over a presentational `pd-needs-ai-card`
(icon by kind, title, message, optional pairing code, an actions slot, optional hint and error). Put the card somewhere neutral
(for example `paddock-ui/states`), because `view` must not depend on `shell/pitwall`. Then:
- When the host provides a Pitwall state (a `VIEW_HOST` capability, or `PitwallStore` present), the `NeedsAi` node renders the
  live `pd-needs-ai feature=…`, and the plugin's `reason` and `setup` are ignored.
- Otherwise it renders the same card with the plugin's reason and action, titled "<feature> needs local AI".

One look, one source of truth, a working repair: this is rule 39 and rule 40 applied.

---

## P2

1. **A row stack spreads its children evenly.** `.pd-view-stack--row > * { flex: 1 1 14rem }` gives every child at least 224 px.
   Five badges sit about 250 px apart and the fifth wraps alone (`*-badges.png`, top of `*-stale.png`). A row of badges, buttons
   or links must pack at `gap: 8px`. Only block children (tables, charts, cards) should share the width. Fix: size by content
   (`flex: 0 1 auto`) for inline kinds (badge, link, button, progress) and keep `1 1 14rem` for block kinds.
2. **`overflow-wrap: anywhere` is the default, so short words break in the middle at 320 px** (rule 40 pattern: the same cause
   in text, code, form, table, matrix, review). In `dark-narrow.png`: "Pass|ing", "Flak|y", "Faili|ng" (three-line table
   cells, against the table rule), "No secret chose|n", "– -lib", "REQ-|2". `anywhere` also lowers min-content width, which is
   why the table squeezes the State column. Fix in one place: use `overflow-wrap: break-word` (it breaks only a word that cannot
   fit alone). Keep status cells, badges and button labels `nowrap`, and give the form "chosen" text a `min-width` before the
   button wraps under it.
3. **A heading inside the view has the same size as the view's title.** `.pd-view-text--heading` uses `--pd-text-title`, the
   same as `.pd-view__title`. "Release readiness" and "Release 1.4: what is left" are twin 18/600 lines (top of `*-stale.png`,
   `*-narrow.png`). Map heading to Subtitle 15/600 and subheading to Body strong 13/600, so the document title is the only
   Title.
4. **The error and stale banners.** (a) The alert glyph uses the text colour, but the guide says "icon in danger". (b) Stale is
   not a failure: the last good document is still on screen, yet it uses the danger border and tint. Use warning (or info) tone
   for stale. (c) The stale copy says "Reload it to see the latest" but the button says "Retry". Label it "Reload" (keep Retry
   for a failed load). (d) Invalid-document text could name the plugin (ADR: "attributed to the plugin"), with an optional
   secondary "Open logs" as in the guide.
5. **Loading and busy are barely designed.** Loading is a plain "Loading…" line; the guide asks for a skeleton at the final
   geometry, and every data view needs one. While `busy` (waiting for the engine's answer), the only sign is that controls go
   disabled and `aria-busy` is set. Show a quiet inline "Working…" status (role=status) near the control that was used, or a 2 px
   indeterminate bar at the top of the view, as other families' panes do. The contact sheet has no loading or busy image at all.
6. **Too many filled accent buttons.** The renderer hard-codes primary for the form submit, the empty-state action, the
   needs-AI setup, and the selected review filter. The sample view at 320 px shows four saturated olive fills (Publish, All (3),
   Run the checks, Set up Pitwall), against "one primary per region" and "calm by default". The review filter is a segmented
   choice: draw the selected chip as the guide's segmented style (accent tint + 1 px accent ring + 600 label), not
   `pd-btn--primary`. Make the empty-state and needs-AI buttons secondary when the view already has a primary, or let
   `ActionRef` carry a `prominence` that the validator caps at one primary per document.
7. **"Changes data" is built three times and differs.** There is CSS in `button.ts` (twice) and `review.ts`. The reason-path
   button in `review.ts` builds its own badge without the pencil glyph ("Suppress | Changes data" in `*-review.png` has no
   pencil, while Accept, Dismiss and Restore do). Make one `pd-view-writes` element and use it everywhere: button, link,
   reason send, and the host's confirm (it gets `writes`).
8. **Tree tone colours the whole row.** `paddock-view` shows a green folder, green check and green label (`*-tree-open.png`). The
   word exists only for screen readers. Colour the glyph only, keep the label in text colour (calm by default), and show the
   word as a small badge or tooltip when the tone is not neutral.
9. **Table numbers are left-aligned, and the schema cannot say otherwise.** "Time (ms)" and "Lines" are left-aligned (guide:
   numbers right-aligned, tabular, unit in the header). `Column` has only `key`, `label` and `sortable`. Add an `align` (or
   `numeric: bool`) to `Column` in Rust and TS in the same change. `ChartSpec` likewise lacks a `unit` for axis and title.
10. **A chosen path is shown only as "Chosen".** `GrantRef` is opaque, so after picking, the user cannot see which folder the
    form will send. `ViewHost.pickPath` should return the grant plus a display name (middle-truncated path with the full path in
    a tooltip, rule 18). Do the same for the secret's name.
11. **The class diagram does not look like a class diagram.** The members are centred and there is no divider between name and
    members. "model" is a bare caption, not a stereotype (`*-diagram-class.png`). Left-align the members under a hairline
    compartment rule, and set the stereotype as «model» in caption style. Centred member lists look amateur.
12. **The review list is wider than its column at 320 px.** Item dividers run about 6 CSS px past the pane edge
    (`dark-narrow.png`, review section). Check `scrollWidth <= clientWidth` for the whole view at 320 px in the Playwright spec
    (rule 18), and fix the overflow (probably the 3 px start border plus padding on a `grid` item without `min-width: 0`).
13. **Evidence gaps** (needed for an approval, not code bugs):
    - No focus, hover, disabled, busy or loading captures, so focus rings ≥ 3:1 cannot be verified from images. The code uses the
      family `--pd-focus-ring`, 2 px, consistently. The tree row (offset −2 px on a hover fill) and the scene `summary` (no
      offset) are worth a capture.
    - `light/dark-stale.png` are blank below the first graph (about 4,500 CSS px of empty page). Either it is a capture artefact
      or content after the graph does not paint at that width. Recapture and check.
    - `contact-sheet.png` is 3,600 × 123,488 px: it cannot be opened at 100%. Split it into sheets of about 4,000 px.

## P3

1. The `code` text style has no inset (no surface-2, padding or radius), so it reads as body text in a mono font. Give it the
   family inline-code look and break only at spaces.
2. The empty-state icon is 24 px; the guide says 32 px muted.
3. The progress track (`--pd-neutral-tint`) is almost invisible in dark. Use `--pd-surface-3` or `border-strong` so the
   remaining part reads.
4. Single-series charts: line and bar use the accent, scatter uses palette slot 1 (sky). Pick one rule. Chart, table and scene
   frames use radius 6 (control) where cards are 10.
5. Edges: forward edges are curves and the back edge (retry) is orthogonal. Labels sit on the path (the halo works, but
   "needs db" reads as struck through at small sizes). Offset the label from the path or use one routing style.
6. "Show as a list" uses the native disclosure triangle in 12 px muted text outside the frame. Use the family chevron and put it
   in the frame's footer.
7. Matrix: the grid shrinks to its content while the frame spans the full width (about 650 px of empty frame). Fit the frame
   (`inline-block` or `width: max-content; max-width: 100%`). The accent underlined link on a success or danger tint sits beside
   a status (guide: accent text never directly beside a warning). Use text colour with an underline inside tinted cells.
8. The unknown-kind fallback is clear and contained. Consider naming the kind in a muted suffix or tooltip for support.
9. The confirm image is the harness's stand-in, not `pd-view`'s (confirmation is `VIEW_HOST.confirm`). It puts the primary left
   of Cancel (guide: cancel left of primary) and has no writes badge. Real hosts must use a modal OS window (rule 17); review
   that window with the host.
10. Scroll regions (table, matrix, scene) always get `tabindex=0`, which adds tab stops even when nothing scrolls. Add them only
    when overflowing, or accept it and say so.
11. Sample: the "Release name" placeholder is the same as its default value ("v1.4"), so the error capture looks as if the value
    were still there. Use a placeholder like "e.g. v1.5".
12. **Signal Forms (rule 37).** Not using Signal Forms is an acceptable exception, provided it is written down: a comment
    citing rule 37, plus a note in the X51 plan. The fields come from data, the values are one `linkedSignal` keyed by the
    document, and `pd-field` takes a `PdFieldStateLike`, so a later move costs little. Signal Forms can model a
    `Record<string, unknown>` with a schema built from the field list, re-created when the form changes, so a short spike is
    worth doing before Pacenotes adds bigger forms. Two gaps either way: `touch` on blur exists only for text, number and
    choice (path, secret and toggle never become "touched"), and `fieldProblem` is a convenience mirror of the engine's checks.
    That is fine, because the engine re-validates.

## What is good (keep)
- All text is bound as text. Tones go through tokens. The glyph and sr-only word in `pd-view-tone` is the right primitive (P1-2
  and P2-8 just need it used everywhere).
- The form reuses `pd-field` (label above, helper, error replacing helper, 14 px alert icon, required marker) in both themes.
- The review list has state badges with glyph + word, per-state actions, a reason asked in the row with the typed text kept on
  a declined confirm, and accessible names that include the finding.
- The paged table has its range in `aria-live`, sortable headers as buttons with `aria-sort`, and a row action with context in
  its name. Wide content scrolls inside its own region instead of the page.
- An invalid document or stale patch keeps the last good document and offers Retry. Unknown kinds degrade per node.
- The sequence diagram is clean and well proportioned in both themes.
