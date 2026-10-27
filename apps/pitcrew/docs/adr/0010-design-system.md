# 10. The design system: tokens, partials and a small set of shared components

- Status: accepted (2026-10-22)
- Related: ADR 0001 (architecture), ADR 0008 (native feel); code: `app/src/styles/`, `app/src/app/shared/`

## Context

The suite's apps (Pitcrew, Spotter, Pitwall, Dyno) share a visual language and Pitcrew is the reference
implementation. The rules lived in review comments; this records them so new screens and the sibling apps copy the
same thing.

## Decision

1. **Tokens first.** `styles/_tokens.scss` is the only place that names a raw colour, size or duration: type scale,
   a 4 px space grid, radii, motion, z-layers, light and dark colour sets (`@mixin light` / `@mixin dark`, applied by
   `prefers-color-scheme` and by the user's choice), status roles, chart palette and shadows. Components use
   `var(--...)` only.
2. **Accessibility is measured, not assumed.** Text pairs are at least 4.5:1 in both themes; status is carried by words
   and icons first, colour second; status hues and the categorical chart palette are checked for colour-vision
   deficiency (adjacent chart colours stay distinguishable under protanopia, deuteranopia and tritanopia). Focus rings
   show on `:focus-visible` only; motion respects `prefers-reduced-motion`.
3. **Shared building blocks live in global partials**, components keep only their layout: `_base` (element defaults,
   native feel), `_controls` (buttons, inputs, toggles), `_surfaces` (pages, cards, badges, callouts, tables, menus,
   dialogs, skeletons). A rule used by a second screen moves from the component into a partial (rule of three for new
   abstractions, two for styles).
4. **Standard states.** Every data view has a skeleton while loading, an empty state with the next step, and an inline
   error with Retry (`pc-load-state`, `pc-empty-state`).
5. **Libraries.** Angular CDK for behaviour (virtual scroll, overlays, a11y), Angular Material selectively (toggle,
   tooltip), themed through the tokens; Apache ECharts for charts (tokens are passed in, never hard-coded); icons are
   Pitcrew's own SVG set registered with `MatIconRegistry` in the `pc` namespace. No icon fonts and no runtime URLs.
6. **Native window chrome.** Mica on Windows, vibrancy and inset traffic lights on macOS, a plain solid window on
   Linux (no compositor effects are assumed); typography is the OS UI font (ADR 0008).
7. **Documentation screenshots** of every screen in both themes are produced by Playwright against the mock backend
   and reviewed before commit.

## Consequences

- A new screen needs no new colour or spacing literal; if it does, the token is added first.
- The sibling apps copy `styles/` and `shared/ui` as their starting point.

## Amendment 2026-10-23: the shared package (`@decryp7/paddock-ui`) is not adopted for v1

Looked at `@decryp7/paddock-ui` 0.1.0 (tokens, base styles, `Su*` components, an ECharts theme). The overlap with
Pitcrew's own code is a status pill, metric, sparkline, card, empty/error state and segmented control, each already
present here with Pitcrew's own markup, specs and e2e tests, and the package's tokens differ from the ones the design
review fixed for Pitcrew (accent never for status, neutral badges, the validated status and chart palettes). Swapping
them would be churn with visual change and a registry dependency in CI (`.npmrc`) for no feature gain. Not forced:
Pitcrew keeps its local components; the package stays an option for the next design round, starting with its chart theme
and the tray popover (which Pitcrew does not have).
