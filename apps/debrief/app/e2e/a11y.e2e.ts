import { expect, test } from './test';
import { gridRows, openApp, openView } from './app';
import { expectNoSeriousViolations } from './axe';
import { onEveryAxeScan } from '../../../../ui/projects/paddock-e2e/src/axe.ts';

for (const scheme of ['dark', 'light'] as const) {
  test(`main screen has no serious accessibility violations (${scheme})`, async ({ page }, testInfo) => {
    await page.emulateMedia({ colorScheme: scheme });
    await openApp(page);
    await gridRows(page).first().click();
    // The click opens the inspector, which slides in: axe scans it once it is there and settled.
    await expect(page.locator('debrief-inspector')).toBeVisible();
    await expectNoSeriousViolations(page, testInfo, `timeline-${scheme}`);
  });
}

test('metrics and profile tabs have no serious accessibility violations', async ({ page }, testInfo) => {
  await openApp(page);
  await openView(page, 'metrics');
  await expect(page.locator('debrief-chart-card').first()).toBeVisible();
  await expectNoSeriousViolations(page, testInfo, 'metrics');
  await page.getByRole('button', { name: 'Profile editor' }).click();
  await expect(page.locator('debrief-profile-editor')).toBeVisible();
  await expectNoSeriousViolations(page, testInfo, 'profile');
});

// debrief.yml runs 8297, 8300, 8373, 8426, 8547 and 8556 (WebKit, 1 in about 145): axe read the Inspector's text at 3.6 to 4.41:1, its
// colours the dark tokens' at 77 to 85% opacity, i.e. the flyout's 140 ms slide-in (opacity 0.4 to 1) running while axe read it. The
// slide is restarted here from inside axe's scan, 100 times slower (a loaded runner), so the order is certain: a helper that only waits
// for motion to end beforehand reads it half-way; one that holds the page still for the scan reads the end state.
test('the Inspector sliding in as the scan starts is read at its end state (dark)', async ({ page }, testInfo) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await openApp(page);
  await gridRows(page).first().click();
  const flyout = page.locator('#debrief-ah-flyout');
  await expect(flyout.locator('debrief-inspector')).toBeVisible();
  await onEveryAxeScan(page, () => {
    const f = document.getElementById('debrief-ah-flyout') as HTMLElement;
    // A CSS animation that has ended is gone from getAnimations(): take the class's animation off and put it back to start it again.
    f.style.animation = 'none';
    void f.offsetWidth;
    f.style.removeProperty('animation');
    for (const a of f.getAnimations()) a.playbackRate = 0.01;
  });
  await expectNoSeriousViolations(page, testInfo, 'inspector-sliding-in-dark');
  expect(await page.evaluate(() => (window as unknown as { axeScans: number }).axeScans), 'the slide was restarted inside the scan').toBe(1);
});
