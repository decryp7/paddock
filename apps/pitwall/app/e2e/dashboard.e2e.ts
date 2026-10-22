import { expect, test } from '@playwright/test';
import { openApp } from './app';

test.describe('dashboard', () => {
  test('shows live tiles, loaded models with idle countdowns, a history chart and the events feed', async ({ page }) => {
    await openApp(page);
    await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();
    await expect(page.getByText('System memory', { exact: true })).toBeVisible();
    await expect(page.getByText('GPU memory', { exact: true })).toBeVisible();
    await expect(page.getByText('Throughput', { exact: true })).toBeVisible();
    await expect(page.locator('pw-hot-models')).toContainText('Qwen3');
    await expect(page.locator('pw-hot-models .when').first()).toBeVisible();
    await expect(page.locator('pw-echart canvas').first()).toBeVisible();
    await expect(page.locator('pw-events-feed')).not.toBeEmpty();
  });

  test('pauses and resumes serving and releases memory from the More actions menu; no quick-link row', async ({ page }) => {
    await openApp(page);
    await expect(page.getByRole('navigation', { name: 'Quick actions' })).toHaveCount(0);
    await page.getByRole('button', { name: 'More actions' }).click();
    await page.getByRole('menuitem', { name: 'Pause serving' }).click();
    await expect(page.getByText('Serving is paused.')).toBeVisible();
    await page.getByRole('button', { name: 'Resume serving' }).click();
    await expect(page.getByText('Serving is paused.')).toHaveCount(0);
    await page.getByRole('button', { name: 'More actions' }).click();
    await page.getByRole('menuitem', { name: 'Release memory' }).click();
    await expect(page.getByRole('status').filter({ hasText: /Released|Nothing to release/ })).toBeVisible();
    await page.getByRole('radio', { name: 'Low memory' }).click();
    await expect(page.getByRole('radio', { name: 'Low memory' })).toHaveAttribute('aria-checked', 'true');
    await page.getByRole('link', { name: 'Models' }).first().click();
    await expect(page.getByRole('heading', { name: 'Models', level: 1 })).toBeVisible();
  });

  test('approves a pairing request from the dashboard', async ({ page }) => {
    await openApp(page);
    const card = page.getByRole('region', { name: /Pairing request from/ });
    await expect(card).toBeVisible();
    await card.getByRole('button', { name: /Approve|Allow|Pair/ }).first().click();
    await expect(page.getByRole('region', { name: /Pairing request from/ })).toHaveCount(0);
  });

  test('a failing server shows an inline error with Retry instead of an endless skeleton', async ({ page }) => {
    await openApp(page, '/', 'mock-fast&mock-static&mock-fail=overview');
    const alert = page.getByRole('alert').first();
    await expect(alert).toContainText('not responding');
    await expect(alert.getByRole('button', { name: 'Retry' })).toBeVisible();
    await expect(page.locator('pw-skeleton')).toHaveCount(0);
  });

  test('the first paint does not wait for the backend', async ({ page }) => {
    // 120 ms of simulated latency per call (no mock-fast): the shell and headings are there at once.
    await page.goto('/?mock-static#/');
    await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible({ timeout: 2000 });
    await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible({ timeout: 2000 });
  });
});

test.describe('crashed engines', () => {
  test('a model given up on after a crash loop is shown with Retry, which clears it', async ({ page }) => {
    await openApp(page, '/', 'mock-fast&mock-static&mock-crash');
    const issue = page.getByRole('alert', { name: /Engine problem: gemma-3-12b-it-q4_k_m/ });
    await expect(issue).toBeVisible();
    await expect(issue).toContainText('keeps crashing');
    await issue.getByRole('button', { name: 'Retry' }).click();
    await expect(issue).toHaveCount(0);
  });

  test('shows no engine problem when all is well', async ({ page }) => {
    await openApp(page);
    await expect(page.getByRole('alert', { name: /Engine problem/ })).toHaveCount(0);
  });
});
