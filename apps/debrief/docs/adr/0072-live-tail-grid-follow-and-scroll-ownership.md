# 72. Live-tail grid: what a refresh round owns, and whose scroll it is

- Status: **Accepted 2026-11-16**, option B (G1-G7 and G8): the user chose it on 2026-11-16 (about 2.5 agent-days). Implemented on
  `fix/debrief-live-tail-adr`.
- Date: 2026-11-16
- Related: ADR 0071 (latest request wins), Paddock rule 55, ER-11, ER-21, ER-37, ER-41, ER-44; `docs/lessons.md`, "Debrief: the
  live tail 'holds the rows while not following' read 120052 for 120028".
- Evidence heads (ADR 0091): `origin/fix/debrief-events-grid-live-tail` `7fa15ba95`, `origin/main` `23213a35e`. None of the
  three fixes below is on main or in a tag (`git tag --contains 334f4e9ff` and `7fa15ba95` print nothing). Nothing here is
  claimed as shipped.

## Context

Tonight I had three scrolling bugs in one feature, "reveal newest" in the live-tail grid. Each is fixed with a test on
`fix/debrief-events-grid-live-tail`.

1. **The reveal stopped one row short** (`334f4e9ff`). A live refresh asks for several pages in one round. A push between two
   answers made a later page report a larger total. `PagedRows` stored it and did not scroll, so the view ended 24 px above the
   newest event. Fix: `PagedRowsStore.arriveLive` follows a later, larger total of the same round.
2. **The first fix pulled the view back** (`7679f9e4d`). The follow-up lived for the whole round. Pages fetched because the user
   scrolled away answered in that round and jumped the view back to the end. Fix: follow only the pages the reveal asked for, and
   end the follow-up at the user's first scroll (`endReveal()`).
3. **In real browsers the reveal stopped 21 rows short** (`7fa15ba95`). `scrollTop` was written before the spacer grew. Chromium
   and WebKit clamp that write and report the clamped position in a scroll event. The grid took it for the user's scroll. jsdom
   does not clamp, so unit tests missed it. Fix: write after the render (`setTop(..., afterRender)` with `afterNextRender`). A
   Playwright e2e covers it in both engines.

The common cause: nothing says what a live-refresh round owns, and nothing says how the grid tells its own scroll writes from the
user's. Today the rules are spread over `PagedRowsStore` (`pendingLive`, `endArrival`), `EventsGrid` (`writtenScroll`,
`liveMode` from the position at the start of each round), `LiveFeed` (`gridMode`) and `LiveFollow` (`follow`, `paused`).

## Common practice (ER-37)

- **xterm.js (VS Code's terminal).** One flag, `isUserScrolling`, in `BufferService`. Scrolling up sets it; reaching the bottom
  clears it. New output moves the view to the bottom only while it is false
  ([BufferService.ts](https://github.com/xtermjs/xterm.js/blob/master/src/common/services/BufferService.ts)). The viewport
  drops its own writes by comparing the scroll position with the row it holds (`_handleScroll`, `diff !== 0`) and a suppress
  flag around its own dimension updates ([Viewport.ts](https://github.com/xtermjs/xterm.js/blob/master/src/browser/Viewport.ts)).
- **VS Code's Output pane.** A scroll lock, set when the user clicks in the view and cleared when the cursor is on the last line
  (`output.smartScroll.enabled`, default on, [output.contribution.ts](https://github.com/microsoft/vscode/blob/main/src/vs/workbench/contrib/output/browser/output.contribution.ts),
  `outputView.ts`).
- **Chrome DevTools console.** User intent comes from input events only: `mousedown`, `wheel` and `touchstart` set
  `muteViewportUpdates` and turn stick-to-bottom off; after `mouseup` and a 200 ms wait it re-reads "scrolled to bottom"
  (within 2 px). It does not listen to `scroll` to find the user. Its own scrolls set stickiness directly
  ([ConsoleView.ts](https://github.com/ChromeDevTools/devtools-frontend/blob/main/front_end/panels/console/ConsoleView.ts)).
- **TanStack Virtual** (3.19.11 on npm). `followOnAppend` follows appends while the list is at the end. A pure
  `classifyScrollEvent` sorts every scroll event into `readBack` (our write, within 1.5 px), `clampedReadBack` and `clampedEcho`
  (our write, clamped because the sizer had not grown: issues
  [#1258](https://github.com/TanStack/virtual/issues/1258), [#1266](https://github.com/TanStack/virtual/issues/1266)),
  `spurious`, and `external` (the user). Only `external` cancels a pending scroll command. `scrollToIndex` re-checks the target
  each frame until it is stable ([index.ts](https://github.com/TanStack/virtual/blob/main/packages/virtual-core/src/index.ts),
  [scroll-classifier.ts](https://github.com/TanStack/virtual/blob/main/packages/virtual-core/src/scroll-classifier.ts)). This is
  our bug 3, met and fixed the same way.
- **AG Grid and the Angular CDK virtual scroll.** No follow-tail option. They give `ensureIndexVisible(index, position)`
  ([AG Grid API](https://www.ag-grid.com/javascript-data-grid/grid-api/)) and `scrollToIndex`, `scrollToOffset`,
  `elementScrolled()`, `checkViewportSize()` ([CDK source](https://github.com/angular/components/blob/main/src/cdk/scrolling/virtual-scroll-viewport.ts)).
  The app decides when to follow.
- **Grafana Explore, live logs.** Pause, or scrolling the logs view, pauses live tailing; Resume continues
  ([Grafana 9.4 docs](https://archive.grafana.com/docs/grafana/v9.4/explore/logs-integration/),
  [Amazon Managed Grafana](https://docs.aws.amazon.com/grafana/latest/userguide/v9-explore-logs.html)).
- **The browser.** `scroll` and `scrollend` fire for user and script scrolls alike, so they cannot tell them apart. `scrollend` is
  in Chrome 114 and Firefox 109, and in Safari only from 26.2 ([caniuse](https://caniuse.com/mdn-api_element_scrollend_event),
  [Chrome blog](https://developer.chrome.com/blog/scrollend-a-new-javascript-event)). TanStack ships it off by default
  (`useScrollendEvent: false`). `wheel`, `pointerdown`, `touchstart` and `keydown` only come from the user.

The common pattern: one "stuck to the end" flag; user intent from input events (DevTools) or from a scroll event that is not an
echo of the app's own write (xterm, TanStack); writes after the size is known (TanStack). Grafana pauses on scroll; xterm and
Debrief keep live updates on and only stop sticking to the end.

## Decision

1. **A live-refresh round owns the pages it asked for.** A round starts at `liveRefresh(mode, ...)`; it owns the set of pages
   it requested and the mode (`end`, `shift`, `keep`). Only answers to those pages may move the view. Each later answer with a
   larger total re-applies the mode: `end` scrolls to the new end, `shift` moves down by the extra rows. The round's hold on the
   view ends at the first of: the last owned page answered, an owned page failed, the user took over (2), a new round or a reset.
   Pages fetched for any other reason never move the view.
2. **One owner of scroll writes, and two signals for the user.** `EventsGrid.setTop` is the only code that writes `scrollTop`
   (true today; a lint keeps it so). The user has taken over when either:
   - an input event arrives on the grid: `wheel`, `pointerdown` (this covers the scrollbar), `touchstart`, or a `keydown` that
     moves the view (arrows, Page Up/Down, Home, End, Space). This is the DevTools signal, and it is the only one that sees a
     wheel in the scaled-scrollbar mode or a keyboard move, because there the grid writes the scroll itself;
   - a `scroll` event that is not an echo of our write. A pure `classifyScroll(read, pendingWrite, lastWrite, maxScroll)` returns
     `own` (within 1 px of the last write, or clamped at the current maximum after our write) or `user`. This is the TanStack
     signal; it covers scrolls with no input event (assistive tech, find-in-page).
   I keep both because each misses cases the other sees. I don't use `scrollend`: it cannot tell who scrolled, and WebKit before
   Safari 26.2 lacks it.
3. **Follow and hold are one small state machine**, with `pinned` as a stored flag (xterm's `isUserScrolling`, inverted), not
   re-derived from the position at each round:

   | State | Event | Next state and action |
   |---|---|---|
   | Following, pinned | new data | round in `end` mode; stays pinned |
   | Following, pinned | resize | stays pinned; write the new end after the render |
   | Following, pinned | user took over | not pinned; end the round's hold; keep the user's position |
   | Following, not pinned | new data | round in `keep` (or `shift` when sorted newest first); position kept on the same events |
   | Following, not pinned | resize | keep the top row |
   | Following, not pinned | user scrolls to the end (within 1 row) | pinned |
   | Following or holding | Follow off, or a manual zoom or pan (pause) | Holding: no round; count "N new events" |
   | Holding | new data | count only |
   | Holding | reveal (pill click) | round in `end` mode (newest first: back to the top); pinned until the user takes over; still holding |
   | Holding | Follow on, or Resume | Following, pinned; round in `end` mode |
   | any | new query or sort order | reset to the top (unchanged) |
4. **Writes happen after the render when the size changes.** A write for a new total or a new viewport height waits for
   `afterNextRender`; its position signal is set at once. At most one write is pending; a newer one replaces it, and a user
   take-over cancels it. A write that does not change the size (a wheel in scaled mode, a keyboard move) writes at once.

## Check

- **Real-browser e2e matrix**, `apps/debrief/app/e2e/live-tail.e2e.ts`, projects `chromium` and `webkit`
  (`playwright.config.ts`), with `?mockPageStaggerMs` and the pinned clock (`page.clock.install`, `runFor`). Each case asserts
  that the last row is the newest event, or that the top row is unchanged:

  | Case | Ascending | Newest first |
  |---|---|---|
  | reveal, pushes between its pages (exists, `7fa15ba95`) | end on newest | top on newest |
  | following over 20 pushes | stays on newest | top row kept when not at start |
  | wheel, scrollbar drag and keyboard away during a reveal | stays where the user left it | same |
  | wheel away in scaled mode (`?mockTotal` above the scale limit) | stays where the user left it | same |
  | resize smaller and larger while pinned | stays on newest | same |
  | resize while not pinned | top row kept | same |
  | follow off, then on | count shown, then newest | same |

  Branch CI runs Debrief's e2e in Chromium only; a full run adds WebKit (`.gitea/workflows/debrief.yml`, the `e2e` job's
  `--project=chromium` unless `full`). So the WebKit half is run locally before landing, and in the full run.
- **Unit invariants** (vitest): a table test of the state machine in (3); a table test of `classifyScroll` (own, clamped own,
  user, NaN); `PagedRowsStore`: an answer to a page the round did not ask for never calls `onLive`, a failed owned page ends the
  hold, `shift` re-applies the extra delta.
- **Lint**, report-only first (ER-41): `no-restricted-syntax` on assignments to `scrollTop` in `src/app/grid/` outside `setTop`.

## Gaps between the branch tip and this decision

Found by reading the code at `7fa15ba95`; G4 and G5 are not yet measured, and the e2e matrix confirms or refutes them.

| # | Gap | Proposed fix | Cost (agent-days) |
|---|---|---|---|
| G1 | A wheel in scaled mode and keyboard moves are written by `setTop`, so `onScroll` sees our own write and never calls `endReveal()`: a reveal page can still pull the view back (bug 2's class). | Input listeners call one `userTookOver()`. | 0.25 |
| G2 | A deferred `afterNextRender` write is not cancelled by a user scroll in the same frame; it overwrites the user's position. | One pending-write slot, cancelled by `userTookOver()`. | 0.15 |
| G3 | `onScroll` treats any read more than 1 px from `writtenScroll` as the user, so a write clamped for another reason (a resize between write and render) is still taken for the user (bug 3's class). | Pure `classifyScroll` with the clamped-at-max case. | 0.25 |
| G4 | "At the end" is re-derived from the position at each round (`top >= maxTop - rowH`). A smaller viewport while pinned moves `maxTop`, and the next round goes to `keep`: following stops without a word. | Stored `pinned` flag; resize re-pins. | 0.3 |
| G5 | Only `end` mode has the follow-up. In `shift` mode (newest first, not at the start) a push between pages shifts the rows again with no correction (bug 1's class). | Follow-up re-applies `shift` with the extra delta on the round's own pages. | 0.25 |
| G6 | A failed reveal page never leaves `endArrival.pages`, so the hold lasts until the user scrolls or a new round. | A failed owned page ends the hold. | 0.1 |
| G7 | The e2e matrix and unit tables above; today only the first matrix row exists. Lint for `scrollTop` writes. | Add them, failing first where a gap applies. | 0.75 |
| G8 | The follow and hold rules are split over four classes. | Move them into one pure reducer that (3) tests as a table. | 0.5 |
| G9 | Found by the WebKit e2e (3 of 7 runs red at `7fa15ba95`): a page fetched outside the round (`fill`, scrolling) reports a larger total after the reveal's last owned page; the total grows, nothing scrolls, the view ends one row short. | `PagedRowsStore.onGrow`: any answer outside a round that raises the total is reported; a pinned view follows the end (oldest first), a not-pinned newest-first view shifts. | 0.2 |

**Options for the user (ER-37).**

- **A, common practice (recommended): G1-G7, about 2 agent-days.** Input events plus an echo check, a stored pinned flag and
  writes after the render are what DevTools, xterm and TanStack do. G3 and G5 are the same classes as tonight's bugs, so I count
  them as part of the fix (ER-44), not extras.
- **B: A plus G8, about 2.5 agent-days.** One reducer instead of four places. It is 0.5 day beyond A, so under the 1-day line,
  but it goes past what xterm and DevTools do (they keep the flag inside the view class).
- **C, cheapest: G1, G2, G6 and the e2e rows for them, about 1 agent-day.** It closes bug 2's class only; resize, clamps and
  `shift` stay open.

No single gap costs more than 1 agent-day beyond common practice, so none needs a separate yes.

## Consequences

- One rule says when the grid may move the view: an owned page of the live round, while the user has not taken over.
- User intent is seen for every way of scrolling, including the scaled scrollbar and the keyboard.
- Real-browser clamping is handled in one tested place, not by write timing alone.
- WebKit coverage depends on a local run or a full CI run until branch CI runs it.

## Implementation (2026-11-16)

- `follow-reducer.ts` (`followStep`, G8) and `scroll-classify.ts` (`classifyScroll`, G3) are pure, each with its table test.
- `EventsGrid`: one pending-write slot dropped by `userTookOver()` (G2); `userTookOver()` from `wheel`, `pointerdown` on the scroller (the
  scrollbar), `touchstart` and moving keys (G1), plus any scroll event `classifyScroll` calls `user`; a stored `pinned` flag, re-applied
  on resize (G4); `setTop` is the single writer (through `scroll-writer.ts`), logging the scheduled, dropped and written writes, scroll events and
  arrivals (ER-27).
- `PagedRowsStore`: the round's follow-up covers `shift` as well as `end` (G5), a failed owned page ends it (G6), `onGrow` (G9).
- Real-browser e2e matrix `e2e/live-tail-follow.e2e.ts` (both sort orders; Chromium and WebKit, run locally for WebKit): on the code before this
  change 3 of 14 failed in each engine (a key or wheel during a reveal, resize while pinned and not pinned, following with a key away).
- Lint, report-only (ER-41): `no-restricted-syntax` on assignments to `scrollTop` in `src/app/grid/`, in `npm run lint:report`
  (`DEBRIEF_LINT_REPORT=1`); first run on the real tree 2026-11-16: no findings, and a probe writer is reported. It moves to the normal lint after
  its first green run in CI (run id to be recorded here).
