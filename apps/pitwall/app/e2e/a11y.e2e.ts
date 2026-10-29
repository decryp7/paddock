import { expect, test } from '@playwright/test';
import { axeInThemes } from '../../../../ui/projects/paddock-e2e/src/axe.ts';
import { openApp } from './app';

// axe (WCAG 2.2 A/AA) over every route in the light and the dark theme (rule 17). A violation names the theme, the rule and
// the nodes. The dialogs are checked with their screens in the layout spec's states; add them here as they get contracts.

const ROUTES = [
  { name: 'dashboard', hash: '/' },
  { name: 'models', hash: '/models' },
  { name: 'engines', hash: '/engines' },
  { name: 'apps', hash: '/apps' },
  { name: 'requests', hash: '/requests' },
  { name: 'playground', hash: '/playground' },
  { name: 'benchmarks', hash: '/benchmarks' },
  { name: 'settings', hash: '/settings' },
  { name: 'tray', hash: '/tray' },
] as const;

// Measured 2026-10-29; marked test.fail so the fix (WP-2/WP-3) removes the entry.
const KNOWN_VIOLATIONS: Readonly<Record<string, string>> = {
  dashboard: 'aria-prohibited-attr on status dots and job dots (aria-label on a span)',
  engines: 'aria-conditional-attr on table rows',
  settings: 'color-contrast on .custom > .p-hint',
  tray: 'aria-prohibited-attr on span[data-tone]',
};

for (const route of ROUTES) {
  test(`${route.name}: no axe violations in light and dark`, async ({ page }) => {
    test.fail(route.name in KNOWN_VIOLATIONS, KNOWN_VIOLATIONS[route.name] ?? '');
    await openApp(page, route.hash);
    await page.waitForLoadState('networkidle');
    expect(await axeInThemes(page)).toEqual([]);
  });
}
