import { DestroyRef, inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, NetworkQuery, NetworkSnapshot } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { Notifier } from '../shared/notifier';

/** The page asks again this often while it is open (the engine caches reads for a moment, so this stays cheap). */
export const NETWORK_POLL_MS = 2000;

export const DEFAULT_QUERY: NetworkQuery = { showLoopback: false, proto: null, state: null, exposedOnly: false, search: '' };

interface NetworkState {
  snapshot: NetworkSnapshot | null;
  query: NetworkQuery;
  error: CommandError | null;
}

/** Open ports and connections: the filters the user chose and the latest snapshot, polled only while the page is open. */
export const NetworkStore = signalStore(
  { providedIn: 'root' },
  withState<NetworkState>({ snapshot: null, query: DEFAULT_QUERY, error: null }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier), destroyRef = inject(DestroyRef, { optional: true })) => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    let open = false;
    const stop = () => {
      open = false;
      if (timer !== null) clearTimeout(timer);
      timer = null;
    };
    const load = async (): Promise<void> => {
      try {
        patchState(store, { snapshot: await backend.network(store.query()), error: null });
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
      if (open) timer = setTimeout(() => void load(), NETWORK_POLL_MS);
    };
    destroyRef?.onDestroy(stop);
    return {
      /** Starts polling (the page calls this on open and `stop` on close). */
      async start(): Promise<void> {
        stop();
        open = true;
        await load();
      },
      stop,
      load,
      async setQuery(patch: Partial<NetworkQuery>): Promise<void> {
        patchState(store, (s) => ({ query: { ...s.query, ...patch } }));
        if (timer !== null) clearTimeout(timer);
        await load();
      },
      /** Opens the lookup page for a public address in the browser; the engine says why when it cannot. */
      async lookup(ip: string): Promise<void> {
        try {
          await backend.lookupIp(ip);
        } catch (e) {
          notifier.show(toCommandError(e).message, 'bad');
        }
      },
    };
  }),
);
