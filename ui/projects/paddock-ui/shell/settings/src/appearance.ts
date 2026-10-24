import { DestroyRef, inject } from '@angular/core';
import { applyPaddockAppearance, SU_DENSITIES, SU_THEMES, type SuDensity, type SuTheme } from '@decryp7/paddock-ui';
import { APPEARANCE_SECTION, type SettingValue } from '../generated/settings';
import { SettingsBackend } from './settings-backend';

/** The look of the shell's Appearance section (`paddock_settings::shell::appearance`), when its values are known ones. */
export function appearanceOf(values: Readonly<Record<string, SettingValue>>): { theme?: SuTheme; density?: SuDensity } {
  const theme = SU_THEMES.find((t) => t === values['theme']);
  const density = SU_DENSITIES.find((d) => d === values['density']);
  return { ...(theme ? { theme } : {}), ...(density ? { density } : {}) };
}

/**
 * Keeps this window's look on the Appearance settings: applied once read and again whenever any window changes them
 * (a side effect on the DOM, outside the signal graph). Call in an injection context (an environment initializer).
 */
export function followAppearance(root: HTMLElement = document.documentElement): void {
  appearanceFollower(root)();
}

/**
 * `followAppearance` in two steps: call this in an injection context, and the returned function when the window should
 * start following (a main window after its start-up reply, so nothing waits on settings before it is ready). Starting
 * twice follows once.
 */
export function appearanceFollower(root: HTMLElement = document.documentElement): () => void {
  const backend = inject(SettingsBackend);
  let off: (() => void) | undefined;
  let started = false;
  let destroyed = false;
  inject(DestroyRef).onDestroy(() => {
    destroyed = true;
    off?.();
  });
  const apply = (values: Readonly<Record<string, SettingValue>>) => applyPaddockAppearance(root, appearanceOf(values));
  return () => {
    if (started || destroyed) return;
    started = true;
    void backend
      .listen((c) => c.section === APPEARANCE_SECTION && apply(c.values))
      .then((u) => (destroyed ? u() : (off = u)))
      .then(() => backend.overview())
      .then((o) => {
        const s = o.sections.find((x) => x.id === APPEARANCE_SECTION);
        if (s && !destroyed) apply(s.values);
      })
      .catch((e: unknown) => console.warn(`[paddock-settings] the appearance settings could not be read: ${e instanceof Error ? e.message : String(e)}`));
  };
}
