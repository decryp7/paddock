/**
 * `@decryp7/paddock-ui/shell/i18n`: internationalisation (ADR 0012 §6, plan S15). `$localize` with translations loaded
 * at run time from bundled JSON chunks (one build for every language): `initI18n` in `main.ts` before the window's
 * modules are imported, the language preference, plural and number helpers. The shell's own messages use `@@pd.` ids;
 * their XLIFF catalog and pseudo-locale ship in the package (`i18n/`), and `paddock-i18n merge` folds them into the
 * app's loader files at build time.
 */
export {
  LANGUAGE_CHOICES,
  isLocaleTag,
  SOURCE_LOCALE,
  PSEUDO_LOCALE,
  language,
  resolveLocale,
  textDirection,
  activeLocale,
  setActiveLocale,
  groupedNumber,
  pluralCategory,
  plural,
  type LanguageChoice,
  type PluralForms,
} from './src/locale';
export { LocalStorageLanguageStorage, parseLanguage, LANGUAGE_KEY, LANGUAGE_VERSION, type LanguageStorage } from './src/language-storage';
export { initI18n, loaderFor, type Translations, type TranslationLoader, type I18nOptions } from './src/boot';
export { LanguageStore, LANGUAGE_CONFIG, type LanguageConfig } from './src/language.store';
export { languageOptions, nativeName, type LanguageOption, type LanguageLabels } from './src/language-options';
