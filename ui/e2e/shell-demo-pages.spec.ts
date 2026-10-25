import {
  axeViolations,
  expect,
  expectNoHorizontalScroll,
  expectWithinWidth,
  installTauriFake,
  LAYOUT_WIDTHS,
  test,
  THEMES,
  type Page,
} from '@decryp7/paddock-e2e';

// The pages frame (plan S14) in the shell demo: navigation, lazy pages with their own providers, conditions and badges,
// the status bar's items, the sidebar by the layout plan at every width, F6 regions, axe in both themes. Only the test
// kit (@decryp7/paddock-e2e, plan S16).

/** The macOS desktop app, played in the browser: a Mac platform and a fake Tauri IPC that answers every call. */
const asMacApp = (page: Page) => installTauriFake(page, { platform: 'mac', otherwise: 'null' });

test.describe('pages frame (plan S14)', () => {
  test('the sidebar navigates between routed pages, with the current page marked, the URL and the window title', async ({ app, page, pageErrors }) => {
    await app.frame.open();
    await expect(page).toHaveURL(/#\/home$/);
    await expect(app.frame.heading).toHaveText('Overview');
    await expect(page).toHaveTitle('Overview – Paddock Shell Demo');
    await expect(app.sidebar.link('Overview')).toHaveAttribute('aria-current', 'page');
    await expect(app.sidebar.links).toHaveText([/^Overview$/, /^Runners/, /^Local AI$/, /^Dialogs$/, /^Schedules$/, /^Files$/, /^Keyboard$/, /^Activity$/, /^About$/]);

    await app.sidebar.link('Keyboard').click();
    await expect(app.frame.heading).toHaveText('Keyboard');
    await expect(page).toHaveURL(/#\/keys$/);
    await expect(page).toHaveTitle('Keyboard – Paddock Shell Demo');
    await expect(app.sidebar.link('Keyboard')).toHaveAttribute('aria-current', 'page');
    // Focus moves to the page, so a screen reader starts there.
    await expect(app.frame.main).toBeFocused();
    await app.sidebar.link('About').click();
    await expect(app.frame.heading).toHaveText('About the shell');
    await page.goBack();
    await expect(app.frame.heading).toHaveText('Keyboard');
    // An unknown address goes to the first page.
    await page.goto('/#/nowhere');
    await expect(app.frame.heading).toHaveText('Overview');
    expect(pageErrors).toEqual([]);
  });

  test('a page is a lazy chunk with providers of its own, new on each visit', async ({ app, page }) => {
    await app.frame.open();
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
    await page.waitForLoadState('networkidle');
    const scripts: string[] = [];
    page.on('request', (r) => r.resourceType() === 'script' && scripts.push(r.url()));
    await app.sidebar.link(/^Runners/).click();
    const host = page.locator('sd-runners-page');
    await expect(host).toHaveAttribute('data-visit', /^\d+$/);
    const first = Number(await host.getAttribute('data-visit'));
    await expect(page.getByRole('grid', { name: 'Runners' }).getByRole('row')).toHaveCount(5);
    // The page's component and its providers were fetched only now.
    expect(scripts.length).toBeGreaterThan(0);
    await expect(page.getByRole('heading', { name: 'No jobs' })).toBeVisible();
    await app.sidebar.link('Overview').click();
    await app.sidebar.link(/^Runners/).click();
    await expect(host).toHaveAttribute('data-visit', String(first + 1));
  });

  test('a page with a condition comes and goes with it; badges follow the context', async ({ app, page }) => {
    await app.frame.open();
    await expect(page.getByTestId('app-context')).toHaveText('App context: loaded');
    const lab = app.sidebar.link('Lab');
    await expect(lab).toHaveCount(0);
    // The Runners badge: the offline runner needs the user.
    await expect(app.sidebar.badges).toHaveText(['1']);
    // Not offered: its address goes to the first page.
    await page.goto('/#/lab');
    await expect(app.frame.heading).toHaveText('Overview');

    await page.getByRole('button', { name: 'Say hello' }).click();
    await page.getByRole('button', { name: 'Say hello' }).click();
    await lab.click();
    await expect(app.frame.heading).toHaveText('Lab');
    // The condition turns false while it is shown: the first page takes over.
    await page.keyboard.press('ControlOrMeta+Shift+H');
    await (await app.menus.item('Help', 'Reset greetings')).click();
    await expect(app.frame.heading).toHaveText('Overview');
    await expect(lab).toHaveCount(0);
  });

  test('the status bar hosts the shell’s items and the app’s, by side and priority', async ({ app, page }) => {
    await app.frame.open();
    const bar = app.statusBar.bar;
    await expect(app.statusBar.items('start')).toHaveCount(1);
    // The table's words, exceptions only; the badge counts the same exception and says so (review R3-1).
    await expect(bar.getByTestId('runners-status')).toHaveText('1 offline · 1 updating');
    await expect(bar.getByTestId('runners-status')).toHaveAttribute('aria-label', '2 of 4 runners online, 1 updating, 1 offline');
    await expect(app.sidebar.badges).toHaveAttribute('title', '1 runner offline');
    await expect(app.statusBar.items('end')).toHaveCount(3);
    expect(await app.statusBar.ids('end')).toEqual(['pd.health', 'pd.jobs', 'pd.notifications']);
    await expect(bar.getByRole('button', { name: 'Jobs' })).toBeVisible();
    await expect(app.statusBar.bell).toBeVisible();
    await page.getByRole('button', { name: 'Break the demo engine' }).click();
    await expect(app.statusBar.chip).toHaveText('Demo engine: stopped');
  });

  test('narrowing the window never drops the running job from the status bar, whatever has focus (review N1)', async ({ app, page }) => {
    await app.frame.open();
    await page.getByRole('button', { name: 'Break the search index' }).click();
    await page.getByRole('button', { name: 'Start quiet job' }).click();
    await expect(app.statusBar.item('pd.jobs')).toBeVisible();
    // Watch every frame from before the resize until well after it.
    await page.evaluate(() => {
      const w = window as unknown as { __dropped: number; __watch: boolean };
      w.__dropped = 0;
      w.__watch = true;
      const tick = () => {
        if (document.querySelector('[data-status-item="pd.jobs"]')?.classList.contains('hidden')) w.__dropped++;
        if (w.__watch) requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
    });
    await page.setViewportSize({ width: 320, height: 640 });
    await expect.poll(() => app.frame.sidebarMode()).toBe('overlay');
    for (let i = 0; i < 4; i++) {
      await page.keyboard.press('F6');
      await expect(app.statusBar.item('pd.jobs')).not.toHaveClass(/hidden/);
    }
    const dropped = await page.evaluate(() => {
      const w = window as unknown as { __dropped: number; __watch: boolean };
      w.__watch = false;
      return w.__dropped;
    });
    expect(dropped).toBe(0);
  });

  test('status items sit at equal distances, the chip included; the chip looks pressed while open (review R3-2)', async ({ app, page }) => {
    await app.frame.open();
    await page.getByRole('button', { name: 'Break the search index' }).click();
    await page.getByRole('button', { name: 'Start quiet job' }).click();
    await expect(app.statusBar.chip).toBeVisible();
    // From the chip's visible edge to the jobs item's dot, and from the jobs text to the bell's icon.
    const gaps = await page.evaluate(() => {
      const r = (sel: string) => document.querySelector(sel)!.getBoundingClientRect();
      const chip = r('[data-testid="service-health-chip"]');
      const dot = r('[data-status-item="pd.jobs"] .pd-dot');
      const text = r('[data-status-item="pd.jobs"] .pd-text');
      const bell = r('[data-testid="notifications-status"] svg');
      return [dot.left - chip.right, bell.left - text.right];
    });
    expect(Math.abs((gaps[0] as number) - (gaps[1] as number))).toBeLessThanOrEqual(3);
    const chip = app.statusBar.chip;
    const before = await chip.evaluate((e) => getComputedStyle(e).boxShadow);
    await chip.click();
    await expect(chip).toHaveAttribute('aria-expanded', 'true');
    expect(await chip.evaluate((e) => getComputedStyle(e).boxShadow)).not.toBe(before);
  });

  test('a narrow status bar shrinks items first, keeps the bell and the chip, then hides the lowest and counts them', async ({ app, page }) => {
    await page.setViewportSize({ width: 320, height: 640 });
    await app.frame.open();
    await page.getByRole('button', { name: 'Break the demo engine' }).click();
    await page.getByRole('button', { name: 'Start quiet job' }).click();
    await page.getByRole('button', { name: 'Notify three times' }).click();
    const bar = app.statusBar.bar;
    // Compact forms: the chip says the state, the jobs item the count; the full text is their name and tooltip.
    const chip = app.statusBar.chip;
    await expect(chip).toHaveText('Engine stopped');
    await expect(chip).toHaveAttribute('aria-label', 'Demo engine: stopped');
    await expect(bar.getByRole('button', { name: '1 running: Update runner' })).toHaveText('1 job');
    // The bell and its count are pinned. The commands run asynchronously (the app context loads first), so the count is
    // asserted once settled: two unread, the engine's failure and the three identical notices merged into one message.
    await expect(app.statusBar.bell).toBeVisible();
    await expect(app.statusBar.bell).toHaveAttribute('aria-label', '2 unread');
    await expect(app.statusBar.bell).toContainText('2');
    await expect(bar.getByTestId('runners-status')).toBeHidden();
    const more = app.statusBar.more;
    await expect(more).toHaveAttribute('aria-label', 'More status (1)');
    await expect(more).toHaveText('1');
    const box = await more.boundingBox();
    expect(box?.height).toBeGreaterThanOrEqual(24);
    await more.click();
    await expect(more).toHaveAttribute('aria-expanded', 'true');
    await expect(bar.getByTestId('runners-status')).toBeVisible();
    await expect(chip).toHaveText('Demo engine: stopped');
    await expectNoHorizontalScroll(page);
    await more.click();
    await expect(bar.getByTestId('runners-status')).toBeHidden();
    // Wide again: the full forms come back.
    await page.setViewportSize({ width: 1280, height: 800 });
    await expect(chip).toHaveText('Demo engine: stopped');
    await expect(bar.getByTestId('runners-status')).toBeVisible();
  });

  test('the sidebar is expanded, a rail or over the page by the layout plan; the user’s collapse is kept', async ({ app, page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    await app.frame.open();
    await expect.poll(() => app.frame.sidebarMode()).toBe('expanded');
    await expect(app.frame.toggle).toHaveAttribute('aria-label', 'Collapse sidebar');
    await app.frame.toggle.click();
    await expect.poll(() => app.frame.sidebarMode()).toBe('rail');
    await expect(app.sidebar.link('Keyboard')).toHaveAttribute('title', 'Keyboard');
    await page.reload();
    await expect(app.frame.heading).toBeVisible();
    await expect.poll(() => app.frame.sidebarMode()).toBe('rail');
    await app.frame.toggle.click();
    await expect.poll(() => app.frame.sidebarMode()).toBe('expanded');

    await page.setViewportSize({ width: 640, height: 800 });
    await expect.poll(() => app.frame.sidebarMode()).toBe('rail');
    await expect(app.frame.toggle).toHaveAttribute('aria-label', 'Show pages');
    await app.frame.toggle.click();
    await expect(app.frame.scrim).toBeVisible();
    await expect(app.sidebar.link('Overview')).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(app.frame.scrim).toHaveCount(0);
    await expect(app.frame.toggle).toBeFocused();

    await page.setViewportSize({ width: 320, height: 640 });
    await expect.poll(() => app.frame.sidebarMode()).toBe('overlay');
    await expect(app.sidebar.nav).toBeHidden();
    await app.frame.toggle.click();
    await expect(app.sidebar.nav).toBeVisible();
    await app.frame.scrim.click({ position: { x: 300, y: 300 } });
    await expect(app.sidebar.nav).toBeHidden();
    await app.frame.toggle.click();
    await app.sidebar.link('Dialogs').click();
    await expect(app.frame.heading).toHaveText('Dialogs');
    await expect(app.sidebar.nav).toBeHidden();
    // Wide again: the drawer is gone and the sidebar is beside the page.
    await page.setViewportSize({ width: 1280, height: 800 });
    await expect.poll(() => app.frame.sidebarMode()).toBe('expanded');
    await expect(app.sidebar.nav).toBeVisible();
  });

  for (const width of LAYOUT_WIDTHS) {
    test(`no horizontal scroll at ${width} px on any page (rule 18)`, async ({ app, page }) => {
      await page.setViewportSize({ width, height: 700 });
      for (const hash of ['#/home', '#/dialogs', '#/schedule', '#/keys', '#/runners', '#/about']) {
        await app.frame.open(hash);
        await expectNoHorizontalScroll(page, `${hash} at ${width}`);
        await expectWithinWidth(page, '#pd-main', width);
      }
    });
  }

  test('F6 and Shift+F6 cycle the title bar, the sidebar, the page and the status bar', async ({ app, page }) => {
    await app.frame.open();
    const region = () => app.frame.focusedRegion();
    await page.keyboard.press('F6');
    expect(await region()).toBe('title');
    await page.keyboard.press('F6');
    expect(await region()).toBe('sidebar');
    await expect(app.sidebar.link('Overview')).toBeFocused();
    await page.keyboard.press('F6');
    expect(await region()).toBe('main');
    await page.keyboard.press('F6');
    expect(await region()).toBe('status');
    await page.keyboard.press('F6');
    expect(await region()).toBe('title');
    await page.keyboard.press('Shift+F6');
    expect(await region()).toBe('status');
    // The arrow keys move between the pages.
    await app.sidebar.link('Overview').focus();
    await page.keyboard.press('ArrowDown');
    await expect(app.sidebar.link(/^Runners/)).toBeFocused();
    await page.keyboard.press('End');
    await expect(app.sidebar.link('About')).toBeFocused();
  });

  test('the macOS app has its menus in the native menu bar only, and keeps clear of the window buttons', async ({ app, page }) => {
    await asMacApp(page);
    await app.frame.open();
    await expect(app.menus.bar).toHaveCount(0);
    const inset = await app.frame.titleBar.evaluate((el) => getComputedStyle(el).paddingLeft);
    expect(parseFloat(inset)).toBeGreaterThanOrEqual(78);
  });

  test('the browser build has the in-app menu bar', async ({ app }) => {
    await app.frame.open();
    await expect(app.menus.bar).toBeVisible();
  });

  test('the toggle keeps its glyph and the app name stays (truncated, with a tooltip) when narrow', async ({ app, page }) => {
    await page.setViewportSize({ width: 320, height: 640 });
    await app.frame.open();
    await expect(app.frame.appName).toBeVisible();
    await expect(app.frame.appName).toHaveAttribute('title', 'Paddock Shell Demo');
    const glyph = () => app.frame.toggle.locator('pd-icon').evaluate((el) => el.innerHTML);
    const narrow = await glyph();
    await page.setViewportSize({ width: 1280, height: 800 });
    await expect.poll(() => app.frame.sidebarMode()).toBe('expanded');
    expect(await glyph()).toBe(narrow);
  });

  test('over the page the sidebar is modal: the scrim covers the status bar, which is inert', async ({ app, page }) => {
    await page.setViewportSize({ width: 320, height: 640 });
    await app.frame.open();
    await app.frame.toggle.click();
    const scrim = await app.frame.scrim.boundingBox();
    expect(scrim && scrim.y + scrim.height).toBe(640);
    await expect(app.statusBar.bar).toHaveAttribute('inert', '');
    await expect(app.frame.main).toHaveAttribute('inert', '');
    await page.keyboard.press('Escape');
    await expect(app.statusBar.bar).not.toHaveAttribute('inert', '');
  });

  test('a skip link leads to the page', async ({ app, page }) => {
    await app.frame.open();
    await page.keyboard.press('Tab');
    const skip = app.frame.skipLink;
    await expect(skip).toBeFocused();
    await skip.press('Enter');
    await expect(app.frame.main).toBeFocused();
  });

  for (const colorScheme of THEMES) {
    test(`axe finds nothing: expanded, rail and over the page (${colorScheme})`, async ({ app, page }) => {
      // Without motion, so the drawer is checked at rest and not mid fade-in.
      await page.emulateMedia({ colorScheme, reducedMotion: 'reduce' });
      await app.frame.open();
      await page.getByRole('button', { name: 'Say hello' }).click();
      await page.getByRole('button', { name: 'Start quiet job' }).click();
      expect(await axeViolations(page)).toEqual([]);
      await page.setViewportSize({ width: 640, height: 700 });
      await expect.poll(() => app.frame.sidebarMode()).toBe('rail');
      expect(await axeViolations(page)).toEqual([]);
      await page.setViewportSize({ width: 320, height: 640 });
      await expect.poll(() => app.frame.sidebarMode()).toBe('overlay');
      await app.frame.toggle.click();
      await expect(app.frame.scrim).toBeVisible();
      expect(await axeViolations(page)).toEqual([]);
    });
  }
});
