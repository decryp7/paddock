import { DOCUMENT } from '@angular/common';
import { DestroyRef, Injectable, computed, inject, signal } from '@angular/core';
import { readPaddockTokens, type PaddockChartTokens } from '@decryp7/paddock-ui';

/**
 * The colours charts draw with, read from the CSS tokens of the page. The shell's appearance store sets the theme,
 * accent, density and contrast attributes on `<html>`; this re-reads the tokens whenever one of them (or the system's
 * colour scheme) changes, so a chart never keeps the old scheme.
 */
@Injectable({ providedIn: 'root' })
export class ChartTokens {
  private readonly doc = inject(DOCUMENT);
  private readonly revision = signal(0);

  /** Chart colours for the scheme in effect. */
  readonly tokens = computed<PaddockChartTokens>(() => {
    this.revision();
    return readPaddockTokens(this.doc.documentElement);
  });

  constructor() {
    const bump = (): void => this.revision.update((n) => n + 1);
    const observer = typeof MutationObserver === 'function' ? new MutationObserver(bump) : null;
    observer?.observe(this.doc.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-accent', 'data-contrast', 'data-density', 'class'] });
    const media = this.doc.defaultView?.matchMedia?.('(prefers-color-scheme: dark)') ?? null;
    media?.addEventListener?.('change', bump);
    inject(DestroyRef).onDestroy(() => {
      observer?.disconnect();
      media?.removeEventListener?.('change', bump);
    });
  }
}
