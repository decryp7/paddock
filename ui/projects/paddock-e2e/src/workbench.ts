import { expect, type Locator, type Page } from '@playwright/test';
import { ShellDialogs, ShellJobs, ShellMenus, ShellNotifications, ShellStatusBar } from './shell.ts';

/*
 * Page objects for the workbench (ADR 0013 §4; plan W7), by accessible names and the workbench's test ids, so an IDE-style
 * app's e2e reads the same in every app: `const wb = new WorkbenchApp(page)`, then `wb.frame`, `wb.activity`,
 * `wb.explorer`, `wb.palette`, `wb.dock`, `wb.assistant`, plus the shell's status bar, notifications, jobs, menus, dialogs.
 */

/** The workbench window: its regions, the stage, the side bar and its sash. */
export class WorkbenchFrame {
  readonly page: Page;
  constructor(page: Page) {
    this.page = page;
  }
  get root(): Locator {
    return this.page.locator('pd-workbench');
  }
  get stage(): Locator {
    return this.page.locator('#pd-stage');
  }
  get sidebar(): Locator {
    return this.page.getByTestId('workbench-sidebar');
  }
  get sash(): Locator {
    return this.page.getByTestId('sidebar-sash');
  }
  get skipLink(): Locator {
    return this.page.getByRole('link', { name: 'Skip to the work area' });
  }
  /** Opens the window and waits for the frame and its activity bar. */
  async open(path = '/'): Promise<void> {
    await this.page.goto(path);
    await expect(this.page.locator('pd-activity-bar')).toBeVisible();
  }
  /** The Explorer section on show, or `closed`. */
  explorer(): Promise<string | null> {
    return this.root.getAttribute('data-explorer');
  }
  /** The region (`title`, `activity`, `sidebar`, `main`, `status`) that has the focus, or null. */
  focusedRegion(): Promise<string | null> {
    return this.page.evaluate(() => document.activeElement?.closest('[data-pd-region]')?.getAttribute('data-pd-region') ?? null);
  }
}

/** The activity bar. */
export class WorkbenchActivityBar {
  readonly page: Page;
  constructor(page: Page) {
    this.page = page;
  }
  get bar(): Locator {
    return this.page.getByRole('navigation', { name: 'Activity bar' });
  }
  /** A button by its item id (`pn.explorer`), or `pd.more`. */
  item(id: string): Locator {
    return this.bar.locator(`[data-activity-id="${id}"]`);
  }
  get more(): Locator {
    return this.item('pd.more');
  }
  /** The fly-out menu of an item (open). */
  menu(id: string): Locator {
    return this.page.locator(`[data-activity-menu="${id}"]`);
  }
  /** The ids of the buttons on show, top to bottom. */
  ids(): Promise<string[]> {
    return this.bar.locator('[data-activity-id]').evaluateAll((els) => els.map((e) => e.getAttribute('data-activity-id') ?? ''));
  }
}

/** The Explorer side bar. */
export class WorkbenchExplorer {
  readonly page: Page;
  constructor(page: Page) {
    this.page = page;
  }
  section(id: string): Locator {
    return this.page.locator(`[data-explorer-section="${id}"]`);
  }
  tree(section: string): Locator {
    return this.section(section).getByRole('tree');
  }
  node(section: string, id: string): Locator {
    return this.section(section).locator(`[data-node-id="${id}"]`);
  }
  /** Expands a node by its chevron and waits for it to say so. */
  async expand(section: string, id: string): Promise<void> {
    const node = this.node(section, id);
    await node.locator('.pd-ex-twisty').click();
    await expect(node).toHaveAttribute('aria-expanded', 'true');
  }
  get more(): Locator {
    return this.page.getByTestId('explorer-more');
  }
  get menu(): Locator {
    return this.page.getByTestId('explorer-menu');
  }
}

/** The command palette. */
export class WorkbenchPalette {
  readonly page: Page;
  constructor(page: Page) {
    this.page = page;
  }
  get dialog(): Locator {
    return this.page.getByTestId('palette');
  }
  get input(): Locator {
    return this.page.getByTestId('palette-input');
  }
  get options(): Locator {
    return this.dialog.getByRole('option');
  }
  get status(): Locator {
    return this.page.getByTestId('palette-status');
  }
  /** Opens it with the keyboard (`Mod+Shift+P` for commands, `Mod+P` for the plain-text provider) and waits. */
  async open(commands = true): Promise<void> {
    await this.page.keyboard.press(commands ? 'ControlOrMeta+Shift+P' : 'ControlOrMeta+P');
    await expect(this.input).toBeFocused();
  }
  /** Types a query (replacing what is there) and waits for the list to settle (not "Searching"). */
  async search(query: string): Promise<void> {
    await this.input.fill(query);
    await expect(this.status).not.toContainText('Searching');
  }
  /** Runs the option whose text contains `label` (Enter on it). */
  async run(label: string | RegExp): Promise<void> {
    await this.options.filter({ hasText: label }).first().click();
    await expect(this.dialog).toHaveCount(0);
  }
}

/** Everything of a workbench window. */
export class WorkbenchApp {
  readonly page: Page;
  readonly frame: WorkbenchFrame;
  readonly activity: WorkbenchActivityBar;
  readonly explorer: WorkbenchExplorer;
  readonly palette: WorkbenchPalette;
  readonly statusBar: ShellStatusBar;
  readonly notifications: ShellNotifications;
  readonly jobs: ShellJobs;
  readonly menus: ShellMenus;
  readonly dialogs: ShellDialogs;

  constructor(page: Page) {
    this.page = page;
    this.frame = new WorkbenchFrame(page);
    this.activity = new WorkbenchActivityBar(page);
    this.explorer = new WorkbenchExplorer(page);
    this.palette = new WorkbenchPalette(page);
    this.statusBar = new ShellStatusBar(page);
    this.notifications = new ShellNotifications(page);
    this.jobs = new ShellJobs(page);
    this.menus = new ShellMenus(page);
    this.dialogs = new ShellDialogs(page);
  }
}
