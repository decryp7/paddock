import { DOCUMENT } from '@angular/common';
import { Injectable, computed, inject } from '@angular/core';
import { AppearanceStore } from '@decryp7/paddock-ui/shell';
import { DARK, LIGHT, type ThemeTokens } from '../shared/tokens';

/**
 * Whether the page is dark right now, for the charts, which take colours as values rather than CSS variables. The choice
 * itself (theme, accent, density, contrast, motion) is the shell's Appearance setting: nothing here sets it.
 */
@Injectable({ providedIn: 'root' })
export class ThemeService {
  // Where no window provides the appearance (a unit test, a stray component) the page follows the system.
  private readonly appearance = inject(AppearanceStore, { optional: true });
  private readonly system = inject(DOCUMENT).defaultView?.matchMedia?.('(prefers-color-scheme: light)');
  readonly dark = computed(() => (this.appearance ? this.appearance.resolved().scheme === 'dark' : !(this.system?.matches ?? false)));
  /** The tokens of the active theme. */
  readonly tokens = computed<ThemeTokens>(() => (this.dark() ? DARK : LIGHT));
}
