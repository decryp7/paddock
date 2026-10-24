import { computed, DestroyRef, effect, inject, InjectionToken, signal, type Signal } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { applyPaddockAppearance, type SuAccent } from '@decryp7/paddock-ui';
import { ContributionRegistry } from '../registry';
import { resolveAppearance, sameAppearance, type Appearance, type ResolvedAppearance, type SystemAppearance } from './appearance';
import { AppearanceBackend } from './appearance-backend';
import { themeTokensFor, type ThemeDef, type ThemeTokens } from './theme';

/** The app's fixed part of the look and the element it is applied to. */
export interface AppearanceConfig {
  readonly accent: SuAccent;
  /** The element that carries the `data-*` attributes and pack tokens (default `document.documentElement`). */
  readonly root: () => HTMLElement;
}

export const APPEARANCE_CONFIG = new InjectionToken<AppearanceConfig>('APPEARANCE_CONFIG');

/** A media query as the system preferences read it (`matchMedia`; a fake in tests). */
export type MatchMedia = (query: string) => Pick<MediaQueryList, 'matches' | 'addEventListener' | 'removeEventListener'>;

export const MATCH_MEDIA = new InjectionToken<MatchMedia | null>('MATCH_MEDIA', {
  providedIn: 'root',
  factory: () => (typeof matchMedia === 'function' ? (q: string) => matchMedia(q) : null),
});

/**
 * What the OS asks for, as a signal that follows changes (`prefers-color-scheme`, `prefers-contrast`,
 * `prefers-reduced-motion`). Unknown means dark (as the tokens do), standard contrast and full motion.
 */
export const SYSTEM_APPEARANCE = new InjectionToken<Signal<SystemAppearance>>('SYSTEM_APPEARANCE', {
  providedIn: 'root',
  factory: () => {
    const mm = inject(MATCH_MEDIA);
    const read = (): SystemAppearance => ({
      dark: !(mm?.('(prefers-color-scheme: light)').matches ?? false),
      moreContrast: mm?.('(prefers-contrast: more)').matches ?? false,
      reducedMotion: mm?.('(prefers-reduced-motion: reduce)').matches ?? false,
    });
    const state = signal(read());
    if (mm) {
      const lists = ['(prefers-color-scheme: light)', '(prefers-contrast: more)', '(prefers-reduced-motion: reduce)'].map((q) => mm(q));
      const onChange = (): void => state.set(read());
      for (const l of lists) l.addEventListener('change', onChange);
      inject(DestroyRef).onDestroy(() => lists.forEach((l) => l.removeEventListener('change', onChange)));
    }
    return state.asReadonly();
  },
});

/** Inline tokens set on each root, so the next pack (or none) removes exactly what the last one set. */
const applied = new WeakMap<HTMLElement, readonly string[]>();

/**
 * Puts an appearance on `root`: the `data-*` attributes (theme, accent, density, contrast, motion), `data-pack`, and the
 * pack's tokens for the resolved scheme as inline custom properties. No pack is applied in high contrast. Idempotent.
 */
export function applyAppearanceTo(root: HTMLElement, a: Appearance, accent: SuAccent, resolved: ResolvedAppearance, pack: ThemeDef | null): void {
  applyPaddockAppearance(root, { theme: a.theme, accent, density: a.density, contrast: a.contrast, motion: a.motion });
  const active = pack && !resolved.highContrast ? pack : null;
  const tokens: ThemeTokens = active ? themeTokensFor(active, resolved.scheme) : {};
  for (const name of applied.get(root) ?? []) if (!(name in tokens)) root.style.removeProperty(name);
  for (const [name, value] of Object.entries(tokens)) root.style.setProperty(name, value);
  applied.set(root, Object.keys(tokens));
  if (active) root.setAttribute('data-pack', active.id);
  else root.removeAttribute('data-pack');
}

/**
 * The window's appearance (one per window, ADR 0012 §4's root singletons): the user's choice, as the backend keeps it and
 * every window hears it, resolved against the OS. Changing it here saves it and tells the other windows; a change from
 * another window arrives through the backend. An effect puts it on `<html>`.
 */
export const AppearanceStore = signalStore(
  withState<{ appearance: Appearance }>(() => ({ appearance: inject(AppearanceBackend).load() })),
  withComputed((store, system = inject(SYSTEM_APPEARANCE), registry = inject(ContributionRegistry)) => {
    const packs = computed(() => registry.items<ThemeDef>('themes'));
    return {
      theme: computed(() => store.appearance().theme),
      contrast: computed(() => store.appearance().contrast),
      motion: computed(() => store.appearance().motion),
      density: computed(() => store.appearance().density),
      /** The theme packs the app contributed (`provideThemes`). */
      packs,
      /** The chosen pack, or null (none chosen, or the chosen one is not registered in this window). */
      pack: computed(() => packs().find((p) => p.id === store.appearance().pack) ?? null),
      /** The scheme, contrast and motion the window shows (charts and the timeline read `resolved().scheme`). */
      resolved: computed(() => resolveAppearance(store.appearance(), system())),
    };
  }),
  withMethods((store, backend = inject(AppearanceBackend)) => {
    const choose = (change: Partial<Appearance>): void => {
      const next: Appearance = Object.freeze({ ...store.appearance(), ...change });
      if (sameAppearance(next, store.appearance())) return;
      patchState(store, { appearance: next });
      backend.save(next);
    };
    return {
      /** Changes the choice in this window and every other window of the app. */
      choose,
      setTheme: (theme: Appearance['theme']) => choose({ theme }),
      setContrast: (contrast: Appearance['contrast']) => choose({ contrast }),
      setMotion: (motion: Appearance['motion']) => choose({ motion }),
      setDensity: (density: Appearance['density']) => choose({ density }),
      setPack: (pack: string | null) => choose({ pack }),
      /** Another window's choice: shown here, not saved or announced again (that window did both). */
      _received(a: Appearance): void {
        if (!sameAppearance(a, store.appearance())) patchState(store, { appearance: a });
      },
    };
  }),
  withHooks({
    onInit(store, backend = inject(AppearanceBackend), config = inject(APPEARANCE_CONFIG)) {
      const unlisten = backend.listen((a) => store._received(a));
      inject(DestroyRef).onDestroy(unlisten);
      // DOM side effect only: the attributes and pack tokens on <html> follow the store.
      effect(() => applyAppearanceTo(config.root(), store.appearance(), config.accent, store.resolved(), store.pack()));
    },
  }),
);

export type AppearanceStore = InstanceType<typeof AppearanceStore>;
