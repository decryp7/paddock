import { expect, test } from '@playwright/test';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { axeInThemes } from '../../../../ui/projects/paddock-e2e/src/axe.ts';
import { openApp } from './app';

// axe (WCAG 2.2 A/AA) over every route in the light and the dark theme (rule 17). a11y.baseline.json holds the exact
// violations measured when this spec was added, per project and route: a violation not in it fails the test, and one that is
// gone fails it too (delete its entry). WP-2/WP-3 own the fixes. Re-record with
// `PW_RECORD_BASELINE=1 npx playwright test e2e/a11y.e2e.ts --project=chromium` and review the diff.

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

const BASELINE_FILE = join(__dirname, 'a11y.baseline.json');
const RECORD = process.env['PW_RECORD_BASELINE'] === '1';
const baseline: Record<string, string[]> = existsSync(BASELINE_FILE) ? JSON.parse(readFileSync(BASELINE_FILE, 'utf8')) : {};
const recorded: Record<string, string[]> = {};

/** Angular's generated ids (`ng-1`, `ng-c123`, `cdk-describedby-message-ng-1-9`) differ between runs. */
const stable = (line: string): string => line.replace(/ng-c?\d+(-\d+)?/g, 'ng-N');

test.afterAll(() => {
  if (!RECORD) return;
  const current: Record<string, string[]> = existsSync(BASELINE_FILE) ? JSON.parse(readFileSync(BASELINE_FILE, 'utf8')) : {};
  const merged = { ...current, ...recorded };
  const sorted = Object.fromEntries(Object.entries(merged).filter(([, v]) => v.length).sort(([x], [y]) => x.localeCompare(y)));
  writeFileSync(BASELINE_FILE, `${JSON.stringify(sorted, null, 2)}\n`);
});

for (const route of ROUTES) {
  test(`${route.name}: no new axe violations in light and dark`, async ({ page }, info) => {
    await openApp(page, route.hash);
    await page.waitForLoadState('networkidle');
    const found = (await axeInThemes(page)).map(stable).sort();
    const id = `${info.project.name}/${route.name}`;
    if (RECORD) {
      recorded[id] = found;
      return;
    }
    expect(found, id).toEqual([...(baseline[id] ?? [])].sort());
  });
}
