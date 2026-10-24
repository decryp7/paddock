import type { EnvironmentProviders } from '@angular/core';
import { makeEnvironmentProviders } from '@angular/core';
import type { SuAccent } from '@decryp7/paddock-ui';
import { defineTheme, IpcAppearanceBackend, msg, provideAppearance, provideThemes } from '@decryp7/paddock-ui/shell';
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

/**
 * The appearance of a demo window (main and dialog windows alike): in the desktop build the windows hear each other's
 * changes through `paddock://appearance`; in the browser through a BroadcastChannel.
 */
export function provideDemoAppearance(): EnvironmentProviders {
  const ipc = appearanceIpc();
  return makeEnvironmentProviders([
    provideAppearance({ accent: DEMO_ACCENT, ...(ipc ? { backend: () => new IpcAppearanceBackend(ipc) } : {}) }),
    provideThemes(WARM_PAPER),
  ]);
}
