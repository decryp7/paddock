import { language, PSEUDO_LOCALE, SOURCE_LOCALE, type LanguageChoice } from './locale';

/** One row of the language picker. */
export interface LanguageOption {
  readonly value: LanguageChoice;
  /** The language's own name ("Deutsch"), or the generic label of `system`, `en` and `pseudo`. */
  readonly label: string;
}

/** Words the picker needs from the page (already translated). */
export interface LanguageLabels {
  readonly system: string;
  readonly english: string;
  readonly pseudo: string;
}

/** The name of `tag` in its own language ("Deutsch"); the tag itself where the platform does not know it. */
export function nativeName(tag: string, names: (locale: string) => Intl.DisplayNames | null = displayNames): string {
  try {
    const own = names(tag)?.of(tag);
    return own && own !== tag ? own.charAt(0).toLocaleUpperCase(tag) + own.slice(1) : tag;
  } catch {
    return tag;
  }
}

function displayNames(locale: string): Intl.DisplayNames | null {
  try {
    return new Intl.DisplayNames([locale], { type: 'language' });
  } catch {
    return null;
  }
}

/**
 * The picker's rows for the locales an app ships: "System default", English, one row per real translation (its own name,
 * in tag order) and the pseudo-locale only where the app ships it. Pure.
 */
export function languageOptions(shipped: readonly string[], labels: LanguageLabels, names: (locale: string) => Intl.DisplayNames | null = displayNames): LanguageOption[] {
  const real = [...new Set(shipped)].filter((l) => l !== PSEUDO_LOCALE && language(l) !== language(SOURCE_LOCALE)).sort();
  return [
    { value: 'system', label: labels.system },
    { value: 'en', label: labels.english },
    ...real.map((tag) => ({ value: tag, label: nativeName(tag, names) })),
    ...(shipped.includes(PSEUDO_LOCALE) ? [{ value: 'pseudo', label: labels.pseudo }] : []),
  ];
}
