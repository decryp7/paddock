import { expect, type Locator, type Page } from '@playwright/test';

/**
 * Keyboard-only flow helpers (plan X49, WCAG 2.1.1 and 2.1.2, 2.4.3 and 2.4.7): walk the Tab order of a window, prove that
 * everything the user needs is reachable, that nothing traps focus, that every stop shows where the focus is, and that
 * Escape gives the focus back to what opened a popup. Playwright's `press` is used throughout, never a click.
 */

/**
 * The key that moves the focus to the next (or, with `back`, the previous) focusable element. Safari and WKWebView on macOS
 * only Tab between text fields and lists unless "Use keyboard navigation to move focus between controls" (full keyboard
 * access) is on: buttons and links are skipped, and Option+Tab reaches them. Playwright's WebKit follows that default, so a
 * test of what a keyboard user can reach emulates full keyboard access with Alt. Everywhere else it is plain Tab.
 */
export function tabKey(engine: string, platform: string, back = false): string {
  const alt = engine === 'webkit' && platform === 'darwin';
  return `${alt ? 'Alt+' : ''}${back ? 'Shift+' : ''}Tab`;
}

/** {@link tabKey} for the page's own browser engine and the machine the tests run on. */
export function pageTabKey(page: Page, back = false): string {
  return tabKey(page.context().browser()?.browserType().name() ?? 'chromium', process.platform, back);
}

/** Presses the key that moves the focus on ({@link tabKey}). */
export async function pressTab(page: Page, back = false): Promise<void> {
  await page.keyboard.press(pageTabKey(page, back));
}

/** The navigator fields that decide the platform of the page (`platform`, `userAgent`). */
export interface NavigatorPlatform {
  readonly platform?: string;
  readonly userAgent?: string;
}

/**
 * The key a binding's `Mod` presses in a page with this navigator: `Meta` (Cmd) on macOS and iOS, `Control` elsewhere. It
 * follows the page, as the app does (paddock-ui's `currentPlatform` in `shell/menu/src/keys.ts`, the same test), not the
 * machine the tests run on, which is what Playwright's `ControlOrMeta` follows: WebKit on a Linux runner emulates
 * `devices['Desktop Safari']`, a Mac, so the app wants Cmd there while `ControlOrMeta` presses Ctrl (CI run 7134). Pure.
 */
export function modKey(nav: NavigatorPlatform): 'Meta' | 'Control' {
  return /Mac|iPhone|iPad/.test(`${nav.platform ?? ''} ${nav.userAgent ?? ''}`) ? 'Meta' : 'Control';
}

/** {@link modKey} for the page's own navigator. */
export async function pageModKey(page: Page): Promise<'Meta' | 'Control'> {
  return modKey(await page.evaluate(() => ({ platform: navigator.platform, userAgent: navigator.userAgent })));
}

/**
 * Presses a binding written with `Mod` (`Mod+Shift+P`, `Mod+Comma`) with the modifier the page binds it to
 * ({@link pageModKey}). Use it instead of Playwright's `ControlOrMeta` for the app's own shortcuts.
 */
export async function pressMod(page: Page, binding: string): Promise<void> {
  const mod = await pageModKey(page);
  await page.keyboard.press(binding.replace(/^Mod\+/, `${mod}+`));
}

/** One element the focus stopped on. */
export interface FocusStop {
  /** Identifies the element within the page: its `data-testid`, else its `id`, else its position in the DOM. */
  readonly key: string;
  readonly tag: string;
  readonly role: string | null;
  /** What a screen reader would call it: aria-label, label text, content, value or title (cut to 60 characters). */
  readonly name: string;
  /** The focus is drawn: an outline or a box-shadow ring is on the element. */
  readonly ring: boolean;
}

/** How a walk through the Tab order ended. */
export type WalkOutcome =
  /** The focus left the page's elements (the browser's own UI or the document): a clean end. */
  | 'left'
  /** The focus came back to the first stop: the order is a closed cycle (a dialog or a menu). */
  | 'cycled'
  /** The focus stopped moving, or went round a loop that does not contain the first stop: a keyboard trap. */
  | 'trapped'
  /** `max` stops were visited without an end: the walk was cut. */
  | 'limit';

/**
 * Classifies the next focus position of a Tab walk. `history` holds the keys visited so far (the first is the start of
 * the walk), `next` the key now focused or null when nothing in the page is. Pure.
 */
export function walkStep(history: readonly string[], next: string | null): 'continue' | Exclude<WalkOutcome, 'limit'> {
  if (next === null) return 'left';
  if (history.length > 0 && next === history[history.length - 1]) return 'trapped';
  if (history.length > 1 && next === history[0]) return 'cycled';
  if (history.includes(next)) return 'trapped';
  return 'continue';
}

/** The stops of a finished walk. */
export interface TabWalk {
  readonly stops: readonly FocusStop[];
  readonly outcome: WalkOutcome;
}

/** The element that has the focus, or null while the document itself has it. */
export function focusStop(page: Page): Promise<FocusStop | null> {
  return page.evaluate(() => {
    let el: Element | null = document.activeElement;
    // Inside open shadow roots and same-origin frames the real element is the innermost one.
    while (el?.shadowRoot?.activeElement) el = el.shadowRoot.activeElement;
    if (!el || el === document.body || el === document.documentElement) return null;
    const path = (e: Element): string => {
      const parts: string[] = [];
      for (let n: Element | null = e; n && n !== document.body; n = n.parentElement) {
        const same = n.parentElement ? [...n.parentElement.children].filter((c) => c.tagName === n?.tagName) : [n];
        parts.unshift(`${n.tagName.toLowerCase()}:${same.indexOf(n) + 1}`);
      }
      return parts.join('>');
    };
    const html = el as HTMLElement;
    const labelled = el.getAttribute('aria-labelledby')?.split(/\s+/).map((id) => document.getElementById(id)?.textContent ?? '').join(' ');
    const label = (el as HTMLInputElement).labels?.[0]?.textContent;
    const raw = el.getAttribute('aria-label') ?? labelled ?? label ?? (html.innerText || (el as HTMLInputElement).value || el.getAttribute('title') || '');
    const style = getComputedStyle(el);
    const ring = (style.outlineStyle !== 'none' && parseFloat(style.outlineWidth) > 0) || style.boxShadow !== 'none';
    return {
      key: el.getAttribute('data-testid') ? `testid:${el.getAttribute('data-testid')}` : el.id ? `id:${el.id}` : path(el),
      tag: el.tagName.toLowerCase(),
      role: el.getAttribute('role'),
      name: raw.replace(/\s+/g, ' ').trim().slice(0, 60),
      ring,
    };
  });
}

export interface TabWalkOptions {
  /** Where to start: this element is focused first (default: the page as it is, the walk begins at the first Tab). */
  readonly from?: Locator;
  /** The key pressed to move on (default {@link pageTabKey}: `Tab`, or Alt+Tab in WebKit on macOS; `Shift+Tab` walks backwards). */
  readonly key?: string;
  /** Stops after this many (default 100). */
  readonly max?: number;
}

/**
 * Presses Tab from `from` (or the start of the page) and records every stop until the focus leaves, cycles, is trapped
 * or `max` is reached. The start element itself is the first stop when `from` is given.
 */
export async function tabWalk(page: Page, options: TabWalkOptions = {}): Promise<TabWalk> {
  const max = options.max ?? 100;
  const key = options.key ?? pageTabKey(page);
  const stops: FocusStop[] = [];
  if (options.from) {
    await options.from.focus();
    const first = await focusStop(page);
    if (first) stops.push(first);
  }
  for (let i = 0; i < max; i++) {
    await page.keyboard.press(key);
    const stop = await focusStop(page);
    const verdict = walkStep(stops.map((s) => s.key), stop?.key ?? null);
    if (verdict !== 'continue') return { stops, outcome: verdict };
    if (stop) stops.push(stop);
  }
  return { stops, outcome: 'limit' };
}

/**
 * Fails when the Tab order of the window (or of `options.from`'s container, see `tabWalk`) traps the focus (WCAG 2.1.2),
 * has no stop at all, or shows a stop without a visible focus indicator (2.4.7), and names the stops in the message.
 * A cycle (dialogs and menus keep the focus inside) is fine.
 */
export async function expectTabOrder(page: Page, options: TabWalkOptions & { readonly label?: string } = {}): Promise<TabWalk> {
  const walk = await tabWalk(page, options);
  const where = options.label ? ` (${options.label})` : '';
  const trail = walk.stops.map((s) => `${s.tag}${s.role ? `[${s.role}]` : ''} "${s.name}"`).join(' > ');
  expect(walk.outcome, `focus must not be trapped${where}: ${trail}`).not.toBe('trapped');
  expect(walk.outcome, `the Tab order was longer than ${options.max ?? 100} stops${where}: ${trail}`).not.toBe('limit');
  expect(walk.stops.length, `something must be focusable by keyboard${where}`).toBeGreaterThan(0);
  const unmarked = walk.stops.filter((s) => !s.ring).map((s) => `${s.tag} "${s.name}"`);
  expect(unmarked, `every stop shows the focus${where}`).toEqual([]);
  return walk;
}

/**
 * The names of the `targets` that Tab never reaches within `max` stops (empty: all reachable). Each target is a
 * locator naming one element; the walk starts where the page is.
 */
export async function unreachableByKeyboard(page: Page, targets: Readonly<Record<string, Locator>>, max = 100): Promise<string[]> {
  const pending = new Map(Object.entries(targets));
  const history: string[] = [];
  const seen = async (): Promise<void> => {
    for (const [name, target] of [...pending]) {
      if (await target.evaluate((el) => el === document.activeElement || !!el.shadowRoot?.activeElement).catch(() => false)) pending.delete(name);
    }
  };
  await seen();
  for (let i = 0; i < max && pending.size > 0; i++) {
    await pressTab(page);
    const stop = await focusStop(page);
    if (walkStep(history, stop?.key ?? null) !== 'continue') break;
    if (stop) history.push(stop.key);
    await seen();
  }
  return [...pending.keys()];
}

/** Fails when any of `targets` cannot be reached with the Tab key (WCAG 2.1.1). */
export async function expectReachableByKeyboard(page: Page, targets: Readonly<Record<string, Locator>>, max = 100): Promise<void> {
  expect(await unreachableByKeyboard(page, targets, max), 'not reachable by keyboard').toEqual([]);
}

/**
 * Opens a popup by keyboard (focus its `trigger`, press `key`), checks that `popup` shows, closes it with Escape and
 * checks that the focus is back on the trigger (WCAG 2.4.3): the contract of every menu, popover and dialog.
 */
export async function expectEscapeReturnsFocus(page: Page, trigger: Locator, popup: Locator, key = 'Enter'): Promise<void> {
  await trigger.focus();
  await page.keyboard.press(key);
  await expect(popup).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(popup).toBeHidden();
  await expect(trigger).toBeFocused();
}
