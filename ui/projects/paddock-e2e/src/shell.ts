import { readFileSync } from 'node:fs';
import { expect, type Locator, type Page } from '@playwright/test';

/*
 * Page objects for the parts of the Paddock shell (plan S14 and the packages before it), by their accessible names and
 * the shell's own test ids, so every app's e2e reads the same: `const app = new ShellApp(page)`, then `app.frame`,
 * `app.sidebar`, `app.statusBar`, `app.notifications`, `app.jobs`, `app.menus`, `app.dialogs`, `app.settings`.
 */

/** The window frame: the page heading, the sidebar toggle and mode, the regions and the skip link. */
export class ShellFrame {
  readonly page: Page;
  constructor(page: Page) {
    this.page = page;
  }
  /** The page's heading (level 1). */
  get heading(): Locator {
    return this.page.getByRole('heading', { level: 1 });
  }
  get main(): Locator {
    return this.page.locator('#pd-main');
  }
  get titleBar(): Locator {
    return this.page.locator('pd-title-bar');
  }
  get appName(): Locator {
    return this.page.getByTestId('app-name');
  }
  get toggle(): Locator {
    return this.page.getByTestId('sidebar-toggle');
  }
  get scrim(): Locator {
    return this.page.getByTestId('sidebar-scrim');
  }
  get splash(): Locator {
    return this.page.locator('.pd-splash');
  }
  get skipLink(): Locator {
    return this.page.getByRole('link', { name: 'Skip to page' });
  }
  /** Opens the window at `hash` (`#/keys`) and waits for its page's heading. */
  async open(hash = ''): Promise<void> {
    await this.page.goto(`/${hash}`);
    await expect(this.heading).toBeVisible();
  }
  /** How the sidebar is shown by the layout plan: `expanded`, `rail` or `overlay`. */
  sidebarMode(): Promise<string | null> {
    return this.page.locator('pd-shell-frame').getAttribute('data-sidebar');
  }
  /** The region (`title`, `sidebar`, `main`, `status`) that has the focus, or null. */
  focusedRegion(): Promise<string | null> {
    return this.page.evaluate(() => document.activeElement?.closest('[data-pd-region]')?.getAttribute('data-pd-region') ?? null);
  }
  /** Presses `key` until the focus is on an element matching `selector` (F6 cycles the regions from wherever focus is). */
  async focusBy(key: string, selector: string, tries = 6): Promise<void> {
    const there = () => this.page.evaluate((sel) => !!document.activeElement?.matches(sel), selector);
    for (let i = 0; i < tries && !(await there()); i++) await this.page.keyboard.press(key);
    expect(await there(), `focus on ${selector} by ${key}`).toBe(true);
  }
}

/** The pages sidebar. */
export class ShellSidebar {
  readonly page: Page;
  constructor(page: Page) {
    this.page = page;
  }
  get nav(): Locator {
    return this.page.getByRole('navigation', { name: 'Pages' });
  }
  get links(): Locator {
    return this.nav.getByRole('link');
  }
  link(name: string | RegExp): Locator {
    return this.nav.getByRole('link', { name });
  }
  get badges(): Locator {
    return this.nav.getByTestId('page-badge');
  }
}

/** The status bar and its items. */
export class ShellStatusBar {
  readonly page: Page;
  constructor(page: Page) {
    this.page = page;
  }
  get bar(): Locator {
    return this.page.getByRole('contentinfo', { name: 'Status bar' });
  }
  /** A status item by its contribution id (`pd.jobs`). */
  item(id: string): Locator {
    return this.bar.locator(`[data-status-item="${id}"]`);
  }
  /** The items on one side, in order. */
  items(side: 'start' | 'end'): Locator {
    return this.bar.locator(`.pd-${side} [data-status-item]`);
  }
  /** The ids of the items on one side, in order. */
  ids(side: 'start' | 'end'): Promise<string[]> {
    return this.items(side).evaluateAll((els) => els.map((e) => (e as HTMLElement).dataset['statusItem'] ?? ''));
  }
  /** The service health chip (plan S4). */
  get chip(): Locator {
    return this.bar.getByTestId('service-health-chip');
  }
  /** The notifications bell (plan S4). */
  get bell(): Locator {
    return this.bar.getByTestId('notifications-status');
  }
  /** The jobs item's button (plan S9); its list opens beside it. */
  get jobs(): Locator {
    return this.item('pd.jobs').getByRole('button').first();
  }
  /** "…" for the items a narrow bar hides. */
  get more(): Locator {
    return this.bar.getByTestId('status-more');
  }
}

/** Toasts, the notification centre and the service details. */
export class ShellNotifications {
  readonly page: Page;
  readonly statusBar: ShellStatusBar;
  constructor(page: Page) {
    this.page = page;
    this.statusBar = new ShellStatusBar(page);
  }
  /** The toast region. */
  get toasts(): Locator {
    return this.page.getByRole('region', { name: 'Notifications', exact: true });
  }
  /** The toasts themselves. */
  get toastItems(): Locator {
    return this.toasts.getByTestId('notification');
  }
  get centre(): Locator {
    return this.page.getByRole('region', { name: 'Notifications centre' });
  }
  /** A message of the centre by its cause key (`health:sd.engine`). */
  notice(cause: string): Locator {
    return this.centre.locator(`[data-cause="${cause}"]`);
  }
  /** The chip's details (the services that need attention). */
  get services(): Locator {
    return this.page.getByRole('region', { name: 'Services' });
  }
  /** Opens the centre from the bell. */
  async openCentre(): Promise<Locator> {
    await this.statusBar.bell.click();
    await expect(this.centre).toBeVisible();
    return this.centre;
  }
}

/** The jobs list (plan S9). */
export class ShellJobs {
  readonly page: Page;
  constructor(page: Page) {
    this.page = page;
  }
  get list(): Locator {
    return this.page.getByRole('region', { name: 'Jobs' });
  }
  get items(): Locator {
    return this.list.getByRole('listitem');
  }
}

/** The in-app menu bar and context menus (plan S13). */
export class ShellMenus {
  readonly page: Page;
  constructor(page: Page) {
    this.page = page;
  }
  get bar(): Locator {
    return this.page.getByRole('menubar', { name: 'Application menu' });
  }
  /** A menu by its name, open or not. */
  menu(name: string): Locator {
    return this.page.getByRole('menu', { name });
  }
  /** Opens a menu of the bar and returns it. */
  async open(name: string): Promise<Locator> {
    await this.bar.getByRole('menuitem', { name, exact: true }).click();
    return this.menu(name);
  }
  /** Opens `menu` and returns its item `item`. */
  async item(menu: string, item: string): Promise<Locator> {
    return (await this.open(menu)).getByRole('menuitem', { name: item });
  }
  /** Whether the window runs on a Mac (keys read ⌘ there). */
  isMac(): Promise<boolean> {
    return this.page.evaluate(() => /Mac|iPhone|iPad/.test(navigator.platform));
  }
}

/** A dialog's contract, as the contract file holds it (ADR 0009). */
export interface DialogContractEntry {
  readonly kind: 'decision' | 'form' | 'viewer' | 'tool';
  readonly modality: 'modal' | 'modeless';
  readonly requiredChoice: boolean;
  readonly instanced?: boolean;
  readonly minSize?: { readonly width: number; readonly height: number };
  readonly commands?: readonly string[];
}

/** The dialogs of an app's contract file (`dialog-contracts.json`, generated from its registry), by id. */
export function dialogContracts(path: string): Readonly<Record<string, DialogContractEntry>> {
  const dialogs = (JSON.parse(readFileSync(path, 'utf8')) as { dialogs?: Record<string, DialogContractEntry> }).dialogs;
  if (typeof dialogs !== 'object' || dialogs === null) throw new Error(`${path} has no "dialogs": regenerate the dialog contracts`);
  return dialogs;
}

/** Where the browser build of a dialog window leaves its answer (`localStorage`). */
export const DIALOG_RESULT_KEY = 'paddock.dialog.browserResult';

/** A Close, Cancel or the like: the ways out of a dialog. */
export const CLOSE_NAMES = /^(close|cancel|not now|dismiss|done|later|got it)\b/i;

/** The URL Rust loads dialog `id`'s window with (`query` goes before the hash: `?settings=damaged`). */
export function dialogWindowUrl(id: string, input: unknown = {}, options: { readonly title?: string; readonly query?: string } = {}): string {
  const title = encodeURIComponent(options.title ?? id);
  return `/${options.query ?? ''}#/dialog?id=${id}&host=main&label=d-main-1&title=${title}&dialogInput=${encodeURIComponent(JSON.stringify(input))}`;
}

/** Dialog windows (by URL, as Rust loads them) and dialogs in the page. */
export class ShellDialogs {
  readonly page: Page;
  constructor(page: Page) {
    this.page = page;
  }
  /** The URL of dialog `id`'s window with `input` (`query` goes before the hash: `?settings=damaged`). */
  url(id: string, input: unknown = {}, options: { readonly title?: string; readonly query?: string } = {}): string {
    return dialogWindowUrl(id, input, options);
  }
  /** Opens dialog `id`'s window in the page. */
  async openWindow(id: string, input: unknown = {}, options: { readonly title?: string; readonly query?: string } = {}): Promise<void> {
    await this.page.goto(this.url(id, input, options));
  }
  /** The dialog window's shell. */
  get shell(): Locator {
    return this.page.locator('pd-dialog-shell');
  }
  /** The shell's own Close (when the content has none). */
  get shellClose(): Locator {
    return this.page.getByTestId('dialog-shell-close');
  }
  /** A way out of the dialog: the shell's Close or the content's Close/Cancel. */
  get wayOut(): Locator {
    return this.shell.locator('[data-dialog-close]').or(this.shell.getByRole('button', { name: CLOSE_NAMES })).first();
  }
  /** Resolves once the window has left Loading without an error. */
  async loaded(): Promise<void> {
    // The shell itself first: before it mounts there is no "loading" marker either, and a window still on the start-up splash would pass.
    await expect(this.shell).toBeVisible();
    await expect(this.shell.getByTestId('dialog-loading')).toHaveCount(0);
    await expect(this.shell.getByTestId('dialog-error')).toHaveCount(0);
  }
  /** What the window answered (`{"result":…}`), or null while it has not. */
  result(): Promise<string | null> {
    return this.page.evaluate((k) => localStorage.getItem(k), DIALOG_RESULT_KEY);
  }
  /** Waits until the window answered `expected` (JSON text). */
  async expectResult(expected: string): Promise<void> {
    await expect.poll(() => this.result()).toBe(expected);
  }
  /** The heights the window was fitted to, in order (the browser build records them). */
  async fitHistory(): Promise<number[]> {
    const h = await this.page.evaluate(() => document.documentElement.dataset['fitHistory'] ?? '');
    return h ? h.split(',').map(Number) : [];
  }
  /** A dialog shown in the page, by its accessible name. */
  inPage(name: string): Locator {
    return this.page.getByRole('dialog', { name });
  }
  /** The backdrop of a modal dialog in the page. */
  get backdrop(): Locator {
    return this.page.getByTestId('dialog-backdrop');
  }
}

/** The Settings window (plan S10). */
export class ShellSettings {
  readonly page: Page;
  readonly dialogs: ShellDialogs;
  constructor(page: Page) {
    this.page = page;
    this.dialogs = new ShellDialogs(page);
  }
  /** The Settings window's URL, at `input.page` when given (`query`: before the hash). */
  url(input: { readonly page?: string } = {}, query = ''): string {
    return this.dialogs.url('pd.settings', input, { title: 'Settings', query });
  }
  /** Opens the Settings window in the page. */
  async open(input: { readonly page?: string } = {}, query = ''): Promise<void> {
    await this.page.goto(this.url(input, query));
  }
  /** The window when shown in the page (from its command). */
  get window(): Locator {
    return this.page.getByRole('dialog', { name: 'Settings' });
  }
  get nav(): Locator {
    return this.page.getByRole('navigation', { name: 'Settings pages' });
  }
  get search(): Locator {
    return this.page.getByRole('searchbox', { name: 'Search settings' });
  }
  /** A setting's field by its key, in `scope` (default the page). */
  field(key: string, scope: Page | Locator = this.page): Locator {
    return scope.locator(`pd-setting-field[data-key="${key}"]`);
  }
  /** A choice field's trigger (the themed select, a `combobox` button showing the chosen label). */
  select(key: string, scope: Page | Locator = this.page): Locator {
    return this.field(key, scope).getByRole('combobox');
  }
  /** Picks `label` in a choice field: opens the themed list and clicks the option (the list lives in the page's overlay). */
  async choose(key: string, label: string, scope: Page | Locator = this.page): Promise<void> {
    await this.select(key, scope).click();
    await this.page.getByRole('option', { name: label, exact: true }).click();
  }
  /** A saved-change announcement (`Theme saved.`). */
  saved(text: string, scope: Page | Locator = this.page): Locator {
    return scope.getByRole('status').filter({ hasText: text });
  }
}

/** Every part of a shell window. */
export class ShellApp {
  readonly page: Page;
  readonly frame: ShellFrame;
  readonly sidebar: ShellSidebar;
  readonly statusBar: ShellStatusBar;
  readonly notifications: ShellNotifications;
  readonly jobs: ShellJobs;
  readonly menus: ShellMenus;
  readonly dialogs: ShellDialogs;
  readonly settings: ShellSettings;

  constructor(page: Page) {
    this.page = page;
    this.frame = new ShellFrame(page);
    this.sidebar = new ShellSidebar(page);
    this.statusBar = new ShellStatusBar(page);
    this.notifications = new ShellNotifications(page);
    this.jobs = new ShellJobs(page);
    this.menus = new ShellMenus(page);
    this.dialogs = new ShellDialogs(page);
    this.settings = new ShellSettings(page);
  }
}
