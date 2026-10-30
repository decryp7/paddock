import { DestroyRef, inject, Injectable } from '@angular/core';

/**
 * What a settings page says when its window is asked to close (by Close, Escape, Cmd/Ctrl+W or the OS):
 * - `close`: nothing waits;
 * - `wait`: an edit is still being saved (a typed value waiting for its pause); the window stays until `done` settles and
 *   then asks again, so the edit reaches the engine before the window goes;
 * - `warn`: an edit could not be saved; the window says `message` (which edit, and why) and closes only when the user says so.
 */
export type SettingsCloseVerdict =
  | { readonly kind: 'close' }
  | { readonly kind: 'wait'; readonly done: Promise<unknown> }
  | { readonly kind: 'warn'; readonly message: string };

/** One page's say on closing: asked synchronously, each time the window is asked to close. */
export type SettingsCloseGuard = () => SettingsCloseVerdict;

const CLOSE: SettingsCloseVerdict = { kind: 'close' };

/**
 * The pages' say on closing the Settings window (pages that save by themselves, outside the window's own drafts). Provided per
 * Settings window by `settingsWindowProviders`; a page uses it through `settingsPageCloseGuard`.
 */
@Injectable()
export class SettingsCloseGuards {
  private guards: readonly SettingsCloseGuard[] = [];

  /** Asks `guard` before the window closes while `until` lives (the page's `DestroyRef`); returns a function that stops early. */
  add(guard: SettingsCloseGuard, until: DestroyRef): () => void {
    let active = true;
    const remove = (): void => {
      if (!active) return;
      active = false;
      this.guards = this.guards.filter((g) => g !== guard);
    };
    this.guards = [...this.guards, guard];
    until.onDestroy(remove);
    return remove;
  }

  /** The first verdict that is not `close` (a `wait` before a `warn`: what is still saving may yet succeed), or `close`. */
  check(): SettingsCloseVerdict {
    let warn: SettingsCloseVerdict | null = null;
    for (const guard of this.guards) {
      let verdict: SettingsCloseVerdict;
      try {
        verdict = guard();
      } catch (e) {
        // A page's bug never keeps the window open.
        console.error('[paddock-settings] a page’s close guard failed', e);
        continue;
      }
      if (verdict.kind === 'wait') return verdict;
      if (verdict.kind === 'warn') warn ??= verdict;
    }
    return warn ?? CLOSE;
  }
}

/**
 * In a settings page's injection context: asks `guard` each time the Settings window is asked to close, while the page is
 * shown. A page that saves typed values after a pause returns `wait` with its flush, so a close never loses the edit, and
 * `warn` for an edit the engine refused, so a close says what was not saved. Outside a Settings window it does nothing.
 */
export function settingsPageCloseGuard(guard: SettingsCloseGuard): () => void {
  const guards = inject(SettingsCloseGuards, { optional: true });
  return guards ? guards.add(guard, inject(DestroyRef)) : () => undefined;
}
