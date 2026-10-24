import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withProps, withState } from '@ngrx/signals';
import { ShellContextHost, SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { errorMessage } from '@decryp7/paddock-ui/shell/store';
import { WorkbenchRegistry, type ExplorerAction, type ExplorerNode, type ExplorerProviderDef } from '@decryp7/paddock-ui/workbench';

/** Nodes asked for per page. */
export const EXPLORER_PAGE = 200;
/** The most nodes kept under one parent (more is "Show more" pages of the provider, up to this bound; rule: bounded). */
export const EXPLORER_MAX_CHILDREN = 10_000;
/** The most characters of a label or description kept (display data, bounded). */
export const EXPLORER_MAX_TEXT = 1024;

/** The children of one parent (a provider's roots have parent `''`): one status union per list (rule 30). */
export interface ChildList {
  readonly nodes: readonly ExplorerNode[];
  readonly status: 'loading' | 'loaded' | { readonly error: string };
  /** The cursor of the next page, when there is one. */
  readonly next?: string;
  readonly total?: number;
}

/** A row of the flattened tree as the component draws it. */
export type TreeRow =
  | { readonly kind: 'node'; readonly section: string; readonly node: ExplorerNode; readonly depth: number; readonly expanded: boolean; readonly key: string; readonly parentKey: string | null }
  | { readonly kind: 'more'; readonly section: string; readonly parent: string; readonly depth: number; readonly key: string; readonly remaining: number | null }
  | { readonly kind: 'status'; readonly section: string; readonly parent: string; readonly depth: number; readonly key: string; readonly status: 'loading' | { readonly error: string } };

interface ExplorerState {
  /** `<section>\u0000<parent id>` -> its children. */
  lists: Readonly<Record<string, ChildList>>;
  /** Expanded nodes: `<section>\u0000<node id>`. */
  expanded: readonly string[];
  /** Collapsed sections (ids). */
  collapsedSections: readonly string[];
  /** The selected row key. */
  selected: string | null;
}

const SEP = '\u0000';
/** The key of a node or a parent within a section. */
export const treeKey = (section: string, id: string): string => `${section}${SEP}${id}`;

function clean(node: ExplorerNode): ExplorerNode | null {
  if (typeof node !== 'object' || node === null || typeof node.id !== 'string' || !node.id || typeof node.label !== 'string') return null;
  const cut = (s: string | undefined): string | undefined => (typeof s === 'string' ? s.slice(0, EXPLORER_MAX_TEXT) : undefined);
  return { ...node, label: node.label.slice(0, EXPLORER_MAX_TEXT), description: cut(node.description), tooltip: cut(node.tooltip), expandable: node.expandable === true } as ExplorerNode;
}

/**
 * The Explorer's trees (plan W4; ADR 0013 §2): the contributed providers' sections, loaded lazily (roots when a section
 * shows, children on expand), paged ("Show more"), and cancellable (collapsing a node or reloading aborts its request;
 * a late answer is dropped). The providers only fetch display data; what a tree contains is the engine's. Provided by the
 * workbench frame.
 */
export const ExplorerStore = signalStore(
  { protectedState: true },
  withState<ExplorerState>({ lists: {}, expanded: [], collapsedSections: [], selected: null }),
  withProps(() => ({
    _registry: inject(WorkbenchRegistry),
    _host: inject(ShellContextHost),
    _shell: inject(SHELL_PORTS),
    _requests: new Map<string, AbortController>(),
  })),
  withComputed((store) => ({
    /** The sections offered now, in order. */
    sections: computed(() => store._registry.availableExplorerProviders()),
    /** Every visible row, in order (sections collapsed, nodes not expanded leave theirs out). */
    rows: computed<readonly TreeRow[]>(() => {
      const out: TreeRow[] = [];
      const expanded = new Set(store.expanded());
      const lists = store.lists();
      const walk = (section: string, parent: string, depth: number, parentKey: string | null): void => {
        const list = lists[treeKey(section, parent)];
        if (!list) return;
        for (const node of list.nodes) {
          const key = treeKey(section, node.id);
          const open = node.expandable && expanded.has(key);
          out.push({ kind: 'node', section, node, depth, expanded: open, key, parentKey });
          if (open) walk(section, node.id, depth + 1, key);
        }
        if (list.status !== 'loaded') out.push({ kind: 'status', section, parent, depth, key: `${treeKey(section, parent)}${SEP}status`, status: list.status });
        else if (list.next !== undefined)
          out.push({ kind: 'more', section, parent, depth, key: `${treeKey(section, parent)}${SEP}more`, remaining: list.total !== undefined ? Math.max(0, list.total - list.nodes.length) : null });
      };
      const collapsed = new Set(store.collapsedSections());
      for (const p of store._registry.availableExplorerProviders()) if (!collapsed.has(p.id)) walk(p.id, '', 0, null);
      return out;
    }),
  })),
  withMethods((store) => {
    const provider = (id: string): ExplorerProviderDef | undefined => store._registry.findExplorerProvider(id);
    const findNode = (section: string, id: string): ExplorerNode | undefined => {
      for (const [k, list] of Object.entries(store.lists())) if (k.startsWith(`${section}${SEP}`)) for (const n of list.nodes) if (n.id === id) return n;
      return undefined;
    };
    const setList = (key: string, list: ChildList | null): void => {
      const lists = { ...store.lists() };
      if (list) lists[key] = list;
      else delete lists[key];
      patchState(store, { lists });
    };

    /** Loads (a page of) the children of `parent` (`''`: the roots). `append` asks for the page after the loaded ones. */
    const load = async (section: string, parent: string, append = false): Promise<void> => {
      const def = provider(section);
      if (!def) return;
      const key = treeKey(section, parent);
      const before = store.lists()[key];
      if (append && (!before || before.next === undefined)) return;
      store._requests.get(key)?.abort();
      const controller = new AbortController();
      store._requests.set(key, controller);
      setList(key, { nodes: append && before ? before.nodes : [], status: 'loading', ...(append && before?.total !== undefined ? { total: before.total } : {}) });
      try {
        const ctx = await store._host.load();
        if (controller.signal.aborted) return;
        const node = parent ? findNode(section, parent) : undefined;
        if (parent && (!node || !def.children)) throw new Error($localize`:@@pd.wb.explorer.no-children:This item has no children to show.`);
        const request = { limit: EXPLORER_PAGE, signal: controller.signal, ...(append && before?.next !== undefined ? { cursor: before.next } : {}) };
        const page = parent ? await def.children!(node!, ctx, request) : await def.roots(ctx, request);
        if (controller.signal.aborted) return;
        const fresh = (Array.isArray(page?.nodes) ? page.nodes : []).map(clean).filter((n): n is ExplorerNode => n !== null);
        const seen = new Set<string>();
        const nodes = [...(append && before ? before.nodes : []), ...fresh].filter((n) => !seen.has(n.id) && !!seen.add(n.id)).slice(0, EXPLORER_MAX_CHILDREN);
        const more = typeof page.next === 'string' && nodes.length < EXPLORER_MAX_CHILDREN;
        setList(key, { nodes, status: 'loaded', ...(more ? { next: page.next } : {}), ...(typeof page.total === 'number' ? { total: page.total } : {}) });
      } catch (e) {
        if (controller.signal.aborted) return;
        setList(key, { nodes: append && before ? before.nodes : [], status: { error: errorMessage(e, $localize`:@@pd.wb.explorer.failed:This list could not be loaded.`) } });
      } finally {
        if (store._requests.get(key) === controller) store._requests.delete(key);
      }
    };

    /** Aborts the requests under a parent and forgets what is loaded and expanded below it (the parent stays as it is). */
    const forget = (section: string, id: string): void => {
      const lists = { ...store.lists() };
      const below = new Set<string>();
      const drop = (parent: string): void => {
        const key = treeKey(section, parent);
        store._requests.get(key)?.abort();
        store._requests.delete(key);
        for (const n of lists[key]?.nodes ?? []) {
          below.add(treeKey(section, n.id));
          drop(n.id);
        }
        delete lists[key];
      };
      drop(id);
      patchState(store, { lists, expanded: store.expanded().filter((k) => !below.has(k)) });
    };

    return {
      /** Loads the roots of every shown section that has none yet (the frame calls it when the side bar shows). */
      ensureRoots(): void {
        const collapsed = new Set(store.collapsedSections());
        for (const p of store.sections()) if (!collapsed.has(p.id) && !store.lists()[treeKey(p.id, '')]) void load(p.id, '');
      },
      /** Reloads a section from its roots (expanded nodes reload as they show). */
      refresh(section: string): Promise<void> {
        forget(section, '');
        return load(section, '');
      },
      toggleSection(section: string): void {
        const collapsed = store.collapsedSections().includes(section);
        patchState(store, { collapsedSections: collapsed ? store.collapsedSections().filter((s) => s !== section) : [...store.collapsedSections(), section] });
        if (collapsed && !store.lists()[treeKey(section, '')]) void load(section, '');
      },
      isSectionCollapsed(section: string): boolean {
        return store.collapsedSections().includes(section);
      },
      expand(section: string, node: ExplorerNode): void {
        if (!node.expandable) return;
        const key = treeKey(section, node.id);
        if (!store.expanded().includes(key)) patchState(store, { expanded: [...store.expanded(), key] });
        if (!store.lists()[key] || typeof store.lists()[key]?.status === 'object') void load(section, node.id);
      },
      /** Collapses a node: its pending request is aborted and its loaded children are dropped (they reload on expand). */
      collapse(section: string, node: ExplorerNode): void {
        const key = treeKey(section, node.id);
        patchState(store, { expanded: store.expanded().filter((k) => k !== key) });
        forget(section, node.id);
      },
      /** The next page of a parent's children. */
      more(section: string, parent: string): Promise<void> {
        return load(section, parent, true);
      },
      /** Tries a failed list again. */
      retry(section: string, parent: string): Promise<void> {
        return load(section, parent);
      },
      /**
       * Expands the tree along `ids` (top level first, loading each level as needed) and selects the last one. Stops where
       * a node is not (yet) in its parent's loaded pages. Resolves the selected key, or null.
       */
      async reveal(section: string, ids: readonly string[]): Promise<string | null> {
        let parent = '';
        for (const [i, id] of ids.entries()) {
          const key = treeKey(section, parent);
          if (!store.lists()[key] || store.lists()[key]?.status !== 'loaded') await load(section, parent);
          const node = store.lists()[key]?.nodes.find((n) => n.id === id);
          if (!node) return null;
          if (i === ids.length - 1) {
            patchState(store, { selected: treeKey(section, id) });
            return treeKey(section, id);
          }
          if (!node.expandable) return null;
          const nodeKey = treeKey(section, id);
          if (!store.expanded().includes(nodeKey)) patchState(store, { expanded: [...store.expanded(), nodeKey] });
          parent = id;
        }
        return null;
      },
      select(key: string | null): void {
        patchState(store, { selected: key });
      },
      /** Activates a node through its provider (Enter, double click); a failure is a notice, never thrown. */
      async open(section: string, node: ExplorerNode): Promise<void> {
        const def = provider(section);
        if (!def?.open) return;
        try {
          await def.open(node, await store._host.load());
        } catch (e) {
          store._shell.notify({ level: 'error', message: errorMessage(e, $localize`:@@pd.wb.explorer.open-failed:${node.label}:item: could not be opened.`) });
        }
      },
      /** The node's context-menu actions (a throwing provider gives none). */
      actions(section: string, node: ExplorerNode): readonly ExplorerAction[] {
        const def = provider(section);
        const ctx = store._host.context();
        if (!def?.actions || !ctx) return [];
        try {
          const list = def.actions(node, ctx);
          return Array.isArray(list) ? list.slice(0, 50) : [];
        } catch {
          return [];
        }
      },
      async run(section: string, action: string, node: ExplorerNode): Promise<void> {
        const def = provider(section);
        if (!def?.run) return;
        try {
          await def.run(action, node, await store._host.load());
        } catch (e) {
          store._shell.notify({ level: 'error', message: errorMessage(e, $localize`:@@pd.wb.explorer.action-failed:That action failed.`) });
        }
      },
      /** What a drag of the node carries, or null (not draggable, or the provider threw). */
      dragData(section: string, node: ExplorerNode): { type: string; data: string } | null {
        const def = provider(section);
        if (!def?.drag) return null;
        try {
          const d = def.drag(node);
          return d && typeof d.type === 'string' && typeof d.data === 'string' ? { type: d.type, data: d.data.slice(0, 64 * 1024) } : null;
        } catch {
          return null;
        }
      },
    };
  }),
);

export type ExplorerStoreType = InstanceType<typeof ExplorerStore>;
