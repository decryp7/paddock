import { ChangeDetectionStrategy, Component, input, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Title } from '@angular/platform-browser';
import { CommandService, defineCommand, defineKeybinding, msg, provideShellContext, SHELL_IDLE, SHELL_PORTS, ShellContextHost } from '@decryp7/paddock-ui/shell';
import { MemoryJobsBackend } from '@decryp7/paddock-ui/shell/jobs';
import { KEY_PLATFORM } from '@decryp7/paddock-ui/shell/menu';
import { MemoryNotificationCentre, MemoryServiceHealth } from '@decryp7/paddock-ui/shell/notify';
import { MockStartupBackend } from '@decryp7/paddock-ui/shell/startup';
import { assertContributionsValid, settle } from '@decryp7/paddock-ui/shell/testing';
import {
  defineActivityItem,
  defineExplorerProvider,
  defineView,
  DOCK_PORT,
  EXPLORER_PLAN,
  provideWorkbenchContributions,
  WORKBENCH_PORTS,
  type DockPort,
  type ExplorerPlan,
} from '@decryp7/paddock-ui/workbench';
import { PaletteStore } from '@decryp7/paddock-ui/workbench/palette';
import { ActivityItems, badgeText, fittingTopItems } from './activity';
import { PdActivityBar } from './activity-bar';
import { PdWorkbench } from './frame';
import {
  clampWidth,
  EXPLORER_MAX_PX,
  EXPLORER_MIN_PX,
  fallbackVerdict,
  loadFramePrefs,
  saveFramePrefs,
  WORKBENCH_FRAME_KEY,
  WORKBENCH_FRAME_STORAGE,
  WorkbenchFrameStore,
  type FrameStorage,
} from './frame.store';
import { FRAME_COMMANDS, JOBS_VIEW, provideShellWorkbench, type ShellWorkbenchOptions } from './provide-workbench';

class MemoryStorage implements FrameStorage {
  readonly map = new Map<string, string>();
  getItem(k: string): string | null {
    return this.map.get(k) ?? null;
  }
  setItem(k: string, v: string): void {
    this.map.set(k, v);
  }
}

interface Ctx {
  readonly problems: () => number;
  readonly log: string[];
}

const problems = signal(3);
const log: string[] = [];
const layout = { area: 'side' as const, min: { width: 320, height: 200 }, pref: { width: 400, height: 300 }, priority: 50 };
const load = () => Promise.resolve(class {});

/** A dock port the test watches. */
function fakeDock(): DockPort & { shown: string[]; closed: string[] } {
  const open = signal<readonly string[]>(['wd.problems']);
  const shown: string[] = [];
  const closed: string[] = [];
  return {
    shown,
    closed,
    showView: (id) => {
      shown.push(id);
      return Promise.resolve(true);
    },
    closeView: (p) => void closed.push(p),
    openPanels: open,
    activePanel: signal('wd.problems'),
    applyPerspective: (id) => Promise.resolve(id === 'wd.known'),
    activePerspective: signal('wd.known'),
  };
}

const CONTRIBUTIONS = {
  commands: [
    defineCommand<Ctx>({ id: 'wd.build', title: msg('wd.build', 'Build'), run: (ctx) => void ctx.log.push('build') }),
    defineCommand<Ctx>({ id: 'wd.hidden', title: msg('wd.hidden', 'Hidden'), when: () => false, run: () => undefined }),
  ],
  keybindings: [defineKeybinding({ command: 'wd.build', key: 'Mod+Shift+B' })],
  views: [defineView<Ctx>({ id: 'wd.problems', title: msg('wd.problems', 'Problems'), icon: 'wd:problems', layout, load })],
  explorerProviders: [
    defineExplorerProvider<Ctx>({ id: 'wd.files', title: msg('wd.files', 'Files'), order: 100, roots: () => Promise.resolve({ nodes: [{ id: 'a', label: 'a.rs', expandable: false }] }) }),
  ],
  activityItems: [
    defineActivityItem<Ctx>({ id: 'wd.act.files', title: msg('wd.act.files', 'Explorer'), icon: 'pd:folder', order: 100, run: { explorer: 'wd.files' } }),
    defineActivityItem<Ctx>({
      id: 'wd.act.problems',
      title: msg('wd.act.problems', 'Problems'),
      icon: 'pd:alert',
      order: 200,
      run: { view: 'wd.problems' },
      badge: (ctx) => ctx.problems(),
      pulse: () => true,
    }),
    defineActivityItem<Ctx>({ id: 'wd.act.build', title: msg('wd.act.build', 'Build'), icon: 'pd:play', order: 300, run: { command: 'wd.build' }, disabled: () => (problems() > 5 ? 'Too many problems' : null), badgeLabel: () => 'x', badge: () => 'NEW!!' }),
    defineActivityItem<Ctx>({ id: 'wd.act.hidden', title: msg('wd.act.hidden', 'Hidden'), icon: 'pd:lock', order: 400, run: { command: 'wd.hidden' } }),
    defineActivityItem<Ctx>({ id: 'wd.act.throws', title: msg('wd.act.throws', 'Throws'), icon: 'pd:lock', order: 450, run: { view: 'wd.problems' }, when: () => { throw new Error('x'); } }),
    defineActivityItem<Ctx>({
      id: 'wd.act.menu',
      title: msg('wd.act.menu', 'Layouts'),
      icon: 'pd:window',
      order: 500,
      active: () => true,
      run: {
        menu: () => [
          { id: 'a', label: 'Problems', checked: true, run: { view: 'wd.problems' } },
          { id: 'b', label: 'Build', keybinding: 'Mod+Shift+B', run: { command: 'wd.build' } },
        ],
      },
    }),
    defineActivityItem<Ctx>({ id: 'wd.act.settings', title: msg('wd.act.settings', 'Settings'), icon: 'pd:settings', order: 900, section: 'bottom', run: { command: 'wd.build' }, keybinding: 'Mod+,' }),
  ],
};

@Component({ imports: [PdWorkbench], changeDetection: ChangeDetectionStrategy.OnPush, template: `<pd-workbench><div slot="stage" data-testid="stage">STAGE</div><span slot="title-end">END</span></pd-workbench>` })
class Root {}

async function boot(options: { dock?: boolean; plan?: ExplorerPlan; storage?: MemoryStorage; extra?: Partial<ShellWorkbenchOptions> } = {}) {
  log.length = 0;
  problems.set(3);
  vi.stubGlobal('ResizeObserver', undefined);
  const storage = options.storage ?? new MemoryStorage();
  const dock = fakeDock();
  const centre = new MemoryNotificationCentre();
  const health = new MemoryServiceHealth([], { centre });
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: SHELL_IDLE, useValue: () => undefined },
      { provide: KEY_PLATFORM, useValue: 'other' },
      provideWorkbenchContributions<Ctx>(CONTRIBUTIONS),
      provideShellContext<Ctx>(() => Promise.resolve(() => ({ problems, log })), { prefetch: 'first-use' }),
      provideShellWorkbench({
        appName: 'Workbench test',
        notify: { health: () => health, notifications: () => centre },
        jobs: { backend: () => new MemoryJobsBackend() },
        startup: { backend: () => new MockStartupBackend() },
        nativeFeel: false,
        ...options.extra,
      }),
      { provide: WORKBENCH_FRAME_STORAGE, useValue: storage },
      ...(options.dock === false ? [] : [{ provide: DOCK_PORT, useValue: dock }]),
      ...(options.plan ? [{ provide: EXPLORER_PLAN, useValue: options.plan }] : []),
    ],
  });
  await TestBed.inject(ShellContextHost).load();
  const fixture = TestBed.createComponent(Root);
  await settle(fixture);
  return { fixture, element: fixture.nativeElement as HTMLElement, dock, storage };
}

describe('activity layout helpers', () => {
  it('fits the top buttons and keeps a slot for More', () => {
    expect(fittingTopItems(5, 1, 0)).toBe(5);
    expect(fittingTopItems(5, 1, 1000)).toBe(5);
    // 4 slots in 214 px: one bottom leaves 3, and More takes one of them: 2 shown.
    expect(fittingTopItems(5, 1, 214)).toBe(2);
    expect(fittingTopItems(5, 1, 10)).toBe(0);
  });

  it('draws badges short', () => {
    expect([badgeText(null), badgeText(''), badgeText(0), badgeText(-1), badgeText(Number.NaN), badgeText(7.6), badgeText(1500), badgeText('NEW!!')]).toEqual([null, null, null, null, null, '7', '999+', 'NEW!']);
  });
});

describe('fallback Explorer plan and prefs', () => {
  it('keeps the Explorer while it fits and returns it only with a margin', () => {
    expect(fallbackVerdict(true, 272, 1440)).toBe('keep');
    expect(fallbackVerdict(true, 272, 600)).toBe('hide');
    expect(fallbackVerdict(false, 272, 1440)).toBe('show');
    // Just wide enough to keep, not wide enough to come back (hysteresis).
    const edge = 48 + 200 + 420 + 10;
    expect(fallbackVerdict(true, 200, edge)).toBe('keep');
    expect(fallbackVerdict(false, 200, edge)).toBe('hide');
  });

  it('loads and saves prefs, falling back on anything odd', () => {
    const s = new MemoryStorage();
    expect(loadFramePrefs(null)).toEqual({ explorerWidth: 272 });
    expect(saveFramePrefs({ explorer: 'wd.files', explorerWidth: 300 }, s)).toBe(true);
    expect(loadFramePrefs(s)).toEqual({ explorer: 'wd.files', explorerWidth: 300 });
    s.setItem(WORKBENCH_FRAME_KEY, JSON.stringify({ version: 1, explorer: null, explorerWidth: 5 }));
    expect(loadFramePrefs(s)).toEqual({ explorer: null, explorerWidth: EXPLORER_MIN_PX });
    s.setItem(WORKBENCH_FRAME_KEY, JSON.stringify({ version: 1, explorer: 7, explorerWidth: 'x' }));
    expect(loadFramePrefs(s)).toEqual({ explorerWidth: 272 });
    s.setItem(WORKBENCH_FRAME_KEY, JSON.stringify({ version: 2 }));
    expect(loadFramePrefs(s)).toEqual({ explorerWidth: 272 });
    s.setItem(WORKBENCH_FRAME_KEY, '{bad');
    expect(loadFramePrefs(s)).toEqual({ explorerWidth: 272 });
    expect(saveFramePrefs({ explorerWidth: 1 }, null)).toBe(false);
    expect(saveFramePrefs({ explorerWidth: 1 }, { setItem: () => { throw new Error('full'); } })).toBe(false);
    expect([clampWidth(1), clampWidth(9999), clampWidth(300.4)]).toEqual([EXPLORER_MIN_PX, EXPLORER_MAX_PX, 300]);
  });

  it('uses the browser storage by default', () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    expect(TestBed.inject(WORKBENCH_FRAME_STORAGE)).toBe(localStorage);
  });
});

describe('WorkbenchFrameStore narrow-window rule', () => {
  it('closes the Explorer by plan, reopens it with the room, keeps one the user reopened, never reopens one the user closed', async () => {
    await boot();
    const frame = TestBed.inject(WorkbenchFrameStore);
    expect(frame.explorerShown()).toBe('wd.files');
    frame.evaluate(1440);
    expect(frame.explorerShown()).toBe('wd.files');
    frame.evaluate(600);
    expect(frame.explorerShown()).toBeNull();
    expect(frame.wanted()).toBe('wd.files');
    frame.evaluate(1440);
    expect(frame.explorerShown()).toBe('wd.files');
    frame.evaluate(600);
    // The user opens it again in the narrow window: it stays.
    frame.show('wd.files');
    frame.evaluate(610);
    expect(frame.explorerShown()).toBe('wd.files');
    frame.evaluate(1440);
    expect(frame.keptByUser()).toBe(false);
    frame.hide();
    frame.evaluate(1440);
    expect(frame.explorerShown()).toBeNull();
    frame.evaluate(0);
  });

  it('asks the dock’s plan when there is one', async () => {
    let answer: 'keep' | 'hide' | 'show' = 'hide';
    const listeners: (() => void)[] = [];
    const plan: ExplorerPlan = { verdict: () => answer, onWindowPlan: (l) => (listeners.push(l), () => listeners.splice(0)) };
    const { fixture } = await boot({ plan });
    const frame = TestBed.inject(WorkbenchFrameStore);
    frame.evaluate(1440);
    expect(frame.explorerShown()).toBeNull();
    answer = 'show';
    listeners[0]!();
    frame.evaluate(1440);
    expect(frame.explorerShown()).toBe('wd.files');
    fixture.destroy();
    expect(listeners).toEqual([]);
  });

  it('remembers the user’s choice and width', async () => {
    const storage = new MemoryStorage();
    storage.setItem(WORKBENCH_FRAME_KEY, JSON.stringify({ version: 1, explorer: null, explorerWidth: 320 }));
    await boot({ storage });
    const frame = TestBed.inject(WorkbenchFrameStore);
    expect(frame.explorerShown()).toBeNull();
    expect(frame.explorerWidth()).toBe(320);
    frame.setWidth(5000);
    expect(JSON.parse(storage.getItem(WORKBENCH_FRAME_KEY)!)).toEqual({ version: 1, explorer: null, explorerWidth: EXPLORER_MAX_PX });
  });
});

describe('ActivityItems', () => {
  it('evaluates items defensively and runs their targets', async () => {
    const { dock } = await boot();
    const items = TestBed.inject(ActivityItems);
    expect(items.top().map((i) => i.id)).toEqual(['wd.act.files', 'wd.act.problems', 'wd.act.build', 'wd.act.menu']);
    expect(items.bottom().map((i) => i.id)).toEqual(['wd.act.settings']);
    const [files, prob, build, menu] = items.top() as [never, never, never, never];
    expect([items.isActive(files), items.isActive(prob), items.isActive(build), items.isActive(menu)]).toEqual([true, false, false, true]);
    expect([items.isOpen(prob), items.isOpen(files), items.isOpen(build)]).toEqual([true, false, false]);
    expect([items.badge(prob), items.badge(build), items.badgeLabel(build), items.pulse(prob), items.pulse(build)]).toEqual(['3', 'NEW!', 'x', true, false]);
    expect(items.menu(menu).map((e) => e.id)).toEqual(['a', 'b']);
    expect(items.menu(files)).toEqual([]);
    expect([items.keyHint(build), items.keyHint(items.bottom()[0]!), items.keyHint(files)]).toEqual(['Ctrl+Shift+B', 'Ctrl+,', '']);
    expect(await items.activate(prob)).toBe(true);
    expect(dock.shown).toEqual(['wd.problems']);
    expect(await items.activate(build)).toBe(true);
    expect(log).toEqual(['build']);
    expect(await items.activate(menu)).toBe(false);
    // The Explorer item toggles the side bar.
    await items.activate(files);
    expect(TestBed.inject(WorkbenchFrameStore).explorerShown()).toBeNull();
    await items.activate(files);
    expect(TestBed.inject(WorkbenchFrameStore).explorerShown()).toBe('wd.files');
    problems.set(9);
    expect(items.disabledReason(build)).toBe('Too many problems');
    expect(await items.activate(build)).toBe(false);
  });

  it('reports a target that throws as a notice', async () => {
    await boot();
    const items = TestBed.inject(ActivityItems);
    const notices: string[] = [];
    vi.spyOn(TestBed.inject(SHELL_PORTS), 'notify').mockImplementation((n) => void notices.push(n.message));
    vi.spyOn(TestBed.inject(WORKBENCH_PORTS), 'showView').mockRejectedValue(new Error('dock gone'));
    expect(await items.run({ view: 'wd.problems' })).toBe(false);
    vi.spyOn(TestBed.inject(CommandService), 'run').mockRejectedValue('plain');
    expect(await items.run({ command: 'wd.build' })).toBe(false);
    expect(notices).toEqual(['dock gone', 'plain']);
  });
});

describe('pd-workbench with provideShellWorkbench', () => {
  it('composes the frame: regions, activity bar, side bar, stage slot, title', async () => {
    const { element } = await boot();
    expect(TestBed.inject(Title).getTitle()).toBe('Workbench test');
    expect([...element.querySelectorAll('[data-pd-region]')].map((e) => e.getAttribute('data-pd-region'))).toEqual(['title', 'activity', 'sidebar', 'main', 'status']);
    expect(element.querySelector('[data-testid="stage"]')?.textContent).toBe('STAGE');
    expect(element.textContent).toContain('END');
    expect(element.querySelector('pd-workbench')?.getAttribute('data-explorer')).toBe('wd.files');
    const ids = [...element.querySelectorAll('pd-activity-bar [data-activity-id]')].map((e) => e.getAttribute('data-activity-id'));
    expect(ids).toEqual(['wd.act.files', 'wd.act.problems', 'wd.act.build', 'wd.act.menu', 'wd.act.settings']);
    const problemsButton = element.querySelector('[data-activity-id="wd.act.problems"]')!;
    expect(problemsButton.getAttribute('aria-label')).toBe('Problems, 3');
    expect(problemsButton.querySelector('.mat-badge-content')?.textContent?.trim()).toBe('3');
    expect(problemsButton.querySelector('.mat-badge-content')?.getAttribute('aria-hidden')).toBe('true');
    expect(element.querySelector('[data-activity-id="wd.act.build"]')?.getAttribute('aria-label')).toBe('Build, x');
    const build = element.querySelector('[data-activity-id="wd.act.build"]')!;
    const tip = async (text: string) => {
      build.dispatchEvent(new MouseEvent('mouseenter'));
      await vi.waitFor(() => expect(document.querySelector('.mat-mdc-tooltip')?.textContent?.trim()).toBe(text));
      build.dispatchEvent(new MouseEvent('mouseleave'));
    };
    expect(build.hasAttribute('title')).toBe(false);
    await tip('Build (Ctrl+Shift+B)');
    problems.set(9);
    await settle();
    await tip('Build (Ctrl+Shift+B) — Too many problems');
    expect(element.querySelector('[data-activity-id="wd.act.build"]')?.getAttribute('aria-disabled')).toBe('true');
  });

  it('passes the contributions lint with the frame’s own contributions', async () => {
    await boot();
    const { Injector } = await import('@angular/core');
    expect(() => assertContributionsValid(TestBed.inject(Injector))).not.toThrow();
  });

  it('cycles regions with F6, skips to the stage, resizes the side bar by keys and pointer', async () => {
    const { element, fixture } = await boot();
    const f6 = new KeyboardEvent('keydown', { key: 'F6', cancelable: true });
    document.dispatchEvent(f6);
    expect(f6.defaultPrevented).toBe(true);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'x' }));
    const prevented = new KeyboardEvent('keydown', { key: 'F6', cancelable: true });
    prevented.preventDefault();
    document.dispatchEvent(prevented);
    element.querySelector<HTMLElement>('.pd-skip')!.click();
    expect(document.activeElement?.id).toBe('pd-stage');
    const frame = TestBed.inject(WorkbenchFrameStore);
    const sash = element.querySelector<HTMLElement>('[data-testid="sidebar-sash"]')!;
    const key = (k: string, shiftKey = false) => sash.dispatchEvent(new KeyboardEvent('keydown', { key: k, shiftKey, cancelable: true }));
    key('ArrowRight');
    expect(frame.explorerWidth()).toBe(288);
    key('ArrowLeft', true);
    expect(frame.explorerWidth()).toBe(224);
    key('End');
    expect(frame.explorerWidth()).toBe(EXPLORER_MAX_PX);
    key('Home');
    expect(frame.explorerWidth()).toBe(EXPLORER_MIN_PX);
    key('q');
    const pointer = (type: string, x: number) => {
      const e = new MouseEvent(type, { clientX: x, cancelable: true }) as unknown as PointerEvent;
      Object.defineProperty(e, 'pointerId', { value: 1 });
      sash.dispatchEvent(e);
    };
    pointer('pointerdown', 100);
    pointer('pointermove', 180);
    expect(frame.explorerWidth()).toBe(EXPLORER_MIN_PX + 80);
    pointer('pointerup', 180);
    pointer('pointermove', 400);
    expect(frame.explorerWidth()).toBe(EXPLORER_MIN_PX + 80);
    await settle(fixture);
    expect(element.querySelector<HTMLElement>('[data-testid="workbench-sidebar"]')?.style.width).toBe(`${EXPLORER_MIN_PX + 80}px`);
  });

  it('routes the window ports to the dock, the side bar and the palette', async () => {
    const { dock } = await boot();
    const ports = TestBed.inject(WORKBENCH_PORTS);
    expect(await ports.showView('wd.problems')).toBe(true);
    ports.closeView('wd.problems');
    expect(await ports.applyPerspective('wd.known')).toBe(true);
    expect([ports.openPanels(), ports.activePanel(), ports.activePerspective()]).toEqual([['wd.problems'], 'wd.problems', 'wd.known']);
    ports.hideExplorer();
    expect(ports.explorerShown()).toBeNull();
    ports.showExplorer('wd.files');
    expect(ports.explorerShown()).toBe('wd.files');
    ports.openPalette('>');
    expect(TestBed.inject(PaletteStore).isOpen()).toBe(true);
    TestBed.inject(SHELL_PORTS).showPage('wd.problems');
    expect(dock.shown).toEqual(['wd.problems', 'wd.problems']);
    expect(dock.closed).toEqual(['wd.problems']);
  });

  it('runs the frame commands: toggle side bar and show jobs; without a dock the ports warn', async () => {
    await boot({ dock: false });
    const commands = TestBed.inject(CommandService);
    const frame = TestBed.inject(WorkbenchFrameStore);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    expect(commands.isChecked(commands.find(FRAME_COMMANDS.toggleSidebar)!)).toBe(true);
    await commands.run(FRAME_COMMANDS.toggleSidebar);
    expect(frame.explorerShown()).toBeNull();
    await commands.run(FRAME_COMMANDS.toggleSidebar);
    expect(frame.explorerShown()).toBe('wd.files');
    expect(await commands.run(FRAME_COMMANDS.jobs)).toBe(true);
    expect(warn).toHaveBeenCalledWith(`[paddock-workbench] no workbench frame in this window; cannot show view ${JOBS_VIEW.id}`);
    warn.mockRestore();
  });

  it('works without jobs, with an icon and reserved keys, and without a section to show', async () => {
    @Component({ selector: 'wd-icon', changeDetection: ChangeDetectionStrategy.OnPush, template: '' })
    class Icon {
      readonly name = input('');
    }
    await boot({ extra: { jobs: undefined, icon: Icon, reservedKeys: [{ key: 'F1', description: 'Help' }], palette: { commonCommands: ['wd.build'] } } });
    const commands = TestBed.inject(CommandService);
    expect(commands.find(FRAME_COMMANDS.jobs)).toBeUndefined();
  });
});

describe('pd-activity-bar', () => {
  it('opens a fly-out with the keyboard and runs an entry; overflows into More', async () => {
    const { element, fixture, dock } = await boot();
    const menuButton = element.querySelector<HTMLElement>('[data-activity-id="wd.act.menu"]')!;
    menuButton.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true }));
    await settle(fixture);
    const entries = [...document.querySelectorAll<HTMLElement>('[data-activity-menu="wd.act.menu"] [role^="menuitem"]')];
    expect(entries.map((e) => e.textContent?.trim())).toEqual(['Problems', 'BuildCtrl+Shift+B']);
    entries[0]!.click();
    await settle(fixture);
    expect(dock.shown).toEqual(['wd.problems']);
    // A short bar collapses items into More.
    const bar = fixture.debugElement.query((d) => d.componentInstance instanceof PdActivityBar).componentInstance as PdActivityBar;
    (bar as unknown as { barHeight: { set(v: number): void } }).barHeight.set(214);
    await settle(fixture);
    const more = element.querySelector<HTMLElement>('[data-activity-id="pd.more"]')!;
    expect(more).not.toBeNull();
    more.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true }));
    await settle(fixture);
    const overflow = [...document.querySelectorAll<HTMLElement>('[data-activity-menu="pd.more"] [role^="menuitem"]')];
    expect(overflow.map((e) => e.textContent?.trim())).toEqual(['Build', 'Layouts']);
    overflow[0]!.click();
    await settle(fixture);
    expect(log).toEqual(['build']);
  });

});

describe('review 2', () => {
  it('P1-B: below 480 px the Explorer the user opens slides over the dock; Escape and the scrim dismiss it', async () => {
    const { element, fixture } = await boot();
    const frame = TestBed.inject(WorkbenchFrameStore);
    frame.evaluate(1440);
    expect(frame.overlay()).toBe(false);
    frame.evaluate(320);
    expect(frame.explorerShown()).toBeNull();
    frame.show('wd.files');
    expect(frame.overlay()).toBe(true);
    await settle(fixture);
    expect(element.querySelector('[data-testid="workbench-sidebar"]')?.classList.contains('pd-side-overlay')).toBe(true);
    expect(element.querySelector('[data-testid="sidebar-sash"]')).toBeNull();
    const esc = new KeyboardEvent('keydown', { key: 'Escape', cancelable: true });
    document.dispatchEvent(esc);
    expect(esc.defaultPrevented).toBe(true);
    expect(frame.explorerShown()).toBeNull();
    expect(frame.wanted()).toBe('wd.files');
    frame.show('wd.files');
    await settle(fixture);
    element.querySelector<HTMLElement>('[data-testid="sidebar-scrim"]')!.click();
    expect(frame.overlay()).toBe(false);
    frame.evaluate(1440);
    expect(frame.explorerShown()).toBe('wd.files');
  });

  it('P2-E and P3-10: an open fly-out is not selected; badges are neutral unless the item gives a tone', async () => {
    const { element, fixture } = await boot();
    const items = TestBed.inject(ActivityItems);
    const [, prob] = items.top() as [never, never];
    expect(items.badgeTone(prob)).toBe('neutral');
    const menuButton = element.querySelector<HTMLElement>('[data-activity-id="wd.act.menu"]')!;
    menuButton.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true }));
    await settle(fixture);
    expect(menuButton.classList.contains('pd-open')).toBe(true);
    expect(element.querySelector('[data-activity-id="wd.act.problems"]')?.getAttribute('data-tone')).toBe('neutral');
  });
});
