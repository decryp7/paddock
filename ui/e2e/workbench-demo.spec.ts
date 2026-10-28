import { axeViolations, expect, expectNoHorizontalScroll, test, THEMES, WorkbenchApp } from '@decryp7/paddock-e2e';

// The workbench demo is the reference consumer of @decryp7/paddock-ui/workbench (ADR 0013 §4; plan W4, W7). It uses only
// the test kit (@decryp7/paddock-e2e): the workbench page objects and the shell's.

for (const colorScheme of THEMES) {
  test.describe(`workbench demo (${colorScheme})`, () => {
    test.use({ colorScheme });

    test('boots with the frame, the Explorer and the welcome view; axe finds no violations', async ({ page, pageErrors }) => {
      const wb = new WorkbenchApp(page);
      await wb.frame.open();
      await expect(wb.explorer.node('wd.files', 'src')).toBeVisible();
      await expect(page.getByRole('heading', { level: 1, name: 'Workbench demo' })).toBeVisible();
      expect(await axeViolations(page)).toEqual([]);
      expect(pageErrors).toEqual([]);
    });
  });
}

test('no horizontal scroll at 320 px (rule 18), and the Explorer yields to the stage by the layout plan', async ({ page }) => {
  const wb = new WorkbenchApp(page);
  await page.setViewportSize({ width: 1280, height: 800 });
  await wb.frame.open();
  await expect.poll(() => wb.frame.explorer()).toBe('wd.files');
  await page.setViewportSize({ width: 600, height: 800 });
  await expect.poll(() => wb.frame.explorer()).toBe('closed');
  await page.setViewportSize({ width: 320, height: 640 });
  await expectNoHorizontalScroll(page);
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect.poll(() => wb.frame.explorer()).toBe('wd.files');
});

test('F6 cycles the regions and the skip link goes to the work area', async ({ page }) => {
  const wb = new WorkbenchApp(page);
  await wb.frame.open();
  const seen = new Set<string | null>();
  for (let i = 0; i < 5; i++) {
    await page.keyboard.press('F6');
    seen.add(await wb.frame.focusedRegion());
  }
  expect([...seen].sort()).toEqual(['activity', 'main', 'sidebar', 'status', 'title']);
});

test.describe('activity bar and Explorer (plan W4)', () => {
  test('the Explorer item toggles the side bar; badges are part of the name', async ({ page }) => {
    const wb = new WorkbenchApp(page);
    await wb.frame.open();
    await expect(wb.activity.item('wd.act.problems')).toHaveAccessibleName('Problems, 3 problems');
    await wb.activity.item('wd.act.explorer').click();
    await expect.poll(() => wb.frame.explorer()).toBe('closed');
    await wb.activity.item('wd.act.explorer').click();
    await expect.poll(() => wb.frame.explorer()).toBe('wd.files');
  });

  test('the tree pages large folders, opens a context menu by keyboard, and middle-truncates long names', async ({ page }) => {
    const wb = new WorkbenchApp(page);
    await wb.frame.open();
    await wb.explorer.expand('wd.files', 'generated');
    await wb.explorer.expand('wd.files', 'generated/fixtures');
    await expect(wb.explorer.more).toContainText('40 left');
    await wb.explorer.more.click();
    await expect(wb.explorer.node('wd.files', 'generated/fixtures/case-240.json')).toBeVisible();
    await wb.explorer.node('wd.files', 'README.md').click();
    await page.keyboard.press('Shift+F10');
    await expect(wb.explorer.menu.getByRole('menuitem', { name: 'Copy path' })).toBeVisible();
    await page.keyboard.press('Escape');
    await wb.explorer.expand('wd.files', 'docs');
    const long = wb.explorer.node('wd.files', 'docs/architecture-overview-of-the-request-pipeline.md');
    await expect(long.getByTestId('path-text')).toHaveAttribute('data-full', /architecture-overview-of-the-request-pipeline\.md$/);
  });
});

test.describe('command palette (plan W5)', () => {
  test('lists commands, runs one, suggests on a typo, and searches providers by prefix', async ({ page }) => {
    const wb = new WorkbenchApp(page);
    await wb.frame.open();
    await wb.palette.open();
    await expect(wb.palette.options.first()).toContainText('Open README');
    await wb.palette.search('>indx repositry');
    await expect(page.getByTestId('palette-did-you-mean')).toBeVisible();
    await wb.palette.run('Index the repository');
    await wb.palette.open(false);
    await wb.palette.search('request handler');
    await expect(wb.palette.options.first()).toContainText('request_handler.rs');
    await wb.palette.search('@handle');
    await expect(wb.palette.options.first()).toContainText('handle_request');
    await wb.palette.search('#listening');
    await expect(wb.palette.options.first()).toContainText('listening on');
    await page.keyboard.press('Escape');
    await expect(wb.palette.dialog).toHaveCount(0);
    expect(await axeViolations(page)).toEqual([]);
  });
});

test.describe('dock (plan W2a, W2b)', () => {
  const tab = (page: import('@decryp7/paddock-e2e').Page, panel: string) => page.locator(`[data-testid="pd-dock-tab"][data-panel-id="${panel}"]`);

  test('opens a file from the Explorer as an instance of the Code view, and keeps the layout over a reload', async ({ page }) => {
    const wb = new WorkbenchApp(page);
    await wb.frame.open();
    await wb.explorer.expand('wd.files', 'src');
    await wb.explorer.node('wd.files', 'src/main.rs').dblclick();
    await expect(tab(page, 'wd.code#src/main.rs')).toBeVisible();
    await expect(page.getByTestId('code-lines')).toContainText('Server::bind');
    await wb.explorer.node('wd.files', 'README.md').dblclick();
    await expect(tab(page, 'wd.code#README.md')).toBeVisible();
    // The layout is saved a moment after a change (debounced); wait for it, then reload.
    await expect.poll(() => page.evaluate(() => Object.keys(localStorage).filter((k) => (localStorage.getItem(k) ?? '').includes('wd.code#README.md')).length)).toBeGreaterThan(0);
    await page.reload();
    await expect(tab(page, 'wd.code#src/main.rs')).toBeVisible();
    await expect(tab(page, 'wd.code#README.md')).toBeVisible();
  });

  test('switches perspectives with the keyboard and keeps each arrangement', async ({ page }) => {
    const wb = new WorkbenchApp(page);
    await wb.frame.open();
    const dock = page.getByTestId('pd-dock');
    await tab(page, 'wd.orientation').click();
    await page.keyboard.press('ControlOrMeta+2');
    await expect(dock).toHaveAttribute('data-perspective', 'wd.review');
    await expect(tab(page, 'wd.problems')).toBeVisible();
    await page.keyboard.press('ControlOrMeta+1');
    await expect(dock).toHaveAttribute('data-perspective', 'wd.understand');
    await expect(tab(page, 'wd.orientation')).toBeVisible();
  });

  test('the Jobs view is a status pane that slides out from the bottom edge', async ({ page }) => {
    const wb = new WorkbenchApp(page);
    await wb.frame.open();
    await wb.palette.open();
    await wb.palette.search('>show jobs');
    await wb.palette.run('Show jobs');
    await expect(page.locator('[data-testid="pd-dock-flyout"][data-panel-id="pd.jobs"], [data-testid="pd-dock-tab"][data-panel-id="pd.jobs"]').first()).toBeVisible();
    // Review 3 P3-3: with the Assistant column open, the bottom slide-out stays over its own column, clear of the composer.
    // The column opens first: a click outside an open slide-out hides it, as it should (review 4 follow-up: the test clicked
    // the activity bar while the slide-out was open and raced its hiding).
    await page.keyboard.press('Escape');
    await wb.activity.item('pd.wb.assistant.activity').click();
    const composer = page.getByTestId('assistant-input');
    await expect(composer).toBeVisible();
    await wb.palette.open();
    await wb.palette.search('>show jobs');
    await wb.palette.run('Show jobs');
    const flyout = page.locator('[data-testid="pd-dock-flyout"][data-panel-id="pd.jobs"]');
    await expect(flyout).toBeVisible();
    await expect.poll(async () => {
      const [f, c] = await Promise.all([flyout.boundingBox(), composer.boundingBox()]);
      return !!f && !!c && f.x + f.width <= c.x + 1;
    }).toBe(true);
  });

  for (const size of [{ width: 1440, height: 900 }, { width: 1920, height: 1080 }]) {
    test(`every docked group is at or above its views' declared minimum at ${size.width}x${size.height} (ADR 0011)`, async ({ page }) => {
      await page.setViewportSize(size);
      const wb = new WorkbenchApp(page);
      await wb.frame.open();
      await wb.explorer.expand('wd.files', 'src');
      await wb.explorer.node('wd.files', 'src/main.rs').dblclick();
      await wb.activity.item('wd.act.problems').click();
      await wb.activity.item('pd.wb.assistant.activity').click();
      await expect(tab(page, 'wd.problems')).toBeVisible();
      const short = await page.locator('[data-min-width]').evaluateAll((groups) =>
        groups
          .map((g) => ({ g, r: g.getBoundingClientRect(), w: Number(g.getAttribute('data-min-width')), h: Number(g.getAttribute('data-min-height')) }))
          .filter(({ r }) => r.width > 0 && r.height > 0)
          .filter(({ r, w, h }) => r.width + 1 < w || r.height + 1 < h)
          .map(({ r, w, h }) => `${Math.round(r.width)}x${Math.round(r.height)} < ${w}x${h}`),
      );
      expect(short).toEqual([]);
      await expectNoHorizontalScroll(page);
    });
  }
});

test.describe('assistant pane (plan W6)', () => {
  for (const colorScheme of THEMES) {
    test(`shows the calm "Pitwall not running" state with its fix (${colorScheme})`, async ({ page, pageErrors }) => {
      await page.emulateMedia({ colorScheme });
      const wb = new WorkbenchApp(page);
      await wb.frame.open();
      await wb.activity.item('pd.wb.assistant.activity').click();
      const pane = page.locator('pd-assistant');
      await expect(pane).toBeVisible();
      await expect(pane).toContainText('not running');
      expect(await axeViolations(page)).toEqual([]);
      expect(pageErrors).toEqual([]);
    });
  }
});

test.describe('pop-out (plan W3)', () => {
  test('moves the active panel to its own window and the dock offers it no more', async ({ page }) => {
    const wb = new WorkbenchApp(page);
    await wb.frame.open();
    await wb.explorer.expand('wd.files', 'src');
    await wb.explorer.node('wd.files', 'src/main.rs').dblclick();
    const tab = page.locator('[data-testid="pd-dock-tab"][data-panel-id="wd.code#src/main.rs"]');
    await expect(tab).toBeVisible();
    await tab.click();
    await wb.palette.open();
    await wb.palette.search('>pop out');
    await wb.palette.run('Pop out to window');
    await expect(tab).toHaveCount(0);
  });
});

test.describe('review 1 (X50)', () => {
  test('P1-1: the Explorer menu opens at the pointer, and at the row for Shift+F10', async ({ page }) => {
    const wb = new WorkbenchApp(page);
    await wb.frame.open();
    const row = wb.explorer.node('wd.files', 'README.md');
    const box = (await row.boundingBox())!;
    const at = { x: box.x + 60, y: box.y + box.height / 2 };
    await page.mouse.click(at.x, at.y, { button: 'right' });
    await expect(wb.explorer.menu).toBeVisible();
    // The overlay settles at the pointer within a frame of opening (CDK positions it after it is attached).
    await expect.poll(async () => {
      const m = (await wb.explorer.menu.boundingBox())!;
      return Math.max(Math.abs(m.x - at.x), Math.abs(m.y - (box.y + box.height + 2)));
    }).toBeLessThanOrEqual(8);
    await expect(row).toHaveClass(/pd-menu-target/);
    await page.keyboard.press('Escape');
    await row.click();
    await page.keyboard.press('Shift+F10');
    await expect(wb.explorer.menu).toBeVisible();
    await expect.poll(async () => {
      const kb = (await wb.explorer.menu.boundingBox())!;
      return Math.abs(kb.y - (box.y + box.height)) <= 8 && kb.x >= box.x;
    }).toBe(true);
  });

  test('review 3 P1-1: Shift+F10 puts the focus in the Explorer menu, arrows move it, Escape gives it back to the row', async ({ page }) => {
    const wb = new WorkbenchApp(page);
    await wb.frame.open();
    const row = wb.explorer.node('wd.files', 'README.md');
    // The mouse-opened menu, for its items and icons.
    const box = (await row.boundingBox())!;
    await page.mouse.click(box.x + 60, box.y + box.height / 2, { button: 'right' });
    const items = wb.explorer.menu.getByRole('menuitem');
    await expect(items.first()).toBeVisible();
    const byMouse = await items.evaluateAll((els) => els.map((e) => `${e.textContent?.trim()}|${e.querySelector('.pd-popmenu__icon pd-icon') ? 'icon' : 'none'}`));
    await page.keyboard.press('Escape');
    await row.click();
    await page.keyboard.press('Shift+F10');
    await expect(items.first()).toBeFocused();
    await page.keyboard.press('ArrowDown');
    await expect(items.nth(1)).toBeFocused();
    const byKeys = await items.evaluateAll((els) => els.map((e) => `${e.textContent?.trim()}|${e.querySelector('.pd-popmenu__icon pd-icon') ? 'icon' : 'none'}`));
    expect(byKeys).toEqual(byMouse);
    await page.keyboard.press('Escape');
    await expect(wb.explorer.menu).toBeHidden();
    await expect(row).toBeFocused();
  });

  test('P1-2: at 320 px the Problem column takes most of the table, with the file folded under it', async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 640 });
    const wb = new WorkbenchApp(page);
    await wb.frame.open();
    await wb.palette.open();
    await wb.palette.search('>show problems');
    await wb.palette.run('Show problems');
    const table = page.locator('wd-problems-view pd-table');
    await expect(table.getByRole('columnheader', { name: /Problem/ })).toBeVisible();
    const [tw, pw] = await Promise.all([
      table.evaluate((t) => t.getBoundingClientRect().width),
      table.getByRole('columnheader', { name: /Problem/ }).evaluate((h) => h.getBoundingClientRect().width),
    ]);
    expect(pw / tw).toBeGreaterThanOrEqual(0.6);
    await expect(table.locator('.where').first()).toBeVisible();
    // Review 2 P1-A: folded rows grow; no cell overflows, the second line clears the row's bottom border by 4 px.
    const overflow = await table.locator('[role=row].pd-row').evaluateAll((rows) =>
      rows.flatMap((r) => {
        const out: string[] = [];
        for (const c of r.querySelectorAll<HTMLElement>('[role=gridcell]')) if (c.scrollHeight > c.clientHeight + 1) out.push(`cell ${c.scrollHeight}>${c.clientHeight}`);
        const where = r.querySelector('.where');
        if (where && r.getBoundingClientRect().bottom - where.getBoundingClientRect().bottom < 4) out.push('second line on the border');
        return out;
      }),
    );
    expect(overflow).toEqual([]);
    // Review 3 P3-7: the severity glyph sits about 12 px from the problem text.
    const gap = await table.locator('[role=row].pd-row').first().evaluate((r) => r.querySelector('.text')!.getBoundingClientRect().left - r.querySelector('.sev')!.getBoundingClientRect().right);
    expect(gap).toBeGreaterThanOrEqual(8);
    expect(gap).toBeLessThanOrEqual(14);
    await expectNoHorizontalScroll(page);
  });

  test('review 2 P1-B: at 320 px the Explorer slides over the dock, which keeps its whole width', async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 640 });
    const wb = new WorkbenchApp(page);
    await wb.frame.open();
    await expect(page.getByTestId('pd-dock')).toBeVisible();
    const before = await page.getByTestId('pd-dock').evaluate((d) => d.getBoundingClientRect().width);
    await wb.activity.item('wd.act.explorer').click();
    await expect(wb.frame.sidebar).toHaveClass(/pd-side-overlay/);
    expect(await page.getByTestId('pd-dock').evaluate((d) => d.getBoundingClientRect().width)).toBe(before);
    // Every control of the dock lies inside the window (none is cut).
    const cut = await page.locator('pd-dock button, pd-dock [role=tab]').evaluateAll((els) =>
      els.filter((e) => {
        const r = e.getBoundingClientRect();
        return r.width > 0 && (r.left < -0.5 || r.right > innerWidth + 0.5);
      }).length,
    );
    expect(cut).toBe(0);
    // The pointer still rests on the activity button, whose matTooltip is open: the first Escape dismisses a tooltip (WCAG 1.4.13), so move away first.
    await page.mouse.move(300, 600);
    await expect(page.locator('.mat-mdc-tooltip')).toHaveCount(0);
    await page.keyboard.press('Escape');
    await expect(wb.frame.sidebar).toHaveCount(0);
    await expectNoHorizontalScroll(page);
  });

  test('review 2 P3-16: the menu is never painted at the window origin, not even on its first frame', async ({ page }) => {
    const wb = new WorkbenchApp(page);
    await wb.frame.open();
    await page.evaluate(() => {
      const w = window as unknown as { firstMenuFrame?: { x: number; y: number; opacity: string } };
      new MutationObserver((_, obs) => {
        const m = document.querySelector('[data-testid="explorer-menu"]');
        if (!m) return;
        obs.disconnect();
        requestAnimationFrame(() => {
          const r = m.getBoundingClientRect();
          w.firstMenuFrame = { x: r.x, y: r.y, opacity: getComputedStyle(m).opacity };
        });
      }).observe(document.body, { childList: true, subtree: true });
    });
    const box = (await wb.explorer.node('wd.files', 'README.md').boundingBox())!;
    await page.mouse.click(box.x + 60, box.y + 6, { button: 'right' });
    const first = await page.waitForFunction(() => (window as unknown as { firstMenuFrame?: unknown }).firstMenuFrame).then((h) => h.jsonValue() as Promise<{ x: number; y: number; opacity: string }>);
    // Transparent until placed (opacity, so a keyboard-opened menu can take the focus at once: review 3 P1-1).
    expect(first.opacity === '0' || (Math.abs(first.x - (box.x + 60)) <= 8 && first.y >= box.y + box.height)).toBe(true);
    // Settled: at the pointer's x, below the row (never over its label: review 2 P3-1).
    await expect.poll(async () => {
      const m = (await wb.explorer.menu.boundingBox())!;
      return Math.abs(m.x - (box.x + 60)) <= 8 && m.y >= box.y + box.height && m.y <= box.y + box.height + 8;
    }).toBe(true);
  });

  test('P1-3: a fitted layout is said in the notifications, never over the dock', async ({ page }) => {
    const wb = new WorkbenchApp(page);
    await page.setViewportSize({ width: 1440, height: 900 });
    await wb.frame.open();
    await wb.explorer.expand('wd.files', 'src');
    await wb.explorer.node('wd.files', 'src/main.rs').dblclick();
    await wb.activity.item('wd.act.problems').click();
    await expect(page.locator('[data-testid="pd-dock-tab"][data-panel-id="wd.problems"]')).toBeVisible();
    await expect.poll(() => page.evaluate(() => Object.keys(localStorage).some((k) => (localStorage.getItem(k) ?? '').includes('wd.problems')))).toBe(true);
    await page.setViewportSize({ width: 700, height: 560 });
    await page.reload();
    await expect(page.locator('pd-dock .pd-dock-notice')).toHaveCount(0);
    await expect(wb.notifications.toasts.getByText('Layout adjusted to fit this window')).toBeVisible();
  });
});

test('a pop-out window boots its shell and its one view (regression: the page kept the main root)', async ({ page, pageErrors }) => {
  await page.goto('/#/popout?panel=wd.code%23src%2Fmain.rs&host=main&label=p-main-1');
  // Without a host in the browser it waits for the host's snapshot first (bounded), then shows the view.
  await expect(page.getByTestId('code-lines')).toBeVisible({ timeout: 15_000 });
  await expect(page.locator('pd-popout-shell')).toBeVisible();
  expect(pageErrors).toEqual([]);
});
