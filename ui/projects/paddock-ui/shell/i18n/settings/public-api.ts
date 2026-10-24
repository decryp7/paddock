/**
 * `@decryp7/paddock-ui/shell/i18n/settings`: the registration of the Settings > Language page (`defineLanguageSettingsPage`),
 * light enough for a main window's registry. It is an entry point of its own because it needs `@decryp7/paddock-ui/shell`,
 * which `shell/i18n` (used by `shell/format`, which the shell's root uses) must not import. The page's component is
 * `@decryp7/paddock-ui/shell/i18n/page`.
 */
export { defineLanguageSettingsPage, LANGUAGE_SETTINGS_PAGE, type LanguageSettingsPageOptions } from './src/language.contributions';
