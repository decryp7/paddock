import { axeEveryDialog, axeTargets, expect, test, type AxeTarget } from '@decryp7/paddock-e2e';
import { FIXTURES } from './dialog-fixtures';

// Accessibility (C33): axe over every Dyno screen, dialog window and the tray popover, in both themes, against WCAG 2.2 AA.
const UI = '../../../ui/projects/paddock-ui/shell';
const CONTRACT_FILES = ['src/app/generated/dialog-contracts.ts', ...['about', 'confirm'].map((p) => `${UI}/${p}/generated/dialog-contracts.ts`)];
const INPUTS = Object.fromEntries(Object.entries(FIXTURES).map(([id, list]) => [id, list[0] ?? {}]));
const PAGES = ['', 'processes', 'why-slow', 'throttling', 'issues', 'inventory', 'network', 'counters', 'benchmarks', 'alarms', 'reports', 'recorder', 'health', 'backup', 'extras', 'lifespan', 'diagnostics', 'cpu'];

const target = (p: string): AxeTarget => ({
  name: `page ${p || 'dashboard'}`,
  url: `/?latency=0#/${p}`,
  ready: async (page) => {
    await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible({ timeout: 45_000 });
    // Settled: nothing is loading and every finite CSS/Web animation (fades, skeleton to content) has finished, so axe sees
    // final colours. getAnimations() cannot see ECharts, which animates on canvas; the charts' own entry is short and the
    // chart text is not what axe measures there.
    await page.waitForFunction(
      () =>
        !document.querySelector('[aria-busy="true"]') &&
        document.getAnimations().every((a) => a.playState === 'finished' || a.effect?.getTiming().iterations === Infinity),
      undefined,
      { timeout: 30_000 },
    ).catch((e: unknown) => {
      throw new Error(`${p || 'dashboard'} never settled (still aria-busy or animating after 30 s): ${String(e)}`);
    });
  },
});
// The network page's rows are the one grid still made of div rows with aria-rowindex (axe: aria-conditional-attr). Rebuilding it
// as the shared table is phase c; it is the only exclusion, and the rest of that page is still checked.
const NETWORK_GRID = ['div[role="row"][aria-rowindex]'];

const tray: AxeTarget = {
  name: 'tray popover',
  url: '/?latency=0#/tray',
  ready: async (page) => {
    await expect(page.getByRole('button', { name: /Open Dyno/ })).toBeVisible();
  },
};

test.describe('Dyno accessibility (axe, WCAG 2.2 AA)', () => {
  test.setTimeout(600_000);
  test('every screen has no violations in either theme', async ({ page }) => {
    expect(await axeTargets(page, PAGES.filter((p) => p !== 'network').map(target))).toEqual({});
  });
  test('the network page has no violations outside its row grid (phase c rebuilds that)', async ({ page }) => {
    expect(await axeTargets(page, [target('network')], { exclude: NETWORK_GRID })).toEqual({});
  });
  test('every dialog window and the tray popover have no violations in either theme', async ({ page }) => {
    expect(await axeEveryDialog(page, CONTRACT_FILES, { inputs: INPUTS })).toEqual({});
    expect(await axeTargets(page, [tray])).toEqual({});
  });
});
