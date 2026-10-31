import { makeEnvironmentProviders, type EnvironmentProviders } from '@angular/core';
import type { SuAccent } from '@decryp7/paddock-ui';
import { provideAppearance } from '@decryp7/paddock-ui/shell';

/** Spotter's accent (`data-accent` in index.html too). */
export const ACCENT: SuAccent = 'spotter';

/** The appearance of a window: the shell's store over the Appearance settings (theme, accent choice, density, contrast, motion). */
export function provideSpotterAppearance(): EnvironmentProviders {
  return makeEnvironmentProviders([provideAppearance({ accent: ACCENT })]);
}
