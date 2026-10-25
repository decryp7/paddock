import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CleanupCategory, CleanupOutcome, CleanupScan, CommandError, DeleteMode, QuarantineEntry } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { defaultPicks, initialSelection } from '../features/cleanup/cleanup-model';
import { formatBytes, plural } from '../shared/format';
import { Notifier } from '../shared/notifier';

/** A finished run, kept on the page so it can be undone. */
export interface RunResult {
  outcome: CleanupOutcome;
  mode: DeleteMode;
}

interface CleanupState {
  categories: CleanupCategory[] | null;
  categoriesError: CommandError | null;
  /** Category ids to scan. */
  picked: string[];
  scan: CleanupScan | null;
  scanning: boolean;
  scanError: CommandError | null;
  /** Item ids the person wants cleaned. */
  selection: string[];
  running: boolean;
  result: RunResult | null;
  quarantines: QuarantineEntry[] | null;
  quarantineError: CommandError | null;
  restoring: string | null;
}

/**
 * Cleanup: categories, a scan (read-only), the person's selection, a run (only from the confirm dialog) and the
 * quarantine. Every rule (what may be offered, the one-hour scan, the re-check) lives in the engine; this store holds
 * what the page shows and the person's choices.
 */
export const CleanupStore = signalStore(
  { providedIn: 'root' },
  withState<CleanupState>({
    categories: null,
    categoriesError: null,
    picked: [],
    scan: null,
    scanning: false,
    scanError: null,
    selection: [],
    running: false,
    result: null,
    quarantines: null,
    quarantineError: null,
    restoring: null,
  }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier)) => {
    const loadQuarantines = async (): Promise<void> => {
      try {
        patchState(store, { quarantines: await backend.quarantine(), quarantineError: null });
      } catch (e) {
        patchState(store, { quarantineError: toCommandError(e) });
      }
    };
    return {
      loadQuarantines,
      async loadCategories(): Promise<void> {
        try {
          const categories = await backend.cleanupCategories();
          patchState(store, { categories, categoriesError: null, picked: store.picked().length ? store.picked() : defaultPicks(categories) });
        } catch (e) {
          patchState(store, { categoriesError: toCommandError(e) });
        }
      },
      togglePick(id: string): void {
        const now = store.picked();
        patchState(store, { picked: now.includes(id) ? now.filter((x) => x !== id) : [...now, id] });
      },
      /** Reads the chosen categories. Deletes nothing. */
      async runScan(): Promise<void> {
        if (store.scanning() || store.picked().length === 0) return;
        patchState(store, { scanning: true, scanError: null, result: null });
        try {
          const scan = await backend.scanCleanup(store.picked());
          patchState(store, { scan, selection: initialSelection(scan), scanning: false });
        } catch (e) {
          patchState(store, { scanning: false, scanError: toCommandError(e) });
        }
      },
      toggleItem(id: string): void {
        const now = store.selection();
        patchState(store, { selection: now.includes(id) ? now.filter((x) => x !== id) : [...now, id] });
      },
      /** Selects or clears every listed item of one category. */
      setCategory(category: string, on: boolean): void {
        const ids = store.scan()?.results.find((r) => r.category === category)?.items.map((i) => i.id) ?? [];
        const rest = store.selection().filter((x) => !ids.includes(x));
        patchState(store, { selection: on ? [...rest, ...ids] : rest });
      },
      clearSelection(): void {
        patchState(store, { selection: [] });
      },
      /** Moves the selected items; called only after the person confirmed in the dialog. */
      async run(mode: DeleteMode): Promise<void> {
        const scan = store.scan();
        if (!scan || store.running() || store.selection().length === 0) return;
        patchState(store, { running: true });
        try {
          const outcome = await backend.runCleanup(scan.id, store.selection(), mode);
          patchState(store, { running: false, result: { outcome, mode }, scan: null, selection: [] });
          notifier.show(
            outcome.deleted ? `Moved ${plural(outcome.deleted, 'item')} (${formatBytes(outcome.freedBytes)})` : 'Nothing was moved',
            outcome.failed.length ? 'warn' : 'ok',
          );
          await loadQuarantines();
        } catch (e) {
          patchState(store, { running: false });
          notifier.error(toCommandError(e));
        }
      },
      dismissResult(): void {
        patchState(store, { result: null });
      },
      /** Puts a quarantine back where it came from. */
      async restore(id: string): Promise<void> {
        if (store.restoring()) return;
        patchState(store, { restoring: id });
        try {
          await backend.restoreQuarantine(id);
          const r = store.result();
          patchState(store, { restoring: null, result: r?.outcome.quarantineId === id ? null : r });
          notifier.show('Put back where it was');
        } catch (e) {
          patchState(store, { restoring: null });
          notifier.error(toCommandError(e));
        }
        await loadQuarantines();
      },
    };
  }),
);
