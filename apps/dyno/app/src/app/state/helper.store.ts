import { inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { CommandError, HelperStatus } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';

/** How often the install is checked while the system's permission prompt is open. */
export const HELPER_POLL_MS = 700;
/** The longest Dyno waits for the prompt to be answered (the agent gives up on its own as well). */
export const HELPER_WAIT_MS = 6 * 60 * 1000;

interface HelperStoreState {
  status: HelperStatus | null;
  error: CommandError | null;
  /** An install, update or removal is in progress (the prompt may be open). */
  working: boolean;
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/**
 * The privileged helper's state (ADR 0016). Installing is only ever started by `install()`, which the confirmation dialog
 * calls after the person said yes; nothing here runs on its own.
 */
export const HelperStore = signalStore(
  { providedIn: 'root' },
  withState<HelperStoreState>({ status: null, error: null, working: false }),
  withComputed((store) => ({
    /** The helper is installed, current and answering: administrator changes can be sent. */
    ready: () => store.status()?.state === 'ready',
  })),
  withMethods((store, backend = inject(BACKEND)) => {
    const load = async (): Promise<void> => {
      try {
        patchState(store, { status: await backend.helperStatus(), error: null });
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
    };
    return {
      load,
      /** Installs or updates the helper and waits for the system prompt to be answered. Resolves `true` when it is ready. */
      async install(): Promise<boolean> {
        patchState(store, { working: true, error: null });
        try {
          let status = await backend.installHelper();
          patchState(store, { status });
          const deadline = Date.now() + HELPER_WAIT_MS;
          while (status.state === 'installing' && Date.now() < deadline) {
            await sleep(HELPER_POLL_MS);
            status = await backend.helperStatus();
            patchState(store, { status });
          }
          return status.state === 'ready';
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
          return false;
        } finally {
          patchState(store, { working: false });
        }
      },
      /** Removes the helper. */
      async uninstall(): Promise<boolean> {
        patchState(store, { working: true, error: null });
        try {
          patchState(store, { status: await backend.uninstallHelper() });
          return true;
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
          return false;
        } finally {
          patchState(store, { working: false });
        }
      },
    };
  }),
);
