import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { CommandError, EngineReport, PruneKind, PrunePreview, PruneResult, PruneSelection } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { idle, pending, settle, type Loadable } from '../../state/loadable';

/** The kinds in the order the form lists them. */
export const PRUNE_KINDS: readonly PruneKind[] = ['stoppedContainers', 'danglingImages', 'unusedImages', 'buildCache', 'volumes'];

/** What is selected before the user changes anything: everything except volumes, which hold data. */
export const DEFAULT_KINDS: readonly PruneKind[] = ['stoppedContainers', 'danglingImages', 'unusedImages', 'buildCache'];

interface EngineState {
  report: Loadable<EngineReport>;
  kinds: PruneKind[];
  olderThanDays: number;
  /** The preview and the selection it was computed for (a different selection makes it stale). */
  preview: { selection: PruneSelection; value: PrunePreview } | null;
  previewing: boolean;
  pruning: boolean;
  result: PruneResult | null;
  /** The failure of the last preview or prune (a `busy` code means a job is running). */
  actionError: CommandError | null;
}

function sameSelection(a: PruneSelection, b: PruneSelection): boolean {
  return a.olderThanDays === b.olderThanDays && a.kinds.length === b.kinds.length && a.kinds.every((k) => b.kinds.includes(k));
}

/** The container engine's usage, and the prune workflow: choose, preview (dry run), prune after a confirmation. */
export const EngineStore = signalStore(
  withState<EngineState>({
    report: idle<EngineReport>(),
    kinds: [...DEFAULT_KINDS],
    olderThanDays: 0,
    preview: null,
    previewing: false,
    pruning: false,
    result: null,
    actionError: null,
  }),
  withComputed((s) => {
    const selection = computed<PruneSelection>(() => ({ kinds: PRUNE_KINDS.filter((k) => s.kinds().includes(k)), olderThanDays: s.olderThanDays() }));
    const currentPreview = computed(() => {
      const p = s.preview();
      return p && sameSelection(p.selection, selection()) ? p.value : null;
    });
    return {
      selection,
      /** The preview, only while it still matches the selection. */
      currentPreview,
      /** Pruning is allowed only after a preview of exactly this selection. */
      canPrune: computed(() => currentPreview() !== null && !s.pruning() && !s.previewing() && selection().kinds.length > 0),
      canPreview: computed(() => selection().kinds.length > 0 && !s.previewing() && !s.pruning()),
      /** Whether prune controls make sense: an engine was found and reports no problem. */
      usable: computed(() => {
        const r = s.report().value;
        return !!r && r.engine !== null && r.problem === null;
      }),
    };
  }),
  withMethods((store, backend = inject(BACKEND)) => {
    const load = async (): Promise<void> => {
      patchState(store, { report: pending(store.report()) });
      patchState(store, { report: await settle(store.report(), () => backend.engineReport()) });
    };
    return {
      load,
      toggle(kind: PruneKind, on: boolean): void {
        const rest = store.kinds().filter((k) => k !== kind);
        patchState(store, { kinds: on ? [...rest, kind] : rest, actionError: null });
      },
      setOlderThanDays(days: number): void {
        const n = Number.isFinite(days) ? Math.max(0, Math.floor(days)) : 0;
        patchState(store, { olderThanDays: n, actionError: null });
      },
      /** The dry run: deletes nothing. */
      async runPreview(): Promise<void> {
        if (!store.canPreview()) return;
        const selection = store.selection();
        patchState(store, { previewing: true, actionError: null, result: null });
        try {
          const value = await backend.previewEnginePrune(selection);
          patchState(store, { preview: { selection, value }, previewing: false });
        } catch (e) {
          patchState(store, { previewing: false, preview: null, actionError: toCommandError(e) });
        }
      },
      /** Prunes the previewed selection (the caller has confirmed), then reloads the report. Returns whether it ran. */
      async prune(): Promise<boolean> {
        if (!store.canPrune()) return false;
        patchState(store, { pruning: true, actionError: null });
        try {
          const result = await backend.pruneEngine(store.selection());
          patchState(store, { pruning: false, result, preview: null });
        } catch (e) {
          patchState(store, { pruning: false, actionError: toCommandError(e) });
          return false;
        }
        await load();
        return true;
      },
      dismissError(): void {
        patchState(store, { actionError: null });
      },
    };
  }),
);
