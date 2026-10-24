import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, test, type Locator, type Page } from '@playwright/test';

// The shell demo in the generated pseudo-locale (plan S15, ADR 0012 §6): accented, longer, bracketed (⟦…⟧) text. A smoke
// test of runtime translation loading and the guard against unmarked English in every part the shell draws: the menu
// bar and its menus, the status items and their popovers, toasts, the service chip, the job card and dialogs. The demo's
// own page text is app content and outside the check; its contributions (commands, menus, dialog titles) are in.

const PSEUDO = /⟦[^⟧]*⟧/;
/** Text that is never translated: the app's name, key names and glyphs. */
const ALLOWED = /^(Ctrl|Shift|Alt|Cmd|Win|Esc|Enter|Tab|F\d+|[A-Z])$/;
const LANGUAGE_KEY = 'paddock.language';

async function openPseudo(page: Page, path = '/'): Promise<void> {
  await page.addInitScript((key) => {
    // Seed once per tab, so a reload keeps what the test chose.
    if (sessionStorage.getItem('paddock.e2e.seeded')) return;
    sessionStorage.setItem('paddock.e2e.seeded', '1');
    localStorage.clear();
    localStorage.setItem(key, JSON.stringify({ version: 1, language: 'pseudo' }));
  }, LANGUAGE_KEY);
  await page.goto(path);
  await expect(page.locator('html')).toHaveAttribute('lang', 'en-XA');
}

/**
 * English words in the visible text and labels of `scope` outside a pseudo-translated ⟦…⟧ run, so an unmarked string
 * shows up as e.g. "Mark all read". Data (job titles, phases, engine messages) must be kept out of `scope` by `skip`.
 */
async function unmarkedEnglish(scope: Locator, skip = ''): Promise<string[]> {
  const texts = await scope.evaluateAll(
    (els, skipSelector) => {
      const out: string[] = [];
      for (const el of els) {
        const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
        for (let n: Node | null = el; n; n = walker.nextNode()) {
          const owner = n instanceof Element ? n : n.parentElement;
          if (!owner || owner.closest('.pd-menu-hint, kbd') || (skipSelector && owner.closest(skipSelector))) continue;
          if (n instanceof Element) {
            const style = getComputedStyle(n);
            if (style.display === 'none' || style.visibility === 'hidden') continue;
            for (const a of ['aria-label', 'title', 'placeholder', 'aria-valuetext']) {
              const v = n.getAttribute(a);
              if (v) out.push(v);
            }
          } else out.push(n.textContent ?? '');
        }
      }
      return out;
    },
    skip,
  );
  const words = new Set<string>();
  for (const t of texts) for (const w of outsidePseudo(t).match(/[A-Za-z]{2,}/g) ?? []) if (!ALLOWED.test(w)) words.add(w);
  return [...words];
}

/**
 * `text` without its pseudo-translated runs. Runs nest ("⟦⟦2 running⟧: Reading files⟧": the phase is data inside a
 * translated message), so the innermost run goes first until none is left.
 */
function outsidePseudo(text: string): string {
  let t = text;
  for (let prev = ''; prev !== t; ) [prev, t] = [t, t.replace(/⟦[^⟦⟧]*⟧/g, ' ')];
  return t;
}

const menuBar = (page: Page) => page.getByRole('menubar');

test.describe('pseudo-locale (plan S15)', () => {
  test.beforeEach(async ({ page }) => {
    await openPseudo(page);
    await expect(page.getByTestId('app-context')).toContainText('loaded');
  });

  test('the shell loads the translations: lang, the menu bar and the status items are pseudo text', async ({ page }) => {
    await expect(page.locator('html')).toHaveAttribute('dir', 'ltr');
    await expect(menuBar(page)).toHaveAttribute('aria-label', PSEUDO);
    await expect(menuBar(page).getByRole('menuitem').first()).toHaveText(PSEUDO);
    await expect(page.locator('pd-jobs-status .pill')).toHaveText(PSEUDO);
    await expect(page.getByTestId('notifications-status')).toHaveAttribute('aria-label', PSEUDO);
  });

  test('no unmarked English in the menu bar and any of its menus and submenus', async ({ page }) => {
    expect(await unmarkedEnglish(menuBar(page))).toEqual([]);
    const top = menuBar(page).getByRole('menuitem');
    const count = await top.count();
    expect(count).toBeGreaterThanOrEqual(3);
    for (let i = 0; i < count; i++) {
      await top.nth(i).click();
      // The overlays stack in opening order: the top menu first, then its open submenu.
      const menu = page.getByRole('menu').first();
      await expect(menu).toBeVisible();
      expect(await unmarkedEnglish(menu), `menu ${i}`).toEqual([]);
      const subs = menu.locator('[aria-haspopup]');
      const subCount = await subs.count();
      for (let j = 0; j < subCount; j++) {
        await subs.nth(j).click();
        const sub = page.getByRole('menu').nth(1);
        await expect(sub).toBeVisible();
        expect(await unmarkedEnglish(sub), `submenu ${j} of menu ${i}`).toEqual([]);
        // Escape closes the whole stack: reopen the top menu for its next submenu.
        await page.keyboard.press('Escape');
        await expect(page.getByRole('menu')).toHaveCount(0);
        await top.nth(i).click();
      }
      await page.keyboard.press('Escape');
      await expect(page.getByRole('menu')).toHaveCount(0);
    }
  });

  test('no unmarked English in the pages frame: title bar, sidebar and its toggle, landmarks', async ({ page }) => {
    // The app's name is the app's own text.
    expect(await unmarkedEnglish(page.locator('pd-shell-frame header.title'), '[data-testid=app-name], [role=menu]')).toEqual([]);
    await expect(page.getByTestId('sidebar-toggle')).toHaveAttribute('aria-label', PSEUDO);
    await expect(page.locator('nav#pd-pages-nav')).toHaveAttribute('aria-label', PSEUDO);
    expect(await unmarkedEnglish(page.locator('nav#pd-pages-nav'))).toEqual([]);
    await expect(page.locator('pd-shell-frame footer.status')).toHaveAttribute('aria-label', PSEUDO);
    await expect(page.locator('pd-shell-frame a.skip')).toHaveText(PSEUDO);
  });

  test('no unmarked English in the status items, the jobs list, the notification centre and toasts', async ({ page }) => {
    // Empty states first.
    await page.locator('pd-jobs-status .pill').click();
    expect(await unmarkedEnglish(page.locator('pd-jobs-status'))).toEqual([]);
    await page.keyboard.press('Escape');
    await page.getByTestId('notifications-status').click();
    expect(await unmarkedEnglish(page.locator('pd-notifications-status'))).toEqual([]);
    await page.keyboard.press('Escape');

    // A running job: its card's buttons, counts and labels (the title and the phase are the engine's data).
    await page.locator('button.cmd[data-command="sd.job.start"]').click();
    await page.locator('pd-jobs-status .pill').click();
    const job = page.locator('pd-jobs-status pd-job').first();
    await expect(job.getByRole('button')).toHaveText(PSEUDO);
    expect(await unmarkedEnglish(page.locator('pd-jobs-status'), '.title, .headline, .subject, [role=status], [role=progressbar]')).toEqual([]);
    await job.getByRole('button').click();
    await page.keyboard.press('Escape');

    // A notice: its toast (the message is the app's) and the centre with it.
    await page.locator('button.cmd[data-command="sd.notify.three"]').click();
    const toast = page.getByRole('region', { name: PSEUDO }).filter({ has: page.getByTestId('notification-dismiss') });
    await expect(toast.getByTestId('notification-dismiss')).toHaveAttribute('aria-label', PSEUDO);
    expect(await unmarkedEnglish(toast, '.title, .body')).toEqual([]);
    await page.getByTestId('notifications-status').click();
    const centre = page.locator('pd-notifications-panel');
    await expect(centre.getByTestId('notices-mark-all')).toHaveText(PSEUDO);
    expect(await unmarkedEnglish(centre, '.title, .body, .time')).toEqual([]);
  });

  test('no unmarked English in the service chip and its details (the engine’s cause text is data)', async ({ page }) => {
    await page.locator('button.cmd[data-command="sd.engine.break"]').click();
    const chip = page.getByTestId('service-health-chip');
    await expect(chip).toHaveText(PSEUDO);
    await chip.click();
    const details = page.getByTestId('service-health-details');
    await expect(details).toBeVisible();
    await expect(details.locator('button').first()).toHaveText(PSEUDO);
    expect(await unmarkedEnglish(page.locator('pd-service-status'), '.svc > strong, .hint, code')).toEqual([]);
  });

  test('a dialog in the page and a dialog window: the shell’s chrome and the contributed title are pseudo text', async ({ page }) => {
    await page.goto('/#/dialogs');
    await page.locator('[data-dialog-open="sd.about"]').click();
    const dialog = page.getByRole('dialog');
    await expect(dialog.getByTestId('dialog-close')).toHaveAttribute('aria-label', PSEUDO);
    await expect(dialog.locator('.title')).toHaveText(PSEUDO);
    expect(await unmarkedEnglish(dialog.locator('.head'))).toEqual([]);
    await dialog.getByTestId('dialog-close').click();

    // A dialog window (the desktop build's OS window, opened by URL in the browser build): the shell's own Close.
    const input = encodeURIComponent('{}');
    await page.goto(`/#/dialog?id=sd.about&host=main&label=d-main-1&title=About&dialogInput=${input}`);
    await expect(page.locator('html')).toHaveAttribute('lang', 'en-XA');
    const shell = page.locator('pd-dialog-shell');
    await expect(shell.getByTestId('dialog-loading')).toHaveCount(0);
    const foot = shell.getByTestId('dialog-shell-foot');
    if (await foot.count()) expect(await unmarkedEnglish(foot)).toEqual([]);
  });

  test('every message of the shell catalog is in the demo’s pseudo-locale file (paddock-i18n merge)', () => {
    const catalog = readFileSync(join(__dirname, '..', 'projects', 'paddock-ui', 'i18n', 'messages.pd.xlf'), 'utf8');
    const ids = [...catalog.matchAll(/<trans-unit id="([^"]+)"/g)].map((m) => m[1]);
    const loaded = JSON.parse(readFileSync(join(__dirname, '..', 'projects', 'shell-demo', 'src', 'locale', 'messages.en-XA.json'), 'utf8')) as {
      translations: Record<string, string>;
    };
    expect(ids.length).toBeGreaterThan(50);
    expect(ids.filter((id) => !(id in loaded.translations))).toEqual([]);
  });
});

test('English stays the default and loads no translation chunk', async ({ page }) => {
  const chunks: string[] = [];
  page.on('request', (r) => r.url().includes('messages') && chunks.push(r.url()));
  await page.addInitScript(() => localStorage.clear());
  await page.goto('/');
  await expect(page.locator('html')).toHaveAttribute('lang', /^en/);
  await expect(page.getByRole('menubar', { name: 'Application menu' })).toBeVisible();
  await expect(page.getByRole('menubar').getByRole('menuitem').first()).toHaveText('File');
  expect(chunks).toEqual([]);
});

test('choosing the pseudo-locale from the menu saves it and reloads the window in it; English again the same way', async ({ page }) => {
  await page.addInitScript(() => {
    if (sessionStorage.getItem('paddock.e2e.seeded')) return;
    sessionStorage.setItem('paddock.e2e.seeded', '1');
    localStorage.clear();
  });
  await page.goto('/');
  await expect(page.getByTestId('app-context')).toContainText('loaded');
  await page.getByRole('menubar').getByRole('menuitem', { name: 'View', exact: true }).click();
  await page.getByRole('menu', { name: 'View' }).getByRole('menuitem', { name: 'Appearance' }).click();
  await page.getByRole('menu', { name: 'Appearance' }).getByRole('menuitemcheckbox', { name: 'Pseudo-locale' }).click();
  await expect(page.locator('html')).toHaveAttribute('lang', 'en-XA');
  expect(JSON.parse((await page.evaluate((k) => localStorage.getItem(k), LANGUAGE_KEY)) ?? '{}')).toEqual({ version: 1, language: 'pseudo' });
  await expect(page.getByTestId('app-context')).toContainText('loaded');
  await page.locator('button.cmd[data-command="sd.pseudo-locale"]').click();
  await expect(page.locator('html')).toHaveAttribute('lang', 'en-US');
  await expect(page.getByRole('menubar', { name: 'Application menu' })).toBeVisible();
});
