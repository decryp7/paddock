import { LANGUAGE_CHOICES, type LanguageChoice } from './locale';

/**
 * Where the language choice is kept. Apart from the other settings so `main.ts` can read it before bootstrap without
 * loading Angular. Implementations never throw.
 */
export interface LanguageStorage {
  load(): LanguageChoice;
  save(choice: LanguageChoice): void;
}

export const LANGUAGE_KEY = 'paddock.language';
export const LANGUAGE_VERSION = 1;

/** Validates an untrusted stored value; anything unknown is "System default". */
export function parseLanguage(raw: unknown): LanguageChoice {
  return LANGUAGE_CHOICES.includes(raw as LanguageChoice) ? (raw as LanguageChoice) : 'system';
}

/** The choice in localStorage (`{ version: 1, language }`); a missing, corrupt or other-version value is "System default". */
export class LocalStorageLanguageStorage implements LanguageStorage {
  constructor(private readonly key = LANGUAGE_KEY) {}

  load(): LanguageChoice {
    try {
      const parsed = JSON.parse(localStorage.getItem(this.key) ?? 'null') as { version?: unknown; language?: unknown } | null;
      return parsed?.version === LANGUAGE_VERSION ? parseLanguage(parsed.language) : 'system';
    } catch {
      return 'system';
    }
  }

  save(choice: LanguageChoice): void {
    try {
      localStorage.setItem(this.key, JSON.stringify({ version: LANGUAGE_VERSION, language: choice }));
    } catch {
      /* storage unavailable: the language stays "System default" */
    }
  }
}
