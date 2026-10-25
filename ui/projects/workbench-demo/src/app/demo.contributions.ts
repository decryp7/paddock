import { defineCommand, defineKeybinding, defineMenuItem, defineStatusItem, msg } from '@decryp7/paddock-ui/shell';
import {
  defineActivityItem,
  defineAssistantContext,
  defineExplorerProvider,
  definePaletteProvider,
  definePerspective,
  defineSharedState,
  defineView,
  type ExplorerNode,
  type PaletteItem,
  type WorkbenchBatch,
} from '@decryp7/paddock-ui/workbench';
import { namesPaletteProvider } from '@decryp7/paddock-ui/workbench/palette';
import type { WorkbenchDemoContext } from './context';

// The demo's contributions: static data and loaders only (plan §9); every view is a chunk of its own.

type Ctx = WorkbenchDemoContext;

/** An Explorer node of the demo's repository (the path is its id). */
interface FileNode extends ExplorerNode {
  readonly folder: boolean;
}

const VIEW = msg('wd.category.view', 'View');
/** The shared glyph of a symbol's kind (palette rows and the Outline); none for a kind it does not know. */
const SYMBOL_GLYPHS: Readonly<Record<string, string | undefined>> = { fn: 'pd:symbol-function', struct: 'pd:symbol-type', mod: 'pd:symbol-module' };

export const DEMO_VIEWS = [
  defineView<Ctx>({
    id: 'wd.orientation',
    title: msg('wd.view.orientation', 'Welcome'),
    icon: 'pd:home',
    layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 800, height: 600 }, priority: 90 },
    order: 100,
    load: () => import('./views/orientation-view').then((m) => m.OrientationView),
  }),
  defineView<Ctx>({
    id: 'wd.code',
    title: msg('wd.view.code', 'Code'),
    icon: 'pd:file',
    instances: true,
    layout: { area: 'document', min: { width: 480, height: 240 }, pref: { width: 900, height: 600 }, priority: 100 },
    order: 200,
    load: () => import('./views/code-view').then((m) => m.CodeView),
  }),
  defineView<Ctx>({
    id: 'wd.problems',
    title: msg('wd.view.problems', 'Problems'),
    icon: 'pd:alert',
    category: msg('wd.category.analysis', 'Analysis'),
    // It folds to one column below 360 px, so its true minimum is small (ADR 0011: declare what the view needs).
    layout: { area: 'analysis', min: { width: 240, height: 160 }, pref: { width: 800, height: 300 }, priority: 55 },
    order: 400,
    load: () => import('./views/problems-view').then((m) => m.ProblemsView),
  }),
];

const files = defineExplorerProvider<Ctx, FileNode>({
  id: 'wd.files',
  title: msg('wd.explorer.files', 'Files'),
  icon: 'pd:folder',
  order: 100,
  roots: (ctx, r) => Promise.resolve(page(ctx, '', r.cursor, r.limit)),
  children: (node, ctx, r) => Promise.resolve(page(ctx, node.id, r.cursor, r.limit)),
  open: (node, ctx) => (node.folder ? undefined : void ctx.openFile(node.id)),
  actions: (node) => [
    ...(node.folder ? [] : [{ id: 'open', label: 'Open', group: 'open' }]),
    { id: 'copy', label: 'Copy path', icon: 'pd:copy', group: 'path' },
  ],
  run: (action, node, ctx) => {
    if (action === 'open') void ctx.openFile(node.id);
    else void navigator.clipboard?.writeText(node.id).catch(() => undefined);
  },
  drag: (node) => (node.folder ? null : { type: 'text/plain', data: node.id }),
  // The Explorer follows the file the user works on (review P3-14): every folder of its path, then the file.
  reveal: (ctx) => {
    const path = ctx.selectedPath();
    if (!path) return null;
    const parts = path.split('/');
    return parts.map((_, i) => parts.slice(0, i + 1).join('/'));
  },
  empty: { text: msg('wd.explorer.files.empty', 'The repository is empty.') },
});

function page(ctx: Ctx, folder: string, cursor: string | undefined, limit: number) {
  const p = ctx.repo.children(folder, cursor, limit);
  return {
    nodes: p.items.map((e): FileNode => ({ id: e.path, label: e.name, tooltip: e.path, expandable: e.folder, folder: e.folder, icon: e.folder ? 'pd:folder' : 'pd:file' })),
    total: p.total,
    ...(p.next !== undefined ? { next: p.next } : {}),
  };
}

const outline = defineExplorerProvider<Ctx>({
  id: 'wd.symbols',
  title: msg('wd.explorer.symbols', 'Outline'),
  order: 200,
  changes: (ctx) => ctx.selectedPath(),
  roots: (ctx) => {
    const path = ctx.selectedPath();
    const symbols = path ? (ctx.repo.file(path)?.symbols ?? []) : [];
    return Promise.resolve({ nodes: symbols.map((s) => ({ id: `${path}#${s.name}`, label: s.name, description: `${s.kind} · line ${s.line}`, truncate: 'end' as const, expandable: false, ...(SYMBOL_GLYPHS[s.kind] ? { icon: SYMBOL_GLYPHS[s.kind] } : {}) })) });
  },
  open: (node, ctx) => void ctx.openFile(node.id.split('#')[0] as string),
  empty: { text: msg('wd.explorer.symbols.empty', 'No symbols in the selected file.') },
});

export const DEMO_CONTRIBUTIONS: WorkbenchBatch<Ctx> = {
  commands: [
    defineCommand<Ctx>({ id: 'wd.reindex', title: msg('wd.cmd.reindex', 'Index the repository'), category: msg('wd.category.repo', 'Repository'), run: (ctx) => ctx.reindex() }),
    defineCommand<Ctx>({ id: 'wd.open-readme', title: msg('wd.cmd.open-readme', 'Open README'), category: msg('wd.category.repo', 'Repository'), run: (ctx) => void ctx.openFile('README.md') }),
    defineCommand<Ctx>({ id: 'wd.show-problems', title: msg('wd.cmd.problems', 'Show problems'), category: VIEW, run: (ctx) => ctx.showPage('wd.problems') }),
  ],
  keybindings: [defineKeybinding({ command: 'wd.reindex', key: 'Mod+Shift+I' })],
  menus: [
    defineMenuItem({ menu: 'file', command: 'wd.open-readme', group: 'open', order: 100 }),
    defineMenuItem({ menu: 'file', command: 'wd.reindex', group: 'repo', order: 200 }),
  ],
  statusItems: [
    defineStatusItem<Ctx>({
      id: 'wd.status.index',
      title: msg('wd.status.index', 'Repository index'),
      side: 'start',
      priority: 600,
      load: () => import('./index-status').then((m) => m.IndexStatus),
    }),
  ],
  views: DEMO_VIEWS,
  explorerProviders: [files, outline],
  activityItems: [
    defineActivityItem<Ctx>({ id: 'wd.act.explorer', title: msg('wd.act.explorer', 'Explorer'), icon: 'pd:folder', order: 100, run: { explorer: 'wd.files' }, keybinding: 'Mod+B' }),
    defineActivityItem<Ctx>({ id: 'wd.act.search', title: msg('wd.act.search', 'Search'), icon: 'pd:search', order: 200, run: { command: 'pd.wb.palette.quick-open' } }),
    defineActivityItem<Ctx>({
      id: 'wd.act.problems',
      title: msg('wd.act.problems', 'Problems'),
      icon: 'pd:alert',
      order: 300,
      run: { view: 'wd.problems' },
      badge: (ctx) => ctx.repo.problems.length,
      badgeLabel: (ctx) => `${ctx.repo.problems.length} problems`,
      badgeTone: (ctx) => (ctx.repo.problems.some((p) => p.level === 'error') ? 'danger' : 'warning'),
    }),
    defineActivityItem<Ctx>({
      id: 'wd.act.layouts',
      title: msg('wd.act.layouts', 'Perspectives'),
      icon: 'pd:window',
      order: 500,
      run: {
        menu: (ctx) => [
          { id: 'understand', label: 'Understand', checked: ctx.perspective() === 'wd.understand', run: { command: 'wd.p.understand' } },
          { id: 'review', label: 'Review', checked: ctx.perspective() === 'wd.review', run: { command: 'wd.p.review' } },
        ],
      },
    }),
  ],
  paletteProviders: [
    definePaletteProvider<Ctx>({
      id: 'wd.goto-file',
      title: msg('wd.palette.files', 'Go to file'),
      placeholder: msg('wd.palette.files.ph', 'Type a file name'),
      order: 100,
      search: (q, ctx, r) => Promise.resolve(ctx.repo.findFiles(q, r.limit).map((f): PaletteItem => ({ id: f.path, label: f.path.split('/').pop() as string, detail: f.path, icon: 'pd:file' }))),
      accept: (item, ctx) => void ctx.openFile(item.id),
    }),
    // The engine's names search (paddock-names, rule 20) behind `@`; the demo's mock engine answers in its shape.
    namesPaletteProvider<Ctx>({
      id: 'wd.goto-symbol',
      title: msg('wd.palette.symbols', 'Go to symbol'),
      prefix: '@',
      placeholder: msg('wd.palette.symbols.ph', 'Type a symbol name'),
      order: 200,
      icon: (match, ctx) => SYMBOL_GLYPHS[ctx.repo.findSymbols(match.display, 50).find((s) => `${s.file.path}#${s.name}` === match.id)?.kind ?? ''],
      search: (request, ctx) =>
        Promise.resolve({
          matches: ctx.repo.findSymbols(request.query, request.limit ?? 10).map((s) => ({ id: `${s.file.path}#${s.name}`, display: s.name, how: 'parts' as const, score: 1, why: s.file.path })),
          clarify: null,
          searchable: 6,
        }),
      accept: (match, ctx) => void ctx.openFile(match.id.split('#')[0] as string),
    }),
    definePaletteProvider<Ctx>({
      id: 'wd.search-text',
      title: msg('wd.palette.text', 'Search text'),
      prefix: '#',
      placeholder: msg('wd.palette.text.ph', 'Type text to find'),
      order: 300,
      debounceMs: 120,
      search: (q, ctx, r) => Promise.resolve(ctx.repo.searchText(q, r.limit).map((m): PaletteItem => ({ id: `${m.path}:${m.line}`, label: m.text, detail: `${m.path}:${m.line}` }))),
      accept: (item, ctx) => void ctx.openFile(item.id.slice(0, item.id.lastIndexOf(':'))),
    }),
  ],
  perspectives: [
    definePerspective<Ctx>({ id: 'wd.understand', title: msg('wd.p.understand', 'Understand'), icon: 'pd:search', order: 100, views: ['wd.orientation', 'pd.assistant'], focus: 'wd.orientation' }),
    definePerspective<Ctx>({ id: 'wd.review', title: msg('wd.p.review', 'Review'), icon: 'pd:check', order: 200, views: ['wd.problems', 'wd.orientation'] }),
  ],
  sharedState: [
    defineSharedState<Ctx, string | null>({
      id: 'wd.selection',
      scope: 'full',
      read: (ctx) => ctx.selectedPath(),
      parse: (raw) => (raw === null || (typeof raw === 'string' && raw.length <= 1024) ? raw : undefined),
      apply: (value, ctx) => ctx.select(value),
    }),
  ],
  assistantContext: [
    defineAssistantContext<Ctx>({
      id: 'wd.file',
      label: msg('wd.ctx.file', 'Current file'),
      icon: 'pd:file',
      value: (ctx) => {
        const path = ctx.selectedPath();
        const file = path ? ctx.repo.file(path) : undefined;
        return file ? { text: `File ${file.path}:\n${file.text}`, detail: file.path } : null;
      },
    }),
  ],
};
