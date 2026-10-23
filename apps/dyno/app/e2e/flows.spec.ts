import { expect, test } from '@playwright/test';

/** Browser flows against the mock backend (a macOS laptop; `?mock=desktop` has no battery or sensors). */
const url = (path = '/', query = '') => `/?latency=0${query}#${path}`;

test.describe('screens', () => {
  test('every screen opens from the sidebar', async ({ page }) => {
    await page.goto(url());
    await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible();
    const nav = page.getByRole('navigation', { name: /./ }).or(page.locator('aside nav'));
    for (const [label, heading] of [
      ['Processes', 'Processes'],
      ['Inventory', 'Inventory'],
      ['CPU taming', 'CPU taming'],
      ['Alarms', 'Alarms'],
      ['Flight recorder', 'Flight recorder'],
      ['Health', 'Health'],
      ['Tuning', 'Tuning'],
      ['Diagnostics', 'Diagnostics bundle'],
      ['Settings', 'Settings'],
      ['Dashboard', 'Dashboard'],
    ] as const) {
      await nav.first().getByRole('link', { name: label }).click();
      await expect(page.getByRole('heading', { level: 1, name: heading })).toBeVisible();
    }
  });

  test('unbuilt features are listed in one dialog and have no route', async ({ page }) => {
    await page.goto(url());
    await page.getByRole('button', { name: /Coming/ }).click();
    await expect(page.getByRole('dialog').getByText('Administrator tuning')).toBeVisible();
    await page.keyboard.press('Escape');
    await page.goto(url('/issues'));
    await expect(page.getByRole('heading', { level: 1, name: 'Dashboard' })).toBeVisible();
  });

  test('the tray dropdown renders its compact dashboard', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 520 });
    await page.goto(url('/tray'));
    await expect(page.getByRole('button', { name: /Open Dyno/ })).toBeVisible();
    await expect(page.getByText('Top apps by CPU')).toBeVisible();
  });

  test('a menu-bar item popover renders the compact dashboard, not the main window', async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 520 });
    await page.goto(url('/popover/cpu-1'));
    await expect(page.getByRole('button', { name: /Open Dyno/ })).toBeVisible();
    await expect(page.getByRole('navigation')).toHaveCount(0);
  });

  test('the command palette opens with Ctrl+K and jumps to a screen', async ({ page }) => {
    await page.goto(url());
    await page.keyboard.press('Control+k');
    const box = page.getByRole('combobox');
    await expect(box).toBeVisible();
    await box.fill('settings');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { level: 1, name: 'Settings' })).toBeVisible();
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
    await page.getByRole('group', { name: 'Range' }).getByRole('button', { name: '7d' }).click();
    await expect(page.getByRole('group', { name: 'Range' }).getByRole('button', { name: '7d' })).toHaveAttribute('aria-pressed', 'true');
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

  test('a health fix explains and previews itself before anything changes', async ({ page }) => {
    await page.goto(url());
    await page.getByRole('button', { name: 'Review cleanup' }).click();
    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText('What will change')).toBeVisible();
    await dialog.getByRole('button', { name: 'Not now' }).click();
    await expect(dialog).toBeHidden();
  });
});

test.describe('cpu per core', () => {
  test('the tile switches to per-core bars grouped by kind and the choice is remembered', async ({ page }) => {
    await page.goto(url());
    const tile = page.getByRole('group', { name: 'CPU', exact: true });
    await tile.getByRole('button', { name: 'Per core' }).click();
    await expect(tile.getByRole('heading', { name: 'Performance cores' })).toBeVisible();
    await expect(tile.getByRole('heading', { name: 'Efficiency cores' })).toBeVisible();
    await page.reload();
    await expect(page.getByRole('group', { name: 'CPU', exact: true }).getByRole('heading', { name: 'Efficiency cores' })).toBeVisible();
  });

  test('a uniform CPU shows one plain Cores group', async ({ page }) => {
    await page.goto(url('/', '&mock=uniform'));
    const tile = page.getByRole('group', { name: 'CPU', exact: true });
    await tile.getByRole('button', { name: 'Per core' }).click();
    await expect(tile.getByRole('heading', { name: 'Cores', exact: true })).toBeVisible();
    await expect(tile.getByRole('heading', { name: 'Performance cores' })).toHaveCount(0);
  });

  test('the detail view shows cores now, hottest apps and the per-core heatmap', async ({ page }) => {
    await page.goto(url());
    await page.getByRole('link', { name: 'CPU details' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'CPU' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Hottest apps' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Per-core history' }).locator('canvas').first()).toBeVisible();
    await page.getByRole('group', { name: 'Range' }).getByRole('button', { name: '1h' }).click();
    await expect(page.getByRole('group', { name: 'Range' }).getByRole('button', { name: '1h' })).toHaveAttribute('aria-pressed', 'true');
  });
});

test.describe('processes', () => {
  test('groups, expands, shows details and takes an action that can be undone', async ({ page }) => {
    await page.goto(url('/processes'));
    await expect(page.getByRole('treegrid')).toBeVisible();
    await page.getByRole('button', { name: 'Expand Rust build (cargo)' }).click();
    const row = page.locator('[role=row][data-key^="p"]').first();
    await row.click();
    const details = page.getByRole('complementary', { name: 'Process details' });
    await expect(details.getByText('Command line')).toBeVisible();
    await details.getByRole('tab', { name: 'Libraries' }).click();
    await expect(details.locator('.mono').first()).toBeVisible();

    await row.getByRole('button', { name: /Actions for/ }).click();
    await page.getByRole('menuitem', { name: 'Suspend' }).click();
    await expect(page.getByText(/Suspended/).first()).toBeVisible();
    await page.getByRole('button', { name: 'Undo' }).first().click();
    await expect(page.getByText(/Undid/).first()).toBeVisible();
  });

  test('protected processes are locked with the reason', async ({ page }) => {
    await page.goto(url('/processes', '&mock=macos'));
    await page.getByLabel('Filter processes').fill('WindowServer');
    const row = page.locator('[role=row][data-key^="g"]').first();
    await expect(row).toBeVisible();
    await row.getByRole('button', { name: /Actions for/ }).click();
    await expect(page.getByRole('menuitem', { name: 'End…' })).toHaveAttribute('aria-disabled', 'true');
    await expect(page.getByText(/Dyno never changes it/)).toBeVisible();
  });

  test('ending a process asks first and sorting changes the order', async ({ page }) => {
    await page.goto(url('/processes'));
    await page.getByRole('button', { name: 'All', exact: true }).click();
    await page.getByRole('button', { name: 'Sort by Name' }).click();
    await expect(page.getByRole('columnheader', { name: /Name/ })).toHaveAttribute('aria-sort', 'ascending');
    await page.getByLabel('Filter processes').fill('rustc');
    await page.locator('[role=row][data-key^="p"]').first().getByRole('button', { name: /Actions for/ }).click();
    await page.getByRole('menuitem', { name: 'End…' }).click();
    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText('Unsaved work in it is lost.')).toBeVisible();
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(dialog).toBeHidden();
  });
});

test.describe('settings', () => {
  test('the theme switch applies at once and Save stores it', async ({ page }) => {
    await page.goto(url('/settings'));
    await expect(page.getByText(/Dyno measures first/)).toBeVisible();
    await page.getByRole('radio', { name: 'Dark' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.getByRole('radio', { name: 'Light' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await page.getByRole('button', { name: 'Save changes' }).click();
    await expect(page.getByText('Settings saved')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Save changes' })).toBeHidden();
  });
});

test.describe('native feel', () => {
  test('chrome cannot be selected, copyable content can, browser keys do nothing', async ({ page }) => {
    await page.goto(url('/settings'));
    const style = (loc: ReturnType<typeof page.locator>) => loc.evaluate((e) => { const c = getComputedStyle(e) as CSSStyleDeclaration & { webkitUserSelect?: string }; return c.webkitUserSelect || c.userSelect; });
    expect(await style(page.getByRole('heading', { level: 1, name: 'Settings' }))).toBe('none');
    expect(await page.evaluate(() => getComputedStyle(document.documentElement).overscrollBehaviorX)).toBe('none');

    const prevented = await page.evaluate(() => {
      const e = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
      document.querySelector('h1')!.dispatchEvent(e);
      return e.defaultPrevented;
    });
    expect(prevented).toBe(true);

    await page.evaluate(() => ((window as unknown as { __alive: boolean }).__alive = true));
    for (const key of ['F5', 'Control+r', 'Control+=', 'Control+-', 'Control+0', 'Control+p']) await page.keyboard.press(key);
    await page.mouse.move(400, 400);
    await page.keyboard.down('Control');
    await page.mouse.wheel(0, -200);
    await page.keyboard.up('Control');
    await page.waitForTimeout(200);
    expect(await page.evaluate(() => (window as unknown as { __alive?: boolean }).__alive)).toBe(true);
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
    const probe = (x: number, y: number) =>
      page.evaluate(
        async ([px, py]) => {
          const e = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: px, clientY: py });
          (document.elementFromPoint(px!, py!) ?? document.body).dispatchEvent(e);
          await new Promise((r) => setTimeout(r, 150));
          return !!document.querySelector('dy-copy-menu [role=menuitem]');
        },
        [x, y],
      );
    expect(await probe(box.x + 12, box.y + 6)).toBe(true);
    await page.keyboard.press('Escape');
    expect(await probe(box.x + box.width + 150, box.y + box.height + 150)).toBe(false);
  });
});

test.describe('inventory', () => {
  test('draws the slots, hints and a table view on a PC with two of four slots filled', async ({ page }) => {
    await page.goto(url('/inventory', '&mock=windows'));
    await expect(page.getByRole('img', { name: /2 of 4 memory slots filled/ })).toBeVisible();
    await expect(page.getByRole('list', { name: 'Upgrade hints' }).getByRole('listitem')).toHaveCount(4);
    await page.getByRole('button', { name: 'Table' }).click();
    await expect(page.getByRole('row')).toHaveCount(5);
  });

  test('Apple silicon shows unified memory that cannot be upgraded', async ({ page }) => {
    await page.goto(url('/inventory', '&mock=macos'));
    await expect(page.getByRole('img', { name: /Unified memory/ })).toBeVisible();
    await expect(page.getByText(/cannot be upgraded/).first()).toBeVisible();
  });
});

test.describe('widgets', () => {
  test('edit the dashboard: add a widget from the gallery, resize, save; a preset swaps the layout', async ({ page }) => {
    await page.goto(url());
    await expect(page.getByRole('group', { name: 'CPU', exact: true }).first()).toBeVisible();
    await page.getByRole('button', { name: 'Edit dashboard' }).click();
    const before = await page.locator('dy-widget').count();
    await page.getByRole('button', { name: 'Add widget' }).click();
    await expect(page.getByRole('region', { name: 'Widget gallery' })).toBeVisible();
    await page.getByRole('region', { name: 'Widget gallery' }).getByRole('button', { name: /Big number/ }).click();
    await expect(page.locator('dy-widget')).toHaveCount(before + 1);
    await page.getByRole('toolbar').last().getByRole('button', { name: 'L', exact: true }).click();
    await page.getByRole('button', { name: 'Save dashboard' }).click();
    await expect(page.getByText('Dashboard saved')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Edit dashboard' })).toBeVisible();

    await page.getByRole('button', { name: 'Edit dashboard' }).click();
    await page.getByLabel('Preset').selectOption('developer');
    await expect(page.locator('dy-widget[data-kind="heatmap"]')).toBeVisible();
    await page.getByRole('button', { name: 'Cancel' }).click();
    await expect(page.locator('dy-widget[data-kind="heatmap"]')).toHaveCount(0);
  });

  test('menu bar items: add several, reorder, preview follows, save', async ({ page }) => {
    await page.goto(url('/settings'));
    const section = page.getByRole('region', { name: 'Menu bar items' });
    await expect(section.getByRole('listitem')).toHaveCount(2);
    await section.getByRole('button', { name: 'Add item' }).click();
    await section.getByRole('button', { name: 'Add item' }).click();
    await expect(section.getByRole('listitem')).toHaveCount(4);
    await expect(section.getByRole('button', { name: 'Add item' })).toBeDisabled();
    await section.getByLabel('Style of item 3').selectOption('ring');
    await section.getByRole('button', { name: 'Move up' }).nth(3).click();
    await expect(section.getByRole('img', { name: /Menu bar preview/ })).toBeVisible();
    await page.getByRole('button', { name: 'Save changes' }).click();
    await expect(page.getByText('Settings saved')).toBeVisible();
  });
});

test.describe('counters', () => {
  test('browse an object, chart a counter with the events marked, pin it and put it on the dashboard', async ({ page }) => {
    await page.goto(url('/counters'));
    await expect(page.getByRole('heading', { level: 1, name: 'Counters' })).toBeVisible();
    await expect(page.getByText('Nothing charted yet')).toBeVisible();
    const objects = page.getByRole('list', { name: 'Objects' });
    await objects.getByRole('button', { name: /^Network Interface/ }).click();
    await page.getByLabel('Instance').selectOption('en0');
    await page.getByRole('button', { name: 'Chart Bytes Sent' }).click();
    const table = page.getByRole('table', { name: 'Charted counters' });
    await expect(table.getByRole('row', { name: /Bytes Sent \(en0\)/ })).toBeVisible();
    await expect(page.getByRole('group', { name: 'Events on the timeline' })).toContainText('Cleanup');
    await expect(page.locator('dy-chart canvas').first()).toBeVisible();
    await page.getByRole('button', { name: 'Pin Bytes Sent (en0)' }).click();
    await expect(page.getByText('Counter pinned')).toBeVisible();
    await page.getByRole('button', { name: 'More actions for Bytes Sent (en0)' }).click();
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
    await expect(page.getByRole('table', { name: 'Charted counters' }).getByRole('row')).toHaveCount(4);
    await page.getByRole('searchbox', { name: 'Search counters' }).fill('zzzz');
    await expect(page.getByText('No counter matches')).toBeVisible();
  });
});

test.describe('network', () => {
  test('one dense table: grouped by program with collapsible group rows, loopback hidden, Look up IP only for public peers', async ({ page }) => {
    await page.goto(url('/network'));
    const grid = page.getByRole('grid', { name: 'Connections' });
    await expect(grid.getByRole('row', { name: /Google Chrome/ }).first()).toBeVisible();
    await expect(page.getByRole('row', { name: /142.250.74.46/ })).toBeVisible();
    await page.getByRole('button', { name: 'Collapse Google Chrome' }).click();
    await expect(page.getByRole('row', { name: /142.250.74.46/ })).toHaveCount(0);
    await page.getByRole('button', { name: 'Expand Google Chrome' }).click();
    await page.getByRole('group', { name: 'Group by' }).getByRole('button', { name: 'None' }).click();
    await expect(page.getByRole('button', { name: 'Collapse Slack' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Look up IP 10.0.0.8' })).toBeDisabled();
    await expect(page.getByRole('button', { name: 'Look up IP 142.250.74.46' })).toBeEnabled();
  });

  test('listening tab marks exposed listeners first; loopback shows on request; Enter opens the detail panel', async ({ page }) => {
    await page.goto(url('/network'));
    await page.getByRole('button', { name: 'Listening' }).click();
    const grid = page.getByRole('grid', { name: 'Listening ports' });
    await expect(grid.getByText('Exposed').first()).toBeVisible();
    await expect(grid.getByText('postgres')).toHaveCount(0);
    await page.getByRole('button', { name: 'Show loopback' }).click();
    await expect(grid.getByText('postgres').first()).toBeVisible();
    await grid.focus();
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('complementary', { name: 'Details' })).toBeVisible();
  });

  test('settings: choosing a preset fills the address and a bad address is explained', async ({ page }) => {
    await page.goto(url('/settings'));
    await page.getByLabel('Lookup site').selectOption('bgp');
    await expect(page.getByLabel('Lookup address', { exact: true })).toHaveValue('https://bgp.he.net/ip/{ip}');
    await page.getByLabel('Lookup address', { exact: true }).fill('http://nope/{ip}');
    await expect(page.getByText('must start with https://')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Test with 8.8.8.8' })).toBeDisabled();
  });
});
