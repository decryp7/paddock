import { expect, test, type Locator, type Page } from '@playwright/test';
import { openApp, settingsDialog } from './app';

/** The popover is 360 px wide inside a 6 px margin; the shell fits the window to it, up to the family's 520 px cap. */
const VIEWPORT = { width: 372, height: 1000 };

async function openTray(page: Page, scenario: 'demo' | 'empty' | 'worst' | 'down' | 'loading' = 'demo') {
  await page.setViewportSize(VIEWPORT);
  await openApp(page, '/tray', `mock-fast&mock-static&mock-popup=${scenario}`);
  const popup = page.locator('pd-tray-popover');
  await expect(popup).toBeVisible();
  if (scenario !== 'loading') {
    await expect(popup.locator('section.pd-w').first()).toBeVisible();
    await expect(popup.locator('[aria-busy="true"]')).toHaveCount(0);
  }
  await page.evaluate(async () => document.fonts.ready);
  return popup;
}

/** Every text in the popup's widgets that runs past its widget's padding or is cut without an ellipsis, as "widget: element what". */
async function textOutOfWidgets(popup: Locator): Promise<string[]> {
  return popup.locator('section.pd-w').evaluateAll((sections) =>
    sections.flatMap((s) => {
      const frame = s.getBoundingClientRect();
      const pad = parseFloat(getComputedStyle(s).paddingRight) || 0;
      const title = s.querySelector('.pd-w-title')?.textContent?.trim();
      // Elements that hold text themselves (a non-blank text node as a child).
      const texts = Array.from(s.querySelectorAll('*')).filter((e) => Array.from(e.childNodes).some((n) => n.nodeType === Node.TEXT_NODE && n.textContent!.trim() !== ''));
      return texts.flatMap((e) => {
        const r = e.getBoundingClientRect();
        const cs = getComputedStyle(e);
        const out: string[] = [];
        if (r.width > 0 && r.right > frame.right - pad + 1) out.push('past the frame padding');
        if (cs.textOverflow !== 'ellipsis' && cs.display !== 'contents' && e.scrollWidth > e.clientWidth + 1 && e.clientWidth > 0) out.push('scrolls sideways');
        // An inline text clipped by a text-less cell around it: the text's own box (a Range over its text nodes) against the
        // nearest ancestor that clips sideways, inside the widget. A text truncated on purpose (ellipsis) is left alone.
        if (cs.textOverflow !== 'ellipsis') {
          for (const n of Array.from(e.childNodes).filter((c) => c.nodeType === Node.TEXT_NODE && c.textContent!.trim() !== '')) {
            const range = document.createRange();
            range.selectNodeContents(n);
            const t = range.getBoundingClientRect();
            for (let a = e.parentElement; a && a !== s.parentElement; a = a.parentElement) {
              const acs = getComputedStyle(a);
              if (acs.overflowX === 'visible' || acs.textOverflow === 'ellipsis') continue;
              const box = a.getBoundingClientRect();
              if (t.width > 0 && (t.right > box.right + 1 || t.left < box.left - 1)) out.push(`clipped by ${a.className || a.tagName}`);
              break;
            }
          }
        }
        return out.map((w) => `${title}: ${e.className || e.tagName} ${w}`);
      });
    }),
  );
}

test.describe('the tray popup', () => {
  // The default popup fits the family's 520 px cap whole, with nothing to scroll, in every state, the worst included. A layout
  // that grows past the cap fails here (the numbers are measured, not assumed).
  for (const scenario of ['demo', 'empty', 'worst', 'down'] as const) {
    test(`the default popup fits 520 px without scrolling: ${scenario}`, async ({ page }) => {
      const popup = await openTray(page, scenario);
      const box = await popup.boundingBox();
      expect(Math.floor(box?.height ?? 0), `${scenario} is at most 520 px tall`).toBeLessThanOrEqual(520);
      const overflow = await popup.locator('.pd-content').evaluate((e) => e.scrollHeight - e.clientHeight);
      expect(overflow, `${scenario} needs no scrolling`).toBeLessThanOrEqual(0);
      expect(box?.width).toBe(360);
    });
  }

  // Text must stay inside its widget in every state, also with another font: the letter-spacing widening makes every text about
  // 6 % wider, as a wider system font would (Windows), so a fit that is only a pixel or two away fails here too. What is checked
  // is every element that holds text itself, not the boxes around it: the shared frame's body and a row's hover tint reach past
  // the padding on purpose (paddock-ui's `.pd-w-body` bleed, ADR 0044), and their containers then measure that bleed as
  // overflow, which is no text running out. A text truncated on purpose (ellipsis) may be scrolled.
  for (const scenario of ['demo', 'empty', 'worst', 'down'] as const) {
    test(`no text runs out of its widget, with a wider font too: ${scenario}`, async ({ page }) => {
      const popup = await openTray(page, scenario);
      await page.addStyleTag({ content: '* { letter-spacing: 0.06em !important; }' });
      await page.evaluate(async () => document.fonts.ready);
      expect(await textOutOfWidgets(popup)).toEqual([]);
      // The main word of a widget (its state, its figure) is never cut, not even by the ellipsis kept as a safety net.
      const cut = await popup.locator('section.pd-w .big').evaluateAll((els) => els.filter((e) => e.scrollWidth > e.clientWidth).map((e) => e.textContent!.trim()));
      expect(cut, 'no main word is truncated').toEqual([]);
    });
  }

  // The check above sees a text that runs out (a positive control, ER-33): a title wider than its widget and a figure clipped
  // without an ellipsis are both reported.
  test('the text check reports a text past the frame and a clipped text', async ({ page }) => {
    const popup = await openTray(page, 'demo');
    await page.addStyleTag({
      content: 'section.pd-w .pd-w-title { white-space: nowrap; min-width: 600px; } section.pd-w .big { width: 4px; overflow: hidden; text-overflow: clip; white-space: nowrap; }',
    });
    const bad = await textOutOfWidgets(popup);
    expect(bad.some((b) => b.endsWith('past the frame padding')), bad.join('\n')).toBe(true);
    expect(bad.some((b) => b.endsWith('scrolls sideways')), bad.join('\n')).toBe(true);
  });

  // A short inline text in a text-less cell that clips it (no ellipsis) is reported too: the cell holds no text itself, so only
  // the text's own box shows it (review of B4/tray).
  test('the text check reports an inline text clipped by a text-less cell', async ({ page }) => {
    const popup = await openTray(page, 'demo');
    await page.addStyleTag({ content: 'section.pd-w .pd-w-content { width: 24px; overflow: hidden; }' });
    const bad = await textOutOfWidgets(popup);
    expect(bad.some((b) => b.includes('clipped by pd-w-content')), bad.join('\n')).toBe(true);
  });

  // The loading twin is the ready widget: same rows (the engine's shape), so the popup does not jump when the values arrive.
  test('each widget is as tall while loading as when ready (the engine says how many rows)', async ({ page }) => {
    const heights = async (scenario: 'demo' | 'loading') => {
      const popup = await openTray(page, scenario);
      await expect(popup.locator('section.pd-w')).toHaveCount(4);
      return popup.locator('section.pd-w').evaluateAll((s) => s.map((e) => [e.querySelector('.pd-w-title')?.textContent?.trim(), Math.round(e.getBoundingClientRect().height)]));
    };
    // Compared with the demo, where Loaded models has no "+N more" line (that line exists only when there is more to say).
    const ready = await heights('demo');
    const loading = await heights('loading');
    // Server, Activity, Memory and Loaded models (2 rows) fill their rows in the demo state.
    expect(loading).toEqual(ready);
  });

  test('an action that failed adds a row in the worst state and the popup box stays within 520 px', async ({ page }) => {
    await page.setViewportSize(VIEWPORT);
    await openApp(page, '/tray', 'mock-fast&mock-static&mock-popup=worst&mock-fail=setServing');
    const popup = page.locator('pd-tray-popover');
    await expect(popup.locator('section.pd-w').first()).toBeVisible();
    await popup.getByRole('button', { name: 'More actions' }).click();
    await page.getByRole('menuitem', { name: 'Resume serving' }).click();
    await expect(popup.locator('p.error')).toBeVisible();
    expect(Math.floor((await popup.boundingBox())!.height)).toBeLessThanOrEqual(520);
  });

  test('says the server is down once, above the widgets, and marks the last readings', async ({ page }) => {
    const popup = await openTray(page, 'down');
    await expect(popup.getByText('The server is not answering')).toHaveCount(1);
    await expect(popup.locator('pw-status-pill')).toContainText('No answer');
    await expect(popup.getByRole('region', { name: 'Memory' })).toBeVisible();
  });

  test('the shared footer: Open Pitwall right-aligned beside one more-actions button; the menu has a separator before Quit', async ({ page }) => {
    const popup = await openTray(page);
    const foot = popup.locator('footer.pd-footer');
    const open = foot.getByRole('button', { name: 'Open Pitwall' });
    const more = foot.getByRole('button', { name: 'More actions' });
    const [o, m, f] = await Promise.all([open.boundingBox(), more.boundingBox(), foot.boundingBox()]);
    expect(o!.x + o!.width).toBeLessThanOrEqual(m!.x);
    expect(m!.x + m!.width).toBeGreaterThan(f!.x + f!.width - 20);
    await more.click();
    const items = page.getByRole('menuitem');
    await expect(items).toHaveText(['Customize widgets…', 'Pause serving', 'Release memory', 'Settings…', 'Quit Pitwall']);
    await expect(page.locator('.mat-mdc-menu-panel hr.pd-tray-sep')).toHaveCount(1);
    // The trigger shows that the menu is its own while open; Escape closes it and gives the focus back.
    await expect(more).toHaveClass(/pd-tray-more--open/);
    await page.keyboard.press('Escape');
    await expect(items).toHaveCount(0);
    await expect(more).toBeFocused();
    await expect(more).not.toHaveClass(/pd-tray-more--open/);
  });

  test('pausing serving from the menu changes the pill and the menu', async ({ page }) => {
    const popup = await openTray(page);
    await popup.getByRole('button', { name: 'More actions' }).click();
    await page.getByRole('menuitem', { name: 'Pause serving' }).click();
    await expect(popup.locator('pw-status-pill')).toContainText('Paused');
    await popup.getByRole('button', { name: 'More actions' }).click();
    await expect(page.getByRole('menuitem', { name: 'Resume serving' })).toBeVisible();
  });

  // The browser build plays both windows in one tab: the popover leaves the route for the main window, which takes it as it
  // starts, as a main window the shell rebuilt does in the app.
  test('"Open Pitwall" opens the main window on the dashboard', async ({ page }) => {
    const popup = await openTray(page);
    await popup.getByRole('button', { name: 'Open Pitwall' }).click();
    await expect(page.locator('pd-shell-frame')).toBeVisible();
    await expect(page).toHaveURL(/#\/dashboard$/);
    await expect(page.locator('pw-tray-popover')).toHaveCount(0);
  });

  test('a loaded model opens Models', async ({ page }) => {
    const popup = await openTray(page);
    await popup.getByRole('region', { name: 'Loaded models' }).locator('.row').first().click();
    await expect(page.locator('pd-shell-frame')).toBeVisible();
    await expect(page).toHaveURL(/#\/models$/);
  });

  test('the pairing notice\'s Review opens the dashboard', async ({ page }) => {
    const popup = await openTray(page, 'worst');
    await popup.getByRole('button', { name: 'Review' }).click();
    await expect(page.locator('pd-shell-frame')).toBeVisible();
    await expect(page).toHaveURL(/#\/dashboard$/);
  });

  test('"Customize widgets…" opens Settings at the Tray popup page with the chooser', async ({ page }) => {
    const popup = await openTray(page);
    await popup.getByRole('button', { name: 'More actions' }).click();
    await page.getByRole('menuitem', { name: 'Customize widgets…' }).click();
    await expect(page.locator('pd-shell-frame')).toBeVisible();
    await expect(settingsDialog(page)).toBeVisible();
    await expect(settingsDialog(page).locator('pd-widget-chooser')).toBeVisible();
    await expect(settingsDialog(page).getByRole('heading', { name: 'Tray popup' })).toBeVisible();
  });
});
