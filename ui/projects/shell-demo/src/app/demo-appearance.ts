import { makeEnvironmentProviders, type EnvironmentProviders } from '@angular/core';
import type { SuAccent } from '@decryp7/paddock-ui';
import { BrowserAppearanceBackend, defineTheme, IpcAppearanceBackend, msg, provideAppearance, provideThemes, type AppearanceBackend } from '@decryp7/paddock-ui/shell';
import { appearanceIpc } from './demo-ipc';

/** The demo's accent (it is Pitwall's reference app first, plan §4.1). */
export const DEMO_ACCENT: SuAccent = 'pitwall';

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

/** The window's own copy of the look and its channel: `paddock://appearance` in the desktop build, a BroadcastChannel in the browser. */
export function demoAppearanceLocal(): AppearanceBackend {
  const ipc = appearanceIpc();
  return ipc ? new IpcAppearanceBackend(ipc) : new BrowserAppearanceBackend();
}

/**
 * The appearance of a demo window. A dialog window follows the others through the channel; the main window passes a
 * backend that also keeps theme and density in the Appearance settings (plan S10).
 */
export function provideDemoAppearance(backend: () => AppearanceBackend = demoAppearanceLocal): EnvironmentProviders {
  return makeEnvironmentProviders([provideAppearance({ accent: DEMO_ACCENT, backend }), provideThemes(WARM_PAPER)]);
}
