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
const tray: AxeTarget = {
  name: 'tray popover',
  url: '/?latency=0#/tray',
  ready: async (page) => {
    await expect(page.getByRole('button', { name: /Open Dyno/ })).toBeVisible();
  },
};
const paletteTarget = (name: string, keys: string, typed: string, expected: RegExp | null): AxeTarget => ({
  name,
  url: '/?latency=0#/',
  ready: async (page) => {
    await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible({ timeout: 45_000 });
    await page.keyboard.press(keys);
    const input = page.getByRole('combobox', { name: /Type a command/ });
    await expect(input).toBeFocused();
    if (typed) await input.fill(typed);
    if (expected) await expect(page.getByRole('option').first()).toHaveText(expected);
    else await expect(page.getByTestId('palette-empty')).toBeVisible();
  },
});
const palettes = [
  paletteTarget('command palette: commands', 'ControlOrMeta+Shift+P', '', /./),
  paletteTarget('command palette: go to, with a query', 'ControlOrMeta+p', 'proc', /Processes/),
  paletteTarget('command palette: go to, nothing matches', 'ControlOrMeta+p', 'zzzzzz', null),
];

test.describe('Dyno accessibility (axe, WCAG 2.2 AA)', () => {
  test.setTimeout(600_000);
  test('every screen has no violations in either theme', async ({ page }) => {
    expect(await axeTargets(page, PAGES.map(target))).toEqual({});
  });
  test('every dialog window and the tray popover have no violations in either theme', async ({ page }) => {
    expect(await axeEveryDialog(page, CONTRACT_FILES, { inputs: INPUTS })).toEqual({});
    expect(await axeTargets(page, [tray, ...palettes])).toEqual({});
  });
});
