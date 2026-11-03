import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { AppGroup, CommandError, Locker, OtherUsers, ProcessDetails, ProcessRef, ProcessSample } from '../core/api.types';
import { BACKEND, type Unsubscribe } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { type Row, type Sort, type ViewMode, buildRows, collapseAllKeys, expandAllKeys } from '../features/processes/process-rows';

interface ProcessesState {
  processes: ProcessSample[];
  groups: AppGroup[];
  otherUsers: OtherUsers | null;
  connected: boolean;
  mode: ViewMode;
  sort: Sort;
  query: string;
  expanded: string[];
  selected: ProcessRef | null;
  details: ProcessDetails | null;
  detailsLoading: boolean;
  detailsError: CommandError | null;
  /** The last request that failed, with the engine's message (shown inline above the table). */
  actionError: CommandError | null;
}

/** The task manager's state: the live process list (subscribed only while the screen is open) and view settings. */
export const ProcessesStore = signalStore(
  { providedIn: 'root' },
  withState<ProcessesState>({
    processes: [],
    groups: [],
    otherUsers: null,
    connected: false,
    mode: 'groups',
    sort: { key: 'cpu', dir: 'desc' },
    query: '',
    expanded: [],
    selected: null,
    details: null,
    detailsLoading: false,
    detailsError: null,
    actionError: null,
  }),
  withComputed((s) => ({
    rows: computed<Row[]>(() => buildRows({ mode: s.mode(), processes: s.processes(), groups: s.groups(), otherUsers: s.otherUsers(), sort: s.sort(), query: s.query(), expanded: new Set(s.expanded()) })),
    total: computed(() => s.processes().length),
  })),
  withMethods((store, backend = inject(BACKEND)) => {
    let stop: Unsubscribe | null = null;
    return {
      /** Starts receiving the full process list (the agent sends it only while someone listens). */
      start(): void {
        if (stop) return;
        stop = backend.onLive('processes', (u) => {
          if (u.processes) patchState(store, { processes: u.processes, groups: u.groups ?? [], otherUsers: u.otherUsers, connected: true });
        });
      },
      stop(): void {
        stop?.();
        stop = null;
      },
      setMode(mode: ViewMode): void {
        patchState(store, { mode });
      },
      setQuery(query: string): void {
        patchState(store, { query });
      },
      /** Sorts by a column; the same column again flips the direction. */
      sortBy(key: Sort['key']): void {
        const s = store.sort();
        patchState(store, { sort: s.key === key ? { key, dir: s.dir === 'asc' ? 'desc' : 'asc' } : { key, dir: key === 'name' || key === 'user' ? 'asc' : 'desc' } });
      },
      /** Sets the sort (a click on a column header of the table). */
      setSort(sort: Sort): void {
        patchState(store, { sort });
      },
      toggle(row: Row): void {
        if (!row.expandable) return;
        patchState(store, (st) => {
          const rest = st.expanded.filter((k) => k !== row.key && k !== `-${row.key}`);
          if (row.expanded) return { expanded: st.mode === 'tree' ? [...rest, `-${row.key}`] : rest };
          return { expanded: [...rest, row.key] };
        });
      },
      /** Opens every group (Apps view) or every process with children (Tree view). */
      expandAll(): void {
        patchState(store, (st) => ({ expanded: expandAllKeys(st.mode, st.processes, st.groups) }));
      },
      /** Folds everything down to the top level. */
      collapseAll(): void {
        patchState(store, (st) => ({ expanded: collapseAllKeys(st.mode, st.processes) }));
      },
      dismissError(): void {
        patchState(store, { actionError: null });
      },
      async select(ref: ProcessRef | null): Promise<void> {
        patchState(store, { selected: ref, details: null, detailsError: null, detailsLoading: ref !== null });
        if (!ref) return;
        try {
          const details = await backend.processDetails(ref);
          if (store.selected()?.pid === ref.pid) patchState(store, { details, detailsLoading: false });
        } catch (e) {
          if (store.selected()?.pid === ref.pid) patchState(store, { detailsLoading: false, detailsError: toCommandError(e) });
        }
      },
      /** Which processes hold a file or folder open; rejects with a `CommandError`. */
      lockers(path: string): Promise<Locker[]> {
        return backend.lockers(path).catch((e: unknown) => Promise.reject(toCommandError(e)));
      },
      async openLocation(ref: ProcessRef): Promise<void> {
        patchState(store, { actionError: null });
        try {
          await backend.openLocation(ref);
        } catch (e) {
          patchState(store, { actionError: toCommandError(e) });
        }
      },
    };
  }),
);
