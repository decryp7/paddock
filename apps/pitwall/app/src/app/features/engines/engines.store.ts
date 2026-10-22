import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { CatalogBuild, CommandError, Engine, InstalledBuild } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Toasts } from '../../core/toasts';

interface EnginesState {
  engines: Engine[];
  loading: boolean;
  loaded: boolean;
  error: CommandError | null;
  /** Actions in flight, by key (`install:<id>`, `remove:<id>:<tag>`, `select`, `rollback`). */
  busy: Record<string, true>;
}

const initial: EnginesState = { engines: [], loading: false, loaded: false, error: null, busy: {} };

/** The Engines screen: llama.cpp builds (installed, catalog, choice, rollback), processes and planned engines. */
export const EnginesStore = signalStore(
  withState(initial),
  withComputed(({ engines, busy }) => ({
    /** The engines that can serve today (llama.cpp first). */
    available: computed(() => engines().filter((e) => e.availability.state === 'available' || e.builds.length > 0)),
    /** Planned or unsupported engines, shown with their honest reason. */
    planned: computed(() => engines().filter((e) => e.availability.state !== 'available' && e.builds.length === 0)),
    isBusy: computed(() => (key: string) => key in busy()),
  })),
  withMethods((store, backend = inject(BACKEND), toasts = inject(Toasts)) => {
    const setBusy = (key: string, on: boolean) => {
      const next = { ...store.busy() };
      if (on) next[key] = true;
      else delete next[key];
      patchState(store, { busy: next });
    };

    async function load(): Promise<void> {
      patchState(store, { loading: true, error: null });
      try {
        const engines = await backend.engines();
        patchState(store, { engines, loading: false, loaded: true });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    }

    /** Runs an action with a busy flag, then reloads; a failure becomes a toast and the last good data stays. */
    async function act(key: string, fn: () => Promise<unknown>, ok: string): Promise<boolean> {
      if (key in store.busy()) return false;
      setBusy(key, true);
      try {
        await fn();
        toasts.ok(ok);
        await load();
        return true;
      } catch (e) {
        toasts.error(toCommandError(e));
        return false;
      } finally {
        setBusy(key, false);
      }
    }

    return {
      load,

      /** Downloads, verifies and self-tests a catalog build; progress arrives as live download updates. */
      install(build: CatalogBuild): Promise<boolean> {
        return act(`install:${build.id}`, () => backend.installBuild(build.id), `Installing llama.cpp ${build.label} ${build.tag}.`);
      },

      remove(build: InstalledBuild): Promise<boolean> {
        return act(`remove:${build.id}:${build.tag}`, () => backend.removeBuild(build.id, build.tag), `Removed ${build.label} ${build.tag}.`);
      },

      /** `auto` or an installed build id. */
      select(choice: string): Promise<boolean> {
        return act('select', () => backend.selectBuild(choice), choice === 'auto' ? 'Pitwall picks the build automatically.' : 'Build selected. Models use it from their next load.');
      },

      rollback(): Promise<boolean> {
        return act('rollback', () => backend.rollbackBuild(), 'Rolled back to the previous build.');
      },
    };
  }),
);

export type EnginesStore = InstanceType<typeof EnginesStore>;
