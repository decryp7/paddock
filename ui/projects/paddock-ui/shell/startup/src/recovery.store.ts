import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { errorMessage, setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import type { StartReport } from '../generated/startup';
import { StartupBackend } from './backend';
import { UI_CHECKPOINT } from './recovery';

/** What the user chose in the unclean-shutdown notice. */
export type RecoveryAction = 'restore' | 'safe-mode' | 'fresh';

interface RecoveryState {
  /** How this run started (null until Rust answered, or when it did not). */
  report: StartReport | null;
  /** The choice being carried out, if any. */
  acting: RecoveryAction | null;
}

/** The notification kind of "your last session was restored". */
export const RESTORED_NOTICE_KIND = 'pd.startup.session-restored';

const NO_ANSWER = (): string => $localize`:@@pd.startup.recovery-no-answer:The app did not answer.`;

/**
 * How this run started and the answer to the unclean-shutdown notice (plan X3): Rust detected the shutdown and kept the
 * session, this store asks for it once, offers Restore / Safe mode / Start fresh and carries the choice out. It holds
 * nothing Rust does not (rule 34: the engine is the source of truth). A failed question shows nothing, because a window
 * that cannot be asked is not in a crash. Provided per window by the frame (`PdShellFrame`); `onInit` asks.
 */
export const RecoveryStore = signalStore(
  { protectedState: true },
  withState<RecoveryState>({ report: null, acting: null }),
  withRequestStatus({
    retry: false,
    why: 'asked once at start, and a failed question shows nothing (a window that cannot be asked is not in a crash); the status also carries the notice actions',
  }),
  withComputed((store) => ({
    /** This run is in safe mode (the title bar shows it). */
    safeMode: computed(() => store.report()?.safeMode ?? false),
    /** The notice to show: the previous run ended badly and the user has not decided. */
    unclean: computed(() => store.report()?.unclean ?? null),
    /** Safe mode is being started: the window is about to go away. */
    restarting: computed(() => store.acting() === 'safe-mode'),
  })),
  withMethods((store, backend = inject(StartupBackend), ports = inject(SHELL_PORTS), checkpoint = inject(UI_CHECKPOINT, { optional: true })) => {
    /** Runs a choice; its failure is shown in the notice, which stays so the user can choose again. */
    async function act(action: RecoveryAction, run: () => Promise<void>): Promise<void> {
      if (store.acting() !== null) return;
      patchState(store, setLoading(), { acting: action });
      try {
        await run();
        patchState(store, setLoaded());
      } catch (e) {
        patchState(store, setError(errorMessage(e, NO_ANSWER())), { acting: null });
      }
    }
    const settle = (): void => patchState(store, (s) => ({ report: s.report && { ...s.report, unclean: null }, acting: null }));
    return {
      /** Asks Rust how this run started. */
      async load(): Promise<void> {
        patchState(store, setLoading());
        try {
          patchState(store, { report: await backend.recoveryStatus() }, setLoaded());
        } catch (e) {
          patchState(store, { report: null }, setError(errorMessage(e, NO_ANSWER())));
        }
      },
      /** Restore: gets the previous session from Rust and hands it to the app's `UiCheckpoint`. */
      restore(): Promise<void> {
        return act('restore', async () => {
          const plan = await backend.recoveryRestore();
          if (checkpoint) {
            const ui = plan.ui;
            await checkpoint.whenLoaded?.();
            if (ui !== null && typeof ui === 'object') await checkpoint.restore(ui);
            await checkpoint.restoreWindows?.(plan.windows);
          }
          settle();
          ports.notify({ level: 'info', kind: RESTORED_NOTICE_KIND, message: $localize`:@@pd.startup.session-restored:Your last session was restored.` });
        });
      },
      /** Start fresh: Rust forgets the previous session. */
      startFresh(): Promise<void> {
        return act('fresh', async () => {
          await backend.recoveryStartFresh();
          settle();
        });
      },
      /** Safe mode: Rust restarts the app with extensions, plugins and the last session off. */
      safeModeRestart(): Promise<void> {
        return act('safe-mode', () => backend.recoverySafeMode());
      },
    };
  }),
  withHooks({
    onInit(store) {
      void store.load();
    },
  }),
);
