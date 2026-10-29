import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import { TimeZoneMode } from '../format';
import { isThemeId } from '../../theme/themes';
import { SCHEME_MODES, SchemeMode, ThemeId, UI_FONTS, UiFont } from '../../theme/theme.types';

export type { SchemeMode, ThemeId, UiFont } from '../../theme/theme.types';

interface UiPrefsState {
  timezone: TimeZoneMode;
  alwaysShowLifecycle: boolean;
  /** The look (ADR 0018): token set and traits. Null follows the system (High contrast when it asks for more contrast). */
  theme: ThemeId | null;
  /** Light, dark, or follow the operating system. */
  scheme: SchemeMode;
  /** The interface font: the operating system's (native feel, ADR 0028) or the theme's. */
  uiFont: UiFont;
  /** Grid sort order (not persisted). */
  descending: boolean;
  extraColumns: string[];
  /** The Explorer's width the user dragged it to, px (always within `EXPLORER_WIDTH`). */
  explorerWidth: number;
  /** The user set the width (a drag or the keys, even back to the default): it wins over the other columns. False: the default width, which yields to them. */
  explorerWidthChosen: boolean;
}

/** The Explorer's declared widths, px: `dock/dock-layout.ts` builds the Explorer's `ViewLayout` from them (ADR 0056). */
export const EXPLORER_WIDTH = { min: 240, default: 272, max: 640 } as const;

/** Clamps a width to the Explorer's declared limits; anything that is not a finite number gives the default. */
export function clampExplorerWidth(width: unknown): number {
  if (typeof width !== 'number' || !Number.isFinite(width)) return EXPLORER_WIDTH.default;
  return Math.round(Math.min(EXPLORER_WIDTH.max, Math.max(EXPLORER_WIDTH.min, width)));
}

const PREFS_KEY = 'debrief.prefs.v1';
/** The preferences kept across sessions. */
export type Prefs = Pick<UiPrefsState, 'timezone' | 'theme' | 'scheme' | 'uiFont' | 'extraColumns' | 'alwaysShowLifecycle' | 'explorerWidth' | 'explorerWidthChosen'>;

const DEFAULTS: Prefs = { timezone: 'utc', alwaysShowLifecycle: true, theme: null, scheme: 'system', uiFont: 'system', extraColumns: [], explorerWidth: EXPLORER_WIDTH.default, explorerWidthChosen: false };

/** Narrows stored (untrusted) preferences: unknown or malformed values fall back to the defaults. */
export function sanitizePrefs(raw: unknown): Prefs {
  const r = typeof raw === 'object' && raw !== null && !Array.isArray(raw) ? (raw as Record<string, unknown>) : {};
  // A width without the flag is not a choice (prefs from before the flag): the default width.
  const chosen = r['explorerWidthChosen'] === true;
  return {
    timezone: r['timezone'] === 'local' || r['timezone'] === 'utc' ? r['timezone'] : DEFAULTS.timezone,
    alwaysShowLifecycle: typeof r['alwaysShowLifecycle'] === 'boolean' ? r['alwaysShowLifecycle'] : DEFAULTS.alwaysShowLifecycle,
    theme: isThemeId(r['theme']) ? r['theme'] : null,
    scheme: isSchemeMode(r['scheme']) ? r['scheme'] : DEFAULTS.scheme,
    uiFont: isUiFont(r['uiFont']) ? r['uiFont'] : DEFAULTS.uiFont,
    extraColumns: Array.isArray(r['extraColumns']) ? r['extraColumns'].filter((c): c is string => typeof c === 'string') : DEFAULTS.extraColumns,
    explorerWidth: chosen ? clampExplorerWidth(r['explorerWidth']) : EXPLORER_WIDTH.default,
    explorerWidthChosen: chosen,
  };
}

/** Narrows untrusted input to an interface font choice. */
export function isUiFont(v: unknown): v is UiFont {
  return (UI_FONTS as readonly unknown[]).includes(v);
}

/** Narrows untrusted input to a scheme choice. */
export function isSchemeMode(v: unknown): v is SchemeMode {
  return (SCHEME_MODES as readonly unknown[]).includes(v);
}

export function loadPrefs(): Prefs {
  try {
    return sanitizePrefs(JSON.parse(localStorage.getItem(PREFS_KEY) ?? '{}'));
  } catch {
    return sanitizePrefs({});
  }
}

export function savePrefs(p: Prefs): void {
  try {
    localStorage.setItem(PREFS_KEY, JSON.stringify(p));
  } catch {
    /* storage unavailable: preferences are a convenience only */
  }
}

/**
 * View preferences: time zone, theme and colour scheme, lane colouring, extra grid columns, lifecycle markers and the
 * grid's sort order. Saving them is not this store's job (`providePrefsPersistence` does it), and applying the theme is
 * `ThemeService`'s, so it has no side effects of its own.
 */
export const UiPrefsStore = signalStore(
  { providedIn: 'root' },
  withState<UiPrefsState>(() => ({ ...loadPrefs(), descending: false })),
  withMethods((store) => ({
    setTimezone: (timezone: TimeZoneMode) => patchState(store, { timezone }),
    setAlwaysShowLifecycle: (alwaysShowLifecycle: boolean) => patchState(store, { alwaysShowLifecycle }),
    setTheme: (theme: ThemeId | null) => patchState(store, { theme }),
    setScheme: (scheme: SchemeMode) => patchState(store, { scheme }),
    setUiFont: (uiFont: UiFont) => patchState(store, { uiFont }),
    toggleSort: () => patchState(store, (s) => ({ descending: !s.descending })),
    addColumn(field: string) {
      if (!store.extraColumns().includes(field)) patchState(store, { extraColumns: [...store.extraColumns(), field] });
    },
    /** Replaces the extra columns (a lens switch); duplicates are dropped. */
    setColumns: (fields: readonly string[]) => patchState(store, { extraColumns: [...new Set(fields)] }),
    /** The user dragged (or keyed) the Explorer to this width; clamped to its declared limits. */
    setExplorerWidth: (width: number) => patchState(store, { explorerWidth: clampExplorerWidth(width), explorerWidthChosen: true }),
    /** Back to the default width, which yields to the other columns again ("Reset Explorer width"). */
    resetExplorerWidth: () => patchState(store, { explorerWidth: EXPLORER_WIDTH.default, explorerWidthChosen: false }),
    /** Applies the Explorer's width and whether the user chose it as one value (a pop-out follows the host; never a width without its flag). */
    applyExplorerPrefs: (explorerWidth: number, explorerWidthChosen: boolean) =>
      patchState(store, explorerWidthChosen ? { explorerWidth: clampExplorerWidth(explorerWidth), explorerWidthChosen: true } : { explorerWidth: EXPLORER_WIDTH.default, explorerWidthChosen: false }),
    removeColumn: (field: string) => patchState(store, { extraColumns: store.extraColumns().filter((c) => c !== field) }),
    /** The preferences to persist. */
    snapshot: (): Prefs => ({
      timezone: store.timezone(),
      theme: store.theme(),
      scheme: store.scheme(),
      uiFont: store.uiFont(),
      extraColumns: store.extraColumns(),
      alwaysShowLifecycle: store.alwaysShowLifecycle(),
      explorerWidth: store.explorerWidth(),
      explorerWidthChosen: store.explorerWidthChosen(),
    }),
  })),
);
