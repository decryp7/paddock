import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { BackupReport, CommandError } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { heroText } from '../features/backup/backup-model';

/** How often the page asks again while a check runs, and how long it waits before giving up (about 90 s). */
export const BACKUP_POLL_MS = 1500;
export const BACKUP_POLL_LIMIT = 60;

interface BackupState {
  report: BackupReport | null;
  error: CommandError | null;
  /** The person asked for a check and the engine is running it. */
  checking: boolean;
  /** Why the last "Check now" failed (the earlier report stays on screen). */
  checkError: CommandError | null;
}

/** Backup health: the last report and a check on request. The backup settings live in the Settings page. */
export const BackupStore = signalStore(
  { providedIn: 'root' },
  withState<BackupState>({ report: null, error: null, checking: false, checkError: null }),
  withComputed(({ report, checking }) => ({
    statuses: computed(() => report()?.statuses ?? []),
    /** A check is running: one the person asked for, or one the engine started on its own. */
    busy: computed(() => checking() || (report()?.checking ?? false)),
    hero: computed(() => heroText(report()?.statuses ?? [])),
  })),
  withMethods((store, backend = inject(BACKEND)) => {
    /** Shows a report and follows the check it reports until the engine says it is done. */
    async function settle(first: BackupReport): Promise<void> {
      let report = first;
      patchState(store, { report, error: null });
      for (let n = 0; report.checking; n++) {
        if (n >= BACKUP_POLL_LIMIT) throw { code: 'timeout', message: 'The backup check is taking longer than expected. Try again in a moment.' } satisfies CommandError;
        await new Promise<void>((resolve) => setTimeout(resolve, BACKUP_POLL_MS));
        report = await backend.backup();
        patchState(store, { report, error: null });
      }
    }
    return {
    /** Reads the last report; while the engine is checking it asks again every 1.5 s, and gives up with an error (and Retry) after about 90 s. */
    async load(): Promise<void> {
      try {
        await settle(await backend.backup());
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
    },
    /** Checks the tools now; the engine answers at once and works in the background, so the page polls until it is done. */
    async check(): Promise<void> {
      if (store.checking()) return;
      patchState(store, { checking: true, checkError: null });
      try {
        await settle(await backend.refreshBackup());
      } catch (e) {
        patchState(store, { checkError: toCommandError(e) });
      } finally {
        patchState(store, { checking: false });
      }
    },
    };
  }),
);
