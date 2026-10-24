import { makeEnvironmentProviders, type EnvironmentProviders } from '@angular/core';
import { provideAppearance } from '@decryp7/paddock-ui/shell';
import { ACCENT } from './names';

/** The appearance of a Pitcrew window: its store over the Appearance settings, with Pitcrew's accent. */
export function providePitcrewAppearance(): EnvironmentProviders {
  return makeEnvironmentProviders([provideAppearance({ accent: ACCENT })]);
}
