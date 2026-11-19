import { expect, Locator, Page } from '@playwright/test';
import { expectMenuAnchoredTo } from '../../../../ui/projects/paddock-e2e/src/ready.ts';

/** Debrief's in-app menu bar (Windows, Linux and the browser build). */
export function menubar(page: Page): Locator {
  return page.getByRole('menubar', { name: 'Application menu' });
}

/** An item (plain or checkable) of an open menu, by its exact label. */
export function menuItem(scope: Page | Locator, name: string | RegExp): Locator {
  const opts = { name, exact: typeof name === 'string' };
  return scope.getByRole('menuitem', opts).or(scope.getByRole('menuitemcheckbox', opts));
}

/**
 * A menu by its accessible name (exact for a string), or every open menu without one: for checks that a menu is closed, open, checked or says
 * something. Specs find menus only here (the lint rule `paddock/e2e-transient-wait`); before pressing in one, wait for its place with `anchoredMenu`.
 */
export function menuNamed(page: Page, name?: string | RegExp): Locator {
  return name === undefined ? page.getByRole('menu') : page.getByRole('menu', { name, exact: typeof name === 'string' });
}

/**
 * How far a CDK menu may sit from its trigger on purpose: `flush` (menu-bar menus and submenus, 0 px: 2 px for rounding) or `popup` (the
 * activity bar's fly-outs, 6 px, and button menus such as the filter bar's More, 4 px: 8 px). An unplaced menu is hundreds of px away or covers
 * its trigger, so either bound tells it apart.
 */
const MENU_OFFSET_PX = { flush: 2, popup: 8 } as const;

/**
 * The menu `name` once it is in its place, opened by `anchor` (a button, a menu-bar or submenu item, a right-clicked element, the row of a
 * Shift+F10). The caller opens it (click, keys, right-click); this waits until it is placed, so a press lands on the item it aims at (a menu
 * drawn before it is positioned moves under the pointer: debrief.yml run 8297).
 */
export async function anchoredMenu(page: Page, anchor: Locator, name?: string | RegExp, offset: keyof typeof MENU_OFFSET_PX = 'flush'): Promise<Locator> {
  const menu = name === undefined ? menuNamed(page).last() : menuNamed(page, name);
  await expect(menu).toBeAttached();
  // Debrief's own context menu (ContextMenuHost: right-click, Shift+F10, panel menus) says when it is placed: it is drawn at its point and only
  // moves to stay inside the window. A CDK menu (menu bar, fly-outs, buttons) is placed by its overlay: wait until it sits against its trigger.
  if (await menu.evaluate((el) => el.closest('debrief-context-menu-host') !== null)) await expect(menu, 'the context menu is placed').toHaveAttribute('data-placed', 'true');
  else await expectMenuAnchoredTo(menu, anchor, MENU_OFFSET_PX[offset]);
  return menu;
}

/** Right-clicks `target` (at `position`, else its centre) and returns its context menu (`name`, default "Actions") once it is placed. */
export async function contextMenu(page: Page, target: Locator, options: { position?: { x: number; y: number }; name?: string | RegExp } = {}): Promise<Locator> {
  await target.click({ button: 'right', ...(options.position ? { position: options.position } : {}) });
  return anchoredMenu(page, target, options.name ?? 'Actions');
}

/** Opens a dock group's panel menu (its ⋯ button) and chooses `item` (Float, Dock, Auto hide, ...) once the menu sits below the button. */
export async function panelMenuButton(page: Page, group: Locator, item: string): Promise<void> {
  const button = group.getByRole('button', { name: 'Panel menu', exact: true });
  await button.click();
  await (await anchoredMenu(page, button, 'Actions')).getByRole('menuitem', { name: item, exact: true }).click();
}

/**
 * Opens a top-level menu (File, Edit, View, ...) and returns it, once it sits below its trigger: the menu is drawn at the window's top-left until
 * the overlay is positioned (debrief.yml run 8297: about 500 ms during the first load in WebKit), and a click on an item there misses once it moves.
 */
export async function openMenu(page: Page, name: string): Promise<Locator> {
  const trigger = menubar(page).getByRole('menuitem', { name, exact: true });
  await trigger.click();
  return anchoredMenu(page, trigger, name);
}

/** Opens the menus along a path (e.g. "View", "Analysis") and returns the last one, open and beside its item. */
export async function openMenuPath(page: Page, ...path: string[]): Promise<Locator> {
  let menu = await openMenu(page, path[0]);
  for (const name of path.slice(1)) {
    const trigger = menu.getByRole('menuitem', { name, exact: true });
    await trigger.click();
    menu = await anchoredMenu(page, trigger, name);
  }
  return menu;
}

/** Chooses a menu item by path, e.g. `chooseMenu(page, 'View', 'Analysis', 'Anomalies')`. */
export async function chooseMenu(page: Page, ...path: string[]): Promise<void> {
  const menu = await openMenuPath(page, ...path.slice(0, -1));
  await menuItem(menu, path[path.length - 1]).click();
}

/** Opens (or focuses) a view by name with View ▸ Other windows…, which lists every view. */
export async function showView(page: Page, title: string): Promise<void> {
  await chooseMenu(page, 'View', 'Other windows…');
  const picker = page.getByRole('dialog', { name: 'Other windows' });
  await picker.getByRole('combobox', { name: 'Type a window name' }).fill(title);
  await picker.getByRole('option', { name: new RegExp(`${title.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`) }).first().click();
}

/** Opens the profile editor (not part of the default layout) from the activity bar. */
export async function showProfileEditor(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Profile editor' }).click();
  await page.locator('debrief-profile-editor').waitFor();
}
