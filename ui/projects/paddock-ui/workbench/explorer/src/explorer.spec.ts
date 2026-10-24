import { TestBed } from '@angular/core/testing';
import { defineCommand, msg, ShellContextHost } from '@decryp7/paddock-ui/shell';
import { provideShellTesting, render, settle, shellPorts } from '@decryp7/paddock-ui/shell/testing';
import { defineExplorerProvider, provideWorkbenchContributions, type ExplorerNode, type ExplorerPage, type ExplorerRequest } from '@decryp7/paddock-ui/workbench';
import { PdExplorer } from './explorer';
import { EXPLORER_MAX_TEXT, ExplorerStore, treeKey } from './explorer.store';

interface Ctx {
  readonly log: string[];
}
const log: string[] = [];

/** A provider the test answers by hand. */
class Tree {
  readonly calls: { parent: string; request: ExplorerRequest; resolve: (p: ExplorerPage) => void; reject: (e: unknown) => void }[] = [];
  ask(parent: string, request: ExplorerRequest): Promise<ExplorerPage> {
    return new Promise((resolve, reject) => this.calls.push({ parent, request, resolve, reject }));
  }
  last(parent: string) {
    return [...this.calls].reverse().find((c) => c.parent === parent)!;
  }
}

const folder = (id: string): ExplorerNode => ({ id, label: id, expandable: true });
const file = (id: string, extra: Partial<ExplorerNode> = {}): ExplorerNode => ({ id, label: id, expandable: false, ...extra });

function setup(tree: Tree, extra: { second?: boolean; throwing?: boolean } = {}) {
  log.length = 0;
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: provideShellTesting<Ctx>({
      context: () => Promise.resolve(() => ({ log })),
      providers: [
        ExplorerStore,
        provideWorkbenchContributions<Ctx>({
          commands: [defineCommand<Ctx>({ id: 'wd.open', title: msg('wd.open', 'Open folder…'), run: (ctx) => void ctx.log.push('cmd') })],
          explorerProviders: [
            defineExplorerProvider<Ctx>({
              id: 'wd.files',
              title: msg('wd.files', 'Files'),
              order: 100,
              roots: (_ctx, r) => tree.ask('', r),
              children: (node, _ctx, r) => tree.ask(node.id, r),
              open: (node, ctx) => {
                if (node.id === 'bad') throw new Error('cannot open');
                ctx.log.push(`open:${node.id}`);
              },
              actions: (node) => {
                if (extra.throwing) throw new Error('x');
                return node.id === 'none' ? ('nope' as never) : [
                  { id: 'copy', label: 'Copy path', icon: 'pd:copy', group: 'a' },
                  { id: 'gone', label: 'Delete', group: 'b', danger: true },
                  { id: 'off', label: 'Locked', group: 'b', disabled: 'Not now' },
                ];
              },
              run: (action, node, ctx) => {
                if (action === 'gone') return Promise.reject(new Error('refused'));
                ctx.log.push(`${action}:${node.id}`);
                return undefined;
              },
              drag: (node) => {
                if (node.id === 'boom') throw new Error('x');
                return node.expandable ? null : { type: 'text/uri-list', data: `file:///${node.id}` };
              },
              empty: { text: msg('wd.files.empty', 'Open a folder to start'), command: 'wd.open' },
            }),
            ...(extra.second
              ? [defineExplorerProvider<Ctx>({ id: 'wd.outline', title: msg('wd.outline', 'Outline'), order: 200, roots: () => Promise.resolve({ nodes: [file('fn main')] }) })]
              : []),
          ],
        }),
      ],
    }),
  });
  return TestBed.inject(ExplorerStore);
}

describe('ExplorerStore', () => {
  it('loads roots, pages children, and keeps one status per list', async () => {
    const tree = new Tree();
    const store = setup(tree);
    store.ensureRoots();
    await settle();
    expect(store.rows()).toEqual([{ kind: 'status', section: 'wd.files', parent: '', depth: 0, key: `${treeKey('wd.files', '')}\u0000status`, status: 'loading' }]);
    tree.last('').resolve({ nodes: [folder('src'), file('README.md'), file('README.md'), { id: '', label: 'x' } as never, null as never], next: 'p2', total: 5 });
    await settle();
    expect(store.rows().map((r) => (r.kind === 'node' ? r.node.id : r.kind))).toEqual(['src', 'README.md', 'more']);
    expect(store.rows()[2]).toMatchObject({ kind: 'more', remaining: 3 });
    void store.more('wd.files', '');
    await settle();
    expect(tree.last('').request.cursor).toBe('p2');
    tree.last('').resolve({ nodes: [file('LICENSE'), file('README.md')] });
    await settle();
    expect(store.rows().map((r) => (r.kind === 'node' ? r.node.id : r.kind))).toEqual(['src', 'README.md', 'LICENSE']);
    // No next page: asking for more does nothing.
    await store.more('wd.files', '');
    expect(tree.calls).toHaveLength(2);
    store.ensureRoots();
    expect(tree.calls).toHaveLength(2);
  });

  it('expands and collapses: collapsing aborts the request and drops the late answer', async () => {
    const tree = new Tree();
    const store = setup(tree);
    store.ensureRoots();
    await settle();
    tree.last('').resolve({ nodes: [folder('src'), file('a.rs')] });
    await settle();
    const src = folder('src');
    store.expand('wd.files', file('a.rs'));
    store.expand('wd.files', src);
    store.expand('wd.files', src);
    await settle();
    const pending = tree.last('src');
    store.collapse('wd.files', src);
    expect(pending.request.signal.aborted).toBe(true);
    pending.resolve({ nodes: [file('late.rs')] });
    await settle();
    expect(store.rows().map((r) => (r.kind === 'node' ? r.node.id : r.kind))).toEqual(['src', 'a.rs']);
    store.expand('wd.files', src);
    await settle();
    tree.last('src').resolve({ nodes: [folder('lib')] });
    await settle();
    store.expand('wd.files', folder('lib'));
    await settle();
    tree.last('lib').resolve({ nodes: [file('mod.rs', { label: 'x'.repeat(5000), description: 'y'.repeat(5000) })] });
    await settle();
    const deep = store.rows().find((r) => r.kind === 'node' && r.node.id === 'mod.rs');
    expect(deep).toMatchObject({ depth: 2, parentKey: treeKey('wd.files', 'lib') });
    expect(deep?.kind === 'node' && deep.node.label.length).toBe(EXPLORER_MAX_TEXT);
    // Collapsing src forgets lib's expansion too.
    store.collapse('wd.files', src);
    expect(store.expanded()).toEqual([]);
  });

  it('shows an error with Retry, and a missing parent as an error', async () => {
    const tree = new Tree();
    const store = setup(tree);
    store.ensureRoots();
    await settle();
    tree.last('').reject(new Error('index is gone'));
    await settle();
    expect(store.rows()[0]).toMatchObject({ kind: 'status', status: { error: 'index is gone' } });
    void store.retry('wd.files', '');
    await settle();
    tree.last('').resolve({ nodes: [folder('src')] });
    await settle();
    await store.retry('wd.files', 'ghost');
    expect(store.lists()[treeKey('wd.files', 'ghost')]?.status).toEqual({ error: 'This item has no children to show.' });
    await store.retry('wd.none', '');
    expect(store.lists()[treeKey('wd.none', '')]).toBeUndefined();
  });

  it('refreshes a section and toggles sections', async () => {
    const tree = new Tree();
    const store = setup(tree, { second: true });
    store.ensureRoots();
    await settle();
    tree.last('').resolve({ nodes: [file('a')] });
    await settle();
    expect(store.rows().map((r) => r.section)).toEqual(['wd.files', 'wd.outline']);
    store.toggleSection('wd.outline');
    expect(store.isSectionCollapsed('wd.outline')).toBe(true);
    expect(store.rows().map((r) => r.section)).toEqual(['wd.files']);
    store.toggleSection('wd.outline');
    void store.refresh('wd.files');
    await settle();
    expect(tree.calls).toHaveLength(2);
    tree.last('').resolve({ nodes: [file('b')] });
    await settle();
    expect(store.rows().filter((r) => r.section === 'wd.files').map((r) => (r.kind === 'node' ? r.node.id : r.kind))).toEqual(['b']);
  });

  it('opens nodes, lists and runs actions, and reports failures as notices', async () => {
    const tree = new Tree();
    const store = setup(tree);
    expect(store.actions('wd.files', file('a'))).toEqual([]);
    await TestBed.inject(ShellContextHost).load();
    expect(store.actions('wd.files', file('a')).map((a) => a.id)).toEqual(['copy', 'gone', 'off']);
    expect(store.actions('wd.files', file('none'))).toEqual([]);
    expect(store.actions('wd.other', file('a'))).toEqual([]);
    await store.open('wd.files', file('a'));
    await store.open('wd.files', file('bad'));
    await store.open('wd.other', file('a'));
    await store.run('wd.files', 'copy', file('a'));
    await store.run('wd.files', 'gone', file('a'));
    await store.run('wd.other', 'copy', file('a'));
    expect(log).toEqual(['open:a', 'copy:a']);
    expect(shellPorts().messages()).toEqual(['cannot open', 'refused']);
    expect(store.dragData('wd.files', file('a'))).toEqual({ type: 'text/uri-list', data: 'file:///a' });
    expect(store.dragData('wd.files', folder('src'))).toBeNull();
    expect(store.dragData('wd.files', file('boom'))).toBeNull();
    expect(store.dragData('wd.other', file('a'))).toBeNull();
    store.select('x');
    expect(store.selected()).toBe('x');
  });

  it('gives no actions when the provider throws', async () => {
    const store = setup(new Tree(), { throwing: true });
    await TestBed.inject(ShellContextHost).load();
    expect(store.actions('wd.files', file('a'))).toEqual([]);
  });
});

describe('PdExplorer', () => {
  async function shown(tree: Tree, nodes: ExplorerNode[]) {
    const store = setup(tree, { second: true });
    const r = await render(PdExplorer);
    tree.last('').resolve({ nodes, next: 'n2' });
    await settle(r.fixture);
    return { store, ...r };
  }

  it('draws sections and a tree with one tab stop, and moves with the keyboard', async () => {
    const tree = new Tree();
    const { store, fixture, element } = await shown(tree, [folder('src'), file('README.md', { badge: 'M', badgeLabel: 'modified', description: '2 KB', icon: 'pd:folder' }), file('long', { truncate: 'end', problem: 'Gone' })]);
    expect([...element.querySelectorAll('[data-explorer-section]')].map((e) => e.getAttribute('data-explorer-section'))).toEqual(['wd.files', 'wd.outline']);
    const items = () => [...element.querySelectorAll<HTMLElement>('[data-explorer-section="wd.files"] [role="treeitem"]')];
    expect(items().map((i) => i.getAttribute('tabindex'))).toEqual(['0', '-1', '-1']);
    expect(items()[1]!.querySelector('.pd-ex-badge')?.getAttribute('aria-label')).toBe('modified');
    expect(items()[2]!.getAttribute('aria-disabled')).toBe('true');
    const tree0 = element.querySelector<HTMLElement>('[data-explorer-section="wd.files"] [role="tree"]')!;
    const key = (k: string, shiftKey = false) => tree0.dispatchEvent(new KeyboardEvent('keydown', { key: k, shiftKey, bubbles: true, cancelable: true }));
    key('ArrowDown');
    key('End');
    key('Home');
    key('ArrowRight');
    await settle(fixture);
    expect(store.expanded()).toEqual([treeKey('wd.files', 'src')]);
    tree.last('src').resolve({ nodes: [file('main.rs')] });
    await settle(fixture);
    key('ArrowRight');
    await settle(fixture);
    expect(store.selected()).toBe(treeKey('wd.files', 'main.rs'));
    key('ArrowLeft');
    await settle(fixture);
    expect(store.selected()).toBe(treeKey('wd.files', 'src'));
    key('ArrowLeft');
    key('ArrowUp');
    key('Enter');
    key('q');
    await settle(fixture);
    expect(store.expanded()).toEqual([]);
    expect(log).toEqual(['open:src']);
  });

  it('clicks, toggles, double-clicks, drags, shows more and opens the context menu', async () => {
    const tree = new Tree();
    const { store, fixture, element } = await shown(tree, [folder('src'), file('a.rs')]);
    await TestBed.inject(ShellContextHost).load();
    const row = (id: string) => element.querySelector<HTMLElement>(`[data-node-id="${id}"]`)!;
    row('a.rs').click();
    expect(store.selected()).toBe(treeKey('wd.files', 'a.rs'));
    row('a.rs').dispatchEvent(new MouseEvent('dblclick'));
    row('src').querySelector<HTMLElement>('.pd-ex-twisty')!.click();
    await settle(fixture);
    expect(store.expanded()).toHaveLength(1);
    row('src').querySelector<HTMLElement>('.pd-ex-twisty')!.click();
    expect(store.expanded()).toHaveLength(0);
    const data = new Map<string, string>();
    const drag = new Event('dragstart') as DragEvent;
    Object.defineProperty(drag, 'dataTransfer', { value: { setData: (t: string, d: string) => data.set(t, d), effectAllowed: '' } });
    row('a.rs').dispatchEvent(drag);
    row('src').dispatchEvent(new Event('dragstart'));
    expect(data.get('text/uri-list')).toBe('file:///a.rs');
    expect(row('a.rs').getAttribute('draggable')).toBe('true');
    expect(row('src').getAttribute('draggable')).toBeNull();
    element.querySelector<HTMLElement>('[data-testid="explorer-more"]')!.click();
    await settle(fixture);
    expect(tree.last('').request.cursor).toBe('n2');
    row('a.rs').click();
    row('a.rs').focus();
    row('a.rs').dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', bubbles: true }));
    row('a.rs').dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true }));
    await settle(fixture);
    const menu = document.querySelector('[data-testid="explorer-menu"]')!;
    expect([...menu.querySelectorAll('[role="menuitem"]')].map((m) => m.textContent?.trim())).toEqual(['Copy path', 'Delete', 'Locked']);
    expect(menu.querySelectorAll('[role="separator"]')).toHaveLength(1);
    menu.querySelector<HTMLElement>('[role="menuitem"]')!.click();
    await settle(fixture);
    expect(log).toEqual(['open:a.rs', 'copy:a.rs']);
    row('src').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
    await settle(fixture);
    document.querySelectorAll<HTMLElement>('[data-testid="explorer-menu"] [role="menuitem"]')[2]?.click();
    await settle(fixture);
    expect(log).toEqual(['open:a.rs', 'copy:a.rs']);
  });

  it('shows the empty state with its command, errors with Retry, refresh and section toggle', async () => {
    const tree = new Tree();
    const { store, fixture, element } = await shown(tree, []);
    expect(element.querySelector('[data-testid="explorer-empty"] p')?.textContent).toBe('Open a folder to start');
    element.querySelector<HTMLElement>('[data-testid="explorer-empty"] button')!.click();
    await settle(fixture);
    expect(log).toEqual(['cmd']);
    element.querySelector<HTMLElement>('[data-explorer-section="wd.files"] .pd-ex-icon-btn')!.click();
    await settle(fixture);
    expect(element.querySelector('[data-testid="explorer-loading"]')).not.toBeNull();
    tree.last('').reject(new Error('broken'));
    await settle(fixture);
    expect(element.querySelector('[data-testid="explorer-error"]')?.textContent).toContain('broken');
    element.querySelector<HTMLElement>('[data-testid="explorer-error"] button')!.click();
    await settle(fixture);
    expect(tree.calls).toHaveLength(3);
    element.querySelector<HTMLElement>('[data-explorer-section="wd.outline"] .pd-ex-title')!.click();
    await settle(fixture);
    expect(store.isSectionCollapsed('wd.outline')).toBe(true);
    expect(element.querySelector('[data-explorer-section="wd.outline"] [role="tree"]')).toBeNull();
  });

  it('says so when nothing is contributed', async () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: provideShellTesting({ providers: [ExplorerStore] }) });
    const { element } = await render(PdExplorer);
    expect(element.textContent).toContain('Nothing to explore.');
  });
});

describe('PdExplorer changes', () => {
  it('reloads a section when the value it depends on changes', async () => {
    const { signal } = await import('@angular/core');
    const selected = signal('a');
    let asked = 0;
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: provideShellTesting<{ sel: () => string }>({
        context: () => Promise.resolve(() => ({ sel: () => selected() })),
        providers: [
          ExplorerStore,
          provideWorkbenchContributions<{ sel: () => string }>({
            explorerProviders: [
              defineExplorerProvider<{ sel: () => string }>({
                id: 'wd.outline',
                title: msg('wd.outline', 'Outline'),
                order: 1,
                changes: (ctx) => ctx.sel(),
                roots: (ctx) => {
                  asked++;
                  return Promise.resolve({ nodes: [file(`sym-of-${ctx.sel()}`)] });
                },
              }),
            ],
          }),
        ],
      }),
    });
    await TestBed.inject(ShellContextHost).load();
    const { fixture, element } = await render(PdExplorer);
    await settle(fixture);
    expect(element.querySelector('[data-node-id="sym-of-a"]')).not.toBeNull();
    selected.set('b');
    await settle(fixture);
    await settle(fixture);
    expect(element.querySelector('[data-node-id="sym-of-b"]')).not.toBeNull();
    expect(asked).toBe(2);
  });
});

describe('PdExplorer reveal', () => {
  it('expands to the active item and selects it, again when it changes, and stops at a missing node', async () => {
    const { signal } = await import('@angular/core');
    const active = signal<readonly string[] | null>(['src', 'src/lib', 'src/lib/a.rs']);
    const tree: Record<string, ExplorerNode[]> = {
      '': [folder('src'), file('README.md')],
      src: [folder('src/lib'), file('src/main.rs')],
      'src/lib': [file('src/lib/a.rs')],
    };
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: provideShellTesting<{ active: () => readonly string[] | null }>({
        context: () => Promise.resolve(() => ({ active: () => active() })),
        providers: [
          ExplorerStore,
          provideWorkbenchContributions<{ active: () => readonly string[] | null }>({
            explorerProviders: [
              defineExplorerProvider<{ active: () => readonly string[] | null }>({
                id: 'wd.files',
                title: msg('wd.files', 'Files'),
                order: 1,
                reveal: (ctx) => ctx.active(),
                roots: () => Promise.resolve({ nodes: tree['']! }),
                children: (node) => Promise.resolve({ nodes: tree[node.id] ?? [] }),
              }),
            ],
          }),
        ],
      }),
    });
    await TestBed.inject(ShellContextHost).load();
    const store = TestBed.inject(ExplorerStore);
    const { fixture } = await render(PdExplorer);
    for (let i = 0; i < 4; i++) await settle(fixture);
    expect(store.selected()).toBe(treeKey('wd.files', 'src/lib/a.rs'));
    expect(store.expanded()).toEqual([treeKey('wd.files', 'src'), treeKey('wd.files', 'src/lib')]);
    active.set(['src', 'src/main.rs']);
    for (let i = 0; i < 3; i++) await settle(fixture);
    expect(store.selected()).toBe(treeKey('wd.files', 'src/main.rs'));
    expect(await store.reveal('wd.files', ['nope'])).toBeNull();
    expect(await store.reveal('wd.files', ['README.md', 'x'])).toBeNull();
    expect(await store.reveal('wd.files', [])).toBeNull();
  });
});
