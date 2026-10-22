import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, SlotFit } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';

/** How long moving a slider pauses before the engine is asked again. */
export const FIT_DELAY_MS = 150;

/** The context sizes the slider offers (tokens per slot), up to the model's trained maximum. */
export function contextSteps(contextMax: number | null): number[] {
  const all = [2048, 4096, 8192, 16384, 32768, 65536, 131072, 262144];
  const cap = contextMax ?? 131072;
  const list = all.filter((c) => c <= cap);
  return list.length ? list : [Math.max(512, cap)];
}

/** The step nearest to `context` (the slider position for a model's current context). */
export function nearestStep(steps: number[], context: number): number {
  let best = 0;
  steps.forEach((s, i) => {
    if (Math.abs(s - context) < Math.abs(steps[best]! - context)) best = i;
  });
  return best;
}

interface SlotsState {
  /** The model being looked at. */
  modelId: string | null;
  fit: SlotFit | null;
  loading: boolean;
  error: CommandError | null;
}

const initial: SlotsState = { modelId: null, fit: null, loading: false, error: null };

/** The slots fit table of one model: asks the engine how much context or concurrency fits, latest question wins. */
export const SlotsStore = signalStore(
  withState(initial),
  withMethods((store, backend = inject(BACKEND)) => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    let seq = 0;

    async function ask(modelId: string, context: number | null, slots: number | null): Promise<void> {
      const mine = ++seq;
      patchState(store, { modelId, loading: true, error: null });
      try {
        const fit = await backend.modelFit(modelId, context, slots);
        if (mine === seq) patchState(store, { fit, loading: false });
      } catch (e) {
        if (mine === seq) patchState(store, { loading: false, error: toCommandError(e) });
      }
    }

    return {
      /** Asks now (opening the panel, retry). */
      query: ask,

      /** Asks after the slider stops moving. */
      schedule(modelId: string, context: number | null, slots: number | null): void {
        if (timer) clearTimeout(timer);
        seq++;
        timer = setTimeout(() => {
          timer = null;
          void ask(modelId, context, slots);
        }, FIT_DELAY_MS);
      },

      /** Forgets the answer (the panel closed). */
      reset(): void {
        if (timer) clearTimeout(timer);
        timer = null;
        seq++;
        patchState(store, initial);
      },
    };
  }),
);

export type SlotsStore = InstanceType<typeof SlotsStore>;
