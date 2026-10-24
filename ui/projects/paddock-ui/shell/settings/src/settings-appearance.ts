import { effect, inject, Injector, untracked, type Signal } from '@angular/core';
import { AppearanceBackend, BrowserAppearanceBackend, sameAppearance, type Appearance, type AppearanceUnlisten } from '@decryp7/paddock-ui/shell';
import { APPEARANCE_SECTION, type SettingValue } from '../generated/settings';
import { appearanceOf } from './appearance';
import { SettingsBackend } from './settings-backend';

/**
 * The window's appearance (plan S15) kept in the engine's Appearance settings (plan S10): one mechanism for the look, with
 * the engine as the source of truth for what it stores (theme and density) and every window following its changes. The
 * parts the settings section does not hold (contrast, motion, the theme pack) and the copy the next start paints with
 * before the engine answers stay in `local` (the window's storage and its cross-window channel by default).
 */
export class SettingsAppearanceBackend extends AppearanceBackend {
  constructor(
    private readonly settings: SettingsBackend,
    private readonly local: AppearanceBackend = new BrowserAppearanceBackend(),
    /** Calls `start` when the window may talk to the engine (at once by default). */
    private readonly whenReady: (start: () => void) => void = (start) => start(),
    private readonly warn: (message: string) => void = (m) => console.warn(m),
  ) {
    super();
  }

  load(): Appearance {
    return this.local.load();
  }

  save(a: Appearance): void {
    const before = this.local.load();
    this.local.save(a);
    if (a.theme === before.theme && a.density === before.density) return;
    this.settings
      .set(APPEARANCE_SECTION, { theme: a.theme, density: a.density })
      .catch((e: unknown) => this.warn(`[paddock-settings] the appearance could not be saved: ${e instanceof Error ? e.message : String(e)}`));
  }

  listen(handler: (a: Appearance) => void): AppearanceUnlisten {
    const offLocal = this.local.listen(handler);
    let offEngine: (() => void) | undefined;
    let stopped = false;
    const fromEngine = (values: Readonly<Record<string, SettingValue>>): void => {
      const now = this.local.load();
      const next: Appearance = Object.freeze({ ...now, ...appearanceOf(values) });
      if (sameAppearance(next, now)) return;
      // Kept for the next start's first paint (the other windows heard the engine too and drop the same value).
      this.local.save(next);
      handler(next);
    };
    this.whenReady(() => {
      if (stopped) return;
      this.settings
        .listen((c) => c.section === APPEARANCE_SECTION && fromEngine(c.values))
        .then((u) => (stopped ? u() : (offEngine = u)))
        .then(() => this.settings.overview())
        .then((o) => {
          const s = o.sections.find((x) => x.id === APPEARANCE_SECTION);
          if (s && !stopped) fromEngine(s.values);
        })
        .catch((e: unknown) => this.warn(`[paddock-settings] the appearance settings could not be read: ${e instanceof Error ? e.message : String(e)}`));
    });
    return () => {
      stopped = true;
      offLocal();
      offEngine?.();
    };
  }
}

export interface SettingsAppearanceOptions {
  /** The window's own copy and channel (default `BrowserAppearanceBackend`; the desktop build passes `IpcAppearanceBackend`). */
  readonly local?: () => AppearanceBackend;
  /** Start talking to the engine once this holds (`() => inject(PAGES_WINDOW_READY).asReadonly()`): start-up sends nothing else. */
  readonly gate?: () => Signal<boolean>;
}

/**
 * The `backend` of `provideAppearance` for an app with Settings: `provideAppearance({ accent, backend:
 * settingsAppearanceBackend({ gate }) })` with `provideShellSettings({ ..., appearance: false })`, so the look has one
 * owner. Call the returned factory in an injection context (the provider does).
 */
export function settingsAppearanceBackend(options: SettingsAppearanceOptions = {}): () => AppearanceBackend {
  return () => {
    const settings = inject(SettingsBackend);
    const local = options.local?.() ?? new BrowserAppearanceBackend();
    const open = options.gate?.();
    if (!open) return new SettingsAppearanceBackend(settings, local);
    const injector = inject(Injector);
    const whenReady = (start: () => void): void => {
      // A side effect outside the signal graph (IPC): start once the window may talk to the engine.
      const ref = effect(
        () => {
          if (!open()) return;
          untracked(start);
          ref.destroy();
        },
        { injector },
      );
    };
    return new SettingsAppearanceBackend(settings, local, whenReady);
  };
}
