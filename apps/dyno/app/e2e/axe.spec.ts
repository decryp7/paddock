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
    await page.waitForTimeout(800);
  },
});
const tray: AxeTarget = {
  name: 'tray popover',
  url: '/?latency=0#/tray',
  ready: async (page) => {
    await expect(page.getByRole('button', { name: /Open Dyno/ })).toBeVisible();
  },
};
const palette: AxeTarget = {
  name: 'command palette',
  url: '/?latency=0#/',
  ready: async (page) => {
    await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible({ timeout: 45_000 });
    await page.keyboard.press('ControlOrMeta+Shift+P');
    await expect(page.getByRole('combobox', { name: /Type a command/ })).toBeFocused();
  },
};

test.describe('Dyno accessibility (axe, WCAG 2.2 AA)', () => {
  test.setTimeout(600_000);
  test('every screen has no violations in either theme', async ({ page }) => {
    expect(await axeTargets(page, PAGES.map(target))).toEqual({});
  });
  test('every dialog window and the tray popover have no violations in either theme', async ({ page }) => {
    expect(await axeEveryDialog(page, CONTRACT_FILES, { inputs: INPUTS })).toEqual({});
    expect(await axeTargets(page, [tray, palette])).toEqual({});
  });
});
