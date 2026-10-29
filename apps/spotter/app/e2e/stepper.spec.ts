import { axeInThemes, expect, expectNoHorizontalScroll, pressTab, resize, settleTransitions, test, type Locator, type Page } from '@decryp7/paddock-e2e';

/** The add/edit watch flow is one linear Material stepper: Source, Configure, Versions, Notify, Review. */

const STEP_HEADINGS = ['Choose a source', 'GitHub', 'Versions and filters', 'Schedule', 'Review'] as const;

async function openGithub(page: Page, repo = 'owner/tool'): Promise<void> {
  await page.goto('/#/watches/new');
  await page.getByRole('button', { name: 'Source kinds', exact: true }).click();
  await page.getByRole('button', { name: /GitHub/ }).click();
  await page.getByLabel('Repository').fill(repo);
}

const next = (page: Page) => page.getByRole('button', { name: 'Next' });

/** Tabs (Option+Tab on WebKit) until the target has focus; fails instead of looping. */
async function tabTo(page: Page, target: Locator, max = 60): Promise<void> {
  for (let i = 0; i < max; i++) {
    if (await target.evaluate((e) => e === document.activeElement)) return;
    await pressTab(page);
  }
  throw new Error('the keyboard never reached the target');
}

test.describe('the watch stepper', () => {
  test('completes the flow, goes back and saves', async ({ page }) => {
    await openGithub(page);
    await expect(page.getByRole('tab', { name: /Configure/ })).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByRole('table', { name: 'Releases found' })).toBeVisible();
    await next(page).click();
    await expect(page.getByRole('heading', { name: STEP_HEADINGS[2] })).toBeFocused();
    await next(page).click();
    await expect(page.getByRole('heading', { name: STEP_HEADINGS[3], exact: true })).toBeFocused();
    await page.getByRole('button', { name: 'Back' }).click();
    await expect(page.getByRole('heading', { name: STEP_HEADINGS[2] })).toBeFocused();
    await next(page).click();
    await next(page).click();
    await expect(page.getByRole('heading', { name: 'Review', level: 2 })).toBeFocused();
    await expect(page.locator('#name')).toHaveValue('tool');
    await page.getByRole('button', { name: 'Add watch' }).click();
    await expect(page.getByRole('heading', { name: 'tool', level: 1 })).toBeVisible();
  });

  test('an unreachable source shows its error on the step and a retry recovers', async ({ page }) => {
    await openGithub(page, 'offline/thing');
    const alert = page.locator('sp-preview-panel [role="alert"]');
    await expect(alert).toContainText('did not answer');
    // The step stays open; the later headers stay usable only once the field is complete, the test is advice.
    await page.getByLabel('Repository').fill('owner/tool');
    await page.getByRole('button', { name: 'Test again' }).click();
    await expect(page.getByRole('table', { name: 'Releases found' })).toBeVisible();
    await expect(alert).toHaveCount(0);
  });

  test('a missing required field keeps the later steps shut', async ({ page }) => {
    await openGithub(page, '');
    await expect(next(page)).toBeDisabled();
    // Linear: the Versions header is disabled (aria-disabled) until the Configure step is complete.
    await expect(page.getByRole('tab', { name: /Versions/ })).toHaveAttribute('aria-disabled', 'true');
    await expect(page.getByRole('tab', { name: /Configure/ })).toHaveAttribute('aria-selected', 'true');
    await page.getByLabel('Repository').fill('owner/tool');
    await expect(next(page)).toBeEnabled();
    // Material marks a step that is neither done nor open aria-disabled even when linear mode lets it open (the one after a
    // complete step), so the click is forced; what is checked is that it opens now.
    await page.getByRole('tab', { name: /Versions/ }).click({ force: true });
    await expect(page.getByRole('tab', { name: /Versions/ })).toHaveAttribute('aria-selected', 'true');
  });

  test('is operated by the keyboard alone', async ({ page }) => {
    await page.goto('/#/watches/new');
    // Pick the Node.js preset with Tab and Enter.
    const preset = page.getByRole('button', { name: /Node\.js/ });
    await tabTo(page, preset);
    await page.keyboard.press('Enter');
    await expect(page.locator('#cfg-h')).toBeFocused();
    await expect(page.getByRole('table', { name: 'Releases found' })).toBeVisible();
    // Next with the keyboard, then back to the Configure header with the arrow keys and open it with Enter.
    await tabTo(page, next(page));
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { name: STEP_HEADINGS[2] })).toBeFocused();
    const versions = page.getByRole('tab', { name: /Versions/ });
    await tabTo(page, versions);
    await page.keyboard.press('ArrowLeft');
    await expect(page.getByRole('tab', { name: /Configure/ })).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page.getByRole('tab', { name: /Configure/ })).toHaveAttribute('aria-selected', 'true');
    await expect(page.locator('#cfg-h')).toBeFocused();
  });

  test('editing a watch starts at Configure and the Source step stays shut', async ({ page }) => {
    await page.goto('/#/watches/w-2/edit');
    await expect(page.getByRole('heading', { name: 'GitHub' })).toBeVisible();
    await page.getByRole('tab', { name: /Source/ }).click();
    await expect(page.getByRole('tab', { name: /Configure/ })).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByRole('button', { name: 'Back' })).toHaveCount(0);
  });

  test('honours reduced motion', async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await openGithub(page);
    await next(page).click();
    const durations = await page.locator('.mat-horizontal-stepper-content').evaluateAll((els) => els.map((e) => getComputedStyle(e).transitionDuration + getComputedStyle(e).animationDuration));
    expect(durations.length).toBeGreaterThan(0);
    // Material writes a 0 ms animation as 0.00001s.
    for (const d of durations) for (const part of d.split(',')) expect(parseFloat(part)).toBeLessThan(0.001);
  });

  test('has no axe violations on any step, in light and dark', async ({ page }) => {
    await openGithub(page);
    expect(await axeInThemes(page)).toEqual([]);
    for (const heading of [STEP_HEADINGS[2], STEP_HEADINGS[3], STEP_HEADINGS[4]]) {
      await next(page).click();
      await expect(page.getByRole('heading', { name: heading, exact: true })).toBeVisible();
      expect(await axeInThemes(page)).toEqual([]);
    }
  });

  for (const width of [900, 1024, 1280]) {
    test(`fits at ${width} px on every step`, async ({ page }) => {
      await resize(page, width, 800);
      await openGithub(page);
      for (let i = 1; i <= 4; i++) {
        await expectNoHorizontalScroll(page, `${width} px, step ${i}`);
        await settleTransitions(page);
        // No clipped text: nothing in the open step sticks out of the stepper's body, no label is ellipsised or cut, and the headers fit.
        const clipped = await page.evaluate(() => {
          const out: string[] = [];
          const body = document.querySelector<HTMLElement>('.mat-horizontal-content-container')!.getBoundingClientRect();
          const open = document.querySelector<HTMLElement>('.mat-horizontal-stepper-content-current')!;
          const skip = '.cdk-visually-hidden, .pd-visually-hidden, mat-icon';
          for (const e of [...Array.from(open.querySelectorAll<HTMLElement>('*')), ...Array.from(document.querySelectorAll<HTMLElement>('.mat-step-header *'))]) {
            const s = getComputedStyle(e);
            if (s.display === 'none' || e.clientWidth === 0 || e.closest(skip)) continue;
            if (e.getBoundingClientRect().right > body.right + 1) out.push(`${e.tagName}.${e.className} sticks out`);
            if ((s.overflowX === 'hidden' || s.textOverflow === 'ellipsis') && e.scrollWidth > e.clientWidth + 1 && e.textContent?.trim()) out.push(`${e.tagName}.${e.className} clips`);
          }
          return out;
        });
        expect(clipped, `clipped text at ${width} px, step ${i}`).toEqual([]);
        const head = await page.locator('.mat-horizontal-stepper-header-container').boundingBox();
        expect(head && head.x + head.width).toBeLessThanOrEqual(width);
        if (i < 4) {
          await next(page).click();
          await expect(page.locator('.mat-step-header').nth(i + 1)).toHaveAttribute('aria-selected', 'true');
        }
      }
    });
  }

  // Screenshots for the independent review (ER-23): STEPPER_SHOTS=<dir> npx playwright test stepper --project=chromium
  const dir = process.env['STEPPER_SHOTS'];
  for (const scheme of ['light', 'dark'] as const) {
    test(`screenshots of every step (${scheme})`, async ({ page }) => {
      test.skip(!dir, 'only when STEPPER_SHOTS names a folder');
      await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
      await page.goto('/#/watches/new');
      await expect(page.getByRole('heading', { name: STEP_HEADINGS[0] })).toBeVisible();
      await page.screenshot({ path: `${dir}/1-source-${scheme}.png` });
      await page.getByRole('button', { name: 'Source kinds', exact: true }).click();
      await page.getByRole('button', { name: /GitHub/ }).click();
      await page.screenshot({ path: `${dir}/2a-configure-missing-${scheme}.png` });
      await page.getByLabel('Repository').fill('offline/thing');
      await expect(page.locator('sp-preview-panel [role="alert"]')).toBeVisible();
      await page.screenshot({ path: `${dir}/2b-configure-error-${scheme}.png` });
      await page.getByLabel('Repository').fill('owner/tool');
      await page.getByRole('button', { name: 'Test again' }).click();
      await expect(page.getByRole('table', { name: 'Releases found' })).toBeVisible();
      await page.screenshot({ path: `${dir}/2c-configure-${scheme}.png` });
      await next(page).click();
      await expect(page.getByRole('heading', { name: STEP_HEADINGS[2] })).toBeVisible();
      await page.screenshot({ path: `${dir}/3-versions-${scheme}.png` });
      await next(page).click();
      await expect(page.getByRole('heading', { name: STEP_HEADINGS[3], exact: true })).toBeVisible();
      await page.screenshot({ path: `${dir}/4-notify-${scheme}.png`, fullPage: true });
      await next(page).click();
      await expect(page.getByRole('heading', { name: 'Review', level: 2 })).toBeVisible();
      await page.screenshot({ path: `${dir}/5-review-${scheme}.png` });
    });
  }
});
