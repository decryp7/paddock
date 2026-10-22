import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CleanupReport, DiskReply } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { StatusStore } from '../../state/status.store';
import { idle, pending, settle, type Loadable } from '../../state/loadable';

interface DiskState {
  runner: string | null;
  disk: Loadable<DiskReply>;
  /** The report of a cleanup run from this page (newer than the disk reply's last cleanup). */
  report: CleanupReport | null;
}

/** One runner's disk: scan, dry-run preview, last cleanup and policy; and "Clean now". */
export const DiskStore = signalStore(
  withState<DiskState>({ runner: null, disk: idle<DiskReply>(), report: null }),
  withMethods((store, backend = inject(BACKEND), status = inject(StatusStore)) => {
    const load = async (runner: string) => {
      const same = store.runner() === runner;
      patchState(store, { runner, disk: pending(same ? store.disk() : undefined), report: same ? store.report() : null });
      const disk = await settle(same ? store.disk() : undefined, () => backend.disk(runner));
      if (store.runner() === runner) patchState(store, { disk });
    };
    return {
      load,
      async clean(): Promise<void> {
        const runner = store.runner();
        if (!runner) return;
        const report = await status.clean(runner);
        if (report) patchState(store, { report });
        await load(runner);
      },
    };
  }),
);
