import { LocalStorageLanguageStorage, type LanguageStorage } from './language-storage';
import { language, PSEUDO_LOCALE, resolveLocale, setActiveLocale, SOURCE_LOCALE, textDirection } from './locale';

/** Message id -> translated text, in the `$localize` runtime format (`{$PLACEHOLDER}` placeholders). */
export type Translations = Record<string, string>;

/**
 * Loads a locale's messages: the app's and the shell's together, as `paddock-i18n merge` wrote them. The JSON is bundled
 * as a lazy chunk (`() => import('./locale/messages.en-XA.json').then((m) => m.default.translations)`): nothing is
 * fetched from the network.
 */
export type TranslationLoader = () => Promise<Translations>;

export interface I18nOptions {
  /** The app's loader table, by BCP 47 tag (the pseudo-locale's included). The source locale needs none. */
  readonly loaders: Readonly<Record<string, TranslationLoader>>;
  readonly storage?: LanguageStorage;
  /** The system's preferred languages (default `navigator.languages`). */
  readonly languages?: readonly string[];
  /** Installs translations (default `loadTranslations` from `@angular/localize`). */
  readonly load?: (translations: Translations) => void;
  readonly root?: HTMLElement;
  /** Where a broken translation file is reported (default `console.warn`). */
  readonly warn?: (message: string, error: unknown) => void;
}

/** The exact locale's messages, else another region's of the same language (never the pseudo-locale's). */
export function loaderFor(locale: string, loaders: Readonly<Record<string, TranslationLoader>>): TranslationLoader | undefined {
  if (loaders[locale]) return loaders[locale];
  if (locale === PSEUDO_LOCALE) return undefined;
  const key = Object.keys(loaders).find((k) => k !== PSEUDO_LOCALE && language(k) === language(locale));
  return key ? loaders[key] : undefined;
}

/** Plain-object check for a loaded translation table: every value a string (a broken chunk must not half-load). */
function checked(t: unknown): Translations {
  if (typeof t !== 'object' || t === null || Array.isArray(t)) throw new Error('the translation file is not an object of messages');
  for (const [id, text] of Object.entries(t)) if (typeof text !== 'string') throw new Error(`message ${id} is not text`);
  return t as Translations;
}

/**
 * Picks the locale from the stored choice and the system languages, loads its translations and sets `lang`/`dir` on the
 * document. Must run before any module that uses `$localize` is imported (boot the window with a dynamic `import()`
 * after it). A missing or broken translation file falls back to English instead of failing the start. Returns the
 * locale the window runs in.
 */
export async function initI18n(options: I18nOptions): Promise<string> {
  const storage = options.storage ?? new LocalStorageLanguageStorage();
  const languages = options.languages ?? (navigator.languages?.length ? navigator.languages : [navigator.language]);
  const root = options.root ?? document.documentElement;
  const warn = options.warn ?? ((m: string, e: unknown) => console.warn(m, e));
  const shipped = Object.keys(options.loaders);
  let locale = resolveLocale(storage.load(), languages, shipped);
  const loader = loaderFor(locale, options.loaders);
  if (loader) {
    try {
      const translations = checked(await loader());
      const load = options.load ?? (await import('@angular/localize')).loadTranslations;
      load(translations);
    } catch (err) {
      warn(`Could not load the ${locale} translations; using English.`, err);
      locale = SOURCE_LOCALE;
    }
  }
  setActiveLocale(locale);
  root.lang = locale;
  root.dir = textDirection(locale);
  return locale;
}
