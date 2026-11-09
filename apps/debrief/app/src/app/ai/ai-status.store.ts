import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { Logger } from '../core/logger';
import { AiBackend, toAiError } from './ai-backend';
import { AiStatus } from './ai.types';

interface AiStatusState {
  status: AiStatus | null;
  error: string | null;
}

/**
 * Whether the local AI assistant can be used in this run, and what Pitwall is doing for it. Small and eager:
 * every AI entry point reads `available` to hide itself (safe mode, AI off) before the
 * lazily loaded AI views are needed.
 */
export const AiStatusStore = signalStore(
  { providedIn: 'root' },
  withState<AiStatusState>({ status: null, error: null }),
  withComputed((s) => ({
    /** AI can be used in this run (it may still need Pitwall). */
    available: computed(() => !!s.status()?.available),
    /** Pitwall serves a model for Debrief. */
    hasModel: computed(() => !!s.status()?.available && !['noModel', 'unavailable'].includes(s.status()?.state ?? 'unavailable')),
    needsSetup: computed(() => s.status()?.state === 'noModel'),
  })),
  withMethods((store, backend = inject(AiBackend), logger = inject(Logger)) => ({
    async refresh(): Promise<AiStatus | null> {
      try {
        const status = await backend.status();
        patchState(store, { status, error: null });
        return status;
      } catch (e) {
        const err = toAiError(e);
        patchState(store, { status: null, error: err.message });
        logger.warn('ai', 'AI status unavailable', { Code: err.code });
        return null;
      }
    },
    setStatus(status: AiStatus): void {
      patchState(store, { status, error: null });
    },
  })),
  withHooks({
    onInit(store, backend = inject(AiBackend)) {
      void store.refresh();
      void backend.onStatus((s) => store.setStatus(s)).catch(() => undefined);
    },
  }),
);
