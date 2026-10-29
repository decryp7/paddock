import { expect, Page, test } from '@playwright/test';
import { expectNothingCovers } from '../../../../ui/projects/paddock-e2e/src/hit-test.ts';
import { openApp, openView } from './app';
import { runCommand } from './analysis-support';
import { chooseMenu, showProfileEditor } from './menu-support';

// A tab strip too narrow for its tabs ends in a chip ("⌄ 2") that lists the hidden tabs.

const tab = (page: Page, name: string) => page.getByRole('tab', { name, exact: true });
const chip = (page: Page) => page.getByRole('button', { name: /^\d+ more tabs$/ });
const list = (page: Page) => page.locator('.dv-tabs-overflow-container');

/** Metrics, Profile, Video and two tools share the document strip beside the timeline (Registry has its own column), so it shows some tabs and hides the rest. */
async function overflowStrip(page: Page): Promise<void> {
  await openApp(page);
  await openView(page, 'metrics');
  await showProfileEditor(page);
  await chooseMenu(page, 'File', 'Open video…');
  await runCommand(page, 'Compare configuration files');
  await runCommand(page, 'Open collection schedules');
  await expect(chip(page)).toBeVisible();
}

test.describe('tab overflow list', () => {
  test('the chip is a named button with a tooltip; clicking it lists the hidden tabs on top, and a row opens its tab', async ({ page }) => {
    await overflowStrip(page);
    await expect(chip(page)).toHaveAttribute('title', /^\d+ more tabs$/);
    await chip(page).click();
    await expect(list(page)).toBeVisible();
    await expectNothingCovers(list(page), { scroll: false, extra: '.dv-tab' });
    // Whichever tab the strip's width hides first (the width is the plan's, not this test's).
    const hidden = list(page).locator('.dv-tab').first();
    const name = ((await hidden.innerText()) ?? '').replace(/[▾×]/g, '').replace('📌', '').trim();
    await hidden.click();
    await expect(list(page)).toHaveCount(0);
    await expect(tab(page, name)).toHaveAttribute('aria-selected', 'true');
  });

  test('the chip opens with the keyboard', async ({ page }) => {
    await overflowStrip(page);
    await chip(page).focus();
    await page.keyboard.press('Enter');
    await expect(list(page)).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(list(page)).toHaveCount(0);
    await chip(page).focus();
    await page.keyboard.press(' ');
    await expect(list(page)).toBeVisible();
  });
});
