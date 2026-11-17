# 0105. Scrollers our code positions make no scroll moves of their own

- Status: Accepted 2026-11-17: the user approved the family-wide redesign (ER-21), relayed by the coordinator with a budget of about
  0.5-1 agent-day. Amended the same day after the review of 8e06ec809: the user narrowed the scope to scrollers whose position our code
  computes (virtual lists, grids and tails); chat and assistant logs keep the browser's anchoring (decision 2). Built on
  `feat/scroller-no-auto-scroll`.
- Date: 2026-11-17
- Related: ER-21 (a class seen twice gets a redesign), ER-22 (share first), ER-33 (a check counts only when its probe can see the thing),
  ER-37 / rule 76 (common practice and cost), ER-41 (new checks start report-only); Debrief ADR 0072 (live-tail grid, scroll ownership).
- Number: 0105. On 2026-11-17 the highest ADR on `origin/main` was 0103 and on any `origin` branch 0104
  (`docs/adr-safe-by-default-under-tests`; `git ls-tree docs/adr/` over every branch).
- Check id: **C138**, from `./dev check new-id --reserve feat/scroller-no-auto-scroll` (`conformance/reserved/C138-feat-scroller-no-auto-scroll`).

Evidence heads read (ADR 0091), re-read before the commit: `origin/main` 97176067c, `origin/fix/debrief-live-tail-adr` b89bf1318 (it merged `4c44f3561`),
`origin/fix/debrief-resize-scroll-origin` 4c44f3561. Nothing cited here is in a tag (`git tag --contains 4c44f3561` and `7fa15ba95`
print nothing); no claim is about shipped code.

## Context: the class

A scroller whose position our code computes (a virtual grid, a recycled table, a tail that places its own rows) compares what it wrote
with what the browser reports. Twice the browser moved the position by itself and the code took it for the user's scroll, both times in
a virtual grid:

1. **Clamping** (Debrief ADR 0072, bug 3, `7fa15ba95`): `scrollTop` written before the spacer grew was clamped and the clamped value
   reported; the reveal stopped 21 rows short. Fixed by writing after the render.
2. **Scroll anchoring** (`4c44f3561`, on `fix/debrief-resize-scroll-origin`): on a resize the grid's row layer, above the spacer, changed
   height and the browser moved `scrollTop` by that change to keep the spacer still. The grid kept it as the user's; the top row was two rows
   off in about 1 in 75 WebKit runs. A probe of 20 resizes per run was red 11 of 40 runs with `overflow-anchor: auto` and 0 of 80 with
   `none` (same build). A WKWebView probe on macOS 27.2.1 (the runtime Tauri uses there) moved a scroller 1000 to 1200 when an element
   above its anchor grew 200 px: users were exposed, not only tests.

Anchoring is on by default in Chromium and, since Safari Technology Preview 241, in WebKit ("Added stable support for CSS scroll
anchoring", 310113@main; https://webkit.org/blog/17917/release-notes-for-safari-technology-preview-241/). On `origin/main` 97176067c C138
finds 21 writes of a scroll position in components whose scrollers carry no opt-out (Debrief 12, Pitwall 1, Pitcrew 1, shared UI 7).

## Common practice (ER-37)

- **Angular CDK** (22.2.4, `node_modules/@angular/cdk/fesm2022/scrolling.mjs`, read 2026-11-17): the virtual scroll viewport's scroller,
  `.cdk-virtual-scrollable`, sets `overflow-anchor: none` and `scroll-behavior: auto`. Our `pd-table` and Pitcrew's log viewer use it, so
  they were already safe.
- **CSS Scroll Anchoring** (https://drafts.csswg.org/css-scroll-anchoring/, not re-read for this ADR): `overflow-anchor: none` on a
  scroll container turns its anchoring off; the property is the documented opt-out. Measured here in both engines (the e2e helper below).
- **WebKit** ships anchoring (STP 241 above); the WKWebView probe confirms the system webview does it.

The common practice is the CDK's: only the virtual viewport, which computes its own position, turns anchoring off; an ordinary
scrolling list keeps it. **Cost:** the cheapest form,
`overflow-anchor: none` in each scroller's own CSS, is about 0.25 agent-day for the 21 sites. This design adds one shared directive,
a check and an e2e helper, about 1 agent-day in all (0.75 beyond), under the 1-day line; the user approved it.

## Decision

1. **One shared opt-out.** `@decryp7/paddock-ui/scroll` exports `PdOwnedScroll` (`[pdOwnedScroll]`): it binds `overflow-anchor: none`
   and `scroll-behavior: auto` on its element (Angular style bindings, CSSOM, allowed by the CSP) and adds the class `pd-owned-scroll`.
   Bound styles work in apps that do not load `paddock.css` (Debrief, Pitcrew). For stylesheets, `_base.scss` has the `owned-scroll`
   mixin and the `.pd-owned-scroll` class. The entry depends on `@angular/core` only, so a table or a log does not pull the root entry's
   weight. `scroll-behavior: auto` is defensive (no scroller in the family sets `smooth` today); it keeps a write landing at once, as the
   CDK does.
2. **Scope (user, 2026-11-17): scrollers whose position our code computes.** A virtual list, grid or tail, or an editor or view that
   places its content itself and writes `scrollTop`/`scrollLeft` (or calls `scrollTo`, `scrollBy`, the CDK's `scrollToOffset` or
   `scrollToIndex`) carries `pdOwnedScroll` (or the class or mixin; a `cdk-virtual-scroll-viewport` counts, the CDK sets the same
   properties). On this branch: Debrief's events grid, config diff tree, inspector context lines, TOML editor (gutter and text), query
   field, flame chart and hotspots; the shared view scene and `pd-table`'s body.
   **Chat and assistant logs keep the browser's anchoring** (the shared assistant log, used by the workbench and Pitwall's Chat; Debrief's
   AI chat log; Pitwall's Compare conversations). They only pin themselves to the end with `scrollTop = scrollHeight`, and anchoring keeps
   the reader's place when content above them changes while they read scrolled up. **Trade-off:** a log's own "am I at the end" read may
   see a move the browser made. Here that is cheap: the assistant re-reads "at the end" on every scroll (`scrolled()`, within
   `SCROLL_STICK_PX`) and pins again when the user sends; Debrief's chat goes to the end on every new turn. Neither classifies scrolls the
   way the grid does, so a browser move cannot leave it on a wrong row, while turning anchoring off would lose the reader's place. Each log write carries a C138 waiver with this reason.
   Other waivers (`// conformance: owned-scroll —`): dockview's group and tab strip (third-party elements, Debrief and the workbench
   dock), the widget grid's drag auto-scroll (a helper that nudges the caller's scroller and reads it back at once) and Pitcrew's config
   editor (decision 3).
3. **Pitcrew** does not use `@decryp7/paddock-ui` (no path mapping, no `ui/dist` in its build). Its log viewer is a CDK viewport (safe);
   its config editor's one jump to an error line is waived. It moves onto the shared directive when Pitcrew adopts the shared UI. Adding the
   dependency now only for this is more than the rule needs.
4. **Check C138** (conformance, report-only first, ER-41): in the TypeScript of `apps/*/` and `ui/projects/`, a write of a scroll position
   in a component whose template (inline or `templateUrl`) or styles carry none of the markers is a warning; a write outside a component, or
   on a third-party element, or a log that keeps anchoring, needs the reasoned waiver. It reads `.x`/`?.x`/`['x']` writes with every
   assignment operator, any receiver of `scrollTo`/`scrollBy` but the page's, a multi-line `Object.assign`, and markers with TypeScript and
   HTML comments blanked. Accepted gaps: `scrollIntoView` (it scrolls every ancestor), `Renderer2.setProperty(el, 'scrollTop', ..)`, and
   per file, not per element (a component with two scrollers passes when either has the style; tracing a write to its element needs the
   template compiler). A positive control on in-memory text runs first; on the real tree it found the 21 sites on `origin/main` and 0 on
   this branch. Wired in `conformance/paddock.ci.json` (`only`) and `docs/gates.toml` (`[[report_only]]`, review by 2026-12-17); it
   blocks after its first green CI run (the run id goes there).
5. **Tests.** `@decryp7/paddock-e2e` has `scrollAnchoringShift` and `expectNoScrollMovesOfItsOwn`: in the page they take the scroller's
   own children out of the anchor choice, put a probe and a tall filler at its start, scroll into the filler, grow the probe 100 px and
   read `scrollTop`; first with anchoring forced on (the positive control: it must move 100, else "not measured"), then with the
   scroller's own style (it must move 0); the verdict is a pure function (`anchoringVerdict`) with its own test. Used for `pd-table` (ui
   e2e) and Debrief's events grid. Red without the style (measured): Debrief's grid in Chromium and WebKit. `pd-table` stays green without
   our directive, because the CDK's own class covers it. No grid or tail row is skipped in either engine. `PdOwnedScroll` has a unit spec
   (the class and both style bindings).
6. **The classifier's browser-shift rules** in Debrief's grid (ADR 0072 decision 2, `spurious`: a far position reset to 0, or a shift by
   about the layout change) may be the same mechanism. The count below cannot decide it, so they stay; a follow-up for the Debrief author.
   This branch does not change the grid's logic.

## Measurements and gaps

- **Seen while measuring the logs (before the scope change), no longer in this ADR's scope:** in WebKit a scroller placed inside
  Debrief's AI chat panel host did not anchor even with anchoring forced (cause not known); Pitwall's Compare `.msgs` grows with its
  content instead of scrolling (a 3518 px pane in a 900 px window), queued by the coordinator as a Pitwall layout bug.
- **`Origin: spurious` over the live-tail matrix (decision 6), not conclusive.** `live-tail-follow.e2e.ts`, 34 tests x 5
  repeats, Chromium and WebKit, 4 workers, on `4c44f3561` with a temporary count of the grid's `Origin: spurious` log lines per test:
  with anchoring off, 0 in 170 runs per engine; with the `overflow-anchor: none` line removed (anchoring on), also 0 in 170 runs per
  engine. The matrix never reaches the `spurious` path in either condition, so the zero cannot show that the rules are dead (ER-33: no
  positive control). Proposal for the Debrief author, not a removal: re-run the reproduction that first showed each shape with anchoring
  off. The "far position reset to 0" shape matches ADR 0072's bug 3 (WebKit clamping a write made before the spacer grew, which this ADR
  does not change); the "shift by about the layout change" shape matches anchoring and is the likelier one to go. Their unit specs
  (`scroll-classify.spec.ts`, the two grid specs) stay until then. The same runs had unrelated WebKit failures: real-input smoke rows
  (dropped input, known) twice, and "reveal ... newest first" once ("and stays there", with anchoring off), for the Debrief author.

## Consequences

- One mechanism, one check and one test helper for a class that has cost two rounds in Debrief; the next virtual list or grid gets it
  from the directive and is caught by C138 when it does not, and a new log has to say in its waiver that it keeps anchoring.
- **Integrator note:** `fix/debrief-live-tail-adr` (b89bf1318, which merged `4c44f3561`) puts `overflow-anchor: none` in the events grid's own CSS and moves
  its writes into `scroll-writer.ts`. When both land: keep `pdOwnedScroll` on the grid's `.viewport`, drop the inline
  `overflow-anchor: none` line, and give `scroll-writer.ts`'s write `// conformance: owned-scroll — the grid's single writer; its scroller is
  events-grid.ts's .viewport (pdOwnedScroll)` (C138 sees it as a write outside a component).
