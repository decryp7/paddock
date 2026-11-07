import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import type { ReadVia } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Lookups } from '../lookup/lookups';
import { provideIcons } from '../../shared/icons';
import { NetworkStore } from '../../state/network.store';
import { nextFrame } from '../../testing/frame';
import { NetworkPage } from './network-page';

const NOW = 1_700_000_000_000;

async function open(how: ReadVia | ((mock: MockBackend) => void) = () => undefined, onCall?: (q: { retryHelper?: boolean }) => void) {
  localStorage.clear();
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  if (typeof how === 'function') how(mock);
  else {
    // Who read the owners of the sockets: the answer's `ownersVia` is set over the mock's own.
    const real = mock.network.bind(mock);
    vi.spyOn(mock, 'network').mockImplementation(async (q) => {
      onCall?.(q);
      return { ...(await real(q)), ownersVia: how };
    });
  }
  TestBed.configureTestingModule({ providers: [provideIcons(), provideRouter([]), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(NetworkPage);
  const settle = async () => {
    for (let i = 0; i < 3; i++) {
      await f.whenStable();
      await nextFrame();
      f.detectChanges();
    }
  };
  f.detectChanges();
  await settle();
  const el = f.nativeElement as HTMLElement;
  const rows = () => [...el.querySelectorAll<HTMLElement>('[role="row"][data-index]')];
  const groups = () => rows().filter((r) => r.hasAttribute('aria-expanded'));
  const press = (target: Element, key: string, init: KeyboardEventInit = {}) => target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }));
  return { f, el, settle, rows, groups, press, store: TestBed.inject(NetworkStore) };
}

afterEach(() => {
  localStorage.clear();
  document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = ''));
});

describe('NetworkPage', () => {
  it('Try again on the owners source line asks the helper now, once, outside the poll', async () => {
    const calls: Array<boolean | undefined> = [];
    const { el, settle } = await open({ unavailable: { reason: 'The helper could not read it.', next: 'retry' } }, (q) => calls.push(q.retryHelper));
    const before = calls.length;
    (el.querySelector('[data-testid=reading-source-step]') as HTMLButtonElement).click();
    await settle();
    expect(calls.slice(before)).toContain(true);
    expect(calls.slice(0, before).every((c) => !c)).toBe(true);
  });

  it('names the helper when it filled the owners, and asks for it when the kernel hides them', async () => {
    const helper = await open('helper');
    expect(helper.el.querySelector('[data-testid=reading-source-helper]')?.textContent).toContain('Read by the administrator helper');
    TestBed.resetTestingModule();
    const needs = await open({ needsHelper: { reason: 'Some sockets belong to other accounts. Turn on Administrator readings in Settings so the helper can read this.', next: { openSettings: 'helper' } } });
    expect(needs.el.querySelector('[data-testid=reading-source-needs]')?.textContent).toContain('belong to other accounts');
    expect(needs.el.querySelector('[data-testid=reading-source-turn-on]')).not.toBeNull();
    TestBed.resetTestingModule();
    const local = await open();
    expect(local.el.querySelector('dy-reading-source [data-testid^=reading-source]')).toBeNull();
  });

  it('is a tree grid grouped by program: group headings at level 1 spanning the row, sockets one level below', async () => {
    const { el, groups, rows } = await open();
    const grid = el.querySelector('[aria-label="Connections"]')!;
    expect(grid.getAttribute('role')).toBe('treegrid');
    expect(groups().length).toBeGreaterThan(1);
    expect(groups()[0]!.getAttribute('aria-level')).toBe('1');
    expect(groups()[0]!.classList.contains('pd-span')).toBe(true);
    expect(groups()[0]!.textContent).toContain('connection');
    const item = rows().find((r) => !r.hasAttribute('aria-expanded'))!;
    expect(item.getAttribute('aria-level')).toBe('2');
    // One tab stop for the grid.
    expect(rows().filter((r) => r.tabIndex === 0)).toHaveLength(1);
  });

  it('per-program traffic: who read it, and other accounts as one total with no names', async () => {
    const real = MockBackend.prototype.network;
    const via: ReadVia = { needsHelper: { reason: 'Traffic per program needs administrator rights on Windows.', next: { openSettings: 'helper' } } };
    const note = 'Traffic per program needs administrator rights on Windows. Turn on Administrator readings in Settings so the helper can read this.';
    const needs = await open((mock) => {
      vi.spyOn(mock, 'network').mockImplementation(async (q) => ({ ...(await real.call(mock, q)), traffic: [], trafficNote: note, trafficVia: via }));
    });
    expect(needs.el.querySelector('[data-testid="reading-source-needs"]')?.textContent).toContain('administrator rights on Windows');
    expect(needs.el.querySelector('[data-testid="reading-source-turn-on"]')?.textContent).toContain('Turn on administrator readings');
    expect(needs.el.querySelector('.traffic-note')).toBeNull();
    TestBed.resetTestingModule();
    const helper = await open((mock) => {
      vi.spyOn(mock, 'network').mockImplementation(async (q) => ({
        ...(await real.call(mock, q)),
        trafficNote: null,
        trafficVia: 'helper',
        trafficOthers: { programs: 2, bytesIn: 2048, bytesOut: 1024 },
      }));
    });
    expect(helper.el.querySelector('[data-testid="reading-source-helper"]')?.textContent).toContain('administrator helper');
    const others = helper.el.querySelector('[data-testid="traffic-others"]')?.textContent ?? '';
    expect(others).toContain('Other users and ended programs: 2 programs');
    expect(others).toMatch(/2(\.0+)? KB/);
  });

  it('folds a program with its chevron and with Left, unfolds it with Right, keeps the choice and the focus', async () => {
    const { f, groups, press, settle, rows } = await open();
    const total = rows().length;
    const first = () => groups()[0]!;
    const title = first().querySelector('strong')!.textContent!;
    first().querySelector<HTMLElement>('.chev')!.click();
    await settle();
    expect(rows().length).toBeLessThan(total);
    expect(first().getAttribute('aria-expanded')).toBe('false');
    expect(JSON.parse(localStorage.getItem('dyno.networkCollapsed')!)).toContain(title);
    first().focus();
    press(first(), 'ArrowRight');
    await settle();
    expect(first().getAttribute('aria-expanded')).toBe('true');
    expect(document.activeElement).toBe(first());
    press(first(), 'ArrowLeft');
    await settle();
    expect(first().getAttribute('aria-expanded')).toBe('false');
    expect(document.activeElement).toBe(first());
    press(first(), 'Enter');
    await settle();
    expect(first().getAttribute('aria-expanded')).toBe('true');
    expect(f.nativeElement.querySelector('aside')).toBeNull();
  });

  it('Collapse all folds every program and Expand all opens them again, and the fold is kept', async () => {
    const { el, groups, rows, settle } = await open();
    const total = rows().length;
    const count = groups().length;
    expect(count).toBeGreaterThan(1);
    el.querySelector<HTMLButtonElement>('[data-testid=tree-collapse-all]')!.click();
    await settle();
    expect(groups().every((g) => g.getAttribute('aria-expanded') === 'false')).toBe(true);
    expect(rows()).toHaveLength(count);
    expect(JSON.parse(localStorage.getItem('dyno.networkCollapsed')!)).toHaveLength(count);
    el.querySelector<HTMLButtonElement>('[data-testid=tree-expand-all]')!.click();
    await settle();
    expect(rows()).toHaveLength(total);
    expect(JSON.parse(localStorage.getItem('dyno.networkCollapsed')!)).toEqual([]);
  });

  it('Enter on a socket opens the details panel and Escape closes it', async () => {
    const { el, rows, press, settle } = await open();
    const item = rows().find((r) => !r.hasAttribute('aria-expanded'))!;
    item.focus();
    press(item, 'Enter');
    await settle();
    expect(el.querySelector('aside[aria-label="Details"]')).not.toBeNull();
    expect(rows().find((r) => r.dataset['key'] === item.dataset['key'])!.getAttribute('aria-selected')).toBe('true');
    press(rows().find((r) => r.dataset['key'] === item.dataset['key'])!, 'Escape');
    await settle();
    expect(el.querySelector('aside[aria-label="Details"]')).toBeNull();
  });

  it('Ctrl+C copies the focused socket', async () => {
    const { rows, press } = await open();
    const write = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: write }, configurable: true });
    const item = rows().find((r) => !r.hasAttribute('aria-expanded'))!;
    item.focus();
    press(item, 'c', { ctrlKey: true });
    expect(write).toHaveBeenCalledTimes(1);
    press(rows().find((r) => r.hasAttribute('aria-expanded'))!, 'c', { ctrlKey: true });
    expect(write).toHaveBeenCalledTimes(1);
  });

  it('offers Look up IP only for public peers, and the actions of a socket in the menu', async () => {
    const { el, f, settle, store } = await open();
    const lookups = TestBed.inject(Lookups);
    expect(lookups).toBeTruthy();
    const lookup = () => [...el.querySelectorAll<HTMLButtonElement>('button[aria-label="Look up IP"]')];
    expect(lookup().some((b) => b.disabled)).toBe(true);
    expect(lookup().some((b) => !b.disabled)).toBe(true);
    const row = el.querySelector<HTMLElement>('[role="row"][data-key]:not([aria-expanded])')!;
    row.focus();
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true }));
    await settle();
    const items = [...document.querySelectorAll('.pd-popmenu__item')].map((i) => i.textContent!.trim());
    expect(items).toEqual(expect.arrayContaining(['Look up IP', 'Resolve host name', 'Copy address', 'Filter to this host', 'Copy as text', 'Show process']));
    expect(store.snapshot()).not.toBeNull();
    expect(f).toBeTruthy();
  });

  it('is a plain grid without groups, and the listening tab lists ports', async () => {
    const { el, f, settle } = await open();
    const click = async (label: string) => {
      [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent!.trim() === label)!.click();
      f.detectChanges();
      await settle();
    };
    await click('None');
    const grid = el.querySelector('[aria-label="Connections"]')!;
    expect(grid.getAttribute('role')).toBe('grid');
    expect(el.querySelectorAll('[role="row"][aria-expanded]')).toHaveLength(0);
    await click('Listening');
    expect(el.querySelector('[aria-label="Listening ports"]')).not.toBeNull();
    expect([...el.querySelectorAll('[role="columnheader"]')].map((h) => h.textContent!.trim())).toEqual(expect.arrayContaining(['Program', 'Service', 'Address', 'Protocol', 'Since']));
  });

  it('names the country of public peers and filters the connections to one country with a way back', async () => {
    const { el, rows, settle, store } = await open();
    expect(el.querySelectorAll('.cc').length).toBeGreaterThan(0);
    expect(el.querySelector('[data-testid="country-filter"]')).toBeNull();
    await store.setQuery({ country: 'IE' });
    await settle();
    expect(el.querySelector('[data-testid="country-filter"]')!.textContent).toContain('IE');
    const items = rows().filter((r) => !r.hasAttribute('aria-expanded'));
    expect(items.length).toBeGreaterThan(0);
    expect(items.every((r) => r.textContent!.includes('IE'))).toBe(true);
    (el.querySelector('[data-testid="country-filter"] button') as HTMLElement).click();
    await settle();
    expect(el.querySelector('[data-testid="country-filter"]')).toBeNull();
  });

  it('marks a flagged connection in its row with the word and names the lists, and counts programs of what the filter shows', async () => {
    const { el, settle, store } = await open();
    const flagged = [...el.querySelectorAll<HTMLElement>('.flagged')];
    expect(flagged.length).toBe(1);
    expect(flagged[0]!.textContent).toContain('flagged');
    expect(flagged[0]!.title).toBe('Flagged by Work blocklist');
    await store.setQuery({ country: 'IE' });
    await settle();
    const summary = el.querySelector('.summary')!.textContent!;
    const programs = new Set(store.snapshot()!.connections.map((c) => (c.owner.kind === 'process' ? c.owner.name : 'unknown'))).size;
    expect(summary).toContain(`${programs} programs`);
  });
});
