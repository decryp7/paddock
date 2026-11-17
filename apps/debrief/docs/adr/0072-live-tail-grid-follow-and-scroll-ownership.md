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
   it requested and the mode (`end`, `shift`, `keep`). Only answers to those pages may move the view, and the round's arrival starts
   with an answer to one of them (a page fetched for another reason does not start it). Each later owned answer with a larger total
   re-applies the mode: `end` scrolls to the new end, `shift` moves down by the extra rows. The round's hold on the view ends at the first
   of: the last owned page answered, an owned page failed (a round whose first owned page failed never starts), the user took over (2:
   a round that has not answered yet downgrades `end` to `keep`; `shift` stays, it only keeps the same events in view), a new round or a
   reset. **Growth outside a round (G9):** a page fetched for another reason that reports a larger total than the cache had is reported
   (`onGrow`); the reducer (3) decides what it does to the view.
2. **One owner of scroll writes, and two signals for the user.** `EventsGrid.setTop` is the only code that writes `scrollTop`
   (true today; a lint keeps it so). The user has taken over when either:
   - an input event arrives on the grid that moves the view **away from the newest end**: `wheel`, `pointerdown` on the scroller itself
     (the scrollbar; the view is not re-pinned until it is released), `touchstart` on the scroller, or a `keydown` that moves away (arrows, Page
     Up/Down, Home, End). An input toward the newest end, or one that cannot move the view, takes nothing over (xterm does the same). This is the DevTools signal, and it is the only one that sees a
     wheel in the scaled-scrollbar mode or a keyboard move, because there the grid writes the scroll itself;
   - a `scroll` event that is not an echo of our write. A pure `classifyScroll(read, pendingWrite, lastWrite, maxScroll, recentInput,
     layoutChanged, layoutDelta)` returns `own` (within 1 px of the last write, or clamped at the current maximum after our write),
     `spurious` or `user`. `spurious` is narrow: no input moved the view since our last write (a gesture counts from its first input to
     the next write, and for as long as the pointer is held, so a drag across live rounds is never spurious) **and** the layout changed
     since that write **and** the position is one of the two shapes the browsers were seen to produce: a far position reset to 0, or a
     shift by about the size of the layout change (`layoutDelta`, the change of the furthest scroll position). Any other position with no
     input (find in page, assistive technology, in any mode) is `user` and is kept. There is no time window: the layout key and the
     gesture decide (a first version used a 300 ms window with no recorded measurement, ER-12, and reverted a real scrollbar drag).
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
   | any | the user moved the view (a position event, `byUser`) | pinned is whether it is on the newest end; the round lets go |
   | any | the grid moved the view itself (`byUser` false) | landing on the newest end pins; anything else leaves `pinned` |
   | Following, pinned | a page outside a round reported a larger total (`grew`) | oldest first: write the new end after the render; newest first: stays at the top |
   | Following, not pinned | `grew` | oldest first: nothing (rows are added below); newest first: shift by the extra rows |
   | Holding | `grew` | pinned (a revealed view): as following; not pinned: nothing (the held rows stay) |
   | Holding | released without Follow on (live tail back, a pause cleared) | following; `pinned` unchanged |
   | any | new query or sort order | reset to the top (unchanged) |
4. **Writes happen after the render when the size changes.** A write for a new total or a new viewport height waits for
   `afterNextRender`; its position signal is set at once. At most one write is pending; a newer one replaces it, and a user
   take-over cancels it. A write that does not change the size (a wheel in scaled mode, a keyboard move) writes at once.

## Check

- **Real-browser e2e matrix**, `apps/debrief/app/e2e/live-tail-follow.e2e.ts` (and the reveal case in `live-tail.e2e.ts`), projects `chromium` and `webkit`
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

## Implementation (2026-11-16, amended 2026-11-17 after the review)

- `follow-reducer.ts` (`followStep`, G8) and `scroll-classify.ts` (`classifyScroll`, G3) are pure, each with its table test. The reducer owns every change
  of `holding` and `pinned`: events `data`, `resize`, `userTookOver`, `positioned{atNewest, byUser}`, `grew`, `hold`, `released`, `reveal`, `followOn`, `reset`.
  `setTop` is a pure writer (it does not touch the follow state); `go` writes and reports the landing as `positioned`.
- `EventsGrid`: one pending-write slot dropped on a take-over (G2), also used for a write the rendered spacer cannot reach yet; `userTookOver()` from a
  `wheel` or key that moves **away** from the newest end, a press on the scrollbar (not re-pinned until it is released) and a touch on the scroller (G1), and
  a position event for any scroll the classifier calls `user`; a stored `pinned` flag re-applied on resize (G4); `setTop` is the single writer (through
  `scroll-writer.ts`). Scroll events the browser causes itself while the layout changes, with no input, are `spurious`: the grid writes its own position back.
  Logs (ER-27): scheduled, dropped and written writes, take-overs with their input, scroll-origin changes (not every event), arrivals and growth.
- `PagedRowsStore`: the round's follow-up covers `shift` as well as `end` (G5); the arrival starts from an owned page; a take-over downgrades a pending `end` to
  `keep`; a failed owned page ends the hold (G6); `onGrow` reports a larger total from a page outside a round (G9).
- Real-browser e2e matrix `e2e/live-tail-follow.e2e.ts` (both sort orders; Chromium and WebKit, WebKit run locally): reveal with pushes between its pages,
  following over 20 pushes, a wheel, a key and a real-mouse scrollbar drag during a reveal, a wheel at once after the reveal click, a wheel toward the newest end,
  a wheel in the scaled mode, resize while pinned and not pinned, Follow off and on. Top-row checks compare event ids. Measurement after the second review: 26 tests x 20
  repeats = 520 runs per engine with 4 workers, on an idle machine Chromium 520 of 520 and WebKit 517 of 520 (dropped or undelivered inputs, see lessons.md), under heavy
  external load Chromium 519 of 520 and WebKit 511 of 520. The earlier 460-run result (460 of 460 per engine) was of the first review round's code. The first matrix (14 tests, 69e230d15) failed 3 of 14 in each engine on the
  code before this ADR and was green 14 of 14 afterwards, but the 20-repeat runs of the first implementation failed 3 to 5 times per engine (see lessons.md).
- Input in the matrix (2026-11-17, after the third round). The ownership rows (a wheel, keys and a scrollbar press during a reveal, a wheel at once after the click, the scaled
  wheel, resize not pinned, following over 20 pushes) dispatch untrusted `WheelEvent`, `KeyboardEvent` and `PointerEvent` on the viewport and move `scrollTop` from the test, as
  TanStack Virtual's and the CDK's own tests do; they wait on page time only. A smoke set keeps real input: the real wheel, the real keys (the viewport is asserted focused first),
  the real scrollbar thumb, and the drag across refresh rounds, with the capped re-send that only fires when neither a take-over nor a scroll event followed the last send. Matrix
  of 32 tests x 20 repeats = 640 runs per engine, `--workers 4`, idle machine: Chromium 640 of 640; WebKit 639 of 640, the one failure a real wheel of the smoke set. Every
  dispatched ownership row is 640 of 640 in both engines. The WebKit residue is measured and its cause is headless WebKit dropping input: on the earlier 520-run matrix 517 of 520
  (the reviewer measured 259 of 260), and in those runs the grid log shows the wheel received and the take-over logged, then no scroll event and no grid write. It is not a view
  pulled back by the grid. G10 (a wheel that arrives before the reveal request is handled; found by the dispatched rows) is closed by the request-sequence rule below.
- Lint, report-only (ER-41): `no-restricted-syntax` on assignments to `scrollTop`, `scrollTo`, `scrollBy`, `scrollIntoView` and `Object.assign(el, { scrollTop })`
  in `src/app/grid/`, and `no-restricted-imports` of `scroll-writer` outside `events-grid.ts`, in `npm run lint:report` (`DEBRIEF_LINT_REPORT=1`); first run on the
  real tree 2026-11-17: no findings, and a probe file with each shape is reported. It moves to the normal lint after its first green run in CI (run id to be recorded here).

## Amendment 2026-11-17: one request-sequence rule (G1, G2, G9, G10)

The four ordering findings had one shape: a request that moves the view was applied by an effect after a later user input. The rule that closes the class:
- Every request that moves the view (a reveal, Follow on, a resize re-pin, a new query) carries a sequence stamp from `nextRequestSeq()` (`grid/request-seq.ts`), taken
  when the request is made (the pill click stamps `LiveFeed.revealSeq`), not when an effect handles it. A user take-over is stamped when the input arrives and the reducer
  keeps it as `FollowState.takeOver`.
- `followStep` drops any request stamped before the last take-over: no write, no pin. A stale reveal or Follow on still reloads the rows, in a round that keeps the view where
  it is. Nothing outside the reducer decides this, so no effect can apply a stale request.
- Table rows in `follow-reducer.spec.ts` cover each request kind arriving after a take-over (and after, applied); the G10 unit spec in `events-grid.spec.ts` covers the real
  order (the wheel before the effect). Without the rule, 7 of those tests fail.
- Measured on the final code (sequence rule, real-input smoke set included): 32 tests x 20 repeats = 640 runs per engine, `--workers 4`, idle machine (CPU idle 74% at the start):
  Chromium 640 of 640, WebKit 640 of 640. The earlier run of the same matrix before the rule had one headless-WebKit dropped real wheel (639 of 640), recorded above.
