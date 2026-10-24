import { EnvironmentProviders, Injector, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { defineCommand, msg, provideShellContext, SHELL_IDLE, ShellContextHost } from '@decryp7/paddock-ui/shell';
import { contributionProblems } from '@decryp7/paddock-ui/shell/testing';
import type { ViewLayout } from '@decryp7/paddock-ui';
import {
  activityTarget,
  defineActivityItem,
  defineAssistantContext,
  defineExplorerProvider,
  definePaletteProvider,
  definePerspective,
  defineSharedState,
  defineView,
  isOffered,
  layoutProblems,
  panelId,
  parsePanelId,
  provideWorkbenchContributions,
  provideLiveContributions,
  targetProblems,
  WORKBENCH_PORTS,
  WorkbenchRegistry,
  type WorkbenchBatch,
} from '../public-api';

const load = () => Promise.resolve(class {});
const doc: ViewLayout = { area: 'document', min: { width: 480, height: 240 }, pref: { width: 900, height: 600 }, priority: 100 };
const side: ViewLayout = { area: 'side', min: { width: 320, height: 240 }, pref: { width: 400, height: 240 }, priority: 60 };

interface Demo {
  readonly ready: boolean;
}

const code = defineView<Demo>({ id: 'wd.code', title: msg('wd.view.code', 'Code'), icon: 'wd:code', layout: doc, instances: true, load, order: 200 });
const assistant = defineView<Demo>({ id: 'wd.assistant', title: msg('wd.view.assistant', 'Assistant'), icon: 'wd:ai', layout: side, load, order: 100 });
const late = defineView<Demo>({ id: 'wd.late', title: msg('wd.view.late', 'Late'), icon: 'wd:late', layout: doc, load, when: (ctx) => ctx.ready, autoOpen: true });
const refresh = defineCommand({ id: 'wd.refresh', title: msg('wd.refresh', 'Refresh'), run: () => undefined });
const files = defineExplorerProvider<Demo>({
  id: 'wd.files',
  title: msg('wd.files', 'Files'),
  order: 100,
  roots: () => Promise.resolve({ nodes: [{ id: 'a', label: 'a', expandable: false }] }),
  empty: { text: msg('wd.files.empty', 'Open a folder'), command: 'wd.refresh' },
});

/** One valid item of every workbench kind: the lint finds nothing in it. */
const VALID: WorkbenchBatch<Demo> = {
  commands: [refresh],
  views: [code, assistant, late],
  activityItems: [
    defineActivityItem<Demo>({ id: 'wd.act.files', title: msg('wd.act.files', 'Files'), icon: 'wd:files', order: 100, run: { explorer: 'wd.files' } }),
    defineActivityItem<Demo>({ id: 'wd.act.ai', title: msg('wd.act.ai', 'Assistant'), icon: 'wd:ai', order: 200, run: { view: 'wd.assistant' }, badge: () => 2 }),
    defineActivityItem<Demo>({ id: 'wd.act.refresh', title: msg('wd.act.refresh', 'Refresh'), icon: 'wd:refresh', order: 300, section: 'bottom', run: { command: 'wd.refresh' } }),
    defineActivityItem<Demo>({ id: 'wd.act.more', title: msg('wd.act.more', 'More'), icon: 'wd:more', order: 400, run: { menu: () => [] } }),
  ],
  explorerProviders: [files],
  paletteProviders: [
    definePaletteProvider<Demo>({ id: 'wd.goto', title: msg('wd.goto', 'Go to file'), order: 100, search: () => Promise.resolve([]), accept: () => undefined }),
    definePaletteProvider<Demo>({ id: 'wd.symbols', title: msg('wd.symbols', 'Go to symbol'), prefix: '@', order: 200, search: () => Promise.resolve([]), accept: () => undefined }),
  ],
  perspectives: [definePerspective<Demo>({ id: 'wd.understand', title: msg('wd.p.understand', 'Understand'), icon: 'wd:eye', order: 100, views: ['wd.code', 'wd.assistant'], focus: 'wd.code' })],
  sharedState: [defineSharedState<Demo, string>({ id: 'wd.selection', scope: 'full', read: () => 'x', parse: (v) => (typeof v === 'string' ? v : undefined), apply: () => undefined })],
  assistantContext: [defineAssistantContext<Demo>({ id: 'wd.selection', label: msg('wd.ctx.selection', 'Selection'), value: () => null })],
};

function lint(...providers: EnvironmentProviders[]): string[] {
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), ...providers] });
  return contributionProblems(TestBed.inject(Injector));
}

const problems = (extra: Record<string, readonly unknown[]>): string[] =>
  lint(provideWorkbenchContributions(VALID), provideWorkbenchContributions(extra as WorkbenchBatch));

describe('workbench contributions lint (plan W1)', () => {
  it('passes a window whose workbench contributions are all valid, registered twice', () => {
    expect(lint(provideWorkbenchContributions(VALID), provideWorkbenchContributions({}))).toEqual([]);
  });

  it('fails on a view without a layout', () => {
    const bare = { ...code, id: 'wd.bare', layout: undefined } as never;
    expect(problems({ views: [bare] })).toEqual(['view wd.bare: has no layout (area, min, pref, priority; ADR 0011)']);
  });

  it('fails on a view whose min exceeds its pref', () => {
    const squeezed = defineView({ ...code, id: 'wd.squeezed', layout: { ...doc, min: { width: 1000, height: 700 } } });
    expect(problems({ views: [squeezed] })).toEqual([
      'view wd.squeezed: layout min > pref: min width 1000 exceeds pref 900',
      'view wd.squeezed: layout min > pref: min height 700 exceeds pref 600',
    ]);
  });

  it('fails on the other view defects', () => {
    const bad = defineView({
      id: 'wd.bad#1',
      title: 'Bad' as never,
      icon: 'bad',
      layout: { area: 'side', min: { width: 0, height: 10 }, pref: { width: 10, height: 10 }, priority: 0, edge: 'top' as never },
      load: 'x' as never,
      providers: 1 as never,
      when: 2 as never,
      autoOpen: true,
      fullHeight: true,
      category: 'x' as never,
      order: -1,
    });
    expect(problems({ views: [bad] })).toEqual(
      [
        'id "wd.bad#1" is not a namespaced id (pn.code)',
        'title has no message id (use msg(\'<ns>.<name>\', \'Text\'))',
        'has no icon (namespace:name)',
        'layout.min.width must be a number from 1 to 8000',
        'layout.priority must be a number from 1 to 100',
        'layout.edge must be left or right',
        'category has no message id (use msg(\'<ns>.<name>\', \'Text\'))',
        'order must be a number from 0 to 100000',
        'has no load function',
        'providers is not a function',
        'when is not a function',
        'fullHeight is for document views only',
      ].map((p) => `view wd.bad#1: ${p}`),
    );
    const noSizes = defineView({ ...code, id: 'wd.nosize', layout: { area: 'nowhere', priority: 50 } as never });
    expect(problems({ views: [noSizes] })).toEqual([
      'view wd.nosize: layout.area "nowhere" is not one of document, side, analysis, status',
      'view wd.nosize: layout.min is missing',
      'view wd.nosize: layout.pref is missing',
    ]);
    const autoWithoutWhen = defineView({ ...code, id: 'wd.auto', autoOpen: true });
    expect(problems({ views: [autoWithoutWhen] })).toEqual(['view wd.auto: autoOpen needs a when (it opens when when starts to hold)']);
    const edgeOnDoc = defineView({ ...code, id: 'wd.edge', layout: { ...doc, edge: 'left' } });
    expect(problems({ views: [edgeOnDoc] })).toEqual(['view wd.edge: layout.edge is for side views only']);
    expect(problems({ views: [code] })).toEqual(['duplicate view wd.code']);
  });

  it('fails on activity items whose target is missing or malformed', () => {
    const item = (id: string, run: unknown) => defineActivityItem({ id, title: msg(id, 'X'), icon: 'wd:x', order: 1, run: run as never });
    expect(
      problems({
        activityItems: [
          item('wd.a1', { view: 'wd.gone' }),
          item('wd.a2', { command: 'wd.gone' }),
          item('wd.a3', { explorer: 'wd.gone' }),
          item('wd.a4', { menu: 'x' }),
          item('wd.a5', {}),
          item('wd.a6', null),
          { ...item('wd.a7', { view: 'wd.code' }), section: 'middle', badge: 1, active: 1, pulse: 1, disabled: 1, when: 1, order: 1e9 },
        ],
      }),
    ).toEqual([
      'activity item wd.a1: shows the missing view wd.gone',
      'activity item wd.a2: runs the missing command wd.gone',
      'activity item wd.a3: shows the missing Explorer section wd.gone',
      'activity item wd.a4: has no menu function',
      'activity item wd.a5: has no run target ({ view }, { command }, { explorer } or { menu })',
      'activity item wd.a6: has no run target ({ view }, { command }, { explorer } or { menu })',
      'activity item wd.a7: order must be a number from 0 to 100000',
      'activity item wd.a7: section must be top or bottom',
      'activity item wd.a7: when is not a function',
      'activity item wd.a7: badge is not a function',
      'activity item wd.a7: active is not a function',
      'activity item wd.a7: pulse is not a function',
      'activity item wd.a7: disabled is not a function',
    ]);
  });

  it('fails on Explorer providers missing their parts', () => {
    const bad = { id: 'wd.tree', title: msg('wd.tree', 'Tree'), order: 1, icon: 'x', roots: 1, children: 1, open: 1, actions: () => [], drag: 1, changes: 1, empty: { text: 'x', command: 'wd.gone' } };
    expect(problems({ explorerProviders: [bad] })).toEqual(
      [
        'has no icon (namespace:name)',
        'has no roots function',
        'children is not a function',
        'open is not a function',
        'has actions but no run function',
        'drag is not a function',
        'changes is not a function',
        "empty.text has no message id (use msg('<ns>.<name>', 'Text'))",
        'empty state runs the missing command wd.gone',
      ].map((p) => `Explorer provider wd.tree: ${p}`),
    );
    expect(problems({ explorerProviders: [{ ...bad, actions: 1, run: 1, icon: undefined, roots: () => null, children: undefined, open: undefined, drag: undefined, changes: undefined, empty: undefined }] })).toEqual([
      'Explorer provider wd.tree: actions is not a function',
      'Explorer provider wd.tree: run is not a function',
    ]);
  });

  it('fails on palette providers with a bad or taken prefix', () => {
    const p = (id: string, prefix: string) => definePaletteProvider({ id, title: msg(id, 'X'), prefix, order: 1, search: () => Promise.resolve([]), accept: () => undefined });
    expect(problems({ paletteProviders: [p('wd.p0', '>')] })).toEqual(["palette provider wd.p0: prefix > is the palette's commands (choose another symbol)"]);
    expect(problems({ paletteProviders: [p('wd.p1', '@'), p('wd.p2', 'ab'), p('wd.p3', 'a'), p('wd.p4', '')] })).toEqual([
      'palette provider wd.p2: prefix "ab" must be empty or one symbol (@, #, :)',
      'palette provider wd.p3: prefix "a" must be empty or one symbol (@, #, :)',
      'palette prefix conflict: wd.symbols and wd.p1 both answer "@"',
      'palette prefix conflict: wd.goto and wd.p4 both answer plain text',
    ]);
    const broken = { id: 'wd.p5', title: msg('wd.p5', 'X'), prefix: 7, order: 1, placeholder: 'x', debounceMs: 9999 };
    expect(problems({ paletteProviders: [broken] })).toEqual(
      [
        'prefix 7 must be empty or one symbol (@, #, :)',
        "placeholder has no message id (use msg('<ns>.<name>', 'Text'))",
        'has no search function',
        'has no accept function',
        'debounceMs must be a number from 0 to 2000',
      ].map((x) => `palette provider wd.p5: ${x}`),
    );
  });

  it('fails on perspectives naming missing views', () => {
    const p = definePerspective({ id: 'wd.review', title: msg('wd.review', 'Review'), icon: 'wd:eye', order: 1, views: ['wd.code', 'wd.gone', 'wd.code'], focus: 'wd.other', description: 'x' as never });
    expect(problems({ perspectives: [p] })).toEqual([
      "perspective wd.review: description has no message id (use msg('<ns>.<name>', 'Text'))",
      'perspective wd.review: opens the missing view wd.gone',
      'perspective wd.review: lists a view twice',
      'perspective wd.review: focus wd.other is not one of its views',
    ]);
    expect(problems({ perspectives: [{ ...p, id: 'wd.empty', views: [], focus: undefined, description: undefined, when: 1 }] })).toEqual([
      'perspective wd.empty: opens no views',
      'perspective wd.empty: when is not a function',
    ]);
  });

  it('fails on shared state and assistant context missing their parts', () => {
    expect(problems({ sharedState: [{ id: 'wd.s', scope: 'all', read: 1, parse: 1, apply: 1, throttleMs: -1 }] })).toEqual([
      'shared state wd.s: scope must be full or display',
      'shared state wd.s: has no read function',
      'shared state wd.s: has no parse function',
      'shared state wd.s: has no apply function',
      'shared state wd.s: throttleMs must be a number from 0 to 10000',
    ]);
    expect(problems({ assistantContext: [{ id: 'wd.c', label: 'x', icon: 'x', value: 1, order: 1 }] })).toEqual([
      "assistant context wd.c: label has no message id (use msg('<ns>.<name>', 'Text'))",
      'assistant context wd.c: has no icon (namespace:name)',
      'assistant context wd.c: has no value function',
    ]);
  });
});

describe('layoutProblems and targetProblems', () => {
  it('reports a missing layout object and passes a good one', () => {
    expect(layoutProblems(null)).toEqual(['has no layout (area, min, pref, priority; ADR 0011)']);
    expect(layoutProblems(doc)).toEqual([]);
    expect(layoutProblems({ ...side, edge: 'left' })).toEqual([]);
  });

  it('answers the target of an item', () => {
    expect(activityTarget({ run: { view: 'x' } })).toEqual({ view: 'x' });
    expect(activityTarget({ run: { menu: () => [] } })).toBeNull();
    expect(targetProblems({ view: 1 }, { has: () => true, items: () => [] })).toEqual(['has no run target ({ view }, { command }, { explorer } or { menu })']);
  });
});

describe('panel ids', () => {
  it('builds and parses view instances', () => {
    expect(panelId('wd.code')).toBe('wd.code');
    expect(panelId('wd.code', 'src/a#b.rs')).toBe('wd.code#src/a#b.rs');
    expect(parsePanelId('wd.code#src/a#b.rs')).toEqual({ panel: 'wd.code#src/a#b.rs', view: 'wd.code', key: 'src/a#b.rs' });
    expect(parsePanelId('wd.code')).toEqual({ panel: 'wd.code', view: 'wd.code', key: null });
    expect(parsePanelId('#x')).toEqual({ panel: '#x', view: '#x', key: null });
    expect(parsePanelId('wd.code#')).toEqual({ panel: 'wd.code#', view: 'wd.code#', key: null });
    expect(() => panelId('wd.code', '')).toThrow(/1 to 1024/);
    expect(() => panelId('wd.code', 'x'.repeat(1025))).toThrow(/1 to 1024/);
  });
});

describe('WorkbenchRegistry', () => {
  function setup(ready: boolean | 'throw' | null): WorkbenchRegistry {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        { provide: SHELL_IDLE, useValue: () => undefined },
        provideWorkbenchContributions(VALID),
        ...(ready === null
          ? []
          : [
              provideShellContext<Demo>(
                () =>
                  Promise.resolve(() => ({
                    get ready(): boolean {
                      if (ready === 'throw') throw new Error('not yet');
                      return ready as boolean;
                    },
                  })),
                { prefetch: 'first-use' },
              ),
            ]),
      ],
    });
    return TestBed.inject(WorkbenchRegistry);
  }

  it('lists contributions in order and finds views, instances and providers', () => {
    const r = setup(null);
    expect(r.views.map((v) => v.id)).toEqual(['wd.assistant', 'wd.code', 'wd.late']);
    expect(r.findView('wd.code#src/main.rs')).toBe(code);
    expect(r.findView('wd.assistant#2')).toBeUndefined();
    expect(r.findView('wd.none')).toBeUndefined();
    expect(r.findPerspective('wd.understand')?.views).toEqual(['wd.code', 'wd.assistant']);
    expect(r.findExplorerProvider('wd.files')).toBe(files);
    expect(r.activityItems.map((a) => a.id)).toEqual(['wd.act.files', 'wd.act.ai', 'wd.act.refresh', 'wd.act.more']);
    expect(r.sharedState).toHaveLength(1);
    expect(r.assistantContext).toHaveLength(1);
    expect(r.availableExplorerProviders()).toEqual([files]);
    expect(r.availablePerspectives()).toHaveLength(1);
  });

  it('offers a view with when only once the context says so', async () => {
    const r = setup(true);
    expect(r.availableViews().map((v) => v.id)).toEqual(['wd.assistant', 'wd.code']);
    expect(r.isViewOffered('wd.late')).toBe(false);
    await TestBed.inject(ShellContextHost).load();
    expect(r.availableViews().map((v) => v.id)).toEqual(['wd.assistant', 'wd.code', 'wd.late']);
    expect(r.isViewOffered('wd.late')).toBe(true);
    expect(r.isViewOffered('wd.none')).toBe(false);
  });

  it('hides a view whose when throws', async () => {
    const r = setup('throw');
    await TestBed.inject(ShellContextHost).load();
    expect(r.isViewOffered('wd.late')).toBe(false);
  });

  it('picks the palette provider by prefix, else the plain one', () => {
    const r = setup(null);
    expect(r.paletteProviderFor('@  main')).toEqual({ provider: r.paletteProviders[1], query: 'main' });
    expect(r.paletteProviderFor('main')).toEqual({ provider: r.paletteProviders[0], query: 'main' });
    expect(r.paletteProviderFor('')).toEqual({ provider: r.paletteProviders[0], query: '' });
  });

  it('answers null when no provider takes plain text', () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideWorkbenchContributions({ paletteProviders: [VALID.paletteProviders![1]!] })] });
    expect(TestBed.inject(WorkbenchRegistry).paletteProviderFor('main')).toBeNull();
  });

  it('isOffered holds without when, and not before the context', () => {
    expect(isOffered(undefined, null)).toBe(true);
    expect(isOffered(() => true, null)).toBe(false);
  });
});

describe('WORKBENCH_PORTS default', () => {
  it('warns and does nothing without a frame', async () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const ports = TestBed.inject(WORKBENCH_PORTS);
    expect(await ports.showView('x')).toBe(false);
    expect(await ports.applyPerspective('x')).toBe(false);
    ports.closeView('x');
    ports.showExplorer('x');
    ports.hideExplorer();
    ports.openPalette();
    expect(ports.openPanels()).toEqual([]);
    expect(ports.activePanel()).toBeNull();
    expect(ports.activePerspective()).toBeNull();
    expect(ports.explorerShown()).toBeNull();
    expect(warn).toHaveBeenCalledTimes(6);
    warn.mockRestore();
  });
});

describe('provideLiveContributions', () => {
  it('builds the batch once per injector and registers every kind it names', () => {
    const build = vi.fn((injector: Injector) => ({
      commands: [defineCommand({ id: 'wd.live', title: msg('wd.live', 'Live'), run: () => void injector.get(WORKBENCH_PORTS).openPalette('>') })],
      views: [code as never],
    }));
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideLiveContributions(build)] });
    expect(contributionProblems(TestBed.inject(Injector))).toEqual([]);
    expect(build).toHaveBeenCalledTimes(1);
    expect(TestBed.inject(WorkbenchRegistry).views).toEqual([code]);
  });
});

describe('panelTitle', () => {
  it('names an instance by the last part of its key', async () => {
    const { panelTitle } = await import('../public-api');
    expect(panelTitle(code, 'wd.code')).toBe('Code');
    expect(panelTitle(code, 'wd.code#src/server/mod.rs')).toBe('mod.rs');
    expect(panelTitle(code, 'wd.code#C:\\x\\a.rs')).toBe('a.rs');
    expect(panelTitle(code, 'wd.code#//')).toBe('//');
  });
});
