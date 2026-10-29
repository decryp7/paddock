import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { defineCommand, defineKeybinding, msg, ShellContextHost } from '@decryp7/paddock-ui/shell';
import { KEY_PLATFORM } from '@decryp7/paddock-ui/shell/menu';
import { provideShellTesting, render, settle, shellPorts } from '@decryp7/paddock-ui/shell/testing';
import { definePaletteProvider, defineView, provideWorkbenchContributions, WORKBENCH_PORTS, type PaletteItem, type WorkbenchPorts } from '@decryp7/paddock-ui/workbench';
import { PdCommandPalette } from './command-palette';
import { namesPaletteProvider, namesToItems, type NamesSearchReply } from './names';
import { PALETTE_TIMERS, PaletteStore, type PaletteTimers } from './palette.store';
import { PALETTE_COMMANDS, providePalette } from './provide-palette';
import { MAX_RECENT, PALETTE_RECENT_KEY, PALETTE_RECENT_STORAGE, PaletteRecent } from './recent';

interface Ctx {
  readonly ran: string[];
  readonly ready: boolean;
}

const ran: string[] = [];
const load = () => Promise.resolve(class {});
const layout = { area: 'document' as const, min: { width: 400, height: 200 }, pref: { width: 800, height: 600 }, priority: 50 };

/** A provider whose searches the test answers by hand (to check cancellation and late answers). */
class Deferred {
  readonly calls: { query: string; signal: AbortSignal; resolve: (items: PaletteItem[]) => void; reject: (e: unknown) => void }[] = [];
  search = (query: string, _ctx: unknown, request: { signal: AbortSignal }) =>
    new Promise<PaletteItem[]>((resolve, reject) => this.calls.push({ query, signal: request.signal, resolve, reject }));
}

class FakeTimers implements PaletteTimers {
  readonly pending = new Map<number, () => void>();
  private n = 0;
  set(run: () => void): unknown {
    this.pending.set(++this.n, run);
    return this.n;
  }
  clear(handle: unknown): void {
    this.pending.delete(handle as number);
  }
  flush(): void {
    for (const [k, run] of [...this.pending]) {
      this.pending.delete(k);
      run();
    }
  }
}

function fakePorts(): WorkbenchPorts & { shown: string[]; opened: string[] } {
  const shown: string[] = [];
  const opened: string[] = [];
  return {
    shown,
    opened,
    showView: (id) => {
      shown.push(id);
      return Promise.resolve(true);
    },
    closeView: () => undefined,
    openPanels: signal([]),
    activePanel: signal(null),
    applyPerspective: () => Promise.resolve(true),
    activePerspective: signal(null),
    showExplorer: () => undefined,
    hideExplorer: () => undefined,
    explorerShown: signal(null),
    openPalette: (prefix) => void opened.push(prefix ?? '>'),
  };
}

function setup(options: { files?: Deferred; symbols?: Deferred; storage?: Map<string, string> | null } = {}) {
  ran.length = 0;
  const ports = fakePorts();
  const timers = new FakeTimers();
  const storage = options.storage === undefined ? new Map<string, string>() : options.storage;
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: provideShellTesting<Ctx>({
      context: () => Promise.resolve(() => ({ ran, ready: true })),
      providers: [
        { provide: KEY_PLATFORM, useValue: 'other' },
        { provide: WORKBENCH_PORTS, useValue: ports },
        { provide: PALETTE_TIMERS, useValue: timers },
        { provide: PALETTE_RECENT_STORAGE, useValue: storage ? { getItem: (k: string) => storage.get(k) ?? null, setItem: (k: string, v: string) => void storage.set(k, v) } : null },
        provideWorkbenchContributions<Ctx>({
          commands: [
            defineCommand<Ctx>({ id: 'wd.open-folder', title: msg('wd.open-folder', 'Open folder…'), category: msg('wd.file', 'File'), run: (ctx) => void ctx.ran.push('open-folder') }),
            defineCommand<Ctx>({ id: 'wd.export', title: msg('wd.export', 'Export events'), run: (ctx) => void ctx.ran.push('export') }),
            defineCommand<Ctx>({ id: 'wd.locked', title: msg('wd.locked', 'Locked thing'), enabled: () => false, run: (ctx) => void ctx.ran.push('locked') }),
          ],
          keybindings: [defineKeybinding({ command: 'wd.open-folder', key: 'Mod+O' })],
          views: [
            defineView({ id: 'wd.timeline', title: msg('wd.timeline', 'Timeline'), icon: 'wd:timeline', layout, load }),
            defineView({ id: 'wd.graph', title: msg('wd.graph', 'Graph'), icon: 'wd:graph', layout, load, category: msg('wd.analysis', 'Analysis') }),
          ],
          paletteProviders: [
            ...(options.files
              ? [definePaletteProvider<Ctx>({ id: 'wd.files', title: msg('wd.files', 'Go to file'), order: 100, placeholder: msg('wd.files.ph', 'File name'), search: options.files.search, accept: (item, ctx) => void ctx.ran.push(`file:${item.id}`) })]
              : []),
            ...(options.symbols
              ? [
                  definePaletteProvider<Ctx>({
                    id: 'wd.symbols',
                    title: msg('wd.symbols', 'Go to symbol'),
                    prefix: '@',
                    order: 200,
                    debounceMs: 100,
                    search: options.symbols.search,
                    accept: () => Promise.reject(new Error('gone')),
                  }),
                ]
              : []),
          ],
        }),
        providePalette({ commonCommands: ['wd.export'] }),
      ],
    }),
  });
  return { store: TestBed.inject(PaletteStore), ports, timers, storage };
}

describe('PaletteStore', () => {
  it('lists commands and views ranked, with keys, categories and disabled reasons', async () => {
    const { store } = setup();
    await TestBed.inject(ShellContextHost).load();
    store.open();
    expect(store.source()).toEqual({ kind: 'commands' });
    // Common command first, then registration order; views follow commands.
    expect(store.rows().map((r) => r.label)).toEqual(['Export events', 'Open folder…', 'Locked thing', 'Show all commands', 'Go to…', 'Open Graph', 'Open Timeline']);
    const folder = store.rows()[1]!;
    expect(folder).toMatchObject({ category: 'File', keybinding: 'Ctrl+O' });
    expect(store.rows()[2]!.disabled).toBe('Not available now');
    expect(store.rows()[5]!.category).toBe('Analysis');
    expect(store.rows()[6]!.category).toBe('View');
    store.setQuery('> folder');
    expect(store.rows().map((r) => r.label)).toEqual(['Open folder…']);
    expect([...store.rows()[0]!.marks]).toEqual([5, 6, 7, 8, 9, 10]);
    expect(store.prefixes()).toEqual([{ prefix: '>', title: 'Commands', active: true }]);
    expect(store.placeholder()).toBe('Type a command or search…');
  });

  it('suggests close commands when nothing matches', () => {
    const { store } = setup();
    store.open('>exprot');
    expect(store.suggesting()).toBe(true);
    expect(store.rows().map((r) => r.label)).toEqual(['Export events']);
    store.setQuery('>zzzzzz');
    expect(store.suggesting()).toBe(false);
    expect(store.rows()).toEqual([]);
  });

  it('runs a command, remembers it as recent, and shows a view', async () => {
    const { store, ports, storage } = setup();
    store.open();
    store.setQuery('>folder');
    expect(await store.accept()).toBe(true);
    expect(ran).toEqual(['open-folder']);
    expect(store.isOpen()).toBe(false);
    expect(JSON.parse(storage!.get(PALETTE_RECENT_KEY)!)).toEqual(['wd.open-folder']);
    store.open();
    expect(store.rows()[0]!.label).toBe('Open folder…');
    store.setQuery('>graph');
    expect(await store.accept(0)).toBe(true);
    expect(ports.shown).toEqual(['wd.graph']);
  });

  it('does nothing for a disabled row or an index past the list', async () => {
    const { store } = setup();
    await TestBed.inject(ShellContextHost).load();
    store.open('>locked');
    expect(await store.accept()).toBe(false);
    expect(await store.accept(9)).toBe(false);
    expect(ran).toEqual([]);
    expect(store.isOpen()).toBe(true);
  });

  it('moves the highlight with wrap-around and clamps setActive', () => {
    const { store } = setup();
    store.open();
    store.move(-1);
    expect(store.active()).toBe(6);
    store.move(1);
    expect(store.active()).toBe(0);
    store.setActive(99);
    expect(store.active()).toBe(6);
    store.setQuery('>zzzzzz');
    store.move(1);
    store.setActive(3);
    expect(store.active()).toBe(0);
  });

  it('asks the plain-text provider, drops a late answer and aborts on the next keystroke (W5 acceptance)', async () => {
    const files = new Deferred();
    const { store } = setup({ files });
    store.open('');
    store.setQuery('mai');
    await settle();
    store.setQuery('main');
    await settle();
    // The empty query's search was replaced before it reached the provider.
    expect(files.calls.map((c) => c.query)).toEqual(['mai', 'main']);
    expect(files.calls[0]!.signal.aborted).toBe(true);
    expect(files.calls[1]!.signal.aborted).toBe(false);
    expect(store.isLoading()).toBe(true);
    files.calls[0]!.resolve([{ id: 'stale', label: 'stale.rs' }]);
    await settle();
    expect(store.rows()).toEqual([]);
    files.calls[1]!.resolve([{ id: 'src/main.rs', label: 'main.rs', detail: 'src', icon: 'wd:file', keybinding: 'Mod+1', highlights: [[0, 4]] }]);
    await settle();
    expect(store.isLoaded()).toBe(true);
    expect(store.rows()).toEqual([{ key: '0:src/main.rs', label: 'main.rs', detail: 'src', icon: 'wd:file', keybinding: 'Ctrl+1', marks: new Set([0, 1, 2, 3]) }]);
    expect(store.placeholder()).toBe('File name');
    expect(await store.accept()).toBe(true);
    expect(ran).toEqual(['file:src/main.rs']);
  });

  it('debounces a provider that asks for it and cancels the pending search on close', async () => {
    const symbols = new Deferred();
    const { store, timers } = setup({ symbols });
    store.open('@');
    store.setQuery('@ ma');
    store.setQuery('@ main');
    expect(timers.pending.size).toBe(1);
    expect(symbols.calls).toEqual([]);
    timers.flush();
    await settle();
    expect(symbols.calls.map((c) => c.query)).toEqual(['main']);
    store.close();
    expect(symbols.calls[0]!.signal.aborted).toBe(true);
    expect(store.requestStatus()).toBe('idle');
    store.open('@x');
    store.close();
    expect(timers.pending.size).toBe(0);
  });

  it('falls back to the commands without a plain-text provider, and shows a provider error with Retry', async () => {
    const symbols = new Deferred();
    const { store, timers } = setup({ symbols });
    store.open('');
    expect(store.source()).toEqual({ kind: 'commands' });
    expect(store.prefixes().map((p) => p.prefix)).toEqual(['>', '@']);
    // The footer marks the mode in use (review 3 P3-10).
    expect(store.prefixes().map((p) => p.active)).toEqual([true, false]);
    store.setQuery('@x');
    expect(store.prefixes().map((p) => p.active)).toEqual([false, true]);
    timers.flush();
    await settle();
    symbols.calls[0]!.reject(new Error('index is gone'));
    await settle();
    expect(store.requestError()).toBe('index is gone');
    store.retry();
    timers.flush();
    await settle();
    expect(symbols.calls).toHaveLength(2);
    symbols.calls[1]!.resolve('nonsense' as never);
    await settle();
    expect(store.rows()).toEqual([]);
  });

  it('reports a provider whose accept fails, and ignores an answer to an aborted signal', async () => {
    const symbols = new Deferred();
    const { store, timers } = setup({ symbols });
    store.open('@a');
    timers.flush();
    await settle();
    symbols.calls[0]!.resolve([{ id: 's1', label: 'main' }]);
    await settle();
    expect(await store.accept()).toBe(false);
    expect(shellPorts().messages()).toEqual(['gone']);
    store.open('@b');
    timers.flush();
    await settle();
    store.setQuery('@c');
    symbols.calls[1]!.reject(new Error('late'));
    await settle();
    expect(store.requestError()).toBeNull();
  });
});

describe('PaletteRecent', () => {
  it('keeps the newest first, bounded, and survives broken storage', () => {
    const storage = new Map<string, string>([[PALETTE_RECENT_KEY, JSON.stringify(['a', 3, 'b'])]]);
    setup({ storage });
    const recent = TestBed.inject(PaletteRecent);
    expect(recent.ids()).toEqual(['a', 'b']);
    for (let i = 0; i < 12; i++) recent.add(`c${i}`);
    expect(recent.ids()).toHaveLength(MAX_RECENT);
    expect(recent.ids()[0]).toBe('c11');
    setup({ storage: new Map([[PALETTE_RECENT_KEY, '{nope']]) });
    expect(TestBed.inject(PaletteRecent).ids()).toEqual([]);
    setup({ storage: new Map([[PALETTE_RECENT_KEY, '{"a":1}']]) });
    expect(TestBed.inject(PaletteRecent).ids()).toEqual([]);
    setup({ storage: null });
    const none = TestBed.inject(PaletteRecent);
    none.add('x');
    expect(none.ids()).toEqual(['x']);
  });

  it('keeps going when the storage throws', () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: provideShellTesting({
        providers: [{ provide: PALETTE_RECENT_STORAGE, useValue: { getItem: () => null, setItem: () => { throw new Error('full'); } } }],
      }),
    });
    const recent = TestBed.inject(PaletteRecent);
    recent.add('x');
    expect(recent.ids()).toEqual(['x']);
  });
});

describe('palette commands', () => {
  it('open the palette through the workbench ports', async () => {
    const { ports } = setup();
    const { CommandService } = await import('@decryp7/paddock-ui/shell');
    const commands = TestBed.inject(CommandService);
    expect(await commands.run(PALETTE_COMMANDS.commands)).toBe(true);
    expect(await commands.run(PALETTE_COMMANDS.quickOpen)).toBe(true);
    expect(ports.opened).toEqual(['>', '']);
  });
});

describe('namesPaletteProvider', () => {
  const reply: NamesSearchReply = {
    matches: [{ id: 'p1', display: 'Graphic.Builder.exe', how: 'typo', score: 0.5, why: 'close in spelling' }],
    clarify: { question: 'By "b", do you mean A or B?', options: ['a', 'b'] },
    searchable: 0,
  };

  it('maps a reply to rows: the indexing note, the question, then matches with why', () => {
    expect(namesToItems(reply, 'wd:proc', 'Indexing')).toEqual([
      { id: 'pd.names.building', label: 'Indexing', disabled: 'Indexing', match: null },
      { id: 'pd.names.clarify', label: 'By "b", do you mean A or B?', disabled: 'By "b", do you mean A or B?', match: null },
      { id: 'p1', label: 'Graphic.Builder.exe', detail: 'close in spelling', icon: 'wd:proc', match: reply.matches[0] },
    ]);
    expect(namesToItems({ matches: [], clarify: null, searchable: 3 })).toEqual([]);
  });

  it('gives each match its own icon when the app maps matches to glyphs (the kind of a symbol, review 2 P3-5)', async () => {
    const two: NamesSearchReply = {
      matches: [
        { id: 'a#run', display: 'run', how: 'parts', score: 1, why: 'a' },
        { id: 'a#Thing', display: 'Thing', how: 'parts', score: 1, why: 'a' },
      ],
      clarify: null,
      searchable: 2,
    };
    expect(namesToItems(two, (m) => (m.display === 'run' ? 'pd:symbol-function' : undefined)).map((i) => i.icon)).toEqual(['pd:symbol-function', undefined]);
    const ctx = { kinds: { 'a#Thing': 'pd:symbol-type' } as Record<string, string> };
    const p = namesPaletteProvider<typeof ctx>({ id: 'wd.sym', title: msg('wd.sym', 'Symbols'), order: 1, search: () => Promise.resolve(two), accept: () => undefined, icon: (m, c) => c.kinds[m.id] });
    const items = await p.search('t', ctx as never, { limit: 5, signal: new AbortController().signal });
    expect(items.map((i) => i.icon)).toEqual([undefined, 'pd:symbol-type']);
  });

  it('calls names_search with a bounded query and limit, and accepts only matches', async () => {
    const search = vi.fn(() => Promise.resolve(reply));
    const accepted: string[] = [];
    const p = namesPaletteProvider<object>({ id: 'wd.names', title: msg('wd.names', 'Names'), prefix: ':', order: 1, search, accept: (m) => void accepted.push(m.id), when: () => true, placeholder: msg('wd.names.ph', 'Name') });
    const signal = new AbortController().signal;
    expect(await p.search('   ', {} as never, { limit: 10, signal })).toEqual([]);
    const items = await p.search(` ${'x'.repeat(300)} `, {} as never, { limit: 500, signal });
    expect(search).toHaveBeenCalledWith({ query: 'x'.repeat(256), limit: 50 }, {}, signal);
    expect(items[0]!.label).toBe('Still indexing names…');
    await p.accept(items[2]!, {} as never);
    await p.accept(items[1]!, {} as never);
    expect(accepted).toEqual(['p1']);
    expect(p.debounceMs).toBe(80);
    const q = namesPaletteProvider<object>({ id: 'wd.n2', title: msg('wd.n2', 'N'), order: 1, search, accept: () => undefined, building: msg('wd.b', 'Wait'), debounceMs: 0 });
    expect(q.prefix).toBe('');
    expect((await q.search('a', {} as never, { limit: 5, signal }))[0]!.label).toBe('Wait');
  });
});

/** A key pressed on the document, as the shortcut handler sees it. */
function press(key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const e = new KeyboardEvent('keydown', { key, cancelable: true, bubbles: true, ...init });
  document.dispatchEvent(e);
  return e;
}

describe('PdCommandPalette', () => {
  it('keeps the keys typed before the input has the focus', async () => {
    const { store } = setup();
    const { fixture, element } = await render(PdCommandPalette);
    store.open();
    // Before change detection draws the input: the shortcut was pressed a moment ago.
    expect(press('f').defaultPrevented).toBe(true);
    press('o');
    press('x');
    press('Backspace');
    press('l');
    expect(store.query()).toBe('>fol');
    // A shortcut's own keys are left to their handlers.
    expect(press('p', { ctrlKey: true }).defaultPrevented).toBe(false);
    expect(store.query()).toBe('>fol');
    await settle(fixture);
    const input = element.querySelector<HTMLInputElement>('[data-testid="palette-input"]')!;
    expect(input.value).toBe('>fol');
    expect(document.activeElement).toBe(input);
    expect(input.selectionStart).toBe(4);
    expect(ran).toEqual([]);
    // Escape before the input is there closes the palette too.
    store.close();
    await settle(fixture);
    store.open();
    expect(press('Escape').defaultPrevented).toBe(true);
    expect(store.isOpen()).toBe(false);
  });

  it('holds an Enter pressed early until the input is there, then accepts the row of what was typed', async () => {
    const { store } = setup();
    const { fixture } = await render(PdCommandPalette);
    store.open();
    press('f');
    press('o');
    press('l');
    expect(press('Enter').defaultPrevented).toBe(true);
    // Nothing ran yet, and not the first row of the list nobody looked at.
    expect(ran).toEqual([]);
    expect(store.isOpen()).toBe(true);
    await settle(fixture);
    expect(ran).toEqual(['open-folder']);
    expect(store.isOpen()).toBe(false);
  });

  it('renders the list, highlights, handles keys and returns focus on Escape', async () => {
    const { store } = setup();
    const opener = document.createElement('button');
    document.body.append(opener);
    opener.focus();
    const { fixture, element } = await render(PdCommandPalette);
    expect(element.querySelector('[data-testid="palette"]')).toBeNull();
    store.open();
    await settle(fixture);
    const input = element.querySelector<HTMLInputElement>('[data-testid="palette-input"]')!;
    expect(document.activeElement).toBe(input);
    expect(element.querySelectorAll('[role="option"]')).toHaveLength(7);
    // The empty list reads as frequent, then the rest (no recent yet), under small headings (review P3-1).
    expect([...element.querySelectorAll('.pd-pal-group')].map((g) => g.textContent)).toEqual(['Frequently used', 'Other commands']);
    expect(input.getAttribute('aria-activedescendant')).toMatch(/-0$/);
    input.value = '>folder';
    input.dispatchEvent(new Event('input'));
    await settle(fixture);
    expect(element.querySelector('mark')?.textContent).toBe('folder');
    expect(element.querySelector('.pd-pal-kbd')?.textContent).toBe('Ctrl+O');
    expect(element.querySelector('.pd-pal-cat')?.textContent).toBe('File:');
    const key = (k: string) => input.dispatchEvent(new KeyboardEvent('keydown', { key: k, cancelable: true }));
    input.value = '>';
    input.dispatchEvent(new Event('input'));
    for (const k of ['ArrowDown', 'ArrowDown', 'ArrowUp', 'PageDown', 'PageUp', 'Tab', 'x']) key(k);
    await settle(fixture);
    expect(store.active()).toBe(0);
    key('Escape');
    await settle(fixture);
    expect(store.isOpen()).toBe(false);
    expect(document.activeElement).toBe(opener);
    opener.remove();
  });

  it('shows suggestions, the empty state, errors with Retry, and runs a clicked row', async () => {
    const symbols = new Deferred();
    const { store, timers } = setup({ symbols });
    const { fixture, element } = await render(PdCommandPalette);
    store.open('>exprot');
    await settle(fixture);
    expect(element.querySelector('[data-testid="palette-did-you-mean"]')?.textContent).toContain('exprot');
    store.setQuery('>zzzzzz');
    await settle(fixture);
    expect(element.querySelector('[data-testid="palette-empty"]')?.textContent).toBe('No command matches “zzzzzz”');
    store.setQuery('@x');
    timers.flush();
    await settle(fixture);
    expect(element.querySelector('[data-testid="palette-status"]')?.textContent).toContain('Searching');
    expect(element.querySelector('[data-testid="palette-empty"]')).toBeNull();
    symbols.calls[0]!.reject(new Error('broken index'));
    await settle(fixture);
    element.querySelector<HTMLButtonElement>('[data-testid="palette-retry"]')!.click();
    timers.flush();
    await settle(fixture);
    symbols.calls[1]!.resolve([{ id: 'a', label: 'alpha', icon: 'wd:sym', detail: 'src/a.rs' }]);
    await settle(fixture);
    const option = element.querySelector<HTMLElement>('[role="option"]')!;
    expect(option.querySelector('.pd-pal-detail [data-testid="path-text"]')?.getAttribute('data-full')).toBe('src/a.rs');
    option.dispatchEvent(new MouseEvent('mousemove'));
    store.setQuery('>export');
    await settle(fixture);
    element.querySelector<HTMLElement>('[role="option"]')!.click();
    await settle(fixture);
    expect(ran).toEqual(['export']);
    store.open();
    await settle(fixture);
    element.querySelector<HTMLElement>('[data-testid="palette-backdrop"]')!.dispatchEvent(new MouseEvent('mousedown'));
    await settle(fixture);
    expect(store.isOpen()).toBe(false);
    store.open('>');
    await settle(fixture);
    element.querySelector<HTMLInputElement>('[data-testid="palette-input"]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', cancelable: true }));
    await settle(fixture);
    expect(ran).toEqual(['export', 'export']);
  });
});

describe('palette defaults', () => {
  it('uses the browser timers and storage by default', () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: provideShellTesting() });
    const timers = TestBed.inject(PALETTE_TIMERS);
    const run = vi.fn();
    timers.clear(timers.set(run, 1000));
    expect(run).not.toHaveBeenCalled();
    expect(TestBed.inject(PALETTE_RECENT_STORAGE)).toBe(localStorage);
  });
});
