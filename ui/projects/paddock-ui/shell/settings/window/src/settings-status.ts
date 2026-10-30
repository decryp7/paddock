import { computed, DestroyRef, inject, Injectable, signal } from '@angular/core';

/** One page's claim on the footer: read reactively, so a signal or a computed keeps the line current. */
type StatusSource = () => string | null;

/**
 * The Settings window's footer status line as the shown page sets it. The window says "Changes are saved as you make them."
 * by default; a page that saves differently (its own Save button, an inline editor) says so instead while it is shown, and
 * the line returns to the default when the page leaves. A notice of the change just made (the store's) still comes first.
 * Provided per Settings window by `settingsWindowProviders`; a page uses it through `settingsPageStatus`.
 */
@Injectable()
export class SettingsStatus {
  private readonly claims = signal<readonly StatusSource[]>([]);

  /** The latest claim's text that is not empty, or null (the window then shows its own line). */
  readonly text = computed(() => {
    const claims = this.claims();
    for (let i = claims.length - 1; i >= 0; i--) {
      const text = claims[i]?.();
      if (text) return text;
    }
    return null;
  });

  /**
   * Shows `source()` as the footer status while `until` lives (the page's `DestroyRef`), and returns a function that
   * clears it early. A null or empty text leaves the window's own line.
   */
  claim(source: StatusSource, until: DestroyRef): () => void {
    let active = true;
    const clear = (): void => {
      if (!active) return;
      active = false;
      this.claims.update((all) => all.filter((c) => c !== source));
    };
    this.claims.update((all) => [...all, source]);
    until.onDestroy(clear);
    return clear;
  }
}

/**
 * In a settings page's injection context (a field or the constructor): shows `text()` as the Settings window's footer
 * status while the page is shown, and clears it when the page leaves. Outside a Settings window it does nothing.
 *
 * ```ts
 * readonly status = settingsPageStatus(() => (this.dirty() ? 'Not saved yet: Save applies the changes.' : 'Save applies the changes on this page.'));
 * ```
 */
export function settingsPageStatus(text: StatusSource): () => void {
  const status = inject(SettingsStatus, { optional: true });
  return status ? status.claim(text, inject(DestroyRef)) : () => undefined;
}
