import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { errorMessage, setError, setLoaded, setLoading, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import type { AiSnapshot, NextStep } from '../generated/pitwall';
import { PitwallBackend, type Unlisten } from './backend';
import { groupCode, pitwallMessage } from './wire';

/** What the user asked for and the engine has not answered yet. One union, never separate flags. */
export type PitwallAction =
  | { readonly kind: 'idle' }
  | { readonly kind: 'connecting' }
  | { readonly kind: 'cancelling' }
  | { readonly kind: 'switching' }
  /** The last command was refused or failed; the message is the engine's, as is. */
  | { readonly kind: 'failed'; readonly message: string };

interface PitwallState {
  /** What the engine last said; `null` until the first answer. */
  snapshot: AiSnapshot | null;
  action: PitwallAction;
}

/**
 * The needs-AI state of one window (rules 31, 32, 34): the engine's `AiSnapshot` (Rust decides; nothing is re-derived here),
 * kept current by its event, and the three things the user can ask for. Pairing is started only by `connect`, which the user
 * chose; `useBuiltIn` is the repair that does not depend on Pitwall.
 */
export const PitwallStore = signalStore(
  withState<PitwallState>({ snapshot: null, action: { kind: 'idle' } }),
  withRequestStatus(),
  withComputed(({ snapshot, action }) => ({
    /** The state's kind, or `null` before the first answer. */
    kind: computed(() => snapshot()?.state.kind ?? null),
    /** Pitwall answers: the feature can run. */
    ready: computed(() => snapshot()?.state.kind === 'ready'),
    /** The feature cannot run on Pitwall and something is to be said or done (not ready, not switched off). */
    needsAttention: computed(() => {
      const s = snapshot();
      return s !== null && s.state.kind !== 'ready' && s.state.kind !== 'disabled';
    }),
    message: computed(() => snapshot()?.message ?? ''),
    actions: computed<readonly NextStep[]>(() => snapshot()?.actions ?? []),
    /** The waiting pairing's code as Pitwall shows it ("482 913"). */
    pairingCode: computed(() => {
      const p = snapshot()?.pairing;
      return p ? groupCode(p.code) : null;
    }),
    app: computed(() => snapshot()?.app ?? ''),
    /** The repair that does not depend on Pitwall is offered: the app has an engine and the session is not on it yet. */
    canUseBuiltIn: computed(() => {
      const s = snapshot();
      return !!s && s.builtInAvailable && !s.builtInForSession;
    }),
    onBuiltIn: computed(() => snapshot()?.builtInForSession === true),
    busy: computed(() => ['connecting', 'cancelling', 'switching'].includes(action().kind)),
    actionError: computed(() => {
      const a = action();
      return a.kind === 'failed' ? a.message : null;
    }),
  })),
  withMethods((store, backend = inject(PitwallBackend)) => {
    const run = async (kind: 'connecting' | 'cancelling' | 'switching', call: () => Promise<AiSnapshot | boolean>, fallback: string): Promise<void> => {
      patchState(store, { action: { kind } });
      try {
        const answer = await call();
        patchState(store, typeof answer === 'boolean' ? { action: { kind: 'idle' } } : { snapshot: answer, action: { kind: 'idle' } });
      } catch (e) {
        patchState(store, { action: { kind: 'failed', message: pitwallMessage(e, fallback) } });
      }
    };
    return {
      /** Asks the engine for its state (also the Retry of a failed first load and "Try again"). */
      async load(): Promise<void> {
        // A refresh keeps the last answer on screen; only the first one shows as loading.
        if (!store.snapshot()) patchState(store, setLoading());
        try {
          patchState(store, { snapshot: await backend.snapshot() }, setLoaded());
        } catch (e) {
          patchState(store, setError(pitwallMessage(e, $localize`:@@pd.pitwall.no-state:Pitwall's state could not be read.`)));
        }
      },
      /** The user chose to connect: the engine starts the pairing and the code appears in the snapshot. */
      connect: () => run('connecting', () => backend.connect(), $localize`:@@pd.pitwall.connect-failed:Pitwall could not be reached to connect.`),
      /** Stops the waiting pairing. */
      cancelPairing: () => run('cancelling', () => backend.cancelPairing(), $localize`:@@pd.pitwall.cancel-failed:The pairing could not be cancelled.`),
      /** Switches this session to the app's own engine. */
      useBuiltIn: () => run('switching', () => backend.useBuiltIn(), $localize`:@@pd.pitwall.builtin-failed:The built-in engine could not be switched on.`),
      /** The engine announced a new snapshot. */
      _announced(snapshot: AiSnapshot): void {
        patchState(store, { snapshot }, setLoaded());
      },
      /** The user saw the failure. */
      dismissError(): void {
        patchState(store, { action: { kind: 'idle' } });
      },
    };
  }),
  withHooks((store, backend = inject(PitwallBackend)) => {
    let off: Unlisten | undefined;
    let destroyed = false;
    return {
      onInit() {
        // Listen first, then read: a change between the two is not lost.
        void backend
          .listen((s) => store._announced(s))
          .then((u) => (destroyed ? u() : (off = u)))
          .catch((e: unknown) => console.warn(`[paddock-pitwall] cannot listen for changes: ${errorMessage(e, 'unknown error')}`));
        void store.load();
      },
      onDestroy() {
        destroyed = true;
        off?.();
      },
    };
  }),
);

export type PitwallStoreType = InstanceType<typeof PitwallStore>;
