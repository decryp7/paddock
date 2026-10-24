import { makeEnvironmentProviders, type EnvironmentProviders } from '@angular/core';
import type { SuAccent } from '@decryp7/paddock-ui';
import { defineTheme, msg, provideAppearance, provideThemes } from '@decryp7/paddock-ui/shell';

/** The demo's accent: the neutral graphite of the demo apps (never an app's colour), matching its silver icon. */
export const DEMO_ACCENT: SuAccent = 'graphite';

/**
 * A sample theme pack (ADR 0012 §5): token overrides only, measured by `themeProblems` like the shell's own tokens.
 * View > Appearance > Warm paper turns it on; high contrast always wins over it.
 */
export const WARM_PAPER = defineTheme({
  id: 'sd.theme.warm-paper',
  name: msg('sd.theme.warm-paper', 'Warm paper'),
  tokens: {
    common: { '--pd-radius-card': '8px' },
    light: { '--pd-bg': '#f6f3ee', '--pd-surface-1': '#fffdf9', '--pd-surface-2': '#f1ece4', '--pd-surface-3': '#fffdf9', '--pd-text': '#1d1a16' },
    dark: { '--pd-bg': '#12100e', '--pd-surface-1': '#1a1714', '--pd-surface-2': '#211d19', '--pd-surface-3': '#27221d', '--pd-text': '#efe9e1' },
  },
});

/**
 * The appearance of a demo window: its store, whose backend comes from the Settings layer (`APPEARANCE_SOURCE`: the
 * engine's Appearance settings, followed by every window), and the demo's theme pack.
 */
export function provideDemoAppearance(): EnvironmentProviders {
  return makeEnvironmentProviders([provideAppearance({ accent: DEMO_ACCENT }), provideThemes(WARM_PAPER)]);
}
