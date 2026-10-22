import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { BuildCapability, CatalogBuild, CommandError, Engine, EnginePinState, InstalledBuild, PackNote, PinChange } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Toasts } from '../../core/toasts';

interface EnginesState {
  engines: Engine[];
  loading: boolean;
  loaded: boolean;
  /** Pack files that were skipped, and non-fatal notes about loaded ones. */
  packProblems: PackNote[];
  packWarnings: PackNote[];
  /** Engine pins and roll back (ADR 0018); null until loaded, and when the call fails the Engines page still works. */
  pins: EnginePinState | null;
  /** What each installed build supports, by build id. */
  capabilities: Record<string, BuildCapability[]>;
  error: CommandError | null;
  /** Actions in flight, by key (`install:<id>`, `remove:<id>:<tag>`, `select`, `rollback`, `pin`). */
  busy: Record<string, true>;
}

const initial: EnginesState = { engines: [], loading: false, loaded: false, packProblems: [], packWarnings: [], pins: null, capabilities: {}, error: null, busy: {} };

/** The Engines screen: llama.cpp builds (installed, catalog, choice, rollback), processes and planned engines. */
export const EnginesStore = signalStore(
  withState(initial),
  withComputed(({ engines, busy, pins }) => ({
    /** The engines that can serve today (llama.cpp first). */
    available: computed(() => engines().filter((e) => e.availability.state === 'available' || e.builds.length > 0 || e.kind === 'pack')),
    /** Planned or unsupported engines, shown with their honest reason. */
    planned: computed(() => engines().filter((e) => e.availability.state !== 'available' && e.builds.length === 0 && e.kind !== 'pack')),
    /** The build pinned for all models. */
    pinnedGlobal: computed(() => pins()?.global ?? null),
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
        // Pack notes are secondary: a failure here must not hide the engines.
        try {
          const packs = await backend.packs();
          patchState(store, { packProblems: packs.problems, packWarnings: packs.warnings });
        } catch {
          patchState(store, { packProblems: [], packWarnings: [] });
        }
        // Pins and capabilities are secondary too: without them the page shows builds, just no pin controls.
        try {
          const [pins, caps] = await Promise.all([backend.enginePins(), backend.buildCapabilities()]);
          patchState(store, { pins, capabilities: Object.fromEntries(caps.map((c) => [c.buildId, c.capabilities])) });
        } catch {
          patchState(store, { pins: null, capabilities: {} });
        }
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    }

    /** Runs an action with a busy flag, then reloads; a failure becomes a toast and the last good data stays. */
    async function act<T>(key: string, fn: () => Promise<T>, ok: string | ((result: T) => string)): Promise<boolean> {
      if (key in store.busy()) return false;
      setBusy(key, true);
      try {
        const result = await fn();
        toasts.ok(typeof ok === 'string' ? ok : ok(result));
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
        return act(`install:${build.id}`, () => backend.installBuild(build.id), `Installing ${build.label} ${build.tag}.`);
      },

      remove(build: InstalledBuild): Promise<boolean> {
        return act(`remove:${build.id}:${build.tag}`, () => backend.removeBuild(build.id, build.tag), `Removed ${build.label} ${build.tag}.`);
      },

      /** `auto` or an installed build id. */
      select(choice: string): Promise<boolean> {
        return act('select', () => backend.selectBuild(choice), choice === 'auto' ? 'Pitwall picks the build automatically.' : 'Build selected. Models use it from their next load.');
      },

      /** Rolls back to the previous good build and pins it; the toast says which loaded models switch on their next load. */
      rollback(): Promise<boolean> {
        return act('rollback', () => backend.rollbackBuild(), (c: PinChange) => c.message);
      },

      /** Pins a build for all models, or unpins it when it is the pinned one. */
      togglePin(build: InstalledBuild): Promise<boolean> {
        const pinned = store.pins()?.global === build.id;
        return act('pin', () => backend.setGlobalPin(pinned ? null : build.id), (c: PinChange) => c.message);
      },
    };
  }),
);

export type EnginesStore = InstanceType<typeof EnginesStore>;
