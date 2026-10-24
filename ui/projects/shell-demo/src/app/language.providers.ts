import { LANGUAGE_CONFIG, LocalStorageLanguageStorage, PSEUDO_LOCALE } from '@decryp7/paddock-ui/shell/i18n';

/**
 * What the Language page needs in the Settings window's providers (loaded with that window, never in the main window's first paint): the locales the demo
 * ships and how a window restarts. The choice is kept in storage, so the main window runs in it at its next start.
 */
export const LANGUAGE_PROVIDERS = [
  {
    provide: LANGUAGE_CONFIG,
    useValue: {
      storage: new LocalStorageLanguageStorage(),
      systemLanguages: () => navigator.languages,
      shipped: [PSEUDO_LOCALE],
      reload: () => location.reload(),
    },
  },
];
