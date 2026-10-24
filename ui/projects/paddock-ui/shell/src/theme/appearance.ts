import {
  SU_CONTRASTS,
  SU_DENSITIES,
  SU_MOTIONS,
  SU_THEMES,
  type SuContrast,
  type SuDensity,
  type SuMotion,
  type SuTheme,
} from '@decryp7/paddock-ui';

/**
 * The user's appearance choice, the same in every window of the app: scheme (system, dark, light), contrast, motion,
 * density and an optional theme pack. The accent is the app's and not a choice. Pure: no Angular, no DOM, so `main.ts`
 * can apply the stored choice before the first paint.
 */
export interface Appearance {
  readonly theme: SuTheme;
  readonly contrast: SuContrast;
  readonly motion: SuMotion;
  readonly density: SuDensity;
  /** A `defineTheme` pack id, or null for the shell's own tokens. */
  readonly pack: string | null;
}

export const DEFAULT_APPEARANCE: Appearance = Object.freeze({ theme: 'system', contrast: 'system', motion: 'system', density: 'standard', pack: null });

/** Version of the stored and broadcast form; another version is ignored (defaults), never half-read. */
export const APPEARANCE_VERSION = 1;

const oneOf = <T extends string>(values: readonly T[], v: unknown, fallback: T): T => (values.includes(v as T) ? (v as T) : fallback);
const PACK_ID = /^[a-z][a-z0-9]*(?:\.[a-z0-9]+(?:-[a-z0-9]+)*)+$/;

/**
 * Parses an untrusted stored or broadcast value (`{ version, theme, ... }`). Unknown fields are dropped and each bad
 * field falls back to its default; a wrong version or a non-object is the default appearance.
 */
export function parseAppearance(raw: unknown): Appearance {
  if (typeof raw !== 'object' || raw === null) return DEFAULT_APPEARANCE;
  const r = raw as Record<string, unknown>;
  if (r['version'] !== APPEARANCE_VERSION) return DEFAULT_APPEARANCE;
  const pack = r['pack'];
  return Object.freeze({
    theme: oneOf(SU_THEMES, r['theme'], DEFAULT_APPEARANCE.theme),
    contrast: oneOf(SU_CONTRASTS, r['contrast'], DEFAULT_APPEARANCE.contrast),
    motion: oneOf(SU_MOTIONS, r['motion'], DEFAULT_APPEARANCE.motion),
    density: oneOf(SU_DENSITIES, r['density'], DEFAULT_APPEARANCE.density),
    pack: typeof pack === 'string' && pack.length <= 128 && PACK_ID.test(pack) ? pack : null,
  });
}

/** The stored and broadcast form of an appearance. */
export function serializeAppearance(a: Appearance): Record<string, unknown> {
  return { version: APPEARANCE_VERSION, theme: a.theme, contrast: a.contrast, motion: a.motion, density: a.density, pack: a.pack };
}

export function sameAppearance(a: Appearance, b: Appearance): boolean {
  return a.theme === b.theme && a.contrast === b.contrast && a.motion === b.motion && a.density === b.density && a.pack === b.pack;
}

/** What the OS asks for (the `prefers-*` media queries). */
export interface SystemAppearance {
  readonly dark: boolean;
  readonly moreContrast: boolean;
  readonly reducedMotion: boolean;
}

/** What the window shows: the choice resolved against the OS. */
export interface ResolvedAppearance {
  readonly scheme: 'dark' | 'light';
  readonly highContrast: boolean;
  readonly reducedMotion: boolean;
}

export function resolveAppearance(a: Appearance, system: SystemAppearance): ResolvedAppearance {
  return {
    scheme: a.theme === 'system' ? (system.dark ? 'dark' : 'light') : a.theme,
    highContrast: a.contrast === 'system' ? system.moreContrast : a.contrast === 'more',
    reducedMotion: a.motion === 'system' ? system.reducedMotion : a.motion === 'reduce',
  };
}
