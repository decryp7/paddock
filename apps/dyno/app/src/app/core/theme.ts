import { DOCUMENT, Injectable, computed, effect, inject, signal } from '@angular/core';
import type { Theme } from './api.types';
import { DARK, LIGHT, type ThemeTokens } from '../shared/tokens';

/**
 * The colour theme: the user's choice (Settings) applied as `data-theme` on <html> (none for `system`), plus whether
 * the page is dark right now (for charts, which take colours as values rather than CSS variables).
 */
@Injectable({ providedIn: 'root' })
export class ThemeService {
  private readonly doc = inject(DOCUMENT);
  readonly preference = signal<Theme>('system');
  private readonly systemDark = signal(false);
  readonly dark = computed(() => this.preference() === 'dark' || (this.preference() === 'system' && this.systemDark()));
  /** The tokens of the active theme. */
  readonly tokens = computed<ThemeTokens>(() => (this.dark() ? DARK : LIGHT));

  constructor() {
    const view = this.doc.defaultView;
    const query = view?.matchMedia?.('(prefers-color-scheme: dark)');
    if (query) {
      this.systemDark.set(query.matches);
      query.addEventListener?.('change', (e) => this.systemDark.set(e.matches));
    }
    effect(() => {
      const theme = this.preference();
      const root = this.doc.documentElement;
      if (theme === 'system') delete root.dataset['theme'];
      else root.dataset['theme'] = theme;
    });
  }

  set(theme: Theme): void {
    this.preference.set(theme);
  }
}
