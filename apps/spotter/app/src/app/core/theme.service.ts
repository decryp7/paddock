import { DOCUMENT } from '@angular/common';
import { Injectable, computed, inject, signal } from '@angular/core';
import { applyPaddockAppearance, readPaddockTokens, type PaddockChartTokens } from '@decryp7/paddock-ui';
import type { Theme } from './api.types';
import { readLocal, writeLocal } from './storage';

const KEY = 'spotter.theme';

/** The scheme actually in effect. */
export type Scheme = 'light' | 'dark';

function isTheme(v: string | null): v is Theme {
  return v === 'system' || v === 'light' || v === 'dark';
}

/**
 * The theme: follows the system unless the user picked one. The choice is cached in localStorage so the first paint
 * is right without waiting for the backend; the settings store then confirms it from the saved settings.
 */
@Injectable({ providedIn: 'root' })
export class ThemeService {
  private readonly doc = inject(DOCUMENT);
  private readonly media = this.doc.defaultView?.matchMedia?.('(prefers-color-scheme: dark)') ?? null;
  private readonly systemDark = signal(this.media?.matches ?? false);
  private readonly pref = signal<Theme>(((v) => (isTheme(v) ? v : 'system'))(readLocal(KEY)));
  private readonly revision = signal(0);

  /** What the user picked. */
  readonly preference = this.pref.asReadonly();
  /** The scheme in effect. */
  readonly scheme = computed<Scheme>(() => {
    const p = this.pref();
    return p === 'system' ? (this.systemDark() ? 'dark' : 'light') : p;
  });
  /** Chart colours for the scheme in effect, re-read from the CSS tokens whenever it changes. */
  readonly chartTokens = computed<PaddockChartTokens>(() => {
    this.scheme();
    this.revision();
    return readPaddockTokens(this.doc.documentElement);
  });

  constructor() {
    this.media?.addEventListener?.('change', (e) => this.systemDark.set(e.matches));
    this.apply(this.pref());
  }

  setPreference(p: Theme): void {
    if (p === this.pref()) return;
    this.apply(p);
    this.pref.set(p);
    writeLocal(KEY, p);
  }

  private apply(p: Theme): void {
    applyPaddockAppearance(this.doc.documentElement, { theme: p, accent: 'spotter', density: 'standard' });
    // The attributes change the computed colours after this call; chart tokens are re-read on the next read.
    this.revision.update((n) => n + 1);
  }
}
