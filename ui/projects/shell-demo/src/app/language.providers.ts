import { LANGUAGE_CONFIG, LocalStorageLanguageStorage, PSEUDO_LOCALE } from '@decryp7/paddock-ui/shell/i18n';
import { tauriIpc } from './demo-ipc';

/**
 * Starts the app again so every window runs in the new language: the desktop app asks the shell (`app_relaunch`: the
 * session is written first, then the whole app restarts); the browser build, and a relaunch that failed, reload this page.
 */
export function relaunchApp(): void {
  const ipc = tauriIpc();
  if (!ipc) return location.reload();
  ipc.invoke('app_relaunch').catch(() => location.reload());
}

/**
 * What the Language page needs in the Settings window's providers (loaded with that window, never in the main window's
 * first paint): the locales the demo ships and how the app restarts. The choice is kept in storage, shared by the windows.
 */
export const LANGUAGE_PROVIDERS = [
  {
    provide: LANGUAGE_CONFIG,
    useValue: {
      storage: new LocalStorageLanguageStorage(),
      systemLanguages: () => navigator.languages,
      shipped: [PSEUDO_LOCALE],
      reload: relaunchApp,
    },
  },
];
