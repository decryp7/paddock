import { DestroyRef, inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, ConnectionRow, ListenerRow, NetworkQuery, NetworkSnapshot, TrafficRow } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { Notifier } from '../shared/notifier';

/** The page asks again this often while it is open (the engine caches reads for a moment, so this stays cheap). */
export const NETWORK_POLL_MS = 2000;

export const DEFAULT_QUERY: NetworkQuery = { showLoopback: false, proto: null, state: null, exposedOnly: false, search: '', country: null };

interface NetworkState {
  snapshot: NetworkSnapshot | null;
  query: NetworkQuery;
  error: CommandError | null;
  /** Whether host names are looked up (off by default: a reverse lookup asks the user's DNS resolver about each address). */
  resolveNames: boolean;
  /** Names the resolver gave, by address (`null` = it has none). */
  names: Record<string, string | null>;
}

/** What one program has open, for the process panel. */
export interface ProcessNetwork {
  connections: ConnectionRow[];
  listeners: ListenerRow[];
  traffic: TrafficRow | null;
  note: string | null;
}

/** Most addresses asked about in one lookup (the engine's limit). */
export const RESOLVE_BATCH = 64;

/** Open ports and connections: the filters the user chose and the latest snapshot, polled only while the page is open. */
export const NetworkStore = signalStore(
  { providedIn: 'root' },
  withState<NetworkState>({ snapshot: null, query: DEFAULT_QUERY, error: null, resolveNames: false, names: {} }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier), destroyRef = inject(DestroyRef, { optional: true })) => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    let open = false;
    const stop = () => {
      open = false;
      if (timer !== null) clearTimeout(timer);
      timer = null;
    };
    /** Asks for the names of public peers not asked about yet; failures are quiet (a name is a nicety). */
    const resolve = async (addresses: string[]): Promise<void> => {
      const todo = [...new Set(addresses)].filter((a) => !(a in store.names())).slice(0, RESOLVE_BATCH);
      if (todo.length === 0) return;
      try {
        const answers = await backend.resolveHosts(todo);
        patchState(store, (s) => ({ names: { ...s.names, ...Object.fromEntries(answers.map((h) => [h.address, h.name])) } }));
      } catch {
        // Nothing to show: the address stays as it is and the next poll asks again only if the toggle is still on.
      }
    };
    const load = async (): Promise<void> => {
      try {
        const snapshot = await backend.network(store.query());
        patchState(store, { snapshot, error: null });
        if (store.resolveNames()) void resolve(snapshot.connections.filter((c) => c.lookupOk).map((c) => c.remote));
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
      /** "Try again" on the owners' source line: one read that asks the helper now (the poll keeps its own pace). */
      async retryHelper(): Promise<void> {
        try {
          patchState(store, { snapshot: await backend.network({ ...store.query(), retryHelper: true }), error: null });
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
        }
      },
      async setQuery(patch: Partial<NetworkQuery>): Promise<void> {
        patchState(store, (s) => ({ query: { ...s.query, ...patch } }));
        if (timer !== null) clearTimeout(timer);
        await load();
      },
      /** Turns host-name lookups on or off; turning on asks for the names of what is listed now. */
      async setResolveNames(on: boolean): Promise<void> {
        patchState(store, { resolveNames: on });
        const snap = store.snapshot();
        if (on && snap) await resolve(snap.connections.filter((c) => c.lookupOk).map((c) => c.remote));
      },
      /** Looks up one address on request (the detail panel and the context menu), whatever the toggle says. */
      async resolveOne(address: string): Promise<void> {
        patchState(store, (s) => {
          const { [address]: _gone, ...rest } = s.names;
          return { names: rest };
        });
        await resolve([address]);
      },
      /** What one program has open now, without touching the page's own filters. */
      async forProcess(pid: number): Promise<ProcessNetwork> {
        const snap = await backend.network({ showLoopback: true, proto: null, state: null, exposedOnly: false, search: '', country: null });
        const mine = (o: { kind: string; pid?: number }) => o.kind === 'process' && o.pid === pid;
        return {
          connections: snap.connections.filter((c) => mine(c.owner)),
          listeners: snap.listeners.filter((l) => mine(l.owner)),
          traffic: snap.traffic?.find((t) => t.pid === pid) ?? null,
          note: snap.trafficNote ?? null,
        };
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
