import { ApplicationRef, ChangeDetectionStrategy, Component, Injectable, inject, input, provideZonelessChangeDetection, signal, type Type } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Title } from '@angular/platform-browser';
import { ActivatedRouteSnapshot, provideRouter, Router, type Route } from '@angular/router';
import {
  CommandService,
  definePage,
  msg,
  provideContributions,
  provideShellContext,
  SHELL_PORTS,
  ShellContextHost,
  type PageDef,
} from '@decryp7/paddock-ui/shell';
import { MemoryJobsBackend } from '@decryp7/paddock-ui/shell/jobs';
import { Keymap, NATIVE_MENU_GATE, RESERVED_KEYS } from '@decryp7/paddock-ui/shell/menu';
import { MemoryNotificationCentre, MemoryServiceHealth, NotificationsStore } from '@decryp7/paddock-ui/shell/notify';
import { MockStartupBackend, UI_CHECKPOINT, WindowReady } from '@decryp7/paddock-ui/shell/startup';
import { PdShellFrame } from './frame';
import { PdPageHost } from './page-host';
import { badgeText, PAGE_DATA, pageOfSnapshot, pageOrder, pagePath, ShellPages } from './pages';
import { loadCollapsed, PAGES_FRAME_KEY, PAGES_FRAME_STORAGE, PagesFrameStore, saveCollapsed, type PagesFrameStorage } from './pages-frame.store';
import { pagesRoutes, PagesTitleStrategy, provideShellPages, type ShellPagesOptions } from './provide-pages';
import { PdPagesSidebar } from './sidebar';
import { sidebarFit, sidebarMode, sidebarSizes, SIDEBAR_SIZES } from './sidebar-plan';

async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    for (let j = 0; j < 8; j++) await Promise.resolve();
    await new Promise((r) => setTimeout(r, 0));
    TestBed.tick();
  }
}

class MemoryStorage implements PagesFrameStorage {
  readonly items = new Map<string, string>();
  fail = false;
  getItem(key: string): string | null {
    return this.items.get(key) ?? null;
  }
  setItem(key: string, value: string): void {
    if (this.fail) throw new Error('quota');
    this.items.set(key, value);
  }
}

describe('sidebar plan', () => {
  it('expands, rails or overlays by the layout plan', () => {
    expect(sidebarFit(1440)).toBe('expanded');
    expect(sidebarFit(776)).toBe('expanded');
    expect(sidebarFit(775)).toBe('rail');
    expect(sidebarFit(400)).toBe('rail');
    expect(sidebarFit(399)).toBe('overlay');
    expect(sidebarFit(320)).toBe('overlay');
    expect(sidebarFit(0)).toBe('overlay');
  });

  it('a collapsed choice turns the expanded sidebar into the rail only', () => {
    expect(sidebarMode('expanded', true)).toBe('rail');
    expect(sidebarMode('expanded', false)).toBe('expanded');
    expect(sidebarMode('overlay', true)).toBe('overlay');
  });

  it('takes valid overrides only', () => {
    expect(sidebarSizes()).toEqual(SIDEBAR_SIZES);
    expect(sidebarSizes({ pageMin: 300, rail: -1, expanded: Number.NaN })).toEqual({ ...SIDEBAR_SIZES, pageMin: 300 });
    expect(sidebarFit(600, sidebarSizes({ pageMin: 300 }))).toBe('expanded');
  });
});

describe('the sidebar choice in storage', () => {
  it('round-trips, and anything missing, corrupt or of another version is not collapsed', () => {
    const storage = new MemoryStorage();
    expect(loadCollapsed(storage)).toBe(false);
    expect(saveCollapsed(true, storage)).toBe(true);
    expect(loadCollapsed(storage)).toBe(true);
    storage.items.set(PAGES_FRAME_KEY, '{"version":2,"collapsed":true}');
    expect(loadCollapsed(storage)).toBe(false);
    storage.items.set(PAGES_FRAME_KEY, '{nope');
    expect(loadCollapsed(storage)).toBe(false);
    expect(loadCollapsed(null)).toBe(false);
    expect(saveCollapsed(true, null)).toBe(false);
    storage.fail = true;
    expect(saveCollapsed(true, storage)).toBe(false);
  });
});

describe('the default sidebar storage', () => {
  it('is localStorage, or none where reading it throws', () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    expect(TestBed.inject(PAGES_FRAME_STORAGE)).toBe(globalThis.localStorage);
    TestBed.resetTestingModule();
    const spy = vi.spyOn(globalThis, 'localStorage', 'get').mockImplementation(() => {
      throw new Error('denied');
    });
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    expect(TestBed.inject(PAGES_FRAME_STORAGE)).toBeNull();
    spy.mockRestore();
  });
});

describe('PagesFrameStore', () => {
  function store(storage: MemoryStorage | null = new MemoryStorage()) {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), PagesFrameStore, { provide: PAGES_FRAME_STORAGE, useValue: storage }] });
    return TestBed.inject(PagesFrameStore);
  }

  it('follows the width; the button collapses and expands where the sidebar fits, and keeps the choice', () => {
    const storage = new MemoryStorage();
    const s = store(storage);
    s.measure(1200);
    expect([s.mode(), s.labelled(), s.drawer()]).toEqual(['expanded', true, false]);
    s.toggle();
    expect([s.mode(), s.labelled()]).toEqual(['rail', false]);
    expect(loadCollapsed(storage)).toBe(true);
    s.toggle();
    expect(s.mode()).toBe('expanded');
    expect(loadCollapsed(storage)).toBe(false);
  });

  it('opens the sidebar over the page where it does not fit, and closes it when it fits again', () => {
    const s = store();
    s.measure(320);
    expect(s.mode()).toBe('overlay');
    s.toggle();
    expect([s.drawer(), s.labelled()]).toEqual([true, true]);
    s.toggle();
    expect(s.drawer()).toBe(false);
    s.measure(500);
    s.toggle();
    expect([s.mode(), s.drawer()]).toEqual(['rail', true]);
    s.measure(500);
    s.measure(Number.NaN);
    s.measure(-3);
    expect(s.drawer()).toBe(true);
    s.measure(1000);
    expect([s.mode(), s.drawer()]).toEqual(['expanded', false]);
    s.closeDrawer();
    expect(s.drawer()).toBe(false);
  });

  it('a choice that cannot be stored lasts for the session', () => {
    const storage = new MemoryStorage();
    storage.fail = true;
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const s = store(storage);
    s.measure(1200);
    s.setCollapsed(true);
    expect(s.mode()).toBe('rail');
    expect(warn).toHaveBeenCalled();
  });

  it('starts from the window width, or 1280 where there is none', () => {
    const inner = globalThis.innerWidth;
    Object.defineProperty(globalThis, 'innerWidth', { value: 0, configurable: true });
    try {
      expect(store(null).width()).toBe(1280);
    } finally {
      Object.defineProperty(globalThis, 'innerWidth', { value: inner, configurable: true });
    }
  });
});

describe('page helpers', () => {
  const page = (id: string, order: number, section?: 'top' | 'bottom') => definePage({ id, title: msg(id, id), icon: 'pd:home', order, section, load: () => Promise.resolve(Object) });

  it('derives paths from ids, with valid overrides', () => {
    expect(pagePath('pw.models')).toBe('models');
    expect(pagePath('pw.models.library')).toBe('models/library');
    expect(pagePath('single')).toBe('single');
    expect(pagePath('pw.models', { 'pw.models': 'library/all' })).toBe('library/all');
    expect(pagePath('pw.models', { 'pw.models': '../Bad Path' })).toBe('models');
  });

  it('orders top before bottom, then by order and id', () => {
    const pages = [page('a.z', 100, 'bottom'), page('a.b', 200), page('a.c', 100), page('a.a', 100)];
    expect([...pages].sort(pageOrder).map((p) => p.id)).toEqual(['a.a', 'a.c', 'a.b', 'a.z']);
  });

  it('draws badges for positive numbers and non-blank text', () => {
    expect([badgeText(3), badgeText(0), badgeText(-1), badgeText(Number.NaN), badgeText(' new '), badgeText('  '), badgeText(null), badgeText({})]).toEqual([
      '3',
      null,
      null,
      null,
      'new',
      null,
      null,
      null,
    ]);
  });

  it('finds the page of the deepest route', () => {
    const snap = (data: Record<string, unknown>, child: ActivatedRouteSnapshot | null) => ({ data, firstChild: child }) as unknown as ActivatedRouteSnapshot;
    expect(pageOfSnapshot(snap({}, snap({ [PAGE_DATA]: 'a.b' }, snap({}, null))))).toBe('a.b');
    expect(pageOfSnapshot(snap({}, null))).toBeNull();
  });

  it('routes the empty path and anything unknown to the first page, after the app’s routes', () => {
    const routes = pagesRoutes([page('a.b', 200), page('a.a', 100)], {}, [{ path: 'tray', children: [] }]);
    expect(routes.map((r) => r.path)).toEqual(['', 'tray', 'a', 'b', '**']);
    expect((routes[2] as Route).title).toBe('a.a');
    expect((routes[2] as Route).canMatch).toBeUndefined();
  });
});

// ---------- The whole frame ----------

interface Ctx {
  readonly count: () => number;
  readonly fail: () => boolean;
}
const count = signal(0);

@Component({ selector: 'p-one', template: `<h1>Page one</h1><input data-testid="field" />`, changeDetection: ChangeDetectionStrategy.OnPush })
class PageOne {}
@Injectable()
class Scoped {
  readonly name = 'scoped';
}
@Component({ selector: 'p-two', template: `<h1>Page two {{ scoped.name }}</h1>`, changeDetection: ChangeDetectionStrategy.OnPush })
class PageTwo {
  readonly scoped = inject(Scoped);
}

const PAGES: PageDef<Ctx>[] = [
  definePage<Ctx>({ id: 't.one', title: msg('t.one', 'One'), icon: 'pd:home', order: 100, load: () => Promise.resolve(PageOne) }),
  definePage<Ctx>({
    id: 't.two',
    title: msg('t.two', 'Two'),
    icon: 'pd:window',
    order: 200,
    badge: (ctx) => ctx.count(),
    badgeLabel: (ctx) => (ctx.count() > 5 ? `${ctx.count()} things` : null),
    providers: () => Promise.resolve([Scoped]),
    load: () => Promise.resolve(PageTwo as Type<unknown>),
  }),
  definePage<Ctx>({ id: 't.lab', title: msg('t.lab', 'Lab'), icon: 't:lab', order: 300, when: (ctx) => ctx.count() > 0, load: () => Promise.resolve(PageOne) }),
  definePage<Ctx>({ id: 't.broken', title: msg('t.broken', 'Broken'), icon: 'pd:info', order: 400, when: (ctx) => ctx.fail(), load: () => Promise.resolve(PageOne) }),
  definePage<Ctx>({ id: 't.about', title: msg('t.about', 'About'), icon: 'pd:info', order: 100, section: 'bottom', load: () => Promise.resolve(PageOne) }),
];

@Component({ changeDetection: ChangeDetectionStrategy.OnPush, template: `<span data-testid="detail">{{ runnerId() }}</span>` })
class Detail {
  readonly runnerId = input.required<string>();
}

@Component({ imports: [PdShellFrame], changeDetection: ChangeDetectionStrategy.OnPush, template: `<pd-shell-frame><span slot="title-end">END</span><p slot="sidebar">FOOT</p></pd-shell-frame>` })
class Root {}

describe('pd-shell-frame with provideShellPages', () => {
  let storage: MemoryStorage;
  
  async function boot(options: Partial<ShellPagesOptions> = {}, contextFails = false) {
    count.set(0);
    storage = new MemoryStorage();
    vi.stubGlobal('ResizeObserver', undefined);
    const centre = new MemoryNotificationCentre();
    const health = new MemoryServiceHealth([{ id: 't.engine', label: 'the engine' }], { centre });
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        provideContributions<Ctx>({ pages: PAGES }),
        provideShellContext<Ctx>(
          () => (contextFails ? Promise.reject(new Error('no context')) : Promise.resolve(() => ({ count: () => count(), fail: () => { throw new Error('x'); } }))),
          { prefetch: 'first-use' },
        ),
        provideShellPages({
          appName: 'Test app',
          notify: { health: () => health, notifications: () => centre },
          jobs: { backend: () => new MemoryJobsBackend() },
          startup: { backend: () => new MockStartupBackend() },
          ...options,
        }),
        { provide: PAGES_FRAME_STORAGE, useValue: storage },
      ],
    });
    const fixture = TestBed.createComponent(Root);
    const router = TestBed.inject(Router);
    await router.navigateByUrl('/');
    await settle();
    await fixture.whenStable();
    const el: HTMLElement = fixture.nativeElement;
    const frame = el.querySelector('pd-shell-frame') as HTMLElement;
    return { fixture, router, el, frame };
  }

  afterEach(() => vi.unstubAllGlobals());

  it('shows the title bar, the pages, the first page and the status bar; the window title names the page', async () => {
    const { el, router } = await boot();
    expect(el.querySelector('[data-testid="app-name"]')?.textContent).toBe('Test app');
    expect(el.querySelector('.pd-end')?.textContent).toBe('END');
    expect(el.querySelector('.pd-slot')?.textContent).toBe('FOOT');
    expect(router.url).toBe('/one');
    expect(el.querySelector('main h1')?.textContent).toBe('Page one');
    const links = [...el.querySelectorAll<HTMLAnchorElement>('nav a.pd-item')].map((a) => a.dataset['pageLink']);
    // Before the context is loaded every page is offered.
    expect(links).toEqual(['t.one', 't.two', 't.lab', 't.broken', 't.about']);
    expect(el.querySelector('a[aria-current="page"]')?.getAttribute('data-page-link')).toBe('t.one');
    expect(el.querySelector('footer[aria-label="Status bar"] pd-status-bar')).not.toBeNull();
    expect(el.querySelector('pd-notification-host')).not.toBeNull();
    expect(el.querySelector('pd-dialog-host')).not.toBeNull();
    expect(TestBed.inject(Title).getTitle()).toBe('One – Test app');
    expect(TestBed.inject(RESERVED_KEYS).map((k) => k.key)).toEqual(['F6', 'Shift+F6']);
  });

  it('binds route parameters to a routed component input, so a detail route needs no wrapper', async () => {
    const { router, el } = await boot({ routes: [{ path: 'runner/:runnerId', component: Detail }] });
    await router.navigateByUrl('/runner/r-42');
    await settle();
    await TestBed.inject(ApplicationRef).whenStable();
    expect(el.querySelector('[data-testid="detail"]')?.textContent).toBe('r-42');
  });

  it('installs the native-feel guard, and leaves it out when asked; the frame draws the Copy menu host', async () => {
    const press = () => {
      const e = new KeyboardEvent('keydown', { key: 'F5', bubbles: true, cancelable: true });
      document.body.dispatchEvent(e);
      return e.defaultPrevented;
    };
    const { el, fixture } = await boot();
    expect(press()).toBe(true);
    expect(el.querySelector('pd-copy-menu')).not.toBeNull();
    fixture.destroy();
    TestBed.resetTestingModule();
    await boot({ nativeFeel: false });
    expect(press()).toBe(false);
  });

  it('loads a page with its own providers, and badges and conditions follow the context', async () => {
    const { el, router, fixture } = await boot();
    await TestBed.inject(ShellContextHost).load();
    await settle();
    expect([...el.querySelectorAll<HTMLAnchorElement>('nav a.pd-item')].map((a) => a.dataset['pageLink'])).toEqual(['t.one', 't.two', 't.about']);
    count.set(3);
    await settle();
    await fixture.whenStable();
    expect(el.querySelector('[data-page-link="t.two"] .mat-badge-content')?.textContent).toBe('3');
    // A badge without a label keeps its count in the link's name (the badge itself is aria-hidden), and the row-end count is the badge.
    expect(el.querySelector('[data-page-link="t.two"]')?.getAttribute('aria-label')).toBe('Two, 3');
    expect(el.querySelector('[data-page-link="t.two"] .pd-count .mat-badge-content')?.textContent).toBe('3');
    expect(el.querySelector('[data-page-link="t.one"]')?.hasAttribute('aria-label')).toBe(false);
    // Expanded, a link has a tooltip only to say what its badge counts.
    count.set(6);
    await settle();
    await fixture.whenStable();
    expect(el.querySelector('[data-page-link="t.two"]')?.getAttribute('aria-label')).toBe('Two, 6 things');
    // The name already says what the count is, so the labelled sidebar adds no tooltip that would read it twice.
    el.querySelector('[data-page-link="t.two"]')?.dispatchEvent(new MouseEvent('mouseenter'));
    expect(document.querySelector('.mat-mdc-tooltip')).toBeNull();
    expect(el.querySelector('[data-page-link="t.two"]')?.hasAttribute('aria-describedby')).toBe(false);
    // The page groups are lists, not navigation landmarks inside the frame's own.
    expect([...el.querySelectorAll('mat-nav-list')].map((l) => l.getAttribute('role'))).toEqual(['list', 'list']);
    count.set(3);
    await settle();
    expect(el.querySelector('[data-page-link="t.lab"]')).not.toBeNull();
    await router.navigateByUrl('/two');
    await settle();
    await fixture.whenStable();
    expect(el.querySelector('main h1')?.textContent).toBe('Page two scoped');
    expect(TestBed.inject(ShellPages).current()).toBe('t.two');
    expect(TestBed.inject(Title).getTitle()).toBe('Two – Test app');
    await router.navigateByUrl('/lab');
    await settle();
    expect(router.url).toBe('/lab');
    // The condition turns false while the page is shown: the first page takes over.
    count.set(0);
    await settle();
    await fixture.whenStable();
    expect(router.url).toBe('/one');
  });

  it('a page whose condition is false, throws or cannot be asked is not routed', async () => {
    const { router } = await boot();
    await router.navigateByUrl('/lab');
    await settle();
    expect(router.url).toBe('/one');
    await router.navigateByUrl('/broken');
    await settle();
    expect(router.url).toBe('/one');
    await router.navigateByUrl('/nowhere');
    await settle();
    expect(router.url).toBe('/one');
  });

  it('a context that fails to load keeps conditional pages out', async () => {
    const { router } = await boot({}, true);
    await router.navigateByUrl('/lab');
    await settle();
    expect(router.url).toBe('/one');
  });

  it('showPage navigates by id; an unknown id is refused with a warning', async () => {
    const { router } = await boot();
    TestBed.inject(SHELL_PORTS).showPage('t.about');
    await settle();
    expect(router.url).toBe('/about');
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    expect(TestBed.inject(ShellPages).show('t.none')).toBe(false);
    expect(warn).toHaveBeenCalledWith('[paddock-pages] there is no page t.none');
    TestBed.inject(SHELL_PORTS).notify({ level: 'info', message: 'hello' });
    expect(TestBed.inject(NotificationsStore).all().some((n) => n.title === 'hello' || n.body === 'hello')).toBe(true);
  });

  it('go resolves once the page is shown, and refuses an unknown id', async () => {
    const { router } = await boot();
    expect(await TestBed.inject(ShellPages).go('t.about')).toBe(true);
    expect(router.url).toBe('/about');
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    expect(await TestBed.inject(ShellPages).go('t.none')).toBe(false);
    expect(warn).toHaveBeenCalledWith('[paddock-pages] there is no page t.none');
  });

  it('the sidebar button collapses to the rail and back, and the rail names its links', async () => {
    const { el, fixture, frame } = await boot();
    const toggle = el.querySelector<HTMLButtonElement>('[data-testid="sidebar-toggle"]') as HTMLButtonElement;
    const store = fixture.debugElement.children[0]?.injector.get(PagesFrameStore) as InstanceType<typeof PagesFrameStore>;
    store.measure(1200);
    await fixture.whenStable();
    expect(frame.dataset['sidebar']).toBe('expanded');
    expect(toggle.getAttribute('aria-label')).toBe('Collapse sidebar');
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    toggle.click();
    await fixture.whenStable();
    expect(frame.dataset['sidebar']).toBe('rail');
    expect(toggle.getAttribute('aria-label')).toBe('Expand sidebar');
    expect(el.querySelector('[data-page-link="t.one"]')?.getAttribute('aria-label')).toBe('One');
    expect(el.querySelector('[data-page-link="t.one"]')?.getAttribute('title')).toBeNull();
    el.querySelector('[data-page-link="t.one"]')?.dispatchEvent(new MouseEvent('mouseenter'));
    await vi.waitFor(() => expect(document.querySelector('.mat-mdc-tooltip')?.textContent?.trim()).toBe('One'));
    toggle.click();
    await fixture.whenStable();
    expect(frame.dataset['sidebar']).toBe('expanded');
  });

  it('narrow: the sidebar opens over the page with a scrim, takes focus, and closes on Escape, the scrim or a page', async () => {
    const { el, fixture, frame, router } = await boot();
    const store = fixture.debugElement.children[0]?.injector.get(PagesFrameStore) as InstanceType<typeof PagesFrameStore>;
    store.measure(320);
    await fixture.whenStable();
    const toggle = el.querySelector<HTMLButtonElement>('[data-testid="sidebar-toggle"]') as HTMLButtonElement;
    const nav = el.querySelector<HTMLElement>('#pd-pages-nav') as HTMLElement;
    expect(frame.dataset['sidebar']).toBe('overlay');
    expect(nav.hidden).toBe(true);
    expect(toggle.getAttribute('aria-label')).toBe('Show pages');
    toggle.click();
    await settle();
    await fixture.whenStable();
    expect(nav.hidden).toBe(false);
    expect(nav.classList).toContain('pd-drawer');
    expect(toggle.getAttribute('aria-label')).toBe('Hide pages');
    expect(document.activeElement?.getAttribute('data-page-link')).toBe('t.one');
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await fixture.whenStable();
    expect(nav.hidden).toBe(true);
    expect(document.activeElement).toBe(toggle);
    toggle.click();
    await fixture.whenStable();
    el.querySelector<HTMLElement>('[data-testid="sidebar-scrim"]')?.click();
    await fixture.whenStable();
    expect(el.querySelector('[data-testid="sidebar-scrim"]')).toBeNull();
    // A page chosen in the drawer closes it and focuses the page.
    toggle.click();
    await settle();
    await fixture.whenStable();
    (el.querySelector('[data-page-link="t.two"]') as HTMLAnchorElement).click();
    await settle();
    await fixture.whenStable();
    expect(router.url).toBe('/two');
    expect(nav.hidden).toBe(true);
    expect(document.activeElement?.id).toBe('pd-main');
    // Escape without a drawer is left alone.
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  });

  it('a navigation while typing elsewhere leaves focus alone', async () => {
    const { el, router, fixture } = await boot();
    document.body.append(el);
    // A field that stays (in another part of the window): the page it was not on changes under it.
    const field = document.createElement('input');
    document.body.append(field);
    field.focus();
    await router.navigateByUrl('/about');
    await settle();
    await fixture.whenStable();
    expect(document.activeElement).toBe(field);
    field.remove();
  });

  it('a navigation chosen in the command palette focuses the page (the palette closes under the focus)', async () => {
    const { el, router, fixture } = await boot();
    document.body.append(el);
    const palette = document.createElement('div');
    palette.dataset['testid'] = 'palette';
    const input = document.createElement('input');
    palette.append(input);
    document.body.append(palette);
    input.focus();
    await router.navigateByUrl('/about');
    await settle();
    await fixture.whenStable();
    expect(document.activeElement).toBe(el.querySelector('#pd-main'));
    palette.remove();
  });

  it('F6 cycles the regions; a handled key is left alone; skip link focuses the page', async () => {
    const { el, frame } = await boot();
    document.body.append(el);
    for (const r of frame.querySelectorAll<HTMLElement>('[data-pd-region]')) r.getClientRects = () => [{}] as unknown as DOMRectList;
    const press = (init: KeyboardEventInit) => {
      const e = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
      document.dispatchEvent(e);
      return e;
    };
    expect(press({ key: 'F6' }).defaultPrevented).toBe(true);
    expect(document.activeElement?.closest('[data-pd-region]')?.getAttribute('data-pd-region')).toBe('title');
    press({ key: 'F6' });
    expect(document.activeElement?.getAttribute('data-page-link')).toBe('t.one');
    press({ key: 'F6', shiftKey: true });
    expect(document.activeElement?.closest('[data-pd-region]')?.getAttribute('data-pd-region')).toBe('title');
    expect(press({ key: 'x' }).defaultPrevented).toBe(false);
    const handled = new KeyboardEvent('keydown', { key: 'F6', bubbles: true, cancelable: true });
    handled.preventDefault();
    document.dispatchEvent(handled);
    (el.querySelector('a.pd-skip') as HTMLAnchorElement).click();
    expect(document.activeElement?.id).toBe('pd-main');
  });

  it('arrow keys, Home and End move between the page links', async () => {
    const { el } = await boot();
    document.body.append(el);
    const links = [...el.querySelectorAll<HTMLAnchorElement>('nav a.pd-item')];
    const key = (from: HTMLElement, k: string) => {
      from.focus();
      // The CDK key manager reads keyCode, which a real keyboard always sets.
      const keyCode = { ArrowDown: 40, ArrowUp: 38, Home: 36, End: 35 }[k] ?? 65;
      const e = new KeyboardEvent('keydown', { key: k, keyCode, bubbles: true, cancelable: true });
      from.dispatchEvent(e);
      return e;
    };
    key(links[0] as HTMLElement, 'ArrowDown');
    expect(document.activeElement).toBe(links[1]);
    key(links[0] as HTMLElement, 'ArrowUp');
    expect(document.activeElement).toBe(links.at(-1));
    key(links[2] as HTMLElement, 'Home');
    expect(document.activeElement).toBe(links[0]);
    key(links[0] as HTMLElement, 'End');
    expect(document.activeElement).toBe(links.at(-1));
    expect(key(links[0] as HTMLElement, 'a').defaultPrevented).toBe(false);
    // A key from the sidebar that is not on a link (the footer slot) is left alone.
    const foot = el.querySelector<HTMLElement>('nav [slot="sidebar"], nav .pd-slot')!;
    foot.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', keyCode: 40, bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(links[0]);
    // Expanded, the names are drawn, so a link has no tooltip of its own, unless its badge needs explaining.
    links[0]?.dispatchEvent(new MouseEvent('mouseenter'));
    expect(document.querySelector('.mat-mdc-tooltip')).toBeNull();
    // A key on the list itself, not on a link, is left alone.
    expect(key(el.querySelector<HTMLElement>('mat-nav-list')!, 'ArrowDown').defaultPrevented).toBe(false);
    // The bottom list has its own keys.
    const bottom = links.at(-1) as HTMLElement;
    key(bottom, 'ArrowUp');
    expect(document.activeElement).toBe(links.at(-2));
  });

  it('a page hidden after the context loads, then shown, keeps its arrow-key place, and the labels carry the family weights', async () => {
    const { el, fixture } = await boot();
    document.body.append(el);
    await TestBed.inject(ShellContextHost).load();
    await settle();
    // Hidden after the context loaded, then shown: the page keeps its place in the arrow-key order.
    expect(el.querySelector('[data-page-link="t.lab"]')).toBeNull();
    count.set(3);
    await settle();
    await fixture.whenStable();
    const link = (id: string) => el.querySelector<HTMLAnchorElement>(`[data-page-link="${id}"]`)!;
    expect([...el.querySelectorAll('nav a.pd-item')].map((a) => (a as HTMLElement).dataset['pageLink'])).toEqual(['t.one', 't.two', 't.lab', 't.about']);
    link('t.two').focus();
    link('t.two').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', keyCode: 40, bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(link('t.lab'));
    link('t.lab').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', keyCode: 40, bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(link('t.about'));
    // The list item's title directive is applied (the weights of its label are its tokens, measured in the e2e).
    expect(link('t.two').querySelector('.pd-label')?.classList.contains('mdc-list-item__primary-text')).toBe(true);
    expect(link('t.two').querySelector('pd-icon')?.classList.contains('mdc-list-item__start')).toBe(true);
    count.set(0);
    await settle();
  });

  it('the native menu bar waits for ready; the recovery and checkpoint are wired', async () => {
    await boot({ startup: { backend: () => new MockStartupBackend(), checkpoint: () => ({ capture: () => ({}), restore: () => undefined }) } });
    await settle();
    expect(TestBed.inject(WindowReady).ready()).toBe(true);
    // The gate's default: ready, and the app context loaded (when idle).
    await vi.waitFor(() => expect(TestBed.inject(NATIVE_MENU_GATE)()).toBe(true));
    expect(TestBed.inject(UI_CHECKPOINT, null)).not.toBeNull();
  });

  it('measures the frame where it can, and stops when it ends', async () => {
    const observers: { disconnect: ReturnType<typeof vi.fn>; cb: () => void }[] = [];
    class RO {
      disconnect = vi.fn();
      constructor(readonly cb: () => void) {
        observers.push(this);
      }
      observe(): void {}
      unobserve(): void {}
    }
    count.set(0);
    await boot();
    vi.stubGlobal('ResizeObserver', RO);
    const f2 = TestBed.createComponent(Root);
    await f2.whenStable();
    expect(observers.length).toBeGreaterThan(0);
    observers[0]?.cb();
    f2.destroy();
    expect(observers.some((o) => o.disconnect.mock.calls.length > 0)).toBe(true);
  });

  it('path URLs, path overrides, the app’s routes, its icon and reserved keys', async () => {
    @Component({ selector: 'my-icon', template: `I`, changeDetection: ChangeDetectionStrategy.OnPush })
    class MyIcon {
      readonly name = input('');
    }
    @Component({ selector: 'tray-page', template: `tray`, changeDetection: ChangeDetectionStrategy.OnPush })
    class Tray {}
    const { router, el } = await boot({
      location: 'path',
      paths: { 't.two': 'second' },
      routes: [{ path: 'tray', component: Tray }],
      icon: MyIcon,
      reservedKeys: [{ key: 'Mod+K', description: 'Palette' }],
      sidebar: { pageMin: 100 },
      dialogs: {},
    });
    expect(el.querySelector('[data-page-link="t.one"] my-icon')).not.toBeNull();
    await router.navigateByUrl('/second');
    await settle();
    expect(TestBed.inject(ShellPages).current()).toBe('t.two');
    await router.navigateByUrl('/tray');
    await settle();
    expect(TestBed.inject(ShellPages).current()).toBeNull();
    expect(TestBed.inject(RESERVED_KEYS).map((k) => k.key)).toEqual(['F6', 'Shift+F6', 'Mod+K']);
    expect(TestBed.inject(Keymap)).toBeTruthy();
    expect(TestBed.inject(CommandService)).toBeTruthy();
  });

  it('without jobs there is no jobs status item', async () => {
    await boot({ jobs: undefined });
    const items = TestBed.inject((await import('@decryp7/paddock-ui/shell')).ContributionRegistry).statusItems().map((i) => i.id);
    expect(items).toEqual(['pd.health', 'pd.notifications']);
  });
});

describe('pd-page-host and the title strategy alone', () => {
  it('a route without a page offers the first page', async () => {
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        provideContributions({ pages: [PAGES[0] as PageDef] }),
        provideRouter([{ path: 'one', component: PdPageHost, data: { [PAGE_DATA]: 't.one' } }, { path: 'x', component: PdPageHost }]),
      ],
    });
    const router = TestBed.inject(Router);
    const { RouterTestingHarness } = await import('@angular/router/testing');
    const harness = await RouterTestingHarness.create();
    await harness.navigateByUrl('/x');
    await settle();
    const host = harness.routeNativeElement as HTMLElement;
    expect(host.textContent).toContain('This page does not exist.');
    host.querySelector<HTMLButtonElement>('pd-error-state button')?.click();
    await settle();
    expect(router.url).toBe('/one');
  });

  it('a route without a title is titled with the app’s name', () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), PagesTitleStrategy, provideRouter([])] });
    const strategy = TestBed.inject(PagesTitleStrategy);
    const snapshot = TestBed.inject(Router).routerState.snapshot;
    strategy.updateTitle(snapshot);
    expect(TestBed.inject(Title).getTitle()).toBe('App');
  });

  it('the sidebar alone renders the pages of the registry', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideContributions({ pages: PAGES as PageDef[] }), provideRouter([])] });
    const fixture = TestBed.createComponent(PdPagesSidebar);
    fixture.componentRef.setInput('labelled', false);
    await fixture.whenStable();
    expect(fixture.nativeElement.classList).toContain('pd-rail');
    // Without the app's context the conditions throw: those pages are not offered.
    expect(fixture.nativeElement.querySelectorAll('a.pd-item')).toHaveLength(3);
  });
});
