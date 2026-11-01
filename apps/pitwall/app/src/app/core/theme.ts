import { DOCUMENT, Injectable, computed, inject, signal } from '@angular/core';
import { AppearanceStore } from '@decryp7/paddock-ui/shell';
import { THEME_TOKENS, type ResolvedTheme, type ThemeTokens } from '../shared/theme-tokens';

/**
 * The scheme the window shows, for the charts (ECharts takes colours, not CSS variables). The look itself is the shell's
 * (Settings ▸ Appearance, `AppearanceStore`, which sets `data-theme` on <html> in every window); this only follows it. Where
 * no appearance store is provided (a spec), it follows the OS preference.
 */
@Injectable({ providedIn: 'root' })
export class ThemeService {
  private readonly appearance = inject(AppearanceStore, { optional: true });
  private readonly media = inject(DOCUMENT).defaultView?.matchMedia?.('(prefers-color-scheme: dark)') ?? null;
  private readonly systemDark = signal(this.media?.matches ?? false);

  readonly resolved = computed<ResolvedTheme>(() => this.appearance?.resolved().scheme ?? (this.systemDark() ? 'dark' : 'light'));
  readonly tokens = computed<ThemeTokens>(() => THEME_TOKENS[this.resolved()]);

  constructor() {
    if (!this.appearance) this.media?.addEventListener?.('change', (e) => this.systemDark.set(e.matches));
  }
}
