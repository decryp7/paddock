import type { Provider } from '@angular/core';
import { contributionProviders } from '@decryp7/paddock-ui/shell/contributions';
import { provideDebriefKinds } from '../extensions/contributions';
import { BUILT_IN_THEMES } from './themes';
import type { ThemeDef } from './theme.types';

/*
 * Theme packs as contributions (workbench.md §4.3 M1): Debrief's own kind (`debriefThemes`) in the shell's registry. The
 * shell's `defineTheme` overrides `--pd-*` colour tokens per scheme only; a Debrief pack also sets type, spacing and
 * chrome sizes, its own token names, row and lane sizes per density, and traits (`data-*` switches the timeline draws
 * by), so it is registered as itself until the shell's packs can hold that (the M1 report records the proposal).
 * `ThemeService` resolves the chosen pack from the registry; Settings ▸ Appearance lists them.
 */

/** Registers theme packs (a plugin's, or a test's); the first registration of an id wins. */
export function provideThemes(themes: readonly ThemeDef[]): Provider[] {
  return [provideDebriefKinds(), ...contributionProviders('debriefThemes', themes)];
}

/** Debrief's built-in packs, in menu order. In every window's providers (main, pop-outs, dialog windows). */
export function provideDebriefThemes(): Provider[] {
  return provideThemes(BUILT_IN_THEMES);
}
