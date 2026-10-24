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
  /** Report rule ids only, without the nodes. */
  readonly idsOnly?: boolean;
}

/** The page's axe violations as `rule: target, target` lines (empty: none), against `WCAG_TAGS`. */
export async function axeViolations(page: Page, options: AxeOptions = {}): Promise<string[]> {
  let builder = new AxeBuilder({ page }).withTags([...WCAG_TAGS]);
  for (const s of options.include ?? []) builder = builder.include(s);
  for (const s of options.exclude ?? []) builder = builder.exclude(s);
  const results = await builder.analyze();
  return results.violations.map((v) => (options.idsOnly ? v.id : `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`));
}
