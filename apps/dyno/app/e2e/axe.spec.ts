import { axeTargets, dialogContracts, dialogWindowUrl, expect, test, type AxeTarget } from '@decryp7/paddock-e2e';
import { FIXTURES } from './dialog-fixtures';

// Accessibility (C33): axe over every Dyno screen, dialog window and the tray popover, in both themes, against WCAG 2.2 AA.
const UI = '../../../ui/projects/paddock-ui/shell';
const CONTRACTS = dialogContracts('src/app/generated/dialog-contracts.ts', ...['about', 'confirm'].map((p) => `${UI}/${p}/generated/dialog-contracts.ts`));
const PAGES = ['', 'processes', 'why-slow', 'throttling', 'issues', 'inventory', 'network', 'counters', 'benchmarks', 'alarms', 'reports', 'recorder', 'health', 'backup', 'extras', 'lifespan', 'diagnostics', 'cpu'];

const pages: AxeTarget[] = PAGES.map((p) => ({
  name: `page ${p || 'dashboard'}`,
  url: `/?latency=0#/${p}`,
  ready: async (page) => {
    await expect(page.getByRole('heading', { level: 1 }).first()).toBeVisible();
    await page.waitForTimeout(800);
  },
}));

const dialogs: AxeTarget[] = Object.keys(CONTRACTS).map((id) => ({
  name: `dialog ${id}`,
  url: dialogWindowUrl(id, FIXTURES[id]?.[0] ?? {}, { title: id }),
  ready: async (page) => {
    await page.locator('html[data-dialog-ready]').waitFor({ timeout: 15_000 });
  },
}));

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
    expect(await axeTargets(page, pages)).toEqual({});
  });
  test('every dialog window and the tray popover have no violations in either theme', async ({ page }) => {
    expect(await axeTargets(page, [...dialogs, tray])).toEqual({});
  });
});
