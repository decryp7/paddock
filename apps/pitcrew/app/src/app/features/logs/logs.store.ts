import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, LogLine } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';

/** Lines kept in the view (older ones drop off the top). */
export const VIEW_CAP = 5_000;
/** Lines per request. */
export const PAGE = 500;

interface LogsState {
  runner: string | null;
  lines: LogLine[];
  following: boolean;
  loading: boolean;
  error: CommandError | null;
  /** A plain-text search and its results (null when not searching). */
  query: string;
  results: LogLine[] | null;
  searching: boolean;
}

/** A runner's log tail (paged by sequence number) and plain-text search. */
export const LogsStore = signalStore(
  withState<LogsState>({ runner: null, lines: [], following: true, loading: false, error: null, query: '', results: null, searching: false }),
  withMethods((store, backend = inject(BACKEND)) => ({
    async open(runner: string): Promise<void> {
      patchState(store, { runner, lines: [], loading: true, error: null, results: null, query: '' });
      try {
        const lines = await backend.logs(runner, null, PAGE);
        if (store.runner() === runner) patchState(store, { lines, loading: false });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    },
    /** Fetches lines newer than the last one shown. */
    async poll(): Promise<number> {
      const runner = store.runner();
      if (!runner || store.loading()) return 0;
      const after = store.lines().at(-1)?.seq ?? null;
      try {
        const newer = await backend.logs(runner, after, PAGE);
        if (store.runner() !== runner || newer.length === 0) return 0;
        patchState(store, { lines: [...store.lines(), ...newer].slice(-VIEW_CAP), error: null });
        return newer.length;
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
        return 0;
      }
    },
    setFollowing(following: boolean): void {
      patchState(store, { following });
    },
    async search(query: string): Promise<void> {
      const runner = store.runner();
      const q = query.trim();
      patchState(store, { query });
      if (!runner || !q) {
        patchState(store, { results: null, searching: false });
        return;
      }
      patchState(store, { searching: true });
      try {
        const results = await backend.searchLogs(runner, q, PAGE);
        if (store.query() === query) patchState(store, { results, searching: false, error: null });
      } catch (e) {
        patchState(store, { searching: false, error: toCommandError(e) });
      }
    },
    clearSearch(): void {
      patchState(store, { query: '', results: null });
    },
  })),
);
