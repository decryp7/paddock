import AxeBuilder from '@axe-core/playwright';
import type { Page } from '@playwright/test';

/** The WCAG rules every Paddock app is held to (2.2 AA). */
export const WCAG_TAGS: readonly string[] = ['wcag2a', 'wcag2aa', 'wcag22aa'];

/** Both themes, for `for (const colorScheme of THEMES)` loops (every screen is checked in each). */
export const THEMES = ['dark', 'light'] as const;
export type Theme = (typeof THEMES)[number];

export interface AxeOptions {
  /** Only these parts of the page (CSS selectors). */
  readonly include?: readonly string[];
  readonly exclude?: readonly string[];
  /** Tags checked besides `WCAG_TAGS` (`best-practice`, say). */
  readonly extraTags?: readonly string[];
  /** Report rule ids only, without the nodes. */
  readonly idsOnly?: boolean;
}

/**
 * Waits until the page's finite animations and CSS transitions have ended: axe reads the colours at the moment it looks, and a
 * theme switch is a colour transition (axe saw the old scheme's text colour on the new background, slowly under emulation).
 * Endless animations (a spinner) are left running.
 */
export async function settleTransitions(page: Page): Promise<void> {
  await page.evaluate(async () => {
    const finite = () => document.getAnimations().filter((a) => a.effect?.getComputedTiming().endTime !== Infinity);
    for (let round = 0; round < 5 && finite().length; round++) await Promise.all(finite().map((a) => a.finished.catch(() => undefined)));
  });
}

/** The page's axe violations as `rule: target, target` lines (empty: none), against `WCAG_TAGS`. */
export async function axeViolations(page: Page, options: AxeOptions = {}): Promise<string[]> {
  await settleTransitions(page);
  let builder = new AxeBuilder({ page }).withTags([...WCAG_TAGS, ...(options.extraTags ?? [])]);
  for (const s of options.include ?? []) builder = builder.include(s);
  for (const s of options.exclude ?? []) builder = builder.exclude(s);
  const results = await builder.analyze();
  return results.violations.map((v) => (options.idsOnly ? v.id : `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`));
}

/** `axeViolations` in each theme (the page is switched with `emulateMedia`), as `theme: rule: targets` lines. */
export async function axeInThemes(page: Page, options: AxeOptions & { readonly themes?: readonly Theme[] } = {}): Promise<string[]> {
  const out: string[] = [];
  for (const theme of options.themes ?? THEMES) {
    await page.emulateMedia({ colorScheme: theme });
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
    await page.emulateMedia({ colorScheme: theme });
    for (const target of targets) {
      // A fresh document each time: a change of the hash alone keeps the previous window (and its theme transition) alive.
      await page.goto('about:blank');
      await page.goto(target.url);
      await target.ready?.(page);
      // Colours are measured at rest, not in the middle of a fade.
      await page.evaluate(() => Promise.all(document.getAnimations().map((a) => a.finished.catch(() => undefined))));
      const found = await axeViolations(page, options);
      if (found.length) failures[`${target.name} (${theme})`] = found;
    }
  }
  return failures;
}
