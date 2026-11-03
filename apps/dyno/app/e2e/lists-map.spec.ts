import { expect, test } from '@decryp7/paddock-e2e';

/** The bad-IP lists, the connections map and the firewall against the mock backend (chromium). */
const url = (hash: string) => `/?latency=0#/${hash}`;

test.use({ reducedMotion: 'reduce' });

test.describe('bad-IP lists', () => {
  test('adds a typed list, previews and applies "Remove duplicates", subscribes by address and removes a list', async ({ page }) => {
    await page.goto(url('lists'));
    await expect(page.getByRole('heading', { level: 1, name: 'Bad-IP lists' })).toBeVisible();
    await expect(page.getByTestId('list-list-1')).toContainText('Work blocklist');
    await expect(page.getByTestId('first-run-note')).toContainText('DB-IP');

    await page.getByLabel('New list name').fill('Messy');
    await page.getByLabel('New list entries').fill('8.8.8.8\n8.8.0.0/16\n8.8.8.8\nx.example\ny.x.example');
    await page.getByTestId('add-list').click();
    const messy = page.getByTestId('list-list-2');
    await expect(messy).toContainText('Messy');

    await messy.getByTestId('dedupe').click();
    await expect(page.getByTestId('preview')).toContainText('is covered by');
    await page.getByTestId('apply-dedupe').click();
    await expect(page.getByTestId('preview')).toHaveCount(0);
    await expect(messy.locator('.counts')).toContainText('2 entries');

    await page.getByTestId('kind-url').check();
    await expect(page.getByTestId('terms')).toContainText('your responsibility');
    await page.getByLabel('New list name').fill('Feed');
    await page.getByLabel('List address').fill('http://lists.example/feed.txt');
    await page.getByTestId('add-list').click();
    await expect(page.getByTestId('edit-error')).toContainText('https://');
    await page.getByLabel('List address').fill('https://lists.example/feed.txt');
    await page.getByTestId('add-list').click();
    await expect(page.getByTestId('list-list-3')).toContainText('refreshed every 24 h');

    await messy.getByRole('button', { name: 'Remove', exact: true }).click();
    await page.getByTestId('confirm-remove').click();
    await expect(page.getByTestId('list-list-2')).toHaveCount(0);
  });

  test('turns a network switch off from the page', async ({ page }) => {
    await page.goto(url('lists'));
    const box = page.getByTestId('switch-own-location').getByRole('checkbox');
    await expect(box).toBeChecked();
    await box.uncheck();
    await expect(box).not.toBeChecked();
    await expect(page.getByTestId('switch-databases').getByRole('checkbox')).toBeChecked();
  });
});

test.describe('connections map', () => {
  test('draws the map with arcs from the home country, a table alternative and the credit', async ({ page }) => {
    await page.goto(url('map'));
    await expect(page.getByRole('heading', { level: 1, name: 'Connections map' })).toBeVisible();
    await expect(page.getByTestId('world-map')).toBeVisible();
    await expect(page.locator('path.arc')).toHaveCount(5);
    await expect(page.getByTestId('home-source')).toContainText('public IP');
    await expect(page.getByTestId('credit')).toContainText('DB-IP');

    const us = page.getByRole('button', { name: /^United States: 54 connections/ });
    await us.focus();
    await expect(us).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page.getByTestId('country-table')).toBeVisible();
    await expect(page.locator('tr.picked')).toContainText('United States');

    await page.getByRole('radio', { name: 'Map' }).click();
    await expect(page.getByTestId('world-map')).toBeVisible();
  });

  test('has no animation under reduced motion', async ({ page }) => {
    await page.goto(url('map'));
    await expect(page.getByTestId('world-map')).toBeVisible();
    expect(await page.locator('svg animate, svg animateMotion, svg animateTransform').count()).toBe(0);
  });
});

test.describe('firewall', () => {
  test('shows the sources read-only and the one that needs the helper', async ({ page }) => {
    await page.goto(url('firewall'));
    await expect(page.getByRole('heading', { level: 1, name: 'Firewall' })).toBeVisible();
    await expect(page.locator('[data-source="applicationFirewall"]')).toContainText('Stealth mode: On');
    await expect(page.locator('[data-source="pf"]').getByTestId('source-note')).toContainText('need the helper');
    await expect(page.getByRole('button', { name: 'Refresh' })).toBeVisible();
  });
});
