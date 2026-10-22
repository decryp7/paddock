import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { CommandError, RequestContent, RequestRecord } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';

/** How many recent requests the inspector asks for. */
export const REQUEST_LIMIT = 1000;

export type StatusFilter = 'all' | 'ok' | 'errors';

export interface RequestFilter {
  /** An app id, or `all`. */
  app: string;
  status: StatusFilter;
  /** Plain text matched against endpoint and model (case-insensitive), never a pattern. */
  text: string;
}

export const NO_FILTER: RequestFilter = { app: 'all', status: 'all', text: '' };

/** The requests that pass a filter (pure). */
export function filterRequests(list: readonly RequestRecord[], f: RequestFilter): RequestRecord[] {
  const text = f.text.trim().toLowerCase();
  return list.filter((r) => {
    if (f.app !== 'all' && r.appId !== f.app) return false;
    if (f.status === 'ok' && r.status >= 400) return false;
    if (f.status === 'errors' && r.status < 400) return false;
    if (text && !r.endpoint.toLowerCase().includes(text) && !(r.modelId ?? '').toLowerCase().includes(text)) return false;
    return true;
  });
}

interface ContentState {
  id: string;
  data: RequestContent | null;
  loading: boolean;
  error: CommandError | null;
}

interface RequestsState {
  records: RequestRecord[];
  loading: boolean;
  loaded: boolean;
  error: CommandError | null;
  filter: RequestFilter;
  selectedId: string | null;
  /** Content of the selected request, only after the user asked for it. */
  content: ContentState | null;
}

const initial: RequestsState = { records: [], loading: false, loaded: false, error: null, filter: NO_FILTER, selectedId: null, content: null };

/** The request inspector: recent requests, the filter, the selected request and its content on demand. */
export const RequestsStore = signalStore(
  withState(initial),
  withComputed(({ records, filter, selectedId }) => ({
    visible: computed(() => filterRequests(records(), filter())),
    selected: computed(() => records().find((r) => r.id === selectedId()) ?? null),
    errorCount: computed(() => records().filter((r) => r.status >= 400).length),
  })),
  withMethods((store, backend = inject(BACKEND)) => ({
    async load(): Promise<void> {
      patchState(store, { loading: true, error: null });
      try {
        const records = await backend.requests(REQUEST_LIMIT);
        patchState(store, { records, loading: false, loaded: true });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    },

    setFilter(change: Partial<RequestFilter>): void {
      patchState(store, { filter: { ...store.filter(), ...change } });
    },

    /** Opens the detail of a request; content from a previously open request is dropped. */
    select(id: string | null): void {
      patchState(store, { selectedId: id, content: null });
    },

    /** Fetches the selected request's prompt and response (only on the user's click). */
    async showContent(): Promise<void> {
      const id = store.selectedId();
      if (!id) return;
      patchState(store, { content: { id, data: null, loading: true, error: null } });
      try {
        const data = await backend.requestContent(id);
        if (store.content()?.id === id) patchState(store, { content: { id, data, loading: false, error: null } });
      } catch (e) {
        if (store.content()?.id === id) patchState(store, { content: { id, data: null, loading: false, error: toCommandError(e) } });
      }
    },

    hideContent(): void {
      patchState(store, { content: null });
    },
  })),
);

export type RequestsStore = InstanceType<typeof RequestsStore>;
