import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { errorMessage, setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import type { SetupCommandError, SetupState } from '../generated/setup';
import { SetupBackend, type Unlisten } from './backend';
import { asSetupError, setupMessage } from './wire';

/** What the user can ask of a step. */
export type SetupAction = 'act' | 'repair' | 'skip';

/** Why the last request did not work, which step it was for and which request it was. */
export interface SetupFailure {
  readonly id: string;
  readonly action: SetupAction;
  readonly error: SetupCommandError;
}

interface SetupStoreState {
  /** What the engine last said; null until the first answer. */
  setup: SetupState | null;
  /** Steps asked to act or repair and not yet answered by an event (the commands return at once). */
  requested: readonly string[];
  failure: SetupFailure | null;
}

/**
 * The first-run checklist as one window sees it (rule 34: the engine is the single source of truth). The store holds the state
 * the engine reported, folds the events that arrive on `paddock://setup` into it, and starts a step's action or repair, which
 * return at once. Nothing times out a running step (rule 12): it shows "working" until the engine says it is over. The page
 * never depends on what it sets up (rule 19): a failed ask is an inline error with Retry, and a step with a problem always
 * has Repair.
 */
export const SetupStore = signalStore(
  withState<SetupStoreState>({ setup: null, requested: [], failure: null }),
  withRequestStatus(),
  withComputed(({ setup }) => {
    const items = computed(() => setup()?.items ?? []);
    return {
      items,
      /** The steps still to do or with a problem, that the user has not skipped. */
      open: computed(() => items().filter((i) => i.status !== 'done' && !i.skipped)),
      /** Every step is done or skipped. */
      allSettled: computed(() => items().every((i) => i.status === 'done' || i.skipped)),
      finished: computed(() => setup()?.finished === true),
      showOnStart: computed(() => setup()?.showOnStart === true),
    };
  }),
  withMethods((store, backend = inject(SetupBackend)) => {
    const load = async (): Promise<void> => {
      // A refresh keeps the state on screen; only the first ask shows as loading.
      if (!store.setup()) patchState(store, setLoading());
      try {
        const setup = await backend.state();
        patchState(store, { setup }, setLoaded());
      } catch (e) {
        patchState(store, setError(setupMessage(e, $localize`:@@pd.setup.no-state:The set-up steps could not be asked.`)));
      }
    };
    const fail = (id: string, action: SetupAction, e: unknown): void => {
      const error = asSetupError(e) ?? { code: 'failed' as const, message: setupMessage(e, $localize`:@@pd.setup.refused:The request was not accepted.`), retryable: true };
      patchState(store, { requested: store.requested().filter((r) => r !== id), failure: { id, action, error } });
    };
    const start = async (id: string, action: 'act' | 'repair'): Promise<void> => {
      if (store.requested().includes(id)) return;
      patchState(store, { requested: [...store.requested(), id], failure: null });
      try {
        await (action === 'act' ? backend.act(id) : backend.repair(id));
      } catch (e) {
        fail(id, action, e);
        // Busy means one runs already (another window): show where it is.
        if (asSetupError(e)?.code === 'busy') await load();
      }
    };
    const answer = async (id: string, action: SetupAction, ask: () => Promise<SetupState>): Promise<void> => {
      patchState(store, { failure: null });
      try {
        patchState(store, { setup: await ask() });
      } catch (e) {
        fail(id, action, e);
      }
    };
    return {
      /** The step is running here (asked, not yet answered) or in another window. */
      isWorking(id: string): boolean {
        return store.requested().includes(id) || store.items().some((i) => i.id === id && i.working);
      },
      /** Asks the engine where every step stands (also the Retry of a failed ask). */
      load(): Promise<void> {
        return load();
      },
      /** Runs the step's action; the outcome follows as events. */
      act(id: string): Promise<void> {
        return start(id, 'act');
      },
      /** Runs the step's repair; the outcome follows as events. */
      repair(id: string): Promise<void> {
        return start(id, 'repair');
      },
      /** Skips the step for now, or takes it up again. */
      skip(id: string, skipped: boolean): Promise<void> {
        return answer(id, 'skip', () => backend.skip(id, skipped));
      },
      /** Finishes the checklist: it does not open by itself again. */
      finish(): Promise<void> {
        return answer('', 'skip', () => backend.finish());
      },
      /** Runs the failed request again. */
      async retry(): Promise<void> {
        const f = store.failure();
        if (!f || !f.error.retryable) return;
        if (f.action === 'skip') await answer(f.id, 'skip', () => (f.id ? backend.skip(f.id, true) : backend.finish()));
        else await start(f.id, f.action);
      },
      /** Hides the failure. */
      dismissFailure(): void {
        patchState(store, { failure: null });
      },
      /** A change from the engine's event: a step working, or its outcome. */
      _update(setup: SetupState): void {
        // A step the engine no longer reports as working has answered whatever was asked of it.
        const still = store.requested().filter((id) => setup.items.some((i) => i.id === id && i.working));
        patchState(store, { setup, requested: still }, setLoaded());
      },
    };
  }),
  withHooks((store, backend = inject(SetupBackend)) => {
    let off: Unlisten | undefined;
    let destroyed = false;
    return {
      onInit() {
        // Listen first, then ask: an event can only follow something the state already shows.
        void backend
          .listen((s) => store._update(s))
          .then((u) => (destroyed ? u() : (off = u)))
          .catch((e: unknown) => console.warn(`[paddock-setup] cannot listen for set-up changes: ${errorMessage(e, 'unknown error')}`));
        void store.load();
      },
      onDestroy() {
        destroyed = true;
        off?.();
      },
    };
  }),
);

export type SetupStoreType = InstanceType<typeof SetupStore>;
