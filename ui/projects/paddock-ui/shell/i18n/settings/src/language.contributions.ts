import { defineSettingsPage, msg, type SettingsPageDef } from '@decryp7/paddock-ui/shell';

/** The id of the Language page of the Settings window (a page of the shell's own component; no engine section). */
export const LANGUAGE_SETTINGS_PAGE = 'pd.language';

export interface LanguageSettingsPageOptions {
  /** The page's component, from the registry: `() => import('@decryp7/paddock-ui/shell/i18n/page').then((m) => m.PdLanguagePage)`. */
  readonly load: NonNullable<SettingsPageDef['load']>;
}

/**
 * The Language page of the Settings window: the language of the app (System default, English, each shipped translation
 * by its own name). Add it to the app's `settingsPages` and to the Settings window's pages, and provide `LANGUAGE_CONFIG`
 * (the locales the app ships, how it restarts) in the Settings window's providers: it is a root token, so a provider added
 * by the lazily loaded page would lose against its default.
 */
export function defineLanguageSettingsPage(options: LanguageSettingsPageOptions): SettingsPageDef {
  return defineSettingsPage({
    id: LANGUAGE_SETTINGS_PAGE,
    group: 'application',
    title: msg('pd.settings.language', 'Language'),
    order: 60,
    load: options.load,
  });
}
