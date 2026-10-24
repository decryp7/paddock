import { EnvironmentProviders, inject, makeEnvironmentProviders, provideEnvironmentInitializer } from '@angular/core';
import { applyPaddockAppearance, type SuAccent } from '@decryp7/paddock-ui';
import { contributionProviders, provideContributionKinds } from '../provide';
import { AppearanceBackend, BrowserAppearanceBackend } from './appearance-backend';
import { APPEARANCE_CONFIG, AppearanceStore } from './appearance.store';
import { THEMES, type ThemeDef } from './theme';

export interface AppearanceOptions {
  /** The app's accent (one per app; not a user choice). */
  readonly accent: SuAccent;
  /**
   * Builds the window's backend (in an injection context). Default: `BrowserAppearanceBackend` (localStorage and a
   * BroadcastChannel). The desktop build passes `() => new IpcAppearanceBackend(ipc)`.
   */
  readonly backend?: () => AppearanceBackend;
  /** The element the look is applied to (default `document.documentElement`). */
  readonly root?: () => HTMLElement;
}

/**
 * The appearance of one window: the store (created at bootstrap, so the choice is applied at once and follows the other
 * windows), its backend and the `themes` contribution kind. Every window of the app calls it in its own bootstrap (rule
 * 34): main windows and dialog windows alike, so a dialog follows a theme change made while it is open.
 */
export function provideAppearance(options: AppearanceOptions): EnvironmentProviders {
  const root = options.root ?? (() => document.documentElement);
  return makeEnvironmentProviders([
    provideContributionKinds(THEMES),
    { provide: AppearanceBackend, useFactory: options.backend ?? (() => new BrowserAppearanceBackend()) },
    { provide: APPEARANCE_CONFIG, useValue: Object.freeze({ accent: options.accent, root }) },
    AppearanceStore,
    provideEnvironmentInitializer(() => void inject(AppearanceStore)),
  ]);
}

/** Registers theme packs (`defineTheme`) in the window's contribution registry. */
export function provideThemes(...themes: readonly ThemeDef[]): EnvironmentProviders {
  return makeEnvironmentProviders(contributionProviders('themes', themes));
}

/**
 * Applies the stored choice before Angular starts (in `main.ts`), so the first paint already has the right scheme,
 * contrast and motion; packs follow when the store starts. Never throws.
 */
export function applyStoredAppearance(root: HTMLElement, accent: SuAccent, backend: AppearanceBackend = new BrowserAppearanceBackend({ channel: null })): void {
  const a = backend.load();
  applyPaddockAppearance(root, { theme: a.theme, accent, density: a.density, contrast: a.contrast, motion: a.motion });
}
