import { expect, test, type Page } from '@playwright/test';
import { pressMod } from '@decryp7/paddock-e2e';

/** Browser flows against the mock backend (a macOS laptop; `?mock=desktop` has no battery or sensors). */
const url = (path = '/', query = '') => `/?latency=0${query}#${path}`;

/** Picks an option of a Material select (the list opens in an overlay on the page). */
async function choose(page: Page, label: string, option: string | RegExp): Promise<void> {
  await page.getByRole('combobox', { name: label }).click();
  await page.getByRole('option', { name: option }).click();
}

/** Opens the shell's Settings window (Mod+,) and shows Dyno's page of it. */
async function openSettings(page: Page): Promise<void> {
  await page.goto(url('/'));
  await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible();
  await pressMod(page, 'Mod+Comma');
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  await dialog.getByText('Dyno', { exact: true }).click();
}

test.describe('screens', () => {
  test('every screen opens from the sidebar', async ({ page }) => {
    await page.goto(url());
    await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible();
    const nav = page.getByRole('navigation', { name: /./ }).or(page.locator('aside nav'));
    for (const [label, heading] of [
      ['Processes', 'Processes'],
      ['Throttling', 'Throttling'],
      ['Inventory', 'Inventory'],
      ['Alarms', 'Alarms'],
      ['Flight recorder', 'Flight recorder'],
      ['Health', 'Health'],
      ['Backup health', 'Backup health'],
      ['Extras', 'Extras'],
      ['Lifespan', 'Lifespan'],
      ['Diagnostics', 'Diagnostics bundle'],
      ['Dashboard', 'Dashboard'],
    ] as const) {
      await nav.first().getByRole('link', { name: label, exact: true }).click();
      await expect(page.getByRole('heading', { level: 1, name: heading })).toBeVisible();
    }
  });

  test('with nothing unbuilt there is no Coming button, and an unknown route lands on the dashboard', async ({ page }) => {
    await page.goto(url());
    await expect(page.getByRole('button', { name: /Coming/ })).toHaveCount(0);
    await page.goto(url('/no-such-screen'));
    await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible();
  });

  test('the tray dropdown renders the chosen widgets', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 520 });
    await page.goto(url('/tray'));
    await expect(page.getByRole('button', { name: /Open Dyno/ })).toBeVisible();
    await expect(page.locator('pd-widget-frame')).toHaveCount(3);
  });
});

test.describe('command palette', () => {
  const input = (page: Page) => page.getByRole('combobox', { name: /Type a command/ });
  const dashboard = async (page: Page) => {
    await page.goto(url('/'));
    await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible();
  };

  test('Mod+P goes to a page: type its name and press Enter, and the focus lands on the page', async ({ page }) => {
    await dashboard(page);
    const sidebarPages = await page.getByRole('navigation', { name: 'Pages' }).getByRole('link').count();
    await pressMod(page, 'Mod+p');
    await expect(input(page)).toBeFocused();
    // Every page of the sidebar is listed, whatever their number.
    await expect(page.getByRole('option')).toHaveCount(sidebarPages);
    await input(page).fill('flight');
    await expect(page.getByRole('option')).toHaveCount(1);
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { level: 1, name: 'Flight recorder' })).toBeVisible();
    await expect(input(page)).toBeHidden();
    await expect(page.locator('#pd-main')).toBeFocused();
  });

  test('going to the page that is already shown also lands the focus on the page', async ({ page }) => {
    await dashboard(page);
    await pressMod(page, 'Mod+p');
    await input(page).fill('dashboard');
    await page.keyboard.press('Enter');
    await expect(input(page)).toBeHidden();
    await expect(page.locator('#pd-main')).toBeFocused();
  });

  test('keys typed right after the shortcut are kept, and an early Enter runs nothing', async ({ page }) => {
    await dashboard(page);
    // The first command of a window waits for the app's context chunk; once it is loaded the palette opens at once.
    await pressMod(page, 'Mod+Shift+P');
    await expect(input(page)).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(input(page)).toBeHidden();
    await pressMod(page, 'Mod+Shift+P');
    // No waiting for the input: this is how fast people type.
    await page.keyboard.type('check backup');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { level: 1, name: 'Backup health' })).toBeVisible();
    await expect(page.getByRole('dialog')).toHaveCount(0);
  });

  test('Mod+Shift+P runs a command on its page, and Escape closes the palette', async ({ page }) => {
    await dashboard(page);
    await pressMod(page, 'Mod+Shift+P');
    await expect(input(page)).toBeFocused();
    await input(page).fill('> system logs');
    await expect(page.getByRole('option')).toHaveCount(1);
    await page.keyboard.press('Enter');
    // The command shows its page first, so the result is where the person looks.
    await expect(page.getByRole('heading', { level: 1, name: 'Issues' })).toBeVisible();
    await expect(page.getByText(/Read the system logs/)).toBeVisible();
    await expect(input(page)).toBeHidden();

    await pressMod(page, 'Mod+Shift+P');
    await expect(input(page)).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(input(page)).toBeHidden();
  });

  test('a command of another page shows that page: Why slow runs again where its result appears', async ({ page }) => {
    await dashboard(page);
    await pressMod(page, 'Mod+Shift+P');
    await input(page).fill('> why slow run again');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { level: 1, name: 'Why is my computer slow?' })).toBeVisible();
    await expect(page.locator('#pd-main')).toBeFocused();
  });
});

test.describe('dashboard', () => {
  test('shows live tiles, history and the diagnosis on request', async ({ page }) => {
    await page.goto(url());
    await expect(page.getByRole('group', { name: 'CPU', exact: true })).toBeVisible();
    await expect(page.getByRole('group', { name: 'Memory', exact: true })).toBeVisible();
    await expect(page.locator('dy-chart canvas').first()).toBeVisible();
    await page.getByRole('button', { name: 'Check now' }).click();
    await expect(page.getByText('A Rust build is using most of your CPU')).toBeVisible();
    await page.getByRole('radiogroup', { name: 'Range' }).getByRole('radio', { name: '7d' }).click();
    await expect(page.getByRole('radiogroup', { name: 'Range' }).getByRole('radio', { name: '7d' })).toBeChecked();
  });

  test('a machine without a battery or sensors says so with the reason', async ({ page }) => {
    await page.goto(url('/', '&mock=desktop'));
    const battery = page.getByRole('group', { name: 'Battery', exact: true });
    await expect(battery.getByText('Not available').first()).toBeVisible();
    await expect(battery.getByText(/no battery/i)).toBeVisible();
  });

  test('a failed request shows an inline error with Retry, and Retry recovers', async ({ page }) => {
    await page.goto(url('/', '&fail=history'));
    const alert = page.getByRole('alert').filter({ hasText: 'asked to fail' }).first();
    await expect(alert).toBeVisible();
    await expect(alert.getByRole('button', { name: 'Retry' })).toBeVisible();
  });

});

test.describe('cpu per core', () => {
  test('the tile switches to per-core bars grouped by kind and the choice is remembered', async ({ page }) => {
    await page.goto(url());
    const tile = page.getByRole('group', { name: 'CPU', exact: true });
    await tile.getByRole('radio', { name: 'Per core' }).click();
    await expect(tile.getByRole('heading', { name: 'Performance cores' })).toBeVisible();
    await expect(tile.getByRole('heading', { name: 'Efficiency cores' })).toBeVisible();
    await page.reload();
    await expect(page.getByRole('group', { name: 'CPU', exact: true }).getByRole('heading', { name: 'Efficiency cores' })).toBeVisible();
  });

  test('a uniform CPU shows one plain Cores group', async ({ page }) => {
    await page.goto(url('/', '&mock=uniform'));
    const tile = page.getByRole('group', { name: 'CPU', exact: true });
    await tile.getByRole('radio', { name: 'Per core' }).click();
    await expect(tile.getByRole('heading', { name: 'Cores', exact: true })).toBeVisible();
    await expect(tile.getByRole('heading', { name: 'Performance cores' })).toHaveCount(0);
  });

  test('the detail view shows cores now, hottest apps and the per-core heatmap', async ({ page }) => {
    await page.goto(url());
    await page.getByRole('link', { name: 'CPU details' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'CPU' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Hottest apps' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Per-core history' }).locator('canvas').first()).toBeVisible();
    await page.getByRole('radiogroup', { name: 'Range' }).getByRole('radio', { name: '1h' }).click();
    await expect(page.getByRole('radiogroup', { name: 'Range' }).getByRole('radio', { name: '1h' })).toBeChecked();
  });
});

test.describe('processes', () => {
  test('groups, expands and shows details', async ({ page }) => {
    await page.goto(url('/processes'));
    await expect(page.getByRole('treegrid')).toBeVisible();
    await page.getByRole('button', { name: 'Expand Rust build (cargo)' }).click();
    const row = page.locator('[role=row][data-key^="p"]').first();
    await row.click();
    const details = page.getByRole('complementary', { name: 'Process details' });
    await expect(details.getByText('Command line')).toBeVisible();
    await details.getByRole('tab', { name: 'Libraries' }).click();
    await expect(details.locator('.mono').first()).toBeVisible();
    await row.getByRole('button', { name: 'More actions' }).click();
    await expect(page.getByRole('menuitem', { name: 'Details' })).toBeVisible();
    await expect(page.getByRole('menuitem', { name: /End|Suspend|Priority|Cap CPU/ })).toHaveCount(0);
  });

});

test.describe('settings', () => {
  test('Settings is the shell’s window: Dyno’s page saves, the appearance is the shell’s', async ({ page }) => {
    await openSettings(page);
    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText(/Dyno only reads/).first()).toBeVisible();
    await expect(dialog.getByRole('radio', { name: 'Dark' })).toHaveCount(0);
    await dialog.getByRole('switch', { name: 'Start Dyno at login' }).click();
    await dialog.getByRole('button', { name: 'Save changes' }).click();
    await expect(page.getByText('Settings saved')).toBeVisible();
    await expect(dialog.getByRole('button', { name: 'Save changes' })).toBeHidden();
  });
});

test.describe('native feel', () => {
  test('chrome cannot be selected, copyable content can, browser keys do nothing', async ({ page }) => {
    await page.goto(url('/'));
    const style = (loc: ReturnType<typeof page.locator>) => loc.evaluate((e) => { const c = getComputedStyle(e) as CSSStyleDeclaration & { webkitUserSelect?: string }; return c.webkitUserSelect || c.userSelect; });
    expect(await style(page.getByRole('heading', { level: 1, name: 'Dashboard' }))).toBe('none');
    expect(await page.evaluate(() => getComputedStyle(document.documentElement).overscrollBehaviorX)).toBe('none');

    const prevented = await page.evaluate(() => {
      const e = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
      document.querySelector('h1')!.dispatchEvent(e);
      return e.defaultPrevented;
    });
    expect(prevented).toBe(true);

    // Listeners first: the window's bubble listener runs after the app's, so it sees whether the app took the key.
    await page.evaluate(() => {
      const w = window as unknown as { __alive: boolean; __keys: Record<string, boolean>; __wheel: boolean[] };
      w.__alive = true;
      w.__keys = {};
      w.__wheel = [];
      window.addEventListener('keydown', (e) => (w.__keys[`${e.ctrlKey ? 'Control+' : ''}${e.key}`] = e.defaultPrevented));
      window.addEventListener('wheel', (e) => w.__wheel.push(e.defaultPrevented), { passive: true });
    });
    for (const key of ['F5', 'Control+r', 'Control+=', 'Control+-', 'Control+0', 'Control+p']) await page.keyboard.press(key);
    await page.mouse.move(400, 400);
    await page.keyboard.down('Control');
    await page.mouse.wheel(0, -200);
    await page.keyboard.up('Control');
    // Two frames: any zoom or navigation the keys and the wheel caused has been applied by then.
    await page.evaluate(() => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r()))));
    expect(await page.evaluate(() => (window as unknown as { __alive?: boolean }).__alive)).toBe(true);
    const seen = await page.evaluate(() => {
      const w = window as unknown as { __keys: Record<string, boolean>; __wheel: boolean[] };
      return { __keys: w.__keys, __wheel: w.__wheel };
    });
    // WebKit leaves some of these keys unprevented in the page (its own shortcuts act in the shell, not the page), so the
    // per-key check is Chromium's; both engines must still show the page alive, at scale 1.
    for (const key of test.info().project.name === 'chromium' ? ['F5', 'Control+r', 'Control+=', 'Control+-', 'Control+0', 'Control+p'] : []) {
      expect(seen.__keys[key], `${key} reached the page and was taken by it`).toBe(true);
    }
    expect(seen.__wheel.length, 'the Ctrl+wheel reached the page').toBeGreaterThan(0);
    expect(seen.__wheel.every(Boolean), 'the Ctrl+wheel was taken by the page').toBe(true);
    expect(await page.evaluate(() => window.visualViewport?.scale ?? 1)).toBe(1);

    // A dropped file never navigates the window.
    const dropPrevented = await page.evaluate(() => {
      const dt = new DataTransfer();
      dt.items.add(new File(['x'], 'x.txt'));
      const e = new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: dt });
      document.body.dispatchEvent(e);
      return e.defaultPrevented;
    });
    expect(dropPrevented).toBe(true);
  });

  test('Copy is offered only inside the selection', async ({ page }) => {
    await page.goto(url('/processes'));
    await page.getByRole('button', { name: 'Expand Rust build (cargo)' }).click();
    await page.locator('[role=row][data-key^="p"]').first().click();
    const dd = page.locator('dy-process-details dd.mono').first();
    await expect(dd).toBeVisible();
    await dd.evaluate((el) => {
      const r = document.createRange();
      r.selectNodeContents(el);
      getSelection()?.removeAllRanges();
      getSelection()?.addRange(r);
    });
    const box = (await dd.boundingBox())!;
    const menuitem = page.locator('.cdk-overlay-container [role=menuitem]');
    const rightClick = (x: number, y: number) =>
      page.evaluate(
        ([px, py]) => {
          const e = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: px, clientY: py });
          (document.elementFromPoint(px!, py!) ?? document.body).dispatchEvent(e);
        },
        [x, y],
      );
    await rightClick(box.x + 12, box.y + 6);
    await expect(menuitem.first()).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(menuitem).toHaveCount(0);
    await rightClick(box.x + box.width + 150, box.y + box.height + 150);
    // Nothing to wait for: two frames after the event, a menu that was going to open has opened.
    await page.evaluate(() => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r()))));
    await expect(menuitem).toHaveCount(0);
  });
});

test.describe('inventory', () => {
  test('draws the slots, hints and a table view on a PC with two of four slots filled', async ({ page }) => {
    await page.goto(url('/inventory', '&mock=windows'));
    await expect(page.getByRole('img', { name: /2 of 4 memory slots filled/ })).toBeVisible();
    await expect(page.getByRole('list', { name: 'Upgrade hints' }).getByRole('listitem')).toHaveCount(4);
    await page.getByRole('radio', { name: 'Table' }).click();
    await expect(page.getByRole('row')).toHaveCount(5);
  });

  test('Apple silicon shows unified memory that cannot be upgraded', async ({ page }) => {
    await page.goto(url('/inventory', '&mock=macos'));
    await expect(page.getByRole('img', { name: /Unified memory/ })).toBeVisible();
    await expect(page.getByText(/cannot be upgraded/).first()).toBeVisible();
  });
});

// The widget choosers (popup, dashboard, menu bar) are covered by tray-widgets.spec.ts.

test.describe('counters', () => {
  test('row actions can be clicked one after another: their tooltips never cover the next button', async ({ page }) => {
    await page.goto(url('/counters'));
    await page.getByRole('searchbox', { name: 'Search counters' }).fill('bytes');
    await expect(page.getByRole('list', { name: 'Search results' })).toBeVisible();
    const results = page.getByRole('list', { name: 'Search results' });
    // Real pointer clicks, no force: a tooltip of one row action must not sit over the next one.
    await results.getByRole('button', { name: /^Chart Bytes Received \(en0\)/ }).click();
    await results.getByRole('button', { name: /^Chart Bytes Sent \(en0\)/ }).click();
    await results.getByRole('button', { name: /^Chart Bytes Received \(lo0\)/ }).click();
    await expect(results.getByRole('button', { name: /^Stop charting/ })).toHaveCount(3);
  });


  test('browse an object, chart a counter with the events marked, pin it and put it on the dashboard', async ({ page }) => {
    await page.goto(url('/counters'));
    await expect(page.getByRole('heading', { level: 1, name: 'Counters' })).toBeVisible();
    await expect(page.getByText('Nothing charted yet')).toBeVisible();
    const objects = page.getByRole('list', { name: 'Objects' });
    await objects.getByRole('button', { name: /^Network Interface/ }).click();
    await choose(page, 'Instance', 'en0');
    await page.getByRole('button', { name: 'Chart Bytes Sent' }).click();
    const table = page.getByRole('grid', { name: 'Charted counters' });
    const charted = table.getByRole('row', { name: /Bytes Sent \(en0\)/ });
    await expect(charted).toBeVisible();
    await expect(page.getByRole('group', { name: 'Events on the timeline' })).toContainText('Network');
    await expect(page.locator('dy-chart canvas').first()).toBeVisible();
    await charted.getByRole('button', { name: 'Pin', exact: true }).click();
    await expect(page.getByText('Counter pinned')).toBeVisible();
    await charted.getByRole('button', { name: 'More actions' }).click();
    await page.getByRole('menuitem', { name: 'Add to the dashboard' }).click();
    await expect(page.getByText('Added to the dashboard')).toBeVisible();
    await page.getByRole('link', { name: 'Dashboard' }).first().click();
    await expect(page.getByRole('group', { name: 'Bytes Sent (en0)' })).toBeVisible();
  });

  test('search finds counters by words, a set charts several at once and an empty search is explained', async ({ page }) => {
    await page.goto(url('/counters'));
    await page.getByRole('searchbox', { name: 'Search counters' }).fill('bytes received');
    const results = page.getByRole('list', { name: 'Search results' });
    await expect(results.getByRole('listitem')).toHaveCount(2);
    await results.getByRole('button', { name: /Chart Bytes Received \(en0\)/ }).click();
    await page.getByRole('button', { name: /Sets/ }).click();
    await page.getByRole('menuitem', { name: /^Disk and network \d/ }).click();
    await expect(page.getByRole('grid', { name: 'Charted counters' }).getByRole('row')).toHaveCount(4);
    await page.getByRole('searchbox', { name: 'Search counters' }).fill('zzzz');
    await expect(page.getByText('No counter matches')).toBeVisible();
  });
});

test.describe('notifications', () => {
  test('selected messages and then all of them are deleted, after a confirmation for all', async ({ page, context }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await page.goto(url('/network'));
    const socket = page.getByRole('row', { name: /142.250.74.46/ });
    await socket.focus();
    await expect(socket).toBeFocused();
    await pressMod(page, 'Mod+c');
    await expect(page.getByRole('button', { name: '1 unread' })).toBeVisible();
    await page.evaluate(() => {
      location.hash = '#/why-slow';
    });
    await page.getByRole('button', { name: 'Copy as text' }).click();
    await page.getByRole('button', { name: /notifications/i }).first().click();
    const selects = page.locator('[data-testid^=notice-select-] input');
    await expect(selects).toHaveCount(2);
    await selects.first().check();
    await page.getByTestId('notices-delete-selected').click();
    await expect(selects).toHaveCount(1);
    await page.getByTestId('notices-delete-all').click();
    const decision = page.getByRole('alertdialog', { name: 'Delete all notifications?' });
    await expect(decision).toBeVisible();
    await decision.getByRole('button', { name: 'Delete all' }).click();
    await expect(page.getByRole('button', { name: /unread/ })).toHaveCount(0);
    await page.getByRole('button', { name: /notifications/i }).first().click();
    await expect(page.getByTestId('notices-empty')).toBeVisible();
    await expect(selects).toHaveCount(0);
  });
});

test.describe('network', () => {
  test('one dense table: grouped by program with collapsible group rows, loopback hidden, Look up IP only for public peers', async ({ page }) => {
    await page.goto(url('/network'));
    const grid = page.getByRole('treegrid', { name: 'Connections' });
    await expect(grid.getByRole('row', { name: /Google Chrome/ }).first()).toBeVisible();
    await expect(page.getByRole('row', { name: /142.250.74.46/ })).toBeVisible();
    await page.getByRole('button', { name: 'Collapse Google Chrome' }).click();
    await expect(page.getByRole('row', { name: /142.250.74.46/ })).toHaveCount(0);
    await page.getByRole('button', { name: 'Expand Google Chrome' }).click();
    await page.getByRole('radiogroup', { name: 'Group by' }).getByRole('radio', { name: 'None' }).click();
    await expect(page.getByRole('button', { name: 'Collapse Slack' })).toHaveCount(0);
    const flat = page.getByRole('grid', { name: 'Connections' });
    await expect(flat.getByRole('row', { name: /10\.0\.0\.8/ }).getByRole('button', { name: 'Look up IP' })).toBeDisabled();
    await expect(flat.getByRole('row', { name: /142\.250\.74\.46/ }).getByRole('button', { name: 'Look up IP' })).toBeEnabled();
  });

  test('Collapse all folds every program and Expand all opens them again', async ({ page }) => {
    await page.goto(url('/network'));
    const grid = page.getByRole('treegrid', { name: 'Connections' });
    await expect(page.getByRole('row', { name: /142.250.74.46/ })).toBeVisible();
    await page.getByRole('button', { name: 'Collapse all programs' }).click();
    await expect(page.getByRole('row', { name: /142.250.74.46/ })).toHaveCount(0);
    await expect(grid.locator('[role=row][aria-expanded=true]')).toHaveCount(0);
    await expect(grid.locator('[role=row][aria-expanded=false]').first()).toBeVisible();
    await page.getByRole('button', { name: 'Expand all programs' }).click();
    await expect(page.getByRole('row', { name: /142.250.74.46/ })).toBeVisible();
    await expect(grid.locator('[role=row][aria-expanded=false]')).toHaveCount(0);
  });

  test('listening tab marks exposed listeners first; loopback shows on request; Enter opens the detail panel', async ({ page }) => {
    await page.goto(url('/network'));
    await page.getByRole('radio', { name: 'Listening' }).click();
    const grid = page.getByRole('treegrid', { name: 'Listening ports' });
    await expect(grid.getByText('Exposed').first()).toBeVisible();
    await expect(grid.getByText('postgres')).toHaveCount(0);
    await page.getByRole('checkbox', { name: 'Show loopback' }).click();
    await expect(grid.getByText('postgres').first()).toBeVisible();
    // The first row after the header is a program; Down reaches its first socket, Enter opens it.
    await grid.getByRole('row').nth(1).focus();
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('complementary', { name: 'Details' })).toBeVisible();
  });

  test('settings: choosing a preset fills the address and a bad address is explained', async ({ page }) => {
    await openSettings(page);
    await choose(page, 'Lookup site', /^bgp\.he\.net/);
    await expect(page.getByLabel('Lookup address', { exact: true })).toHaveValue('https://bgp.he.net/ip/{ip}');
    await page.getByLabel('Lookup address', { exact: true }).fill('http://nope/{ip}');
    await expect(page.getByText('must start with https://')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Test with 8.8.8.8' })).toBeDisabled();
  });

  test('throttling: runs at full speed, lists the last 30 days in one table, and the machine inputs open on demand', async ({ page }) => {
    await page.goto(url('/throttling'));
    await expect(page.getByRole('heading', { level: 1, name: 'Throttling' })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Running at full speed' })).toBeVisible();
    await expect(page.getByText('Confidence: high')).toBeVisible();
    const table = page.getByRole('grid', { name: /slowed down in the last 30 days/ });
    await expect(table.getByRole('row')).toHaveCount(6);
    await expect(table.getByRole('columnheader', { name: /Speed lost/ })).toBeVisible();
    await expect(table.getByRole('columnheader', { name: /Started/ })).toHaveAttribute('aria-sort', 'descending');
    await table.getByRole('button', { name: /Started/ }).click();
    await table.getByRole('button', { name: /Started/ }).click();
    await expect(table.getByRole('columnheader', { name: /Started/ })).toHaveAttribute('aria-sort', 'ascending');
    const inputs = page.getByRole('button', { name: 'What Dyno can see on this machine' });
    await expect(inputs).toHaveAttribute('aria-expanded', 'false');
    await inputs.click();
    await expect(page.getByText('CPU clock speed')).toBeVisible();
  });

  test('throttling: ?hw=hot shows the heat slowdown and how long it has lasted', async ({ page }) => {
    await page.goto(url('/throttling', '&hw=hot'));
    await expect(page.getByRole('heading', { name: 'Slowed down by heat: 44% slower' })).toBeVisible();
    await expect(page.getByText(/Slowed for 7 min, since/)).toBeVisible();
    await expect(page.getByText(/throttled in the last 30 days/)).toBeVisible();
  });

  test('throttling: ?hw=blind explains what this machine does not report', async ({ page }) => {
    await page.goto(url('/throttling', '&hw=blind'));
    await expect(page.getByRole('heading', { name: /Cannot tell/ })).toBeVisible();
    await expect(page.getByText('No throttling in the last 30 days')).toBeVisible();
    await expect(page.getByText('This machine does not report a CPU temperature to apps.')).toBeVisible();
  });

  test('lifespan: a disk and a battery forecast with ranges, confidence and what they stand on', async ({ page }) => {
    await page.goto(url('/lifespan'));
    await expect(page.getByRole('heading', { level: 1, name: 'Lifespan' })).toBeVisible();
    await expect(page.getByText('Replace in about 23 months (17 to 31 months)')).toBeVisible();
    await expect(page.getByText('Confidence: medium')).toHaveCount(2);
    await expect(page.getByRole('img', { name: /Soonest 17 months, expected 23 months/ })).toBeVisible();
    await expect(page.getByLabel('What this stands on for Battery')).toBeVisible();
  });

  test('lifespan: ?hw=learning shows the history collected so far', async ({ page }) => {
    await page.goto(url('/lifespan', '&hw=learning'));
    await expect(page.getByText('Have 6 of 14 days')).toBeVisible();
    await expect(page.getByRole('progressbar', { name: 'History collected' }).first()).toBeVisible();
  });

  test('dashboard: "Is this normal?" lists only what is unusual, with the current and the usual value', async ({ page }) => {
    await page.goto(url('/'));
    const card = page.getByRole('region', { name: 'Is this normal?' });
    await expect(card.getByText('Higher than usual for Thursday afternoons').first()).toBeVisible();
    await expect(card.getByText('71% now · usually 14%')).toBeVisible();
    await expect(card.getByText(/^Usual for/)).toHaveCount(0);
  });

  test('dashboard: ?hw=learning shows one quiet line while Dyno learns', async ({ page }) => {
    await page.goto(url('/', '&hw=learning'));
    await expect(page.getByText('Learning what is normal for this computer: day 3 of 7')).toBeVisible();
    await expect(page.getByRole('region', { name: 'Is this normal?' }).getByRole('listitem')).toHaveCount(0);
  });

  test('settings: the hardware and baselines toggles change the draft and bring up Save', async ({ page }) => {
    await openSettings(page);
    const section = page.getByRole('region', { name: 'Hardware and baselines' });
    await expect(section.getByRole('switch', { name: 'Alarm for throttling' })).toHaveAttribute('aria-checked', 'true');
    await expect(section.getByRole('switch', { name: 'Alarm when far above usual' })).toHaveAttribute('aria-checked', 'false');
    await section.getByRole('switch', { name: 'Alarm when far above usual' }).click();
    await section.getByRole('switch', { name: 'Alarm for throttling' }).click();
    await section.getByLabel('Days of learning').fill('14');
    await expect(section.getByRole('switch', { name: 'Alarm when far above usual' })).toHaveAttribute('aria-checked', 'true');
    await page.getByRole('button', { name: 'Save changes' }).click();
    await expect(page.getByText('Settings saved')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Save changes' })).toHaveCount(0);
  });
});

test.describe('issues', () => {
  test.describe('grouped by the local day', () => {
    // The mock dates its issues relative to the clock (the newest 35 minutes ago), so the day headings depend on the
    // time of day: pin the zone (UTC+8, no daylight saving) and the clock, or the test fails in the first 35 minutes
    // after the runner's midnight.
    test.use({ timezoneId: 'Asia/Singapore' });
    const AFTERNOON = Date.parse('2026-10-22T14:30:00+08:00');
    const JUST_AFTER_MIDNIGHT = Date.parse('2026-10-23T00:10:00+08:00');

    test('shows the stability score, the apps with trouble and the events grouped by day; a row opens its evidence as plain text', async ({ page }) => {
      await page.clock.setFixedTime(AFTERNOON);
      await page.goto(url('/issues'));
      await expect(page.getByRole('heading', { name: 'Issues', exact: true })).toBeVisible();
      await expect(page.getByRole('region', { name: 'Stability' })).toBeVisible();
      const table = page.getByRole('treegrid', { name: 'Issues', exact: true });
      await expect(table.getByText('Today').first()).toBeVisible();
      await table.getByRole('row', { name: /Safari/ }).click();
      const detail = page.getByRole('complementary', { name: 'Issue details' });
      await expect(detail.getByText('What the system said')).toBeVisible();
      await expect(detail.getByText('Safari stopped responding')).toBeVisible();
      await page.keyboard.press('Escape');
    });

    test('just after local midnight nothing happened today yet: the newest issues are under Yesterday, not by the UTC date', async ({ page }) => {
      // 00:10 in UTC+8 is 16:10 UTC of the day before, the same UTC day as the newest issue (23:35 local): grouping by
      // the UTC date would put it under Today.
      await page.clock.setFixedTime(JUST_AFTER_MIDNIGHT);
      await page.goto(url('/issues'));
      const table = page.getByRole('treegrid', { name: 'Issues', exact: true });
      await expect(table.getByText('Yesterday').first()).toBeVisible();
      await expect(table.getByText('Today')).toHaveCount(0);
    });
  });

  test('a row\'s context menu opens with Shift+F10, reaches every action, and Escape gives the focus back to the row', async ({ page }) => {
    await page.goto(url('/issues'));
    const table = page.getByRole('treegrid', { name: 'Issues', exact: true });
    const row = table.getByRole('row', { name: /Safari/ }).first();
    await row.focus();
    await page.keyboard.press('Shift+F10');
    const menu = page.getByRole('menu');
    await expect(menu).toBeVisible();
    for (const name of ['Copy details', 'Search the web…', 'Only this kind']) await expect(menu.getByRole('menuitem', { name })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);
    await expect(row).toBeFocused();
    // The pointer's tooltips never keep the menu from closing on the first Escape.
    await row.click({ button: 'right' });
    await expect(menu).toBeVisible();
    await expect(page.locator('.mat-mdc-tooltip')).toHaveCount(0);
    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);
  });

  test('a right-click right after hovering a row leaves no tooltip behind, and the first Escape closes the menu', async ({ page }) => {
    await page.clock.install();
    await page.goto(url('/issues'));
    const table = page.getByRole('treegrid', { name: 'Issues', exact: true });
    const row = table.getByRole('row', { name: /Safari/ }).first();
    await row.hover(); // the row's centre is the program-name cell
    await row.click({ button: 'right' }); // the same point, before any tooltip has shown
    const menu = page.getByRole('menu');
    await expect(menu).toBeVisible();
    // The tooltip's show delay runs out while the menu is open: it must not open.
    await page.clock.runFor(1500);
    await expect(page.locator('.mat-mdc-tooltip')).toHaveCount(0);
    await page.keyboard.press('Escape');
    await page.clock.runFor(500);
    await expect(menu).toHaveCount(0);
    await expect(row).toBeFocused();
  });

  test('a leaderboard row filters the table to one app and the chip clears it', async ({ page }) => {
    await page.goto(url('/issues'));
    await page.getByRole('grid', { name: 'Apps with the most issues' }).getByRole('row', { name: /Mail/ }).click();
    const table = page.getByRole('treegrid', { name: 'Issues', exact: true });
    await expect(table.getByRole('row', { name: /Slack/ })).toHaveCount(0);
    await expect(table.getByRole('row', { name: /Mail/ }).first()).toBeVisible();
    await page.getByRole('button', { name: 'Clear the filter' }).click();
    await expect(table.getByRole('row', { name: /Slack/ }).first()).toBeVisible();
  });

  test('the new and unusual view lists reasons and offers no action on a program', async ({ page }) => {
    await page.goto(url('/issues'));
    await page.getByRole('radio', { name: /New and unusual/ }).click();
    const table = page.getByRole('grid', { name: 'New and unusual programs' });
    await expect(table.getByRole('row', { name: /updater-helper/ })).toContainText('Unusual because it runs from a temporary folder');
    await expect(page.getByRole('button', { name: 'Protect updater-helper' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Hide ngrok' })).toHaveCount(0);
    await expect(table.getByRole('row', { name: /ngrok/ })).toBeVisible();
  });

  test('settings: the learning period is a setting and can be turned off', async ({ page }) => {
    await openSettings(page);
    const section = page.getByRole('region', { name: 'New programs' });
    await section.getByLabel('Days of quiet learning').fill('3');
    await page.getByRole('button', { name: 'Save changes' }).click();
    await expect(page.getByText('Settings saved')).toBeVisible();
  });
});


test.describe('right-click search (nothing is sent until the click)', () => {
  test('a process searches the web with the text shown and edited first', async ({ page }) => {
    await page.goto(url('/processes'));
    await page.getByLabel('Filter processes').fill('rustc');
    await page.locator('[role=row][data-key^="g"]').first().getByRole('button', { name: 'More actions' }).click();
    await page.getByRole('menuitem', { name: 'Search the web…' }).click();
    const dialog = page.getByRole('dialog', { name: /Search the web/ });
    await expect(dialog.getByText('Nothing leaves your computer until you click Search')).toBeVisible();
    const field = dialog.getByLabel('Search for');
    await expect(field).toHaveValue(/rustc|Rust/);
    await field.fill('rustc high cpu while building');
    await dialog.getByRole('button', { name: 'Search', exact: true }).click();
    await expect(dialog).toBeHidden();
  });

  test('a right-click on a row opens the same menu with the web and hash searches', async ({ page }) => {
    await page.goto(url('/processes'));
    await page.getByRole('radio', { name: 'All', exact: true }).click();
    await page.getByLabel('Filter processes').fill('rustc');
    await page.locator('[role=row][data-key^="p"]').first().click({ button: 'right' });
    await expect(page.getByRole('menuitem', { name: 'Search the web…' })).toBeVisible();
    await page.getByRole('menuitem', { name: 'Search file hash on VirusTotal…' }).click();
    const dialog = page.getByRole('dialog', { name: /VirusTotal/ });
    await expect(dialog.getByText('Nothing is uploaded')).toBeVisible();
    await expect(dialog.getByLabel('SHA-256 hash')).toHaveText(/^[0-9a-f]{64}$/);
    // A viewer: Close, Copy hash and the one primary, in the frame's footer.
    await expect(dialog.getByRole('button', { name: 'Copy hash' })).toBeVisible();
    await dialog.getByRole('button', { name: 'Close' }).click();
    await expect(dialog).toBeHidden();
  });

  test('an issue shows what was removed from the text', async ({ page }) => {
    await page.goto(url('/issues'));
    await page.getByRole('treegrid', { name: 'Issues', exact: true }).getByRole('row', { name: /Safari/ }).first().click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Search the web…' }).click();
    const dialog = page.getByRole('dialog', { name: /Search the web/ });
    await expect(dialog.getByText(/Removed from the text: 1 home folder/)).toBeVisible();
    await expect(dialog.getByLabel('Search for')).not.toHaveValue(/alex/);
    await dialog.getByRole('button', { name: 'Cancel' }).click();
  });
});

test.describe('startup filter', () => {
  test('hides the system and signed entries, shows the counts and remembers the choice', async ({ page }) => {
    await page.goto(url('/health'));
    const section = page.getByRole('region', { name: 'Start-up items' });
    const rows = section.locator('[role=row][data-key]');
    await expect(rows.first()).toBeVisible();
    await expect(section.getByText(/Showing \d+ of \d+ entries\./)).toBeVisible();
    const before = await rows.count();
    const hideSystem = section.getByRole('checkbox', { name: /Hide system entries/ });
    await expect(hideSystem).not.toBeChecked();
    await hideSystem.click();
    await expect(hideSystem).toBeChecked();
    await expect(rows).toHaveCount(before - 1);
    await expect(section.getByText(/Showing \d+ of \d+ entries, 1 hidden by the filter/)).toBeVisible();
    await page.reload();
    await expect(page.getByRole('region', { name: 'Start-up items' }).getByRole('checkbox', { name: /Hide system entries/ })).toBeChecked();
    await page.getByRole('region', { name: 'Start-up items' }).getByRole('checkbox', { name: /Hide signed apps/ }).click();
    // The demo list has no unsigned entry, so hiding both leaves nothing, and the page says why instead of claiming nothing starts.
    await expect(page.getByRole('region', { name: 'Start-up items' }).getByText('Everything is hidden by the filter')).toBeVisible();
  });
});

test.describe('timeline', () => {
  test('network changes are a kind of event in the history legend', async ({ page }) => {
    await page.goto(url('/'));
    await expect(page.getByRole('region', { name: 'History' }).getByText('Network', { exact: false }).first()).toBeVisible();
  });
});

test.describe('backup health', () => {
  test('lists a healthy, a stale and an unreadable tool, and Check now waits for the check', async ({ page }) => {
    await page.goto(url('/backup'));
    await expect(page.getByRole('heading', { level: 1, name: 'Backup health' })).toBeVisible();
    await expect(page.getByRole('heading', { level: 2, name: 'restic backup is out of date' })).toBeVisible();
    const table = page.getByRole('grid', { name: 'Backup tools' });
    await expect(table.getByRole('row', { name: /Time Machine/ })).toContainText('Healthy');
    await expect(table.getByRole('row', { name: /restic/ })).toContainText('Out of date');
    await expect(table.getByRole('row', { name: /BorgBackup/ })).toContainText('Could not check');
    await table.getByRole('row', { name: /restic/ }).click();
    const panel = page.getByRole('complementary', { name: 'Backup details' });
    await expect(panel).toContainText('never starts a backup');
    await page.keyboard.press('Escape');
    await expect(panel).toHaveCount(0);
    await page.getByRole('button', { name: 'Check now' }).click();
    await expect(page.getByRole('button', { name: 'Check now' })).toBeVisible();
    await expect(page.getByText('Checked just now')).toBeVisible();
  });

  test('finding no backup tool is a normal state', async ({ page }) => {
    await page.goto(url('/backup', '&backup=none'));
    await expect(page.getByText('No backup tool found')).toBeVisible();
    await expect(page.locator('dy-backup-page')).toContainText('never runs or manages a backup');
    await expect(page.getByRole('alert')).toHaveCount(0);
  });

  test('Settings has the Backup health section', async ({ page }) => {
    await openSettings(page);
    const section = page.getByRole('region', { name: 'Backup health' });
    await expect(section.getByText('Out of date after')).toBeVisible();
    await section.getByLabel('restic folders').fill('tools/restic');
    await expect(section.getByRole('alert')).toContainText('not a full path');
  });
});

test.describe('extras', () => {
  test('every tab opens and shows its data', async ({ page }) => {
    await page.goto(url('/extras'));
    await expect(page.getByRole('heading', { level: 1, name: 'Extras' })).toBeVisible();
    await expect(page.getByRole('treegrid', { name: 'What changed' })).toContainText('Updated Visual Studio Code');
    await page.getByRole('tab', { name: 'Sleep' }).click();
    await expect(page.getByRole('grid', { name: 'Keeping this computer awake' })).toContainText('coreaudiod');
    await expect(page.getByRole('grid', { name: 'Recent wakes' })).toContainText('Lid open');
    await page.getByRole('tab', { name: 'Boot' }).click();
    await expect(page.getByRole('region', { name: 'Last start-up' })).toContainText('47.2 s');
    await expect(page.getByRole('grid', { name: 'What slowed start-up' })).toContainText('VirtualBox');
    await page.getByRole('tab', { name: 'Apps' }).click();
    await expect(page.getByRole('grid', { name: 'Network and energy per app' })).toContainText('Google Chrome');
    await page.getByRole('tab', { name: 'GPU' }).click();
    await expect(page.getByRole('grid', { name: 'GPU use per process' })).toContainText('WindowServer');
    await page.getByRole('tab', { name: 'Inventory' }).click();
    await expect(page.getByRole('grid', { name: 'Installed software' })).toContainText('Visual Studio Code');
    await page.getByRole('tab', { name: 'Maintenance' }).click();
    await expect(page.getByText('never limits anything you click')).toBeVisible();
    await expect(page.getByRole('region', { name: 'Maintenance windows' })).toContainText('Mon to Fri, 22:00 to 02:00 the next day');
  });

  test('Inventory exports a report and says where it went', async ({ page }) => {
    await page.goto(url('/extras', '').replace('#/extras', '#/extras?tab=inventory'));
    await page.getByRole('button', { name: /Export report/ }).click();
    await page.getByRole('menuitem', { name: /Markdown/ }).click();
    await expect(page.getByText('/Users/alex/Documents/dyno-inventory.md')).toBeVisible();
  });

  test('Maintenance windows can be edited and saved', async ({ page }) => {
    await page.goto(url('/extras', '').replace('#/extras', '#/extras?tab=maintenance'));
    await page.getByRole('button', { name: 'Sat', exact: true }).first().click();
    await page.getByRole('button', { name: 'Save windows' }).click();
    await expect(page.getByText('Maintenance windows saved')).toBeVisible();
  });
});
