import { DestroyRef, inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, DedupePreview, EditOutcome, IntelEdit, IntelStatus, NetSwitches } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { Notifier } from '../shared/notifier';

/** How often the page asks again while a subscription is updating. */
export const UPDATE_POLL_MS = 1500;
/** The longest it keeps asking for one update. */
export const UPDATE_POLL_MAX_MS = 60_000;

interface IntelState {
  status: IntelStatus | null;
  error: CommandError | null;
  /** The last edit's refusal, shown beside the form that made it. */
  editError: CommandError | null;
  /** The "Remove duplicates" preview waiting for the user's choice. */
  preview: { id: string; preview: DedupePreview } | null;
  /** Subscriptions being updated now. */
  updating: string[];
}

/** The user's IP lists, the allow list and the network switches. */
export const IntelStore = signalStore(
  { providedIn: 'root' },
  withState<IntelState>({ status: null, error: null, editError: null, preview: null, updating: [] }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier), destroyRef = inject(DestroyRef, { optional: true })) => {
    const timers = new Set<ReturnType<typeof setTimeout>>();
    destroyRef?.onDestroy(() => timers.forEach((t) => clearTimeout(t)));
    const load = async (): Promise<void> => {
      try {
        patchState(store, { status: await backend.intelStatus(), error: null });
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
    };
    const edit = async (e: IntelEdit): Promise<EditOutcome | null> => {
      try {
        const outcome = await backend.intelEdit(e);
        patchState(store, { editError: null });
        await load();
        return outcome;
      } catch (err) {
        patchState(store, { editError: toCommandError(err) });
        return null;
      }
    };
    const watch = (id: string, since: number | null): void => {
      const started = Date.now();
      const tick = async (): Promise<void> => {
        await load();
        const row = store.status()?.lists.find((l) => l.id === id);
        const changed = row !== undefined && row.counts.lastAttemptMs !== since;
        if (changed || row === undefined || Date.now() - started > UPDATE_POLL_MAX_MS) {
          patchState(store, (s) => ({ updating: s.updating.filter((x) => x !== id) }));
          return;
        }
        const t = setTimeout(() => {
          timers.delete(t);
          void tick();
        }, UPDATE_POLL_MS);
        timers.add(t);
      };
      void tick();
    };
    return {
      load,
      edit,
      dismissEditError(): void {
        patchState(store, { editError: null });
      },
      /** Asks what "Remove duplicates" would do; nothing changes until `applyDedupe`. */
      async previewDedupe(id: string): Promise<void> {
        const out = await edit({ dedupePreview: { id } });
        if (out !== null && typeof out === 'object' && 'preview' in out) patchState(store, { preview: { id, preview: out.preview } });
      },
      cancelPreview(): void {
        patchState(store, { preview: null });
      },
      async applyDedupe(id: string): Promise<void> {
        if ((await edit({ dedupeApply: { id } })) !== null) {
          patchState(store, { preview: null });
          notifier.show('Duplicates removed.', 'ok');
        }
      },
      /** Starts the update of a subscription as a job and follows it on the page. */
      async updateNow(id: string): Promise<void> {
        const since = store.status()?.lists.find((l) => l.id === id)?.counts.lastAttemptMs ?? null;
        patchState(store, (s) => ({ updating: [...s.updating, id] }));
        try {
          await backend.startJob({ updateList: { id } });
          watch(id, since);
        } catch (e) {
          patchState(store, (s) => ({ updating: s.updating.filter((x) => x !== id) }));
          notifier.show(toCommandError(e).message, 'bad');
        }
      },
      async setSwitches(switches: NetSwitches): Promise<void> {
        await edit({ setSwitches: { switches } });
      },
    };
  }),
);
