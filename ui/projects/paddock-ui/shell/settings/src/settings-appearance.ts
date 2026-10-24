import { effect, EnvironmentProviders, inject, Injector, makeEnvironmentProviders, untracked, type Signal } from '@angular/core';
import { APPEARANCE_SOURCE, AppearanceBackend, BrowserAppearanceBackend, sameAppearance, type Appearance, type AppearanceUnlisten } from '@decryp7/paddock-ui/shell';
import { APPEARANCE_SECTION, type SettingValue } from '../generated/settings';
import { appearanceFromSettings, appearanceToSettings } from './appearance';
import { SettingsBackend } from './settings-backend';

/**
 * How a window learns the Appearance settings: `overview` asks the engine once and follows its changes (a main window and
 * the Settings window, which may call `settings_overview`); `events` only follows the changes (`paddock://settings`, an
 * event every window hears, so a dialog whose contract has no settings command still follows) and starts from the copy.
 */
export type AppearanceRead = 'overview' | 'events';

/**
 * The window's appearance (plan S15) kept in the engine's Appearance settings (plan S10): the engine is the one source of
 * truth for the whole look (theme, contrast, motion, density, pack) and every window follows its changes (rule 34).
 * `copy` only keeps the last known look so the next start paints right before the engine answers; it announces
 * nothing (no second path between windows).
 */
export class SettingsAppearanceBackend extends AppearanceBackend {
  constructor(
    private readonly settings: SettingsBackend,
    private readonly copy: AppearanceBackend = new BrowserAppearanceBackend({ channel: null }),
    /** Calls `start` when the window may talk to the engine (at once by default). */
    private readonly whenReady: (start: () => void) => void = (start) => start(),
    private readonly read: AppearanceRead = 'overview',
    private readonly warn: (message: string) => void = (m) => console.warn(m),
  ) {
    super();
  }

  load(): Appearance {
    return this.copy.load();
  }

  /** Sends the look to the engine; the engine's change event brings it to every window, this one included. */
  save(a: Appearance): void {
    this.copy.save(a);
    this.settings
      .set(APPEARANCE_SECTION, appearanceToSettings(a))
      .catch((e: unknown) => this.warn(`[paddock-settings] the appearance could not be saved: ${e instanceof Error ? e.message : String(e)}`));
  }

  listen(handler: (a: Appearance) => void): AppearanceUnlisten {
    let off: (() => void) | undefined;
    let stopped = false;
    const fromEngine = (values: Readonly<Record<string, SettingValue>>): void => {
      const now = this.copy.load();
      const next: Appearance = Object.freeze({ ...now, ...appearanceFromSettings(values) });
      if (!sameAppearance(next, now)) this.copy.save(next);
      handler(next);
    };
    this.whenReady(() => {
      if (stopped) return;
      const listening = this.settings.listen((c) => c.section === APPEARANCE_SECTION && fromEngine(c.values)).then((u) => (stopped ? u() : (off = u)));
      const first =
        this.read === 'overview'
          ? listening
              .then(() => this.settings.overview())
              .then((o) => {
                const s = o.sections.find((x) => x.id === APPEARANCE_SECTION);
                if (s && !stopped) fromEngine(s.values);
              })
          : listening;
      first.catch((e: unknown) => this.warn(`[paddock-settings] the appearance settings could not be read: ${e instanceof Error ? e.message : String(e)}`));
    });
    return () => {
      stopped = true;
      off?.();
    };
  }
}

export interface SettingsAppearanceOptions {
  /** The window's settings backend (default: the injected `SettingsBackend`). */
  readonly settings?: () => SettingsBackend;
  /** The copy the next start paints with (default `BrowserAppearanceBackend` without a channel). */
  readonly copy?: () => AppearanceBackend;
  /** Start talking to the engine once this holds (`() => inject(PAGES_WINDOW_READY).asReadonly()`): start-up sends nothing else. */
  readonly gate?: () => Signal<boolean>;
  /** `overview` (default) or `events` (a dialog window whose contract has no settings command). */
  readonly read?: AppearanceRead;
}

/**
 * The factory of a window's `SettingsAppearanceBackend`, called in an injection context. `provideShellSettings` provides
 * it as the window's `APPEARANCE_SOURCE`; a dialog window's base providers do the same with `read: 'events'`.
 */
export function settingsAppearanceBackend(options: SettingsAppearanceOptions = {}): () => AppearanceBackend {
  return () => {
    const settings = options.settings?.() ?? inject(SettingsBackend);
    const copy = options.copy?.() ?? new BrowserAppearanceBackend({ channel: null });
    const open = options.gate?.();
    if (!open) return new SettingsAppearanceBackend(settings, copy, undefined, options.read);
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
    return new SettingsAppearanceBackend(settings, copy, whenReady, options.read);
  };
}

/**
 * Makes the Appearance settings the source of this window's look (`APPEARANCE_SOURCE`) outside `provideShellSettings`: a
 * dialog window's base providers, `provideSettingsAppearance({ settings: () => new TauriSettingsBackend(ipc), read: 'events' })`.
 */
export function provideSettingsAppearance(options: SettingsAppearanceOptions = {}): EnvironmentProviders {
  return makeEnvironmentProviders([{ provide: APPEARANCE_SOURCE, useValue: settingsAppearanceBackend(options) }]);
}
