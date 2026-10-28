import { ChangeDetectionStrategy, Component, LOCALE_ID, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { PD_CLOCK_SOURCE } from '@decryp7/paddock-ui';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { PdSunburst, PdSunburstCentre } from './sunburst';
import type { PdSunburstColourBy, PdSunburstNode, PdSunburstPoint } from './sunburst-layout';

const leaf = (id: string, value: number, extra: Partial<PdSunburstNode> = {}): PdSunburstNode => ({ id, label: id.toUpperCase(), value, ...extra });
const TREE = (): PdSunburstNode => ({
  id: 'root',
  label: 'Root',
  value: 1000,
  detail: '12 items',
  children: [
    leaf('a', 500, { detail: '9 items', children: [leaf('a1', 300), leaf('a2', 200)] }),
    leaf('b', 300, { hasMore: true }),
    leaf('c', 100),
    leaf('hid', 50, { kind: 'hidden' }),
    leaf('pend', 20, { kind: 'pending' }),
    leaf('oth', 10, { kind: 'other' }),
    leaf('tiny1', 2),
    leaf('tiny2', 2),
    leaf('zero', 0),
  ],
});
const format = (v: number): string => `${v} KB`;

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdSunburst, PdSunburstCentre],
  template: `
    <pd-sunburst
      label="Disk usage"
      [root]="root()"
      [(centre)]="centre"
      [format]="fmt()"
      [state]="state()"
      [live]="live()"
      [motion]="motion()"
      [highlight]="highlight()"
      [scale]="scale()"
      [colourBy]="colourBy()"
      [rings]="rings()"
      [maxSegments]="maxSegments()"
      emptyTitle="No data"
      errorText="Could not read the disk."
      (hoverChange)="hovers.push($event)"
      (activate)="activations.push($event)"
      (loadChildren)="loads.push($event)"
      (openOther)="others.push($event)"
      (contextMenu)="menus.push($event)"
      (folded)="foldeds.push($event)"
      (retry)="retries = retries + 1"
    >
      @if (custom()) {
        <ng-template pdSunburstCentre let-point>
          <span data-testid="custom-centre">{{ point.node.label }}!</span>
        </ng-template>
      }
      <button pdSunburstEmptyAction type="button">Scan</button>
    </pd-sunburst>
  `,
})
class Host {
  readonly root = signal<PdSunburstNode | null>(TREE());
  readonly centre = signal<string | null>(null);
  readonly fmt = signal(format);
  readonly state = signal<'ready' | 'loading' | 'empty' | 'error'>('ready');
  readonly live = signal(false);
  readonly highlight = signal<string | null>(null);
  readonly motion = signal<'auto' | 'reduce'>('auto');
  readonly scale = signal<number | null>(null);
  readonly colourBy = signal<PdSunburstColourBy>('branch');
  readonly rings = signal(4);
  readonly maxSegments = signal(1000);
  readonly custom = signal(false);
  readonly hovers: (PdSunburstPoint | null)[] = [];
  readonly activations: PdSunburstPoint[] = [];
  readonly loads: string[] = [];
  readonly others: { parent: string; ids: string[] }[] = [];
  readonly menus: { point: PdSunburstPoint; x: number; y: number }[] = [];
  readonly foldeds: { segments: number; minAngle: number }[] = [];
  retries = 0;
}

const PALETTE = ['#60aeeb', '#f3aa25', '#3dd39d', '#d68cc2', '#f07d3a', '#7a70ff', '#e8dc5a', '#a2acb1'];

async function settle(f: { whenStable(): Promise<unknown> }): Promise<void> {
  await f.whenStable();
  TestBed.tick();
  await f.whenStable();
  TestBed.tick();
}

describe('pd-sunburst', () => {
  let now = 0;
  beforeEach(() => {
    PALETTE.forEach((c, i) => document.documentElement.style.setProperty(`--pd-chart-${i + 1}`, c));
    document.documentElement.style.setProperty('--pd-text', '#ffffff');
    document.documentElement.style.setProperty('--pd-surface-1', '#10141a');
    now = 1_000_000;
  });
  afterEach(() => {
    vi.useRealTimers();
    document.documentElement.removeAttribute('data-motion');
    document.documentElement.removeAttribute('style');
    document.body.innerHTML = '';
  });

  async function setup(locale = 'en-US') {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), { provide: PD_CLOCK_SOURCE, useValue: () => now }, { provide: LOCALE_ID, useValue: locale }] });
    const f = TestBed.createComponent(Host);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    const items = () => Array.from(el.querySelectorAll<SVGPathElement>('path[role=treeitem]'));
    const item = (id: string) => items().find((p) => p.getAttribute('data-id') === id) as SVGPathElement;
    return { f, h: f.componentInstance, el, items, item };
  }

  it('draws a named tree with level, set and expanded semantics and the centre text', async () => {
    const { el, item } = await setup();
    const tree = el.querySelector('[role=tree]');
    expect(tree?.getAttribute('aria-label')).toBe('Disk usage');
    const a = item('a');
    expect(a.getAttribute('aria-level')).toBe('1');
    expect(a.getAttribute('aria-posinset')).toBe('1');
    expect(a.getAttribute('aria-setsize')).toBe('7');
    expect(a.getAttribute('aria-expanded')).toBe('true');
    expect(a.getAttribute('aria-label')).toBe('A, 500 KB, 50% of Root, 9 items');
    expect(item('c').getAttribute('aria-expanded')).toBeNull();
    expect(item('b').getAttribute('aria-expanded')).toBe('false');
    expect(item('a1').getAttribute('aria-level')).toBe('2');
    expect(a.getAttribute('tabindex')).toBe('0');
    expect(item('b').getAttribute('tabindex')).toBe('-1');
    expect(item('hid').getAttribute('style')).toContain('url("#');
    expect(item('pend').classList.contains('pd-arc--pending')).toBe(true);
    expect(item('hid').classList.contains('pd-arc--hidden')).toBe(true);
    expect(a.style.fill).toContain('var(--pd-chart-');
    expect(item('a2').style.fill).toContain('color-mix');
    expect(el.querySelector('[data-testid=sunburst-centre-name]')?.textContent).toBe('Root');
    expect(el.querySelector('.pd-centre-value')?.textContent?.replace(/\s+/g, ' ').trim()).toBe('1000 KB');
    expect(el.querySelector('.pd-up')).toBeNull();
    expect(el.textContent).toContain('12 items');
    expect(item('tiny1')).toBeUndefined();
    expect(el.querySelector('[aria-label*="smaller items"]')?.getAttribute('aria-label')).toContain('3 smaller items, 4 KB');
  });

  it('draws curved labels on wide ring one and two segments', async () => {
    const { el } = await setup();
    const texts = Array.from(el.querySelectorAll('text.pd-label')).map((t) => t.textContent?.trim());
    expect(texts).toContain('A  500 KB');
    expect(texts).toContain('A1');
    expect(el.querySelectorAll('defs path').length).toBe(texts.length);
    expect(el.querySelector('text.pd-label textPath')?.getAttribute('href')).toMatch(/^#pd-sb-\d+-l0$/);
  });

  it('omits labels when no ink reaches 4.5:1 or tokens are missing', async () => {
    document.documentElement.style.setProperty('--pd-text', '#a0a0a0');
    document.documentElement.style.setProperty('--pd-surface-1', '#b0b0b0');
    const { el } = await setup();
    expect(el.querySelectorAll('text.pd-label').length).toBe(0);
    document.documentElement.style.setProperty('--pd-text', '');
    const second = await setup();
    expect(second.el.querySelectorAll('text.pd-label').length).toBe(0);
  });

  it('drills in on click, announces the centre, and goes up with the centre button', async () => {
    const { f, h, el, items, item } = await setup();
    expect(el.querySelector('[data-testid=sunburst-live]')?.textContent).toBe('');
    item('a').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await settle(f);
    expect(h.centre()).toBe('a');
    expect(items().map((p) => p.getAttribute('data-id'))).toEqual(['a1', 'a2']);
    expect(el.querySelector('[data-testid=sunburst-live]')?.textContent).toBe('A, 500 KB, 9 items');
    expect(el.querySelector('[data-testid=sunburst-centre-name]')?.textContent).toBe('A');
    expect(el.textContent).toContain('50% of Root');
    const up = el.querySelector<HTMLButtonElement>('.pd-up');
    expect(up?.textContent).toContain('Up to Root');
    up?.click();
    await settle(f);
    expect(h.centre()).toBeNull();
    expect(h.loads).toEqual([]);
  });

  it('goes up one level at a time through the centre circle and does nothing at the root', async () => {
    const { f, h, el } = await setup();
    const circle = () => el.querySelector('circle.pd-centre-circle') as SVGCircleElement;
    circle().dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(h.centre()).toBeNull();
    h.centre.set('a1');
    await settle(f);
    expect(el.querySelector('.pd-up')?.textContent).toContain('Up to A');
    circle().dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await settle(f);
    expect(h.centre()).toBe('a');
    circle().dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await settle(f);
    expect(h.centre()).toBeNull();
  });

  it('drilling into a node with more to load asks the consumer', async () => {
    const { f, h, item } = await setup();
    item('b').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await settle(f);
    expect(h.centre()).toBe('b');
    expect(h.loads).toEqual(['b']);
  });

  it('opens folded and other segments as lists and reports the ones that cannot be drilled', async () => {
    const { f, h, item, el } = await setup();
    el.querySelector<SVGPathElement>('path[aria-label*="smaller items"]')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    item('oth').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    item('hid').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    item('c').dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await settle(f);
    expect(h.others).toEqual([
      { parent: 'root', ids: ['tiny1', 'tiny2', 'zero'] },
      { parent: 'root', ids: ['oth'] },
    ]);
    expect(h.activations.map((p) => p.node.id)).toEqual(['hid', 'c']);
    expect(h.centre()).toBeNull();
  });

  it('shows the hovered segment in the centre, fades the rest and reports the point', async () => {
    const { f, item, items, el, h } = await setup();
    item('a1').dispatchEvent(new MouseEvent('mouseenter'));
    await settle(f);
    const last = h.hovers[h.hovers.length - 1];
    expect(last?.path).toEqual(['a', 'a1']);
    expect(last?.share).toBeCloseTo(0.6);
    expect(item('a1').classList.contains('pd-arc--active')).toBe(true);
    expect(item('a').style.opacity).toBe('');
    expect(item('b').style.opacity).toBe('0.32');
    expect(items().filter((p) => p.style.opacity === '0.32').length).toBe(items().length - 2);
    expect(el.querySelector('[data-testid=sunburst-centre-name]')?.textContent).toBe('A1');
    const labels = Array.from(el.querySelectorAll<SVGTextElement>('text.pd-label'));
    expect(labels.filter((t) => t.style.opacity === '0').length).toBe(labels.length - 2);
    expect(el.textContent).toContain('60% of A');
    expect(el.querySelector('.pd-up')).toBeNull();
    el.querySelector('.pd-wrap')?.dispatchEvent(new MouseEvent('mouseleave'));
    el.querySelector('.pd-wrap')?.dispatchEvent(new MouseEvent('mouseleave'));
    await settle(f);
    expect(h.hovers[h.hovers.length - 1]).toBeNull();
    expect(h.hovers.length).toBe(2);
    expect(item('b').style.opacity).toBe('');
  });

  it('moves with the keyboard: arrows among siblings, Right and Left between levels, type-ahead, Enter drills', async () => {
    const { f, h, item, el } = await setup();
    const svg = el.querySelector('[role=tree]') as SVGElement;
    const press = async (key: string, init: KeyboardEventInit = {}) => {
      const e = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
      (document.activeElement && document.activeElement !== document.body ? document.activeElement : svg).dispatchEvent(e);
      await settle(f);
      await new Promise((r) => setTimeout(r));
      return e;
    };
    expect((await press('ArrowDown')).defaultPrevented).toBe(false);
    item('a').focus();
    await settle(f);
    expect(el.querySelector('.pd-focus-ring')).not.toBeNull();
    expect(h.hovers[h.hovers.length - 1]?.node.id).toBe('a');
    expect((await press('ArrowDown')).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(item('b'));
    await press('ArrowUp');
    expect(document.activeElement).toBe(item('a'));
    await press('ArrowRight');
    expect(document.activeElement).toBe(item('a1'));
    await press('ArrowLeft');
    expect(document.activeElement).toBe(item('a'));
    await press('c');
    expect(document.activeElement).toBe(item('c'));
    expect(item('c').getAttribute('tabindex')).toBe('0');
    expect((await press('ArrowDown', { ctrlKey: true })).defaultPrevented).toBe(false);
    expect((await press('Home', { metaKey: true })).defaultPrevented).toBe(false);
    expect((await press('ArrowUp', { altKey: true })).defaultPrevented).toBe(false);
    expect((await press('q')).defaultPrevented).toBe(false);
    await press('Home');
    await press('Enter');
    expect(h.centre()).toBe('a');
    expect(document.activeElement).toBe(item('a1'));
    await press(' ');
    expect(h.centre()).toBe('a');
    expect(h.activations.map((p) => p.node.id)).toEqual(['a1']);
    h.activations.length = 0;
    h.centre.set(null);
    await settle(f);
    item('hid').focus();
    await settle(f);
    await press('Enter');
    expect(h.activations.map((p) => p.node.id)).toEqual(['hid']);
    item('hid').blur();
    await settle(f);
    expect(el.querySelector('.pd-focus-ring')).toBeNull();
    expect(h.hovers[h.hovers.length - 1]).toBeNull();
  });

  it('opens the menu once per right-click (even after a real hover), Shift+F10 and the Menu key, and never for nothing', async () => {
    const { f, h, item, el } = await setup();
    const svg = el.querySelector('[role=tree]') as SVGElement;
    item('b').dispatchEvent(new MouseEvent('mouseenter'));
    await settle(f);
    const ev = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 12, clientY: 34 });
    item('b').dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
    expect(h.menus.length).toBe(1);
    expect(h.menus[0]).toMatchObject({ x: 12, y: 34 });
    expect(h.menus[0]?.point.node.id).toBe('b');
    const outside = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    svg.dispatchEvent(outside);
    expect(outside.defaultPrevented).toBe(false);
    expect(h.menus.length).toBe(1);
    item('c').focus();
    await settle(f);
    item('c').dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true }));
    item('c').dispatchEvent(new KeyboardEvent('keydown', { key: 'ContextMenu', bubbles: true }));
    item('c').dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', bubbles: true }));
    expect(h.menus.map((m) => m.point.node.id)).toEqual(['b', 'c', 'c']);
  });

  it('shows the skeleton, the empty state with its action, and the error with Retry', async () => {
    const { f, h, el } = await setup();
    h.state.set('loading');
    await settle(f);
    expect(el.querySelector('[data-testid=sunburst-loading]')?.getAttribute('role')).toBe('status');
    expect(el.querySelectorAll('.pd-skeleton').length).toBe(4);
    h.state.set('empty');
    await settle(f);
    expect(el.querySelector('[data-testid=sunburst-empty]')?.textContent).toContain('No data');
    expect(el.querySelector('[pdSunburstEmptyAction]')?.textContent).toBe('Scan');
    h.state.set('error');
    await settle(f);
    expect(el.querySelector('[data-testid=sunburst-error]')?.textContent).toContain('Could not read the disk.');
    el.querySelector<HTMLButtonElement>('.pd-retry')?.click();
    expect(h.retries).toBe(1);
    h.state.set('ready');
    h.root.set(null);
    await settle(f);
    expect(el.querySelector('[data-testid=sunburst-empty]')).not.toBeNull();
    expect(el.querySelector('[role=tree]')).toBeNull();
  });

  it('shows the empty state when nothing is drawable, and survives odd values', async () => {
    const { f, h, el, items } = await setup();
    h.root.set({ id: 'r', label: 'Disk', value: NaN, children: [leaf('x', -1)] });
    await settle(f);
    expect(items().length).toBe(0);
    expect(el.querySelector('[data-testid=sunburst-empty]')).not.toBeNull();
    h.root.set({ id: 'r', label: 'Disk', value: NaN, children: [leaf('x', 3), leaf('y', 1)] });
    await settle(f);
    expect(items().length).toBe(2);
    h.fmt.set((v) => String(v));
    await settle(f);
    expect(el.querySelector('.pd-centre-unit')).toBeNull();
    expect(el.querySelector('.pd-centre-value')?.textContent?.trim()).toBe('4');
    h.root.set({ id: 'r', label: 'Disk', value: 10, children: [leaf('x', 10)] });
    h.rings.set(NaN);
    await settle(f);
    expect(items().length).toBe(1);
  });

  it('words tiny and whole shares', async () => {
    const { f, h, item } = await setup();
    h.root.set({ id: 'r', label: 'R', value: 1_000_000, children: [leaf('big', 999_999), leaf('sliver', 1)] });
    await settle(f);
    expect(item('r\u0000folded').getAttribute('aria-label')).toBe('1 smaller item, 1 KB, <0.1% of R');
    expect(item('big').getAttribute('aria-label')).toBe('BIG, 999999 KB, 100% of R');
  });

  it('shows a custom centre', async () => {
    const { f, h, el, item } = await setup();
    h.custom.set(true);
    await settle(f);
    expect(el.querySelector('[data-testid=custom-centre]')?.textContent).toBe('Root!');
    item('a').dispatchEvent(new MouseEvent('mouseenter'));
    await settle(f);
    expect(el.querySelector('[data-testid=custom-centre]')?.textContent).toBe('A!');
  });

  it('reports a raised minimum angle', async () => {
    const { f, h } = await setup();
    h.root.set({ id: 'r', label: 'R', value: 4000, children: Array.from({ length: 200 }, (_, i) => leaf(`k${i}`, 20)) });
    h.maxSegments.set(10);
    await settle(f);
    expect(h.foldeds[h.foldeds.length - 1]?.segments).toBeLessThanOrEqual(10);
    expect(h.foldeds[h.foldeds.length - 1]?.minAngle).toBeGreaterThan(1.5);
  });

  it('colours by identity and keeps arcs fixed to a scale', async () => {
    const { f, h, item } = await setup();
    h.colourBy.set((n) => (n.id === 'a' ? 5 : 'neutral'));
    h.scale.set(2000);
    await settle(f);
    expect(item('a').style.fill).toContain('var(--pd-chart-6)');
    expect(item('b').style.fill).toContain('var(--pd-chart-8)');
  });

  it('follows reduced motion from the attribute or the input, and stops animating while live', async () => {
    const { f, h, el } = await setup();
    const host = el.querySelector('pd-sunburst') as HTMLElement;
    expect(host.classList.contains('pd-reduce')).toBe(false);
    expect(el.querySelector('.pd-arcs')?.classList.contains('pd-fade')).toBe(true);
    h.motion.set('reduce');
    h.live.set(true);
    await settle(f);
    expect(host.classList.contains('pd-reduce')).toBe(true);
    expect(el.querySelector('.pd-arcs')?.classList.contains('pd-fade')).toBe(false);
    h.motion.set('auto');
    document.documentElement.setAttribute('data-motion', 'reduce');
    await new Promise((r) => setTimeout(r));
    await settle(f);
    expect(host.classList.contains('pd-reduce')).toBe(true);
  });

  it('coalesces live updates to four per second and always shows the last', async () => {
    const { f, h, items } = await setup();
    h.live.set(true);
    await settle(f);
    const pending: (() => void)[] = [];
    const real = globalThis.setTimeout;
    vi.spyOn(globalThis, 'setTimeout').mockImplementation(((cb: () => void, ms?: number) => {
      if (ms && ms > 0 && ms <= 250) {
        pending.push(cb);
        return 0 as unknown as ReturnType<typeof setTimeout>;
      }
      return real(cb, ms);
    }) as typeof setTimeout);
    const grow = (n: number): PdSunburstNode => ({ id: 'root', label: 'Root', value: 100 * n, children: Array.from({ length: n }, (_, i) => leaf(`k${i}`, 100)) });
    now += 300;
    h.root.set(grow(2));
    await settle(f);
    expect(items().length).toBe(2);
    now += 100;
    h.root.set(grow(3));
    await settle(f);
    now += 50;
    h.root.set(grow(4));
    await settle(f);
    expect(items().length).toBe(2);
    now += 250;
    pending.forEach((cb) => cb());
    await settle(f);
    expect(items().length).toBe(4);
    now += 1000;
    h.root.set(grow(5));
    await settle(f);
    expect(items().length).toBe(5);
    h.root.set(null);
    await settle(f);
    expect(items().length).toBe(0);
    h.root.set(grow(6));
    await settle(f);
    expect(items().length).toBe(6);
    now += 10;
    h.root.set(grow(7));
    await settle(f);
    now += 10;
    h.root.set(grow(8));
    await settle(f);
    expect(items().length).toBe(6);
    f.destroy();
  });

  it('re-reads the palette when the theme changes', async () => {
    const { f, h, item } = await setup();
    document.documentElement.setAttribute('data-theme', 'light');
    await new Promise((r) => setTimeout(r));
    await settle(f);
    expect(item('a')).toBeDefined();
    expect(h.centre()).toBeNull();
    document.documentElement.removeAttribute('data-theme');
  });

  it('draws a full ring as two circles filled even-odd so it never paints over the ring inside', async () => {
    const { f, h, item, items } = await setup();
    h.root.set({ id: 'r', label: 'R', value: 10, children: [leaf('only', 10, { children: [leaf('deep', 10)] })] });
    await settle(f);
    expect(item('only').getAttribute('d')?.match(/M /g)?.length).toBe(2);
    expect(items().every((p) => p.getAttribute('fill-rule') === 'evenodd')).toBe(true);
  });

  it('keeps the way up in the tab order while a segment is focused, and goes up with Alt+Up, Command+Up and Backspace', async () => {
    const { f, h, item, el } = await setup();
    const up = () => el.querySelector<HTMLButtonElement>('.pd-up');
    expect(up()).toBeNull();
    const rootKey = new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true, cancelable: true });
    item('a').focus();
    item('a').dispatchEvent(rootKey);
    expect(rootKey.defaultPrevented).toBe(false);
    for (const init of [{ key: 'ArrowUp', altKey: true }, { key: 'ArrowUp', metaKey: true }, { key: 'Backspace' }]) {
      h.centre.set('a');
      await settle(f);
      item('a1').focus();
      await settle(f);
      expect(up()).not.toBeNull();
      expect(up()?.getAttribute('tabindex')).toBeNull();
      const e = new KeyboardEvent('keydown', { ...init, bubbles: true, cancelable: true });
      item('a1').dispatchEvent(e);
      await settle(f);
      await new Promise((r) => setTimeout(r));
      expect(e.defaultPrevented).toBe(true);
      expect(h.centre()).toBeNull();
      expect(document.activeElement).toBe(item('a'));
    }
    h.centre.set('a');
    await settle(f);
    item('a1').dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', ctrlKey: true, bubbles: true }));
    item('a1').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', ctrlKey: true, altKey: true, bubbles: true }));
    expect(h.centre()).toBe('a');
    up()?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    expect(h.centre()).toBe('a');
  });

  it('hides the up button visually while a segment is hovered, but keeps it in the DOM', async () => {
    const { f, h, item, el } = await setup();
    h.centre.set('a');
    await settle(f);
    expect(el.querySelector('.pd-up--hidden')).toBeNull();
    item('a1').dispatchEvent(new MouseEvent('mouseenter'));
    await settle(f);
    expect(el.querySelector('.pd-up--hidden')).not.toBeNull();
  });

  it('keeps the hovered folder lit together with its own descendants', async () => {
    const { f, item, el } = await setup();
    item('a').dispatchEvent(new MouseEvent('mouseenter'));
    await settle(f);
    expect(item('a1').style.opacity).toBe('');
    expect(item('a2').style.opacity).toBe('');
    expect(item('b').style.opacity).toBe('0.32');
    el.querySelector('.pd-wrap')?.dispatchEvent(new MouseEvent('mouseleave'));
  });

  it('sizes labels in screen pixels: smaller box, fewer and larger-in-units labels; ring one falls back to the name', async () => {
    const full = await setup();
    const count = full.el.querySelectorAll('text.pd-label').length;
    expect(count).toBeGreaterThan(0);
    expect((full.el.querySelector('text.pd-label') as SVGTextElement).style.fontSize).toBe('12px');
    full.h.fmt.set((v) => `${v}000000000000 KB`);
    await settle(full.f);
    const names = Array.from(full.el.querySelectorAll('text.pd-label')).map((t) => t.textContent?.trim());
    expect(names).toContain('C');
    document.body.innerHTML = '';
    const box = (width: number) => ({ width, height: width, top: 0, left: 0, right: width, bottom: width, x: 0, y: 0, toJSON: () => ({}) });
    const spy = vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(box(280));
    const narrow = await setup();
    const labels = Array.from(narrow.el.querySelectorAll<SVGTextElement>('text.pd-label'));
    expect(labels.length).toBeGreaterThan(0);
    labels.forEach((t) => expect(t.style.fontSize).toBe('24px'));
    document.body.innerHTML = '';
    spy.mockReturnValue(box(100));
    const tiny = await setup();
    expect(tiny.el.querySelectorAll('text.pd-label').length).toBe(0);
    spy.mockRestore();
  });

  it('gives folded sectors a pattern of their own and the ghost a visible dash', async () => {
    const { el, item } = await setup();
    expect(item('root\u0000folded').style.fill).toContain('-other');
    expect(item('oth').style.fill).toContain('-other');
    expect(el.querySelector('pattern[id$="-other"]')).not.toBeNull();
  });

  it('emits folded once per change, words folded items in the locale, and keeps the cap on the DOM', async () => {
    const { f, h, items } = await setup('de');
    const many = (n: number): PdSunburstNode => ({ id: 'r', label: 'R', value: 100_000, children: [leaf('big', 100_000 - n), ...Array.from({ length: n }, (_, i) => leaf(`s${i}`, 1))] });
    h.root.set(many(1234));
    await settle(f);
    expect(items().find((p) => p.getAttribute('data-id')?.endsWith('folded'))?.getAttribute('aria-label')).toContain('1.234 smaller items');
    h.maxSegments.set(2);
    h.root.set({ id: 'r', label: 'R', value: 4000, children: Array.from({ length: 200 }, (_, i) => leaf(`k${i}`, 20)) });
    await settle(f);
    const n = h.foldeds.length;
    expect(n).toBeGreaterThan(0);
    h.root.set({ id: 'r', label: 'R', value: 4000, children: Array.from({ length: 200 }, (_, i) => leaf(`k${i}`, 20)) });
    await settle(f);
    expect(h.foldeds.length).toBe(n);
    expect(items().length).toBeLessThanOrEqual(2);
  });

  it('marks a highlighted segment like a hover without emitting, and lets a real hover win', async () => {
    const { f, h, item, el } = await setup();
    h.highlight.set('a1');
    await settle(f);
    expect(item('a1').classList.contains('pd-arc--active')).toBe(true);
    expect(item('b').style.opacity).toBe('0.32');
    expect(el.querySelector('[data-testid=sunburst-centre-name]')?.textContent).toBe('A1');
    expect(h.hovers.length).toBe(0);
    item('c').dispatchEvent(new MouseEvent('mouseenter'));
    await settle(f);
    expect(item('c').classList.contains('pd-arc--active')).toBe(true);
    expect(h.hovers.length).toBe(1);
    el.querySelector('.pd-wrap')?.dispatchEvent(new MouseEvent('mouseleave'));
    await settle(f);
    expect(item('a1').classList.contains('pd-arc--active')).toBe(true);
    h.highlight.set('missing');
    await settle(f);
    expect(el.querySelector('.pd-arc--active')).toBeNull();
    h.highlight.set(null);
    await settle(f);
  });
});
