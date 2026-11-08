/**
 * `@decryp7/paddock-ui/locale`: the UI language and the locale every formatter uses. Pure: no Angular, no `$localize`,
 * so a formatter, a Node script or a Playwright spec can import it without the application's runtime (the language
 * store, loader and Settings page stay in `@decryp7/paddock-ui/shell/i18n`, which re-exports these).
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
