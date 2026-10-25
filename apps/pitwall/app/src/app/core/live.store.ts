import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import type { AppPermissions, CommandError, Preset } from './api.types';
import { BACKEND, type Unsubscribe } from './backend';
import { toCommandError } from './command-error';
import { INITIAL_LIVE, applyUpdate, type LiveState } from './live-reducer';
import { Toasts } from './toasts';
import { bytes } from '../shared/format';
import { PRESET_LABEL } from '../shared/labels';

interface LiveStoreState extends LiveState {
  loading: boolean;
  error: CommandError | null;
  /** Actions in flight, by key (`serving`, `release`, `job:<id>`, `model:<id>`, `pairing:<id>`, `preset`). */
  busy: Record<string, true>;
}

const initial: LiveStoreState = { ...INITIAL_LIVE, loading: false, error: null, busy: {} };

/**
 * The live picture of the server shared by the dashboard, the tray popover and the shell: the overview, folded with
 * every pushed update (one subscription for the whole app, no polling), plus the quick actions.
 */
export const LiveStore = signalStore(
  { providedIn: 'root' },
  withState(initial),
  withComputed(({ overview, busy }) => ({
    memory: computed(() => overview()?.memory ?? null),
    models: computed(() => overview()?.models ?? []),
    queue: computed(() => overview()?.queue ?? null),
    throughput: computed(() => overview()?.throughput ?? null),
    pairings: computed(() => overview()?.pairings ?? []),
    issues: computed(() => overview()?.issues ?? []),
    /** The automatic llama.cpp install (idle when nothing is being installed). */
    engineSetup: computed(() => overview()?.engineSetup ?? null),
    server: computed(() => overview()?.server ?? null),
    apps: computed(() => overview()?.apps ?? []),
    paused: computed(() => overview()?.server.serving !== 'serving' && overview() !== null),
    /** Pitwall was quit with "stop serving" and has not been opened since. */
    stopped: computed(() => overview()?.server.serving === 'stopped'),
    isBusy: computed(() => (key: string) => key in busy()),
  })),
  withMethods((store, backend = inject(BACKEND), toasts = inject(Toasts)) => {
    let unsubscribe: Unsubscribe | null = null;

    const setBusy = (key: string, on: boolean) => {
      const next = { ...store.busy() };
      if (on) next[key] = true;
      else delete next[key];
      patchState(store, { busy: next });
    };

    /** Runs an action with a busy flag; errors become a toast (the panel keeps its last good data). */
    async function act<T>(key: string, fn: () => Promise<T>, ok?: (v: T) => string): Promise<T | undefined> {
      if (key in store.busy()) return undefined;
      setBusy(key, true);
      try {
        const v = await fn();
        if (ok) toasts.ok(ok(v));
        return v;
      } catch (e) {
        toasts.error(toCommandError(e));
        return undefined;
      } finally {
        setBusy(key, false);
      }
    }

    return {
      /** Loads the overview, recent events and downloads; a failure shows the error with Retry. */
      async load(): Promise<void> {
        patchState(store, { loading: true, error: null });
        try {
          const [overview, events, downloads] = await Promise.all([backend.overview(), backend.events(50), backend.downloads()]);
          patchState(store, { overview, events, downloads, loading: false });
        } catch (e) {
          patchState(store, { loading: false, error: toCommandError(e) });
        }
      },

      /** Starts the one live subscription (idempotent). */
      connect(): void {
        if (unsubscribe) return;
        unsubscribe = backend.subscribe((u) => patchState(store, (s) => applyUpdate(s as LiveState, u) as Partial<LiveStoreState>));
      },

      disconnect(): void {
        unsubscribe?.();
        unsubscribe = null;
      },

      setServing(paused: boolean) {
        return act('serving', () => backend.setServing(paused), () => (paused ? 'Serving paused. New requests wait until you resume.' : 'Serving resumed.'));
      },

      releaseMemory() {
        return act('release', () => backend.releaseMemory(), (freed) => (freed > 0 ? `Released ${bytes(freed)}.` : 'Nothing to release: every hot model is pinned or busy.'));
      },

      async applyPreset(preset: Preset) {
        const s = await act('preset', () => backend.applyPreset(preset), () => `${PRESET_LABEL[preset]} preset applied.`);
        const o = store.overview();
        if (s && o) patchState(store, { overview: { ...o, preset: s.memory.preset } });
      },

      cancelJob(id: string) {
        return act(`job:${id}`, () => backend.cancelJob(id), () => 'Job cancelled.');
      },

      async approvePairing(id: string, permissions: AppPermissions) {
        const done = await act(`pairing:${id}`, () => backend.approvePairing(id, permissions).then(() => true), () => 'App paired.');
        const o = store.overview();
        if (done && o) patchState(store, { overview: { ...o, pairings: o.pairings.filter((p) => p.id !== id) } });
      },

      async denyPairing(id: string) {
        const done = await act(`pairing:${id}`, () => backend.denyPairing(id).then(() => true), () => 'Pairing denied.');
        const o = store.overview();
        if (done && o) patchState(store, { overview: { ...o, pairings: o.pairings.filter((p) => p.id !== id) } });
      },

      /** Tries the automatic llama.cpp install again after it failed (offline, network error). */
      async retryEngineSetup() {
        const setup = await act('engineSetup', () => backend.retryEngineSetup());
        const o = store.overview();
        if (setup && o) patchState(store, { overview: { ...o, engineSetup: setup } });
      },

      loadModel(id: string) {
        return act(`model:${id}`, () => backend.loadModel(id));
      },

      unloadModel(id: string) {
        return act(`model:${id}`, () => backend.unloadModel(id));
      },

      async pinModel(id: string, pinned: boolean) {
        await act(`model:${id}`, () => backend.pinModel(id, pinned), () => (pinned ? 'Pinned: stays loaded until you unpin it.' : 'Unpinned: the idle rule applies again.'));
      },
    };
  }),
  withHooks({
    onDestroy(store) {
      store.disconnect();
    },
  }),
);

export type LiveStore = InstanceType<typeof LiveStore>;
