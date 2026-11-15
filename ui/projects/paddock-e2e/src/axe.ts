import AxeBuilder from '@axe-core/playwright';
import { test, type Page } from '@playwright/test';

/** The WCAG rules every Paddock app is held to (2.2 AA). */
export const WCAG_TAGS: readonly string[] = ['wcag2a', 'wcag2aa', 'wcag22aa'];

/** Both themes, for `for (const colorScheme of THEMES)` loops (every screen is checked in each). */
export const THEMES = ['dark', 'light'] as const;
export type Theme = (typeof THEMES)[number];

export interface AxeOptions {
  /** Only these parts of the page (CSS selectors). */
  readonly include?: readonly string[];
  readonly exclude?: readonly string[];
  /** Report rule ids only, without the nodes. */
  readonly idsOnly?: boolean;
}

/**
 * Brings the page's finite animations and CSS transitions to their end state at once: axe reads the colours at the moment it
 * looks, and a theme switch is a colour transition (axe saw the old scheme's text colour on the new background). They are
 * finished, not waited for: a transition that restarts on every tick (a countdown bar's width) never ends by itself, and
 * waiting for it cost axe 1-23 s per theme. Endless animations (a spinner) are left running.
 */
export async function settleTransitions(page: Page): Promise<void> {
  await page.evaluate(() => {
    const finite = () => document.getAnimations().filter((a) => a.effect?.getComputedTiming().endTime !== Infinity && a.playState !== 'finished');
    for (let round = 0; round < 5 && finite().length; round++) for (const a of finite()) a.finish();
  });
}

/**
 * Properties whose change moves a colour axe reads, by whole name: these and every `*-color`. A width, transform,
 * `stroke-dashoffset` (Dyno's ticking ring) or `border-radius` transition moves no colour.
 */
const COLOUR_PROPERTIES: readonly string[] = [
  'color',
  'background',
  'background-color',
  'background-image',
  'opacity',
  'filter',
  'backdrop-filter',
  'fill',
  'stroke',
  'box-shadow',
  'text-shadow',
  'visibility',
  'border',
  'outline',
];

/**
 * The most scans one screen needed, measured 2026-11-15 over ui's shell-demo, workbench-demo and gallery data specs and
 * Dyno's axe, dashboard-grid, tray-widgets and screenshot specs (Chromium, 359 + 31 tests), and Pitwall's a11y and
 * chat-composer specs (WebKit, CI image, 19 tests): 3, the workbench demo's assistant pane (a tooltip hiding, then a
 * background transition). Every other screen needed 1 except three that needed 2 (docs/benchmarks.md, "axe scans per screen").
 */
const MEASURED_MAX_SCANS = 3;
/** One more than measured, so one more late start does not fail a screen, while a page that animates colours for ever does. */
const MAX_SCANS = MEASURED_MAX_SCANS + 1;

/** When axeInThemes or axeTargets last switched a page's colour scheme (for the rescan log; ER-27). */
const lastSchemeSwitch = new WeakMap<Page, { readonly scheme: Theme; readonly at: number }>();

/**
 * Switches the page's colour scheme and waits until it is live (ER-33: settling before that measures the old scheme): the
 * page's `matchMedia` reports it, then two animation frames pass, so the rendering update that applies the scheme to the
 * styles (WebKit applies an emulated scheme there, not when `emulateMedia` returns) has run and its transitions exist to be
 * settled.
 */
async function switchScheme(page: Page, scheme: Theme): Promise<void> {
  await page.emulateMedia({ colorScheme: scheme });
  lastSchemeSwitch.set(page, { scheme, at: Date.now() });
  await page.waitForFunction((s) => matchMedia(`(prefers-color-scheme: ${s})`).matches, scheme);
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
}

/**
 * The colour transitions and animations that began on the page since the last call, plus those still running.
 * `transitionrun` and `animationstart` fire for every one that starts, however short, but only at the page's next rendering
 * update, so one that began just now is found among the running animations instead. An animation counts as colour when its
 * own keyframes (read from the CSSAnimation) name a colour property; one whose keyframes are empty or cannot be read (CDK's
 * `cdk-text-field-autofill-*`, `{ }`) does not.
 *
 * The first call installs two capture listeners on the document; they stay until the page navigates (one pair per document,
 * a few strings kept between two calls).
 */
async function colourMotionSinceLastLook(page: Page): Promise<string[]> {
  return page.evaluate((names) => {
    const isColour = (property: string) => names.includes(property) || property.endsWith('-color');
    const kebab = (key: string) => key.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
    const META = new Set(['offset', 'computedOffset', 'easing', 'composite']);
    const colourKeyframes = (a: Animation): boolean => {
      try {
        const frames = (a.effect as KeyframeEffect | null)?.getKeyframes() ?? [];
        return frames.some((k) => Object.keys(k).some((key) => !META.has(key) && isColour(kebab(key))));
      } catch {
        return false;
      }
    };
    const named = (a: Animation) => (a as CSSAnimation).animationName;
    const w = window as unknown as { __pdAxeMotion?: string[] };
    if (!w.__pdAxeMotion) {
      const log: string[] = (w.__pdAxeMotion = []);
      document.addEventListener('transitionrun', (e) => void (isColour(e.propertyName) && log.push(`transition ${e.propertyName}`)), true);
      document.addEventListener(
        'animationstart',
        (e) => {
          const target = e.target instanceof Element ? e.target : null;
          const mine = (target?.getAnimations() ?? []).filter((a) => named(a) === e.animationName);
          if (mine.some(colourKeyframes)) log.push(`animation ${e.animationName}`);
        },
        true,
      );
    }
    const running = document
      .getAnimations()
      .filter((a) => a.playState !== 'finished' && a.effect?.getComputedTiming().endTime !== Infinity)
      .flatMap((a) => {
        const property = (a as CSSTransition).transitionProperty;
        if (typeof property === 'string') return isColour(property) ? [`transition ${property}`] : [];
        return colourKeyframes(a) ? [`animation ${named(a) ?? '(script)'}`] : [];
      });
    return [...w.__pdAxeMotion.splice(0), ...running];
  }, COLOUR_PROPERTIES);
}

/** Says that a scan counted for nothing, in the log and as an annotation of the running test (ER-27). */
async function reportDiscardedScan(page: Page, scan: number, moved: readonly string[]): Promise<void> {
  const last = lastSchemeSwitch.get(page);
  const dark = await page.evaluate(() => matchMedia('(prefers-color-scheme: dark)').matches).catch(() => null);
  const since = last ? `${Date.now() - last.at} ms since emulateMedia(${last.scheme})` : 'no scheme switch by axeInThemes or axeTargets';
  const description = `scan ${scan} of ${MAX_SCANS} counted for nothing: ${[...new Set(moved)].join(', ')} started during it; ${since}; prefers-color-scheme: dark matches ${dark}`;
  console.log(`axe: ${description}`);
  try {
    test.info().annotations.push({ type: 'axe-rescan', description });
  } catch {
    // Not inside a Playwright test (a script): the log line is the record.
  }
}

/**
 * The page's axe violations as `rule: target, target` lines (empty: none), against `WCAG_TAGS`.
 *
 * Colours read while they change are not a measurement: a scan during which a colour transition or animation started (a
 * theme switch the browser applied late, a form field's hint fading in; Pitwall run 8134 read a row half-way to the dark
 * theme) counts for nothing, is logged and annotated with what moved, and the page is settled and scanned again, at most
 * `MAX_SCANS` times; then it throws naming what kept changing. The last scan's violations are reported as they are.
 */
export async function axeViolations(page: Page, options: AxeOptions = {}): Promise<string[]> {
  // A click leaves the pointer on its target, and a hover tooltip (Material's opens after 500 ms and fades in) would appear
  // while axe scans, half transparent. Moving the pointer away cancels the pending tooltip and closes an open one; this runs
  // before settling so a tooltip that was already open finishes closing.
  await page.mouse.move(0, 0);
  let builder = new AxeBuilder({ page }).withTags([...WCAG_TAGS]);
  for (const s of options.include ?? []) builder = builder.include(s);
  for (const s of options.exclude ?? []) builder = builder.exclude(s);
  let moved: string[] = [];
  for (let scan = 1; scan <= MAX_SCANS; scan++) {
    await settleTransitions(page);
    await colourMotionSinceLastLook(page);
    const results = await builder.analyze();
    moved = await colourMotionSinceLastLook(page);
    if (!moved.length) return results.violations.map((v) => (options.idsOnly ? v.id : `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`));
    await reportDiscardedScan(page, scan, moved);
  }
  throw new Error(
    `axe: colours kept changing during ${MAX_SCANS} scans (${[...new Set(moved)].join(', ')}), so no scan measured them. Next step: find what starts them on this page, or settle it before the scan.`,
  );
}

/** `axeViolations` in each theme (the page is switched with `emulateMedia`), as `theme: rule: targets` lines. */
export async function axeInThemes(page: Page, options: AxeOptions & { readonly themes?: readonly Theme[] } = {}): Promise<string[]> {
  const out: string[] = [];
  for (const theme of options.themes ?? THEMES) {
    await switchScheme(page, theme);
    for (const line of await axeViolations(page, options)) out.push(`${theme}: ${line}`);
  }
  return out;
}

/** One window or screen to check: a URL (relative to the base URL) and what to wait for before axe looks. */
export interface AxeTarget {
  /** A name for the report (a dialog id, a page title). */
  readonly name: string;
  readonly url: string;
  /** Resolves when the screen has finished loading (default: nothing beyond the page load). */
  readonly ready?: (page: Page) => Promise<void>;
}

/**
 * Opens each target in each theme and returns the violations by target name (a target with none is left out), so a spec
 * is `expect(await axeTargets(page, targets)).toEqual({})` and a failure names the window. Targets are visited in order
 * on one page.
 */
export async function axeTargets(page: Page, targets: readonly AxeTarget[], options: AxeOptions & { readonly themes?: readonly Theme[] } = {}): Promise<Record<string, string[]>> {
  const failures: Record<string, string[]> = {};
  for (const theme of options.themes ?? THEMES) {
    await switchScheme(page, theme);
    for (const target of targets) {
      // A fresh document each time: a change of the hash alone keeps the previous window (and its theme transition) alive.
      await page.goto('about:blank');
      await page.goto(target.url);
      await target.ready?.(page);
      const found = await axeViolations(page, options);
      if (found.length) failures[`${target.name} (${theme})`] = found;
    }
  }
  return failures;
}
