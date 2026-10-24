/**
 * The UI language and the locale every formatter uses. Pure: no Angular, no `$localize`, so it runs in `main.ts` before
 * the application (and any translated module) is loaded. Ported from Debrief's `i18n/locale.ts` without app names.
 */

/** What the user picked (Settings ▸ General ▸ Language): the system's language, English, or the pseudo-locale. */
export type LanguageChoice = 'system' | 'en' | 'pseudo';

export const LANGUAGE_CHOICES: readonly LanguageChoice[] = ['system', 'en', 'pseudo'];

/** The language the source strings are written in (no translations are loaded for it). */
export const SOURCE_LOCALE = 'en-US';

/** The generated pseudo-locale (accented, longer, bracketed text) used to find unmarked strings and overflow. */
export const PSEUDO_LOCALE = 'en-XA';

const RTL_LANGUAGES = new Set(['ar', 'arc', 'ckb', 'dv', 'fa', 'he', 'ks', 'ku', 'ps', 'sd', 'syr', 'ug', 'ur', 'yi']);

/** The language subtag ("en" for "en-GB"), lower-case. */
export function language(tag: string): string {
  return tag.trim().toLowerCase().split(/[-_]/)[0] ?? '';
}

/**
 * The locale to run in. "System default" uses the first preferred system language the app ships (keeping the user's
 * region, e.g. en-GB, for formatting) and otherwise English. `shipped` lists the real translations (BCP 47 tags; the
 * source locale is always shipped, the pseudo-locale never counts).
 */
export function resolveLocale(choice: LanguageChoice, preferred: readonly string[], shipped: readonly string[] = []): string {
  if (choice === 'en') return SOURCE_LOCALE;
  if (choice === 'pseudo') return PSEUDO_LOCALE;
  const all = [SOURCE_LOCALE, ...shipped.filter((l) => l !== PSEUDO_LOCALE)];
  for (const tag of preferred) {
    if (!tag || all.every((l) => language(l) !== language(tag))) continue;
    try {
      return Intl.getCanonicalLocales(tag)[0] ?? SOURCE_LOCALE;
    } catch {
      /* not a valid tag: try the next one */
    }
  }
  return SOURCE_LOCALE;
}

/** Text direction for a locale. */
export function textDirection(locale: string): 'ltr' | 'rtl' {
  return RTL_LANGUAGES.has(language(locale)) ? 'rtl' : 'ltr';
}

let active = SOURCE_LOCALE;
let numberFormat: Intl.NumberFormat | null = null;
let pluralRules: Intl.PluralRules | null = null;

/** The locale the window runs in (set once at start-up by `initI18n`, before bootstrap). */
export function activeLocale(): string {
  return active;
}

/** Sets the running locale. Called by `initI18n`; tests may call it and must restore `SOURCE_LOCALE`. */
export function setActiveLocale(locale: string): void {
  active = locale;
  numberFormat = null;
  pluralRules = null;
}

/** Integers with the active locale's grouping ("12,345" in English). */
export function groupedNumber(n: number): string {
  numberFormat ??= new Intl.NumberFormat(active === PSEUDO_LOCALE ? SOURCE_LOCALE : active);
  return numberFormat.format(n);
}

/** The plural category of `n` in the active locale ("one", "other", ...). */
export function pluralCategory(n: number): Intl.LDMLPluralRule {
  pluralRules ??= new Intl.PluralRules(active === PSEUDO_LOCALE ? SOURCE_LOCALE : active);
  return pluralRules.select(n);
}

/** Plural forms of one message; `other` is required, `=0` (if given) wins for zero. */
export type PluralForms = { other: string; '=0'?: string } & Partial<Record<Intl.LDMLPluralRule, string>>;

/**
 * Chooses the form for `n`: the TypeScript counterpart of an ICU plural in a template (`$localize` has no ICU). Each form
 * is its own `$localize` message, e.g.
 * `plural(n, { one: $localize`:@@pd.jobs.count.one:${n}:count: job`, other: $localize`:@@pd.jobs.count.other:${n}:count: jobs` })`.
 */
export function plural(n: number, forms: PluralForms): string {
  if (n === 0 && forms['=0'] !== undefined) return forms['=0'];
  return forms[pluralCategory(n)] ?? forms.other;
}
