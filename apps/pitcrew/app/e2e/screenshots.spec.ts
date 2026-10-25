import { type Page, test } from '@playwright/test';
import { join } from 'node:path';

/**
 * Documentation screenshots of every screen in both themes, against the mock backend:
 * `SCREENSHOTS=1 npx playwright test --project=screenshots` → ../docs/screenshots/<theme>/<screen>.png (1440×900 at 2x).
 */
const OUT = join(__dirname, '..', '..', 'docs', 'screenshots');
/** A fixed clock so elapsed times and "x min ago" read the same on every run. */
const NOW = new Date('2026-10-22T14:40:00');

interface Shot {
  name: string;
  path: string;
  query?: string;
  /** A taller window for a card that does not fit in 900 px. */
  height?: number;
  /** The part of a tall window to capture (cards that are far down a long page). */
  clip?: (page: Page) => Promise<{ x: number; y: number; width: number; height: number }>;
  /** Prepares the view after load (open a dialog, scroll). */
  prepare?: (page: Page) => Promise<void>;
}

const scrollMain = (y: number) => async (page: Page) => {
  await page.evaluate((top) => document.querySelector('main')?.scrollTo({ top }), y);
};

/**
 * Nothing may be cut mid-card: when a block of the page (card, table, callout) crosses the bottom edge, everything
 * from just below the last block that fits completely is covered with the page background.
 */
const coverCutBlocks = async (page: Page) => {
  await page.evaluate(() => {
    const main = document.querySelector('main');
    if (!main) return;
    const height = window.innerHeight;
    const all = Array.from(main.querySelectorAll<HTMLElement>('.card, table, .callout, .tile, article, .empty, .chips'));
    const blocks = all.filter((e) => !all.some((o) => o !== e && o.contains(e)));
    const rects = blocks.map((b) => b.getBoundingClientRect()).filter((r) => r.height > 0);
    const visible = rects.filter((r) => r.top < height);
    if (!visible.some((r) => r.bottom > height - 2)) return;
    const crossing = visible.filter((r) => r.bottom > height - 2).map((r) => r.top);
    const fitting = visible.filter((r) => r.bottom <= height - 2).map((r) => r.bottom);
    if (!fitting.length) return;
    // Cut above the highest block that crosses the edge, so a row of cards is never left half there.
    const cut = Math.max(...fitting.filter((b) => b <= Math.min(...crossing) + 2)) + 4;
    if (!Number.isFinite(cut)) return;
    const box = main.getBoundingClientRect();
    // The colour of the page just above the cut: the first opaque background from the element there upwards.
    const paper = (y: number) => {
      let el: Element | null = document.elementFromPoint(box.left + 6, Math.max(1, y - 6));
      while (el) {
        const c = getComputedStyle(el).backgroundColor;
        if (c && c !== 'rgba(0, 0, 0, 0)' && c !== 'transparent') return c;
        el = el.parentElement;
      }
      return getComputedStyle(document.body).backgroundColor;
    };
    const cover = document.createElement('div');
    cover.style.cssText = `position:fixed;left:${box.left}px;width:${box.width}px;top:${cut}px;bottom:0;z-index:99999;background:${paper(cut)};pointer-events:none`;
    document.body.appendChild(cover);
  });
};

const SHOTS: Shot[] = [
  { name: 'dashboard', path: '/' },
  { name: 'dashboard-history', path: '/', prepare: scrollMain(640) },
  {
    name: 'dashboard-load',
    path: '/',
    prepare: async (page) => {
      await page.getByRole('heading', { name: 'Average job duration' }).evaluate((h) => h.scrollIntoView({ block: 'start' }));
      await page.evaluate(() => document.querySelector('main')?.scrollBy({ top: -64 }));
    },
  },
  { name: 'runners', path: '/runners' },
  { name: 'runner-detail', path: '/runners/build-win' },
  { name: 'runner-updates', path: '/runners/ui-tests?tab=updates' },
  { name: 'runner-disk', path: '/runners/docker-lab?tab=disk' },
  { name: 'runner-config-form', path: '/runners/docker-lab?tab=config' },
  {
    name: 'runner-config-advanced',
    path: '/runners/ui-tests?tab=config',
    prepare: async (page) => {
      await page.getByRole('radio', { name: 'Advanced (YAML)' }).click();
      const editor = page.getByRole('textbox', { name: 'config.yaml' });
      const text = await editor.inputValue();
      await editor.fill(text.replace('  capacity: 1\n', '  capacity: 1\n\t  retries: 3\n'));
      await page.getByTestId('yaml-status').waitFor();
      await page.waitForTimeout(600);
      await page.getByRole('button', { name: /Error on line/ }).hover();
      await editor.evaluate((e) => e.parentElement?.parentElement?.scrollTo({ top: 0 }));
    },
  },
  {
    name: 'runner-config-diff',
    path: '/runners/build-win?tab=config',
    prepare: async (page) => {
      await page.getByLabel('Capacity').fill('4');
      await page.getByLabel('Job timeout').fill('2h');
      await page.getByRole('button', { name: 'Review changes' }).click();
      await page.getByRole('dialog').waitFor();
    },
  },
  { name: 'runner-processes', path: '/runners/build-win?tab=processes' },
  { name: 'activity', path: '/activity' },
  {
    name: 'activity-jobs',
    path: '/activity',
    height: 2400,
    clip: async (page) => {
      const r = await page.getByRole('heading', { name: 'Recent jobs' }).evaluate((h) => {
        const card = (h.closest('section,article') ?? h) as HTMLElement;
        const rows = Array.from(card.querySelectorAll('tbody tr'));
        const last = rows[Math.min(9, rows.length - 1)]?.getBoundingClientRect().bottom ?? card.getBoundingClientRect().bottom;
        return { top: card.getBoundingClientRect().top, bottom: last };
      });
      return { x: 224, y: Math.max(0, r.top - 10), width: 1216, height: r.bottom - r.top + 2 };
    },
  },
  { name: 'insights', path: '/insights' },
  { name: 'insights-repos', path: '/insights', prepare: scrollMain(700) },
  { name: 'storage', path: '/storage' },
  {
    name: 'storage-prune',
    path: '/storage',
    prepare: async (page) => {
      await page.getByTestId('preview').click();
      await page.getByTestId('engine-preview').waitFor();
      await page.getByRole('heading', { name: 'Prune' }).evaluate((h) => h.scrollIntoView({ block: 'start' }));
      await page.evaluate(() => document.querySelector('main')?.scrollBy({ top: -24 }));
    },
  },
  {
    name: 'storage-cache',
    path: '/storage',
    prepare: async (page) => {
      await page.getByRole('heading', { name: 'Actions cache' }).evaluate((h) => h.scrollIntoView({ block: 'start' }));
      await page.evaluate(() => document.querySelector('main')?.scrollBy({ top: -24 }));
    },
  },
  { name: 'logs', path: '/logs?runner=build-win' },
  { name: 'updates', path: '/updates' },
  { name: 'settings', path: '/settings' },
  { name: 'settings-defaults', path: '/settings?section=defaults' },
  {
    name: 'settings-processes',
    path: '/settings?section=defaults',
    prepare: async (page) => {
      await page.getByRole('heading', { name: 'Leftover processes' }).scrollIntoViewIfNeeded();
      await page.evaluate(() => document.querySelector('main')?.scrollBy({ top: 200 }));
    },
  },
  { name: 'settings-tokens', path: '/settings?section=tokens' },
  {
    name: 'settings-disk-guard',
    path: '/settings?section=defaults',
    height: 2800,
    clip: async (page) => {
      const top = await page.getByRole('heading', { name: 'Disk guard' }).evaluate((h) => h.closest('section,article')?.getBoundingClientRect().top ?? 0);
      const bottom = await page.getByRole('heading', { name: 'Pause windows' }).evaluate((h) => h.closest('section,article')?.getBoundingClientRect().bottom ?? 0);
      return { x: 320, y: Math.max(0, top - 10), width: 1100, height: Math.min(1000, bottom - top + 20) };
    },
  },
  {
    name: 'settings-channels',
    path: '/settings?section=channels',
    height: 1400,
    prepare: async (page) => {
      await page.getByRole('button', { name: 'Add channel' }).first().click();
      await page.getByLabel('Name', { exact: true }).fill('Team chat webhook');
      await page.getByLabel('Address').fill('https://hooks.example.com/pitcrew');
    },
  },
  {
    name: 'register-wizard',
    path: '/runners',
    prepare: async (page) => {
      await page.getByRole('button', { name: 'Register runner' }).first().click();
      const dialog = page.getByRole('dialog');
      await dialog.getByLabel('Server URL').fill('https://git.example.com');
      await dialog.getByLabel('Registration token').fill('demo-registration-token');
      await dialog.getByRole('button', { name: 'Continue' }).click();
      await dialog.getByLabel('Name in Pitcrew').fill('nightly');
      await dialog.getByRole('button', { name: 'Continue' }).click();
      await dialog.getByText('Run as the service account').click();
    },
  },
  { name: 'service-unavailable', path: '/', query: 'mock=notInstalled' },
  { name: 'empty', path: '/', query: 'mock=empty' },
];

for (const theme of ['dark', 'light'] as const) {
  test.describe(`${theme} theme`, () => {
    test.use({ colorScheme: theme });

    for (const shot of SHOTS) {
      test(shot.name, async ({ page }) => {
        await page.clock.setFixedTime(NOW);
        await page.addInitScript((t) => localStorage.setItem('pitcrew.theme', t), theme);
        if (shot.height) await page.setViewportSize({ width: 1440, height: shot.height });
        await page.emulateMedia({ reducedMotion: 'reduce' });
        await page.goto(`/?simulate=0${shot.query ? '&' + shot.query : ''}#${shot.path}`);
        await page.waitForLoadState('networkidle');
        await page.waitForTimeout(1200);
        await shot.prepare?.(page);
        await page.waitForTimeout(700);
        await coverCutBlocks(page);
        const clip = shot.clip ? await shot.clip(page) : undefined;
        await page.screenshot({ path: join(OUT, theme, `${shot.name}.png`), clip });
      });
    }
  });
}
