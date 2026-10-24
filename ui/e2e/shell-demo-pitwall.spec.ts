import { axeViolations, expect, expectNoHorizontalScroll, test } from '@decryp7/paddock-e2e';

// The shared "needs local AI" state (paddock-pitwall, ADR 0004) in the shell demo: <pd-needs-ai> over the demo's in-memory
// engine, which plays the app's Rust `Link` (the same snapshots, pairing steps and repair). Nothing talks to a real Pitwall.
// Chromium only in branches (rule 3). Only the test kit (@decryp7/paddock-e2e).

test('a feature without Pitwall says why and offers the way out, including the repair that does not need Pitwall', async ({ app, page, pageErrors }) => {
  await page.goto('/#/pitwall');
  await expect(app.frame.heading).toHaveText('Local AI');
  const region = page.getByTestId('needs-ai');
  await expect(region).toHaveAttribute('data-kind', 'notInstalled');
  await expect(region).toContainText('Ask about this log needs local AI');
  await expect(page.getByTestId('needs-ai-message')).toContainText('not installed on this computer');
  expect(await axeViolations(page, { idsOnly: true })).toEqual([]);

  // One primary action; the steps only the app can run come out as events.
  await expect(region.locator('.pd-btn--primary')).toHaveCount(1);
  await page.getByTestId('needs-ai-installPitwall').click();
  await expect(page.getByTestId('pitwall-last')).toHaveText('step: installPitwall');
  await page.getByTestId('needs-ai-settings').click();
  await expect(page.getByTestId('pitwall-last')).toHaveText('settings opened');

  // The repair works with Pitwall gone (rule 19): the session moves to the built-in engine and says so.
  await page.getByTestId('needs-ai-builtin').click();
  await expect(page.getByTestId('needs-ai-builtin')).toHaveCount(0);
  await expect(page.getByTestId('needs-ai-on-builtin')).toHaveText('This session uses the built-in engine.');
  await expect(page.getByTestId('pitwall-answer')).toContainText('built-in engine');
  expect(pageErrors).toEqual([]);
});

test('connecting shows the code to compare, can be cancelled, and the state clears once Pitwall approves', async ({ app, page }) => {
  await page.goto('/#/pitwall');
  await expect(app.frame.heading).toHaveText('Local AI');
  await page.getByTestId('pitwall-state-needsPairing').click();
  await expect(page.getByTestId('needs-ai-message')).toContainText('is not connected to Pitwall yet');
  await expect(page.getByTestId('needs-ai-code')).toHaveCount(0);

  await page.getByTestId('needs-ai-pair').click();
  await expect(page.getByTestId('needs-ai-code')).toHaveText('482 913');
  await expect(page.getByTestId('needs-ai-code')).toHaveAttribute('aria-label', 'Pairing code');
  await expect(page.getByTestId('needs-ai-approve')).toContainText('Approve Shell Demo in Pitwall');
  expect(await axeViolations(page, { idsOnly: true })).toEqual([]);

  await page.getByTestId('needs-ai-cancel').click();
  await expect(page.getByTestId('needs-ai-code')).toHaveCount(0);
  await page.getByTestId('needs-ai-pair').click();
  await expect(page.getByTestId('needs-ai-code')).toBeVisible();

  await page.getByTestId('pitwall-approve').click();
  await expect(page.getByTestId('needs-ai')).toHaveCount(0);
  await expect(page.getByTestId('pitwall-answer')).toContainText('Pitwall answers this feature with qwen3-8b-q4km');
});

test('an app without an engine of its own offers no built-in repair, and nothing scrolls sideways at 320 px', async ({ app, page }) => {
  await page.goto('/#/pitwall');
  await expect(app.frame.heading).toHaveText('Local AI');
  await page.getByTestId('pitwall-no-engine').click();
  await page.getByTestId('pitwall-state-noModel').click();
  await expect(page.getByTestId('needs-ai-message')).toContainText('no model that suits Shell Demo');
  await expect(page.getByTestId('needs-ai-builtin')).toHaveCount(0);
  await expect(page.getByTestId('needs-ai-installModel')).toBeVisible();

  await page.getByTestId('pitwall-state-incompatible').click();
  await expect(page.getByTestId('needs-ai')).toHaveAttribute('data-kind', 'incompatible');
  await expect(page.getByTestId('needs-ai-updateApp')).toBeVisible();

  await page.setViewportSize({ width: 320, height: 800 });
  await expectNoHorizontalScroll(page);
  await page.getByTestId('pitwall-state-needsPairing').click();
  await page.getByTestId('needs-ai-pair').click();
  await expect(page.getByTestId('needs-ai-code')).toBeVisible();
  await expectNoHorizontalScroll(page);
  expect(await axeViolations(page, { idsOnly: true })).toEqual([]);
});
