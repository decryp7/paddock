import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { CacheCleaned, CacheInfo, CacheLimits, CachePlan, CommandError } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { idle, pending, settle, type Loadable } from '../../state/loadable';

interface CacheState {
  runner: string | null;
  report: Loadable<CacheInfo>;
  /** The form: empty means no limit of that kind. */
  maxGb: number | null;
  maxAgeDays: number | null;
  preview: { limits: CacheLimits; plan: CachePlan } | null;
  previewing: boolean;
  cleaning: boolean;
  result: CacheCleaned | null;
  actionError: CommandError | null;
}

const positive = (n: number | null): n is number => n !== null && Number.isFinite(n) && n > 0;

/** One runner's Actions cache: the report, and the trim workflow (limits, preview, clean after a confirmation). */
export const CacheStore = signalStore(
  withState<CacheState>({
    runner: null,
    report: idle<CacheInfo>(),
    maxGb: null,
    maxAgeDays: null,
    preview: null,
    previewing: false,
    cleaning: false,
    result: null,
    actionError: null,
  }),
  withComputed((s) => {
    /** The limits the form describes, or null while none is set. */
    const limits = computed<CacheLimits | null>(() => {
      const gb = s.maxGb();
      const days = s.maxAgeDays();
      if (!positive(gb) && !positive(days)) return null;
      return { maxMb: positive(gb) ? Math.max(1, Math.round(gb * 1000)) : 0, maxAgeDays: positive(days) ? Math.max(1, Math.floor(days)) : 0 };
    });
    const currentPreview = computed(() => {
      const p = s.preview();
      const l = limits();
      return p && l && p.limits.maxMb === l.maxMb && p.limits.maxAgeDays === l.maxAgeDays ? p.plan : null;
    });
    return {
      limits,
      currentPreview,
      canPreview: computed(() => limits() !== null && s.runner() !== null && !s.previewing() && !s.cleaning()),
      canClean: computed(() => currentPreview() !== null && !s.previewing() && !s.cleaning()),
    };
  }),
  withMethods((store, backend = inject(BACKEND)) => {
    const reload = async (runner: string): Promise<void> => {
      patchState(store, { report: pending(store.report()) });
      const report = await settle(store.report(), () => backend.cacheReport(runner));
      if (store.runner() === runner) patchState(store, { report });
    };
    return {
      /** Shows another runner (or reloads the same one). The form is kept; the preview and result belong to a runner and go. */
      async load(runner: string): Promise<void> {
        const same = store.runner() === runner;
        patchState(store, {
          runner,
          report: same ? store.report() : idle<CacheInfo>(),
          preview: same ? store.preview() : null,
          result: same ? store.result() : null,
          actionError: null,
        });
        await reload(runner);
      },
      async retry(): Promise<void> {
        const r = store.runner();
        if (r) await reload(r);
      },
      setMaxGb(v: number | null): void {
        patchState(store, { maxGb: v, actionError: null });
      },
      setMaxAgeDays(v: number | null): void {
        patchState(store, { maxAgeDays: v, actionError: null });
      },
      async runPreview(): Promise<void> {
        const runner = store.runner();
        const limits = store.limits();
        if (!runner) return;
        if (!limits) {
          patchState(store, { actionError: { code: 'badRequest', message: 'Set a size limit, an age limit, or both.' } });
          return;
        }
        patchState(store, { previewing: true, actionError: null, result: null });
        try {
          const plan = await backend.previewCacheClean(runner, limits);
          if (store.runner() === runner) patchState(store, { preview: { limits, plan }, previewing: false });
        } catch (e) {
          patchState(store, { previewing: false, preview: null, actionError: toCommandError(e) });
        }
      },
      /** Cleans with the previewed limits (the caller has confirmed), then reloads the report. Returns whether it ran. */
      async clean(): Promise<boolean> {
        const runner = store.runner();
        const limits = store.limits();
        if (!runner || !limits || !store.canClean()) return false;
        patchState(store, { cleaning: true, actionError: null });
        try {
          const result = await backend.cleanCache(runner, limits);
          patchState(store, { cleaning: false, result, preview: null });
        } catch (e) {
          patchState(store, { cleaning: false, actionError: toCommandError(e) });
          return false;
        }
        await reload(runner);
        return true;
      },
    };
  }),
);
