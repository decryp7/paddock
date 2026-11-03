import { inject, PendingTasks } from '@angular/core';

/** Runs `work` and returns its result; the app is not stable until it has ended. */
export type TrackLoad = <T>(work: () => Promise<T>) => Promise<T>;

/**
 * The one way a dialog (or its store) starts async work when it is created or opened (ADR 0026 §6): the work is a pending
 * task, so `whenStable` and the dialog window's `data-fit-settled` wait for it, and the window is sized to the content the
 * work produced. `resource()` does the same by itself; use this for a call that is not a resource. The `paddock/dialog-load-tracked`
 * ESLint rule fails an untracked `await`, `.then()` or `void call()` in a dialog content's start-up or a store's `onInit`.
 * Call it in an injection context (a field initializer, a constructor or a store's `withHooks`).
 */
export function injectTrackLoad(): TrackLoad {
  const pending = inject(PendingTasks);
  return async (work) => {
    const done = pending.add();
    try {
      return await work();
    } finally {
      done();
    }
  };
}
