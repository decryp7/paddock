# 11. Design system: tokens for everything, system fonts, two themes, a colour-vision-checked palette

- Status: accepted (2026-10-22)
- Related: ADR 0009 (native feel), AGENTS.md "Style", `app/src/styles.scss`, `app/src/app/shared/tokens.ts`,
  `tokens.spec.ts`, `color.ts`

## Context

Dyno is a dense tool: gauges, charts, tables of processes, status colours that mean "fine" or "look here". It runs on
three OSes in dark and light. Colour alone must never carry meaning (colour-blind users, glare, e-ink-like
displays), and every colour pair must be readable.

## Decision

1. **Everything is a token.** `styles.scss` defines custom properties on `:root` for type (`--font-*`, `--fs-2xs` to
   `--fs-display`, `--lh-*`, `--fw-*`), space (`--sp-*`), radius, elevation (`--shadow-1..3`), motion (`--dur-instant..slow`,
   `--ease-*`), colour and the Angular Material theme variables. Components use `var(--…)` only; raw colours, pixel
   sizes in templates and ad-hoc shadows fail review (and the units/format specs).
2. **System fonts, nothing fetched.** `--font-sans` is `system-ui` with Segoe UI Variable, SF Pro and Roboto behind it;
   `--font-mono` is `ui-monospace` with Cascadia/SF Mono/Menlo/Consolas. The CSP forbids remote fonts anyway.
3. **Two themes that follow the system.** Dark and light via `prefers-color-scheme`, overridable by
   `[data-theme='dark'|'light']` on `<html>` (Settings). `[data-vibrancy]`, set by the shell before Angular boots,
   makes the window background translucent where the OS offers Mica or vibrancy.
4. **Colours are checked, not chosen by eye.** `tokens.ts` mirrors the theme values (`LIGHT`, `DARK`) and
   `tokens.spec.ts` asserts, per theme:
   - every text/background pair reaches **4.5:1** (WCAG AA);
   - the accent, focus ring and status colours (ok, warn, bad, info) reach **3:1** against the card;
   - every chart series reaches 3:1 against the card;
   - the first five chart series stay distinguishable under **protanopia, deuteranopia and tritanopia** simulation
     (`simulate` in `color.ts`, CIE ΔE ≥ 10; ≥ 20 for normal vision) and stay apart from the brand accent (ΔE ≥ 15).
   The categorical palette is a colour-blind-safe set (Okabe-Ito-like blues, oranges and greens) in the light and a
   lighter variant in the dark theme. `styles.scss` and `tokens.ts` change together; the spec is the guard.
5. **Colour never stands alone.** Status colours always come with words or numbers (`heat()` washes a cell but the
   number is always printed); series are told apart by legend and tooltip as well as hue.
6. **Brand.** The accent is a magenta-pink (`#be3584` light, `#de559f` dark) chosen to stay clear of every chart series.
7. **Accessibility basics.** Focus rings on `:focus-visible` only but always visible there; real buttons; `aria-label`
   on icon buttons; `prefers-reduced-motion` shortens or removes transitions. Icons are Dyno's own SVG set in the `dy`
   namespace through `MatIconRegistry` (no icon fonts).
8. **Charts** use Apache ECharts with these tokens read at run time (`shared/echarts-setup.ts`), so a theme switch
   restyles charts without a rebuild.

## Consequences

- Adding a colour means adding a token in both themes and a line to `tokens.ts` so the spec covers it.
- A palette change that makes two series confusable under any simulated deficiency fails the unit tests.
