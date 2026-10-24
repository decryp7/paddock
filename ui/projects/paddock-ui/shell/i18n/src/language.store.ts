import { computed, inject, InjectionToken } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import { LocalStorageLanguageStorage, type LanguageStorage } from './language-storage';
import { activeLocale, resolveLocale, type LanguageChoice } from './locale';

/** The language preference's ports: storage, the system languages, the shipped locales and how the window restarts. */
export interface LanguageConfig {
  readonly storage: LanguageStorage;
  readonly systemLanguages: () => readonly string[];
  /** The locales with translations (the keys of the app's loader table). */
  readonly shipped: readonly string[];
  /**
   * Starts the window again so `initI18n` loads the new language: the desktop app relaunches keeping the session, the
   * browser build reloads the page.
   */
  readonly reload: () => void;
}

export const LANGUAGE_CONFIG = new InjectionToken<LanguageConfig>('LANGUAGE_CONFIG', {
  providedIn: 'root',
  factory: () => ({
    storage: new LocalStorageLanguageStorage(),
    systemLanguages: () => (navigator.languages?.length ? navigator.languages : [navigator.language]),
    shipped: [],
    reload: () => location.reload(),
  }),
});

/**
 * The language preference of the app. Translations are loaded once at start-up, so a new choice is saved and applies
 * after a restart: `reloadPending` says one is needed; the Settings page asks the user and calls `reload`. Configure it
 * with `LANGUAGE_CONFIG` (the shipped locales and how the app restarts).
 */
export const LanguageStore = signalStore(
  // One per window (each window is its own bootstrap, rule 34): an app-wide singleton like the appearance.
  { providedIn: 'root' },
  withState<{ choice: LanguageChoice }>(() => ({ choice: inject(LANGUAGE_CONFIG).storage.load() })),
  withComputed((store, config = inject(LANGUAGE_CONFIG)) => ({
    /** The locale this window runs in. */
    locale: computed(() => activeLocale()),
    /** The saved choice differs from the language this window runs in. */
    reloadPending: computed(() => resolveLocale(store.choice(), config.systemLanguages(), config.shipped) !== activeLocale()),
  })),
  withMethods((store, config = inject(LANGUAGE_CONFIG)) => ({
    /** Saves the choice (the next start uses it). */
    choose(choice: LanguageChoice): void {
      if (choice === store.choice()) return;
      patchState(store, { choice });
      config.storage.save(choice);
    },
    reload(): void {
      config.reload();
    },
  })),
);

export type LanguageStore = InstanceType<typeof LanguageStore>;
