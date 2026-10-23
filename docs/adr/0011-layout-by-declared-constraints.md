# 11. Layout by declared constraints

- Status: Proposed
- Date: 2026-10-24
- Related: rule 18 (panes lay out by container queries; no horizontal scroll at any width >= 320 px), rule 39, rule 40
- First adopter: Debrief ADR 0056 (replaces the placement heuristics in `dock-placement.ts`).

## Context

Debrief's docked workspace (dockview) was tuned by adding a rule after each complaint: grid squeezed, then Home
cramped, then the AI pane 180 px wide, then Health finding titles one letter wide. Each fix moved the problem; the
constants (320 px side tools, a 560/420 main column, "tab Home into Events when there is no room") live inside placement
code that runs only when a panel is added. Only one built-in view declares a minimum size. Container queries (rule 18)
make a pane usable at its minimum but cannot decide which pane yields.

Survey (read-only, 2026-10-24):

| App | Layout today | Exposure |
|---|---|---|
| Pitcrew, Pitwall, Spotter | a fixed shell: sidebar plus one routed page plus toasts; no docking; some resizing (3-5 files) | Low for docking; the same squeeze appears as sidebar + page + side panels at 800 px |
| Dyno | sidebar (collapsible) plus pages and a popover; resizing in 14 files; dashboards with tiles | Medium: dashboard tiles and a collapsible sidebar are exactly the "what yields first" problem |
| Sighter | a studio (video/images, labelling, review queue, inspector) | High: a studio is a docked workspace; adopt from the start |
| Pacenotes | a code browser with tree, editor/diagram, search, assistant | High: same shape as Debrief |

## Decision

1. **Every view declares layout constraints**: area (`document | side | analysis | status`), min and preferred
   width and height, and a priority.
2. **One pure layout function** (`planLayout(viewport, openViews, savedHints) -> LayoutPlan`), DOM-free and
   deterministic, places views: never below a minimum; when there is no room, the lowest-priority views are tabbed or
   auto-hidden instead of squeezed; leftover space goes to the highest-priority document. Window resizes, view opens
   and closes and perspective loads re-plan; a user's drag becomes the new preference, still clamped to the minimum.
3. **Saved layouts** that violate minimums (a smaller monitor, a new minimum after an update) are fitted at load by the
   same function, not rewritten on disk, with one notification and an Undo.
4. **Shared** (rule 39): `planLayout`, its types and tests, and a Playwright helper that asserts each pane against its
   declared minimum at 1440x900 and 1920x1080 (plus a resize case) live in `@decryp7/paddock-ui`. Each app keeps its
   adapter (dockview, a fixed shell, a tile grid) and its view declarations. Apps without docking still use the
   function to decide which of sidebar / list / detail / assistant is visible at a given width.
5. **Tests required per app**: table-driven plans over viewport sizes, a property test that no placed view is under its
   minimum, and the scenario spec at both standard sizes.

## Consequences

- Layout complaints become a change to a declaration plus a scenario, not a new branch.
- Small windows hide or tab lower-priority views; the notification explains and offers Undo.
- Apps with fixed shells gain a way to prove "no horizontal scroll at 320 px" at shell level.

## Migration plan

1. Debrief adopts (ADR 0056: plan function and scenario table before 1.1; switch-over when green).
2. Extract `planLayout` and the helper to `paddock-ui` after 1.1.
3. Dyno dashboards, then Sighter and Pacenotes from the first studio/browser screen; Pitcrew, Pitwall and Spotter use it
   for sidebar and detail panes when they next change their shell.

## Alternatives considered

- **Per-app heuristics.** The Debrief history is the case against.
- **Container queries only.** Needed, not sufficient (see Context).
- **Replace dockview with CSS grid.** Loses docking and floating; the constraints map onto dockview's own API.
