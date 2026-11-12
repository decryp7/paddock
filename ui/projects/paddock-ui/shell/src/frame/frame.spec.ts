import { ChangeDetectionStrategy, Component, inject, Injectable, InjectionToken, input, type OnDestroy, provideZonelessChangeDetection, signal, type Type } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import {
  defineStatusItem,
  type LazyPart,
  msg,
  provideContributions,
  provideShellContext,
  type StatusItemDef,
} from '@decryp7/paddock-ui/shell/contributions';
import { iconLetter, PdIcon, PdShellIcon, provideShellIcon } from './icon';
import { PD_ICON_PATHS } from '@decryp7/paddock-ui/icons';
import { PdPartOutlet } from './part-outlet';
import { cycleRegions, regionIndex, regionKey, regionsOf, regionTarget, REGION_ATTR, shown } from './regions';
import { PdStatusBar, STATUS_COMPACT } from './status-bar';
import { planStatusBar, statusWidth } from './status-layout';
import { PdTitleBar } from './title-bar';

async function settle(): Promise<void> {
  for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 6; j++) await Promise.resolve();
    TestBed.tick();
  }
}

/** A ResizeObserver driven by hand (jsdom has none). */
class FakeResizeObserver {
  static all: FakeResizeObserver[] = [];
  readonly targets = new Set<Element>();
  constructor(readonly callback: ResizeObserverCallback) {
    FakeResizeObserver.all.push(this);
  }
  observe(el: Element): void {
    this.targets.add(el);
  }
  unobserve(el: Element): void {
    this.targets.delete(el);
  }
  disconnect(): void {
    this.targets.clear();
  }
  fire(targets: Element[]): void {
    this.callback(targets.map((target) => ({ target }) as ResizeObserverEntry), this as unknown as ResizeObserver);
  }
}

describe('icons', () => {
  beforeEach(() => TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] }));

  it('draws a pd: path, and a letter for any other name', async () => {
    const fixture = TestBed.createComponent(PdShellIcon);
    fixture.componentRef.setInput('name', 'pd:home');
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('path').getAttribute('d')).toBe(PD_ICON_PATHS.home);
    fixture.componentRef.setInput('name', 'pw:models');
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('svg')).toBeNull();
    expect(fixture.nativeElement.textContent.trim()).toBe('M');
  });

  it('takes the letter of the last part, or a question mark', () => {
    expect(iconLetter('pw:models')).toBe('M');
    expect(iconLetter('sd.page/äpfel')).toBe('Ä');
    expect(iconLetter('')).toBe('?');
    expect(iconLetter('-')).toBe('?');
  });

  it('pd-icon uses the window’s icon component', async () => {
    @Component({ selector: 'my-icon', template: `my:{{ name() }}`, changeDetection: ChangeDetectionStrategy.OnPush })
    class MyIcon {
      readonly name = input('');
    }
    TestBed.configureTestingModule({ providers: [provideShellIcon(MyIcon)] });
    const fixture = TestBed.createComponent(PdIcon);
    fixture.componentRef.setInput('name', 'pw:x');
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toBe('my:pw:x');
  });
});

describe('regions', () => {
  function dom(): HTMLElement {
    const root = document.createElement('div');
    root.innerHTML = `
      <header ${REGION_ATTR}="title" tabindex="-1"><button id="menu">File</button></header>
      <nav ${REGION_ATTR}="sidebar" tabindex="-1"><a href="#a" id="a">A</a><a href="#b" id="b" aria-current="page">B</a></nav>
      <main ${REGION_ATTR}="main" tabindex="-1" id="main"><p>text</p></main>
      <footer ${REGION_ATTR}="status" hidden tabindex="-1"></footer>
      <button id="outside">x</button>`;
    document.body.append(root);
    return root;
  }
  const all = () => true;
  const notHidden = (el: HTMLElement) => !el.hidden;

  afterEach(() => (document.body.innerHTML = ''));

  it('lists the regions on screen in document order', () => {
    const root = dom();
    expect(regionsOf(root, notHidden).map((r) => r.getAttribute(REGION_ATTR))).toEqual(['title', 'sidebar', 'main']);
    expect(regionsOf(root, all)).toHaveLength(4);
    // jsdom lays nothing out: nothing counts as shown by default.
    expect(shown(root)).toBe(false);
    expect(regionsOf(root)).toEqual([]);
  });

  it('focuses the current item, else the first control, else the region', () => {
    const root = dom();
    const [title, sidebar, main] = regionsOf(root, notHidden) as [HTMLElement, HTMLElement, HTMLElement];
    expect(regionTarget(title).id).toBe('menu');
    expect(regionTarget(sidebar).id).toBe('b');
    expect(regionTarget(main).id).toBe('main');
    expect(regionIndex([title, sidebar], root.querySelector('#a'))).toBe(1);
    expect(regionIndex([title, sidebar], null)).toBe(-1);
  });

  it('cycles forwards and backwards, wrapping, and starts at an end from outside', () => {
    const root = dom();
    const active = () => document.activeElement?.id;
    expect(cycleRegions(root, 1, root.querySelector('#outside'), notHidden)).toBe(true);
    expect(active()).toBe('menu');
    cycleRegions(root, 1, document.activeElement, notHidden);
    expect(active()).toBe('b');
    cycleRegions(root, 1, document.activeElement, notHidden);
    expect(active()).toBe('main');
    cycleRegions(root, 1, document.activeElement, notHidden);
    expect(active()).toBe('menu');
    cycleRegions(root, -1, document.activeElement, notHidden);
    expect(active()).toBe('main');
    cycleRegions(root, -1, null, notHidden);
    expect(active()).toBe('main');
    expect(cycleRegions(document.createElement('div'), 1, null, all)).toBe(false);
  });

  it('reads F6 and Shift+F6 only', () => {
    const k = (key: string, mods: Partial<KeyboardEvent> = {}) => ({ key, shiftKey: false, ctrlKey: false, altKey: false, metaKey: false, ...mods });
    expect(regionKey(k('F6'))).toBe(1);
    expect(regionKey(k('F6', { shiftKey: true }))).toBe(-1);
    expect(regionKey(k('F6', { ctrlKey: true }))).toBeNull();
    expect(regionKey(k('F7'))).toBeNull();
  });
});

describe('planStatusBar', () => {
  const items = [
    { id: 'health', priority: 1000, width: 120 },
    { id: 'jobs', priority: 900, width: 100 },
    { id: 'bell', priority: 800, width: 30 },
    { id: 'greet', priority: 50, width: 80 },
  ];

  it('shows everything that fits', () => {
    expect(planStatusBar(items, 1000, 8, 28)).toEqual({ shown: ['health', 'jobs', 'bell', 'greet'], hidden: [] });
  });

  it('hides the lowest priority first and keeps room for the overflow button', () => {
    // 128 + 108 + 38 + 88 = 362: without greet 274 fits 310 with the button's 28 kept free, not 290.
    expect(planStatusBar(items, 310, 8, 28)).toEqual({ shown: ['health', 'jobs', 'bell'], hidden: ['greet'] });
    expect(planStatusBar(items, 290, 8, 28).hidden).toEqual(['greet', 'bell']);
  });

  it('never hides a pinned item; any other may go when there is no room', () => {
    const pinned = items.map((i) => (i.id === 'bell' || i.id === 'health' ? { ...i, pinned: true } : i));
    expect(planStatusBar(pinned, 10, 8, 28)).toEqual({ shown: ['health', 'bell'], hidden: ['greet', 'jobs'] });
    // 128 + 38 pinned leave 164 of 330; without greet, jobs (108) fits beside the button's 28.
    expect(planStatusBar(pinned, 330, 8, 28)).toEqual({ shown: ['health', 'jobs', 'bell'], hidden: ['greet'] });
    expect(planStatusBar(pinned, 300, 8, 28).hidden).toEqual(['greet', 'jobs']);
    expect(planStatusBar(items, 10, 8, 28)).toEqual({ shown: [], hidden: ['greet', 'bell', 'jobs', 'health'] });
    expect(planStatusBar([{ id: 'x', priority: 1, width: -5 }], -1, 0, 0).shown).toEqual(['x']);
    expect(statusWidth([{ width: 10 }, { width: -3 }], 8)).toBe(26);
  });
});

describe('pd-title-bar', () => {
  it('shows the name between the slots', async () => {
    @Component({
      imports: [PdTitleBar],
      changeDetection: ChangeDetectionStrategy.OnPush,
      template: `<pd-title-bar appName="Pitwall"><button slot="lead">L</button><span>menus</span><b slot="end">E</b></pd-title-bar>`,
    })
    class Host {}
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const fixture = TestBed.createComponent(Host);
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    expect(el.querySelector('.pd-lead')?.textContent).toBe('L');
    expect(el.querySelector('[data-testid="app-name"]')?.textContent).toBe('Pitwall');
    expect(el.querySelector('.pd-menus')?.textContent).toBe('menus');
    expect(el.querySelector('.pd-end')?.textContent).toBe('E');
  });
});

const TOKEN = new InjectionToken<string>('part token');

@Component({ selector: 'part-a', template: `A {{ token }}`, changeDetection: ChangeDetectionStrategy.OnPush })
class PartA {
  readonly token = inject(TOKEN, { optional: true });
}

@Component({ selector: 'part-boom', template: ``, changeDetection: ChangeDetectionStrategy.OnPush })
class PartBoom {
  constructor() {
    throw new Error('no provider for the store');
  }
}

describe('pd-part-outlet', () => {
  @Component({
    imports: [PdPartOutlet],
    changeDetection: ChangeDetectionStrategy.OnPush,
    template: `
      @if (on()) {
        <pd-part-outlet [part]="part()" label="Models" [variant]="variant()" />
      }
    `,
  })
  class Host {
    readonly part = signal<LazyPart>({ load: () => Promise.resolve(PartA) });
    readonly variant = signal<'page' | 'inline'>('page');
    readonly on = signal(true);
  }

  beforeEach(() => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
  });

  async function mount(part: LazyPart, variant: 'page' | 'inline' = 'page') {
    const fixture = TestBed.createComponent(Host);
    fixture.componentInstance.part.set(part);
    fixture.componentInstance.variant.set(variant);
    await settle();
    await fixture.whenStable();
    const outlet = () => fixture.nativeElement.querySelector('pd-part-outlet') as HTMLElement;
    return { fixture, outlet };
  }

  it('shows a skeleton while loading, then the component in an injector with the part’s providers', async () => {
    let resolve!: (t: Type<unknown>) => void;
    const { fixture, outlet } = await mount({ load: () => new Promise((r) => (resolve = r)), providers: () => Promise.resolve([{ provide: TOKEN, useValue: 'scoped' }]) });
    expect(outlet().getAttribute('data-part-status')).toBe('loading');
    expect(outlet().querySelector('[data-testid="part-loading"]')?.getAttribute('aria-label')).toBe('Loading Models');
    resolve(PartA);
    await settle();
    await fixture.whenStable();
    expect(outlet().getAttribute('data-part-status')).toBe('loaded');
    expect(outlet().textContent).toContain('A scoped');
  });

  it('contains a failed load: an error with Try again that loads again', async () => {
    let fail = true;
    const { fixture, outlet } = await mount({ load: () => (fail ? Promise.reject(new Error('chunk missing')) : Promise.resolve(PartA)) });
    expect(outlet().getAttribute('data-part-status')).toBe('error');
    expect(outlet().textContent).toContain('Models could not be opened.');
    expect(outlet().textContent).toContain('chunk missing');
    fail = false;
    outlet().querySelector<HTMLButtonElement>('pd-error-state button')?.click();
    await settle();
    await fixture.whenStable();
    expect(outlet().textContent).toContain('A');
    expect(outlet().getAttribute('data-part-status')).toBe('loaded');
  });

  it('contains a component that throws when created, and destroys its injector', async () => {
    const { outlet } = await mount({ load: () => Promise.resolve(PartBoom), providers: () => Promise.resolve([{ provide: TOKEN, useValue: 'x' }]) });
    expect(outlet().getAttribute('data-part-status')).toBe('error');
    expect(outlet().textContent).toContain('no provider for the store');
  });

  it('inline: nothing while loading, a small Retry button on failure', async () => {
    let fail = true;
    const { fixture, outlet } = await mount({ load: () => (fail ? Promise.reject('offline') : Promise.resolve(PartA)) }, 'inline');
    const retry = outlet().querySelector<HTMLButtonElement>('button[data-testid="part-error"]');
    expect(retry?.getAttribute('aria-label')).toBe('Models did not load. Try again');
    expect(retry?.hasAttribute('title')).toBe(false);
    retry?.dispatchEvent(new MouseEvent('mouseenter'));
    await vi.waitFor(() => expect(document.querySelector('.mat-mdc-tooltip')?.textContent?.trim()).toBe('Models did not load: offline'));
    retry?.dispatchEvent(new MouseEvent('mouseleave'));
    fail = false;
    retry?.click();
    await settle();
    await fixture.whenStable();
    expect(outlet().textContent?.trim()).toBe('A');
  });

  it('drops a load that finishes after a newer part replaced it, and cleans up on destroy', async () => {
    let resolveOld!: (t: Type<unknown>) => void;
    const destroyed: string[] = [];
    @Injectable()
    class Tracker implements OnDestroy {
      constructor() {
        destroyed.push('created');
      }
      ngOnDestroy(): void {
        destroyed.push('destroyed');
      }
    }
    const { fixture, outlet } = await mount({ load: () => new Promise((r) => (resolveOld = r)), providers: () => Promise.resolve([Tracker]) });
    fixture.componentInstance.part.set({ load: () => Promise.resolve(PartA), providers: () => Promise.resolve([{ provide: TOKEN, useValue: 'new' }]) });
    await settle();
    await fixture.whenStable();
    expect(outlet().textContent).toContain('A new');
    resolveOld(PartA);
    await settle();
    await fixture.whenStable();
    expect(outlet().textContent).toContain('A new');
    // A load that fails after it was replaced is dropped too.
    fixture.componentInstance.part.set({ load: () => Promise.reject(new Error('late')) });
    fixture.componentInstance.part.set({ load: () => Promise.resolve(PartA) });
    await settle();
    await fixture.whenStable();
    expect(outlet().getAttribute('data-part-status')).toBe('loaded');
    fixture.componentInstance.on.set(false);
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('pd-part-outlet')).toBeNull();
  });
});

@Component({ selector: 'status-a', template: `{{ compact() ? 'A' : 'Alpha' }}`, changeDetection: ChangeDetectionStrategy.OnPush })
class StatusA {
  protected readonly compact = inject(STATUS_COMPACT);
}

describe('pd-status-bar', () => {
  const item = (id: string, side: 'start' | 'end', priority: number, extra: Partial<StatusItemDef<{ show: () => boolean }>> = {}) =>
    defineStatusItem<{ show: () => boolean }>({ id, title: msg(`t.${id}`, id), side, priority, load: () => Promise.resolve(StatusA), ...extra });
  const show = signal(true);

  function setup(options: { observer?: boolean } = {}) {
    FakeResizeObserver.all = [];
    if (options.observer !== false) vi.stubGlobal('ResizeObserver', FakeResizeObserver);
    else vi.stubGlobal('ResizeObserver', undefined);
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        provideContributions<{ show: () => boolean }>({
          statusItems: [
            item('t.greet', 'start', 50),
            item('t.bell', 'end', 800),
            item('t.jobs', 'end', 900),
            item('t.health', 'end', 1000),
            item('t.cond', 'end', 10, { when: (ctx) => ctx.show() }),
            item('t.tie', 'end', 800),
          ],
        }),
        provideShellContext(() => Promise.resolve(() => ({ show: () => show() })), { prefetch: 'first-use' }),
      ],
    });
  }

  afterEach(() => vi.unstubAllGlobals());

  it('lays out the items by side and priority; an item whose when is false is not shown', async () => {
    setup();
    const fixture = TestBed.createComponent(PdStatusBar);
    await settle();
    await fixture.whenStable();
    const ids = (side: string) => [...fixture.nativeElement.querySelectorAll(`.pd-${side} [data-status-item]`)].map((e) => (e as HTMLElement).dataset['statusItem']);
    expect(ids('start')).toEqual(['t.greet']);
    // Before the app's context is loaded every item is offered.
    expect(ids('end')).toEqual(['t.health', 't.jobs', 't.bell', 't.tie', 't.cond']);
    expect(fixture.nativeElement.textContent).toContain('Alpha');
    show.set(false);
    await TestBed.inject((await import('@decryp7/paddock-ui/shell/contributions')).ShellContextHost).load();
    await settle();
    await fixture.whenStable();
    expect(ids('end')).not.toContain('t.cond');
    show.set(true);
  });

  it('hides the lowest priorities when narrow; the overflow button shows them all and back', async () => {
    setup();
    const fixture = TestBed.createComponent(PdStatusBar);
    await settle();
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    const observer = FakeResizeObserver.all[0] as FakeResizeObserver;
    const items = [...el.querySelectorAll<HTMLElement>('[data-status-item]')];
    for (const i of items) i.getBoundingClientRect = () => ({ width: 60 }) as DOMRect;
    Object.defineProperty(el, 'clientWidth', { value: 260, configurable: true });
    observer.fire([el, ...items]);
    // The full forms (6 x 64 px) do not fit 260 px: the items turn compact. Until they are measured so, nothing is hidden
    // for its full width (regression: the jobs item blinked out for a frame when the window narrowed, review N1).
    const bar = fixture.componentInstance;
    expect(bar.compact()).toBe(true);
    expect([...bar.hidden()]).toEqual([]);
    // Unknown targets and unchanged widths are ignored.
    observer.fire([document.createElement('div'), items[0] as HTMLElement]);
    await fixture.whenStable();
    await settle();
    const hidden = () => [...el.querySelectorAll<HTMLElement>('[data-status-item].pd-hidden')].map((e) => e.dataset['statusItem']);
    expect(el.classList).toContain('pd-compact');
    expect(el.querySelector('[data-status-item="t.jobs"]')?.textContent?.trim()).toBe('A');
    // Re-measured in the compact form once rendered (60 px here too): the three lowest give way.
    expect(hidden()).toEqual(['t.greet', 't.tie', 't.cond']);
    expect(el.querySelector('[data-status-item="t.greet"]')?.hasAttribute('inert')).toBe(true);
    const more = el.querySelector<HTMLButtonElement>('[data-testid="status-more"]') as HTMLButtonElement;
    expect(more.getAttribute('aria-label')).toBe('More status (3)');
    expect(more.textContent?.trim()).toBe('3');
    expect(more.getAttribute('aria-expanded')).toBe('false');
    expect(more.hasAttribute('title')).toBe(false);
    more.dispatchEvent(new MouseEvent('mouseenter'));
    await vi.waitFor(() => expect(document.querySelector('.mat-mdc-tooltip')?.textContent?.trim()).toBe('More status (3)'));
    more.dispatchEvent(new MouseEvent('mouseleave'));
    more.click();
    await fixture.whenStable();
    expect(hidden()).toEqual([]);
    expect(el.classList).toContain('pd-all');
    expect(more.getAttribute('aria-label')).toBe('Show fewer status items');
    more.click();
    await fixture.whenStable();
    expect(hidden()).toHaveLength(3);
    // Measured compact (40 px each, 44 with the 4 px gap; 260 px, jsdom has no padding): the two lowest give way.
    for (const i of items) i.getBoundingClientRect = () => ({ width: 40 }) as DOMRect;
    observer.fire(items);
    await fixture.whenStable();
    expect(hidden()).toEqual(['t.greet', 't.cond']);
    // Wide enough for the full forms again: nothing compact, nothing hidden.
    Object.defineProperty(el, 'clientWidth', { value: 600, configurable: true });
    observer.fire([el]);
    await fixture.whenStable();
    expect(el.classList).not.toContain('pd-compact');
    expect(hidden()).toEqual([]);
    fixture.destroy();
    expect(observer.targets.size).toBe(0);
  });

  it('shows every item where it cannot measure', async () => {
    setup({ observer: false });
    const fixture = TestBed.createComponent(PdStatusBar);
    await settle();
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelectorAll('[data-status-item].pd-hidden')).toHaveLength(0);
    expect(fixture.nativeElement.querySelector('[data-testid="status-more"]')).toBeNull();
  });
});
