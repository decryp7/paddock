import { computed, inject, InjectionToken } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withProps, withState } from '@ngrx/signals';
import { CommandService, ShellContextHost, SHELL_PORTS, type CommandDef } from '@decryp7/paddock-ui/shell';
import { formatKey, Keymap } from '@decryp7/paddock-ui/shell/menu';
import { errorMessage, setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import { COMMANDS_PREFIX, WORKBENCH_PORTS, WorkbenchRegistry, type PaletteItem, type PaletteProviderDef } from '@decryp7/paddock-ui/workbench';
import { didYouMean, marksOf, plain, queryMarks, rank, type Rankable, type Ranked } from './match';
import { PaletteRecent } from './recent';

/** The most results a provider is asked for, and the most rows the list shows. */
export const PALETTE_LIMIT = 50;

/** One row of the list: display data only (what accepting it does is kept by the store). */
export interface PaletteRow {
  /** Unique within the list. */
  readonly key: string;
  readonly label: string;
  readonly category?: string;
  readonly detail?: string;
  readonly icon?: string;
  /** The key binding as shown (`⌘⇧P`, `Ctrl+Shift+P`). */
  readonly keybinding?: string;
  readonly marks: ReadonlySet<number>;
  /** Shown but inert with this reason. */
  readonly disabled?: string;
  /** The heading drawn before this row (the empty commands list: "Recently used", "Frequently used", "Other commands"). */
  readonly heading?: string;
  /** `detail` is a path: drawn shortened in the middle (rule 18). */
  readonly detailIsPath?: boolean;
}

/** Where the list's rows come from: the palette's own commands and views, or a contributed provider. */
export type PaletteSource = { readonly kind: 'commands' } | { readonly kind: 'provider'; readonly id: string };

/** Schedules the debounce of a provider search; replaced by tests (a plain timer, never a timeout on work: rule 12). */
export interface PaletteTimers {
  set(run: () => void, ms: number): unknown;
  clear(handle: unknown): void;
}

export const PALETTE_TIMERS = new InjectionToken<PaletteTimers>('PADDOCK_PALETTE_TIMERS', {
  providedIn: 'root',
  factory: () => ({ set: (run, ms) => setTimeout(run, ms), clear: (h) => clearTimeout(h as ReturnType<typeof setTimeout>) }),
});

/** Commands most often reached for, listed after the recent ones when the commands list opens empty (the app's choice). */
export const PALETTE_COMMON_COMMANDS = new InjectionToken<readonly string[]>('PADDOCK_PALETTE_COMMON_COMMANDS', { providedIn: 'root', factory: () => [] });

interface PaletteState {
  isOpen: boolean;
  /** What the user typed, prefix included. */
  query: string;
  source: PaletteSource;
  /** Rows of a provider's last answer (the commands list is computed). */
  providerRows: readonly PaletteRow[];
  active: number;
}

const initialState: PaletteState = { isOpen: false, query: '', source: { kind: 'commands' }, providerRows: [], active: 0 };

const VIEW_PREFIX = 'view:';

/** A command or view as the commands list ranks it. */
interface CommandEntry extends Rankable {
  readonly kind: 'command' | 'view';
  readonly target: string;
  readonly disabled?: string;
  readonly keybinding?: string;
}

/**
 * The command palette (plan W5; Debrief's `command-palette.ts`, `palette.service.ts`): opens with a prefix, picks the
 * source by the query's first character (`>` the palette's commands and views, otherwise a contributed provider by its
 * prefix, else the plain-text provider, else the commands), and lists results. Provider searches are debounced, take an
 * `AbortSignal` that is aborted on the next keystroke or on close, and a late answer of an aborted search is dropped.
 * Provided by the workbench frame (one per window).
 */
export const PaletteStore = signalStore(
  { protectedState: true },
  withState(initialState),
  withRequestStatus({ retry: false, kind: 'permanent', why: 'the status is one search for one query: the next keystroke asks again' }),
  withProps(() => ({
    _registry: inject(WorkbenchRegistry),
    _commands: inject(CommandService),
    _keymap: inject(Keymap),
    _host: inject(ShellContextHost),
    _ports: inject(WORKBENCH_PORTS),
    _shell: inject(SHELL_PORTS),
    _recent: inject(PaletteRecent),
    _timers: inject(PALETTE_TIMERS),
    _common: inject(PALETTE_COMMON_COMMANDS),
    /** Items of the provider's last answer by row key (what accepting a row passes back). */
    _items: new Map<string, PaletteItem>(),
    _pending: { controller: null as AbortController | null, timer: null as unknown, generation: 0 },
  })),
  withComputed((store) => {
    /** The palette's own entries: available commands, then one "Open <view>" per offered view. */
    const entries = computed<CommandEntry[]>(() => {
      const viewCategory = $localize`:@@pd.wb.palette.category.view:View`;
      const commands = store._commands.available().map((c: CommandDef): CommandEntry => {
        const key = store._keymap.keyFor(c.id);
        return {
          kind: 'command',
          id: c.id,
          target: c.id,
          title: c.title.text,
          ...(c.category ? { category: c.category.text } : {}),
          ...(store._commands.isEnabled(c) ? {} : { disabled: $localize`:@@pd.wb.palette.unavailable:Not available now` }),
          ...(key ? { keybinding: formatKey(key, store._keymap.platform) } : {}),
        };
      });
      const views = store._registry.availableViews().map(
        (v): CommandEntry => ({
          kind: 'view',
          id: `${VIEW_PREFIX}${v.id}`,
          target: v.id,
          title: $localize`:@@pd.wb.palette.open-view:Open ${v.title.text}:view:`,
          category: v.category?.text ?? viewCategory,
        }),
      );
      return [...commands, ...views];
    });
    /** The text searched: the query without the commands prefix. */
    const commandQuery = computed(() => {
      const q = store.query();
      return q.startsWith(COMMANDS_PREFIX) ? q.slice(COMMANDS_PREFIX.length).trimStart() : q;
    });
    const ranked = computed<Ranked<CommandEntry>[]>(() =>
      store.source().kind === 'commands' ? rank(entries(), commandQuery(), store._recent.ids(), store._common).slice(0, PALETTE_LIMIT) : [],
    );
    /** "Did you mean" when the commands list matches nothing. */
    const suggestions = computed<Ranked<CommandEntry>[]>(() =>
      store.source().kind !== 'commands' || ranked().length || !commandQuery().trim()
        ? []
        : didYouMean(entries(), commandQuery(), (e) => (e.category ? `${e.category} ${e.title}` : e.title)).map(plain),
    );
    /** The group of a row of the empty list, so it reads as recent, frequent, then the rest (review P3-1). */
    const groupOf = (id: string): string | null =>
      commandQuery().trim()
        ? null
        : store._recent.ids().includes(id)
          ? $localize`:@@pd.wb.palette.group.recent:Recently used`
          : store._common.includes(id)
            ? $localize`:@@pd.wb.palette.group.common:Frequently used`
            : $localize`:@@pd.wb.palette.group.other:Other commands`;
    const commandRows = computed<PaletteRow[]>(() => {
      let last: string | null = null;
      return (ranked().length ? ranked() : suggestions()).map((r) => {
        const group = groupOf(r.item.id);
        const heading = group !== null && group !== last ? group : undefined;
        last = group;
        return {
          key: r.item.id,
          label: r.item.title,
          ...(r.showCategory && r.item.category ? { category: r.item.category } : {}),
          ...(r.item.keybinding ? { keybinding: r.item.keybinding } : {}),
          ...(r.item.disabled ? { disabled: r.item.disabled } : {}),
          ...(heading ? { heading } : {}),
          marks: r.marks,
        };
      });
    });
    const provider = computed<PaletteProviderDef | null>(() => {
      const s = store.source();
      return s.kind === 'provider' ? (store._registry.paletteProviders.find((p) => p.id === s.id) ?? null) : null;
    });
    return {
      _entries: entries,
      /** What the user searched for, without a prefix (for "Nothing matches “…”", review P2-9). */
      searched: computed(() => {
        const s = store.source();
        if (s.kind === 'commands') return commandQuery().trim();
        const q = store.query();
        const prefix = provider()?.prefix ?? '';
        return (prefix && q.startsWith(prefix) ? q.slice(prefix.length) : q).trim();
      }),
      suggesting: computed(() => suggestions().length > 0),
      rows: computed<readonly PaletteRow[]>(() => (store.source().kind === 'commands' ? commandRows() : store.providerRows())),
      provider,
      /** The prefixes the palette answers, for the hint line: the commands' and each offered provider's; `active` marks the mode in use. */
      prefixes: computed(() => {
        const s = store.source();
        return [
          { prefix: COMMANDS_PREFIX, title: $localize`:@@pd.wb.palette.prefix.commands:Commands`, active: s.kind === 'commands' },
          ...store._registry
            .availablePaletteProviders()
            .filter((p) => p.prefix)
            .map((p) => ({ prefix: p.prefix, title: p.title.text, active: s.kind === 'provider' && s.id === p.id })),
        ];
      }),
      placeholder: computed(() => provider()?.placeholder?.text ?? $localize`:@@pd.wb.palette.placeholder:Type a command or search…`),
    };
  }),
  withMethods((store) => {
    const cancel = (): void => {
      store._pending.generation++;
      store._pending.controller?.abort();
      store._pending.controller = null;
      if (store._pending.timer !== null) store._timers.clear(store._pending.timer);
      store._pending.timer = null;
    };

    /** Asks the provider for the query; drops the answer when a newer keystroke or a close came first. */
    const search = async (provider: PaletteProviderDef, query: string, generation: number): Promise<void> => {
      const controller = new AbortController();
      store._pending.controller = controller;
      try {
        const ctx = await store._host.load();
        if (generation !== store._pending.generation) return;
        const items = await provider.search(query, ctx, { limit: PALETTE_LIMIT, signal: controller.signal });
        if (generation !== store._pending.generation || controller.signal.aborted) return;
        store._items.clear();
        const rows = (Array.isArray(items) ? items : []).slice(0, PALETTE_LIMIT).map((item, i): PaletteRow => {
          const key = `${i}:${String(item.id)}`;
          store._items.set(key, item);
          const label = String(item.label ?? '');
          return {
            key,
            label,
            ...(item.detail ? { detail: String(item.detail), ...(/[\\/]/.test(String(item.detail)) ? { detailIsPath: true } : {}) } : {}),
            ...(item.icon ? { icon: item.icon } : {}),
            ...(item.keybinding ? { keybinding: formatKey(item.keybinding, store._keymap.platform) } : {}),
            ...(item.disabled ? { disabled: item.disabled } : {}),
            marks: item.highlights ? marksOf(label, item.highlights) : queryMarks(query, label),
          };
        });
        patchState(store, { providerRows: rows, active: 0 }, setLoaded());
      } catch (e) {
        if (generation !== store._pending.generation || controller.signal.aborted) return;
        patchState(store, { providerRows: [] }, setError(errorMessage(e, $localize`:@@pd.wb.palette.failed:The search failed.`)));
      }
    };

    /** Picks the source for the query and starts a provider search (debounced when the provider asks for it). */
    const update = (query: string): void => {
      cancel();
      const first = query.charAt(0);
      const chosen = first === COMMANDS_PREFIX ? null : store._registry.paletteProviderFor(query);
      if (!chosen) {
        patchState(store, { query, source: { kind: 'commands' }, providerRows: [], active: 0 }, setLoaded());
        return;
      }
      const { provider, query: text } = chosen;
      const generation = store._pending.generation;
      patchState(store, { query, source: { kind: 'provider', id: provider.id }, active: 0 }, setLoading());
      const delay = provider.debounceMs ?? 0;
      if (delay > 0) store._pending.timer = store._timers.set(() => void search(provider, text, generation), delay);
      else void search(provider, text, generation);
    };

    const close = (): void => {
      cancel();
      patchState(store, { isOpen: false, query: '', providerRows: [], active: 0, source: { kind: 'commands' } }, { requestStatus: 'idle' as const });
      store._items.clear();
    };

    return {
      /** Opens the palette with `prefix` typed (`>` for commands; `''` for the plain-text provider). */
      open(prefix = COMMANDS_PREFIX): void {
        patchState(store, { isOpen: true });
        update(prefix);
      },
      close,
      setQuery(query: string): void {
        update(query);
      },
      /** Searches again (after an error). */
      retry(): void {
        update(store.query());
      },
      setActive(index: number): void {
        const n = store.rows().length;
        patchState(store, { active: n ? Math.max(0, Math.min(n - 1, index)) : 0 });
      },
      /** Moves the highlight by `delta`, wrapping around. */
      move(delta: number): void {
        const n = store.rows().length;
        if (!n) return;
        patchState(store, { active: (((store.active() + delta) % n) + n) % n });
      },
      /**
       * Accepts a row (the highlighted one by default): runs the command, shows the view, or hands the item back to its
       * provider. Closes the palette first; a disabled row does nothing. Resolves whether something ran.
       */
      async accept(index = store.active()): Promise<boolean> {
        const row = store.rows()[index];
        if (!row || row.disabled) return false;
        const source = store.source();
        const provider = store.provider();
        const item = store._items.get(row.key);
        const entry = source.kind === 'commands' ? store._entries().find((e) => e.id === row.key) : undefined;
        close();
        if (entry?.kind === 'view') return store._ports.showView(entry.target);
        if (entry) {
          store._recent.add(entry.id);
          return store._commands.run(entry.target);
        }
        if (!provider || !item) return false;
        try {
          await provider.accept(item, await store._host.load());
          return true;
        } catch (e) {
          store._shell.notify({ level: 'error', message: errorMessage(e, $localize`:@@pd.wb.palette.accept-failed:That result could not be opened.`) });
          return false;
        }
      },
    };
  }),
);

export type PaletteStoreType = InstanceType<typeof PaletteStore>;
