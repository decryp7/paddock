import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, RecorderSnapshot, RecorderSnapshotInfo } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { peakIndex } from '../features/recorder/recorder-options';
import { Notifier } from '../shared/notifier';

interface RecorderState {
  snapshots: RecorderSnapshotInfo[];
  listLoaded: boolean;
  listError: CommandError | null;
  selectedId: number | null;
  snapshot: RecorderSnapshot | null;
  snapshotError: CommandError | null;
  /** The sample the user picked (the busiest one when a snapshot opens). */
  moment: number;
  capturing: boolean;
}

/** Saved flight-recorder snapshots (newest first), the one being looked at and the moment picked on its timeline. */
export const RecorderStore = signalStore(
  { providedIn: 'root' },
  withState<RecorderState>({ snapshots: [], listLoaded: false, listError: null, selectedId: null, snapshot: null, snapshotError: null, moment: 0, capturing: false }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier)) => {
    let token = 0;
    const open = async (id: number): Promise<void> => {
      const mine = ++token;
      patchState(store, { selectedId: id, snapshot: null, snapshotError: null });
      try {
        const snapshot = await backend.recorderSnapshot(id);
        if (mine === token) patchState(store, { snapshot, moment: peakIndex(snapshot.samples) });
      } catch (e) {
        if (mine === token) patchState(store, { snapshotError: toCommandError(e) });
      }
    };
    const load = async (): Promise<void> => {
      patchState(store, { listError: null });
      try {
        const snapshots = [...(await backend.recorderSnapshots())].sort((a, b) => b.at - a.at);
        patchState(store, { snapshots, listLoaded: true });
        const keep = snapshots.some((s) => s.id === store.selectedId());
        if (!keep && snapshots[0]) await open(snapshots[0].id);
        else if (!keep) patchState(store, { selectedId: null, snapshot: null });
      } catch (e) {
        patchState(store, { listError: toCommandError(e) });
      }
    };
    return {
      load,
      /** Opens a snapshot (a stale answer for an earlier choice is dropped). */
      select: open,
      /** Retries whichever request failed. */
      async retry(): Promise<void> {
        const id = store.selectedId();
        if (store.listLoaded() && id !== null) await open(id);
        else await load();
      },
      /** Saves the last ten minutes now and opens the new snapshot. */
      async capture(): Promise<void> {
        if (store.capturing()) return;
        patchState(store, { capturing: true });
        try {
          const info = await backend.capture();
          patchState(store, (s) => ({ snapshots: [info, ...s.snapshots.filter((x) => x.id !== info.id)], listLoaded: true, listError: null }));
          notifier.show('Saved the last ten minutes');
          await open(info.id);
        } catch (e) {
          notifier.error(toCommandError(e));
        } finally {
          patchState(store, { capturing: false });
        }
      },
      /** Picks a moment, clamped to the snapshot. */
      setMoment(index: number): void {
        const n = store.snapshot()?.samples.length ?? 0;
        if (n > 0 && Number.isFinite(index)) patchState(store, { moment: Math.min(n - 1, Math.max(0, Math.round(index))) });
      },
      /** Moves the picked moment by whole seconds (arrow keys). */
      stepMoment(delta: number): void {
        const n = store.snapshot()?.samples.length ?? 0;
        if (n > 0) patchState(store, { moment: Math.min(n - 1, Math.max(0, store.moment() + delta)) });
      },
      jumpToPeak(): void {
        const s = store.snapshot();
        if (s) patchState(store, { moment: peakIndex(s.samples) });
      },
    };
  }),
);
