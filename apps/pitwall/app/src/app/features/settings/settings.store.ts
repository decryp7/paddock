import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { CacheClearScope, CacheInfo, CommandError, Preset, Settings } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Toasts } from '../../core/toasts';
import { PRESET_LABEL } from '../../shared/labels';

type Section = keyof Settings;

interface SettingsState {
  /** What the server has. */
  saved: Settings | null;
  /** The form: `saved` plus unsaved edits. */
  draft: Settings | null;
  loading: boolean;
  saving: boolean;
  error: CommandError | null;
  /** The last save's error, shown next to the Save bar. */
  saveError: CommandError | null;
  /** The caches' sizes and hit counts (ADR 0019); null until loaded. */
  cacheInfo: CacheInfo | null;
  cacheError: CommandError | null;
  /** A cache clear is running. */
  clearing: boolean;
}

const initial: SettingsState = { saved: null, draft: null, loading: false, saving: false, error: null, saveError: null, cacheInfo: null, cacheError: null, clearing: false };

/** Applies an edit to one section; any memory edit other than the preset itself makes the preset `custom`. */
export function editSettings<S extends Section>(s: Settings, section: S, values: Partial<Settings[S]>): Settings {
  const next = { ...s, [section]: { ...s[section], ...values } } as Settings;
  if (section === 'memory' && !('preset' in values)) next.memory = { ...next.memory, preset: 'custom' };
  return next;
}

/** Settings: the saved values, a draft the form edits, Save / Discard, and presets. */
export const SettingsStore = signalStore(
  withState(initial),
  withComputed(({ saved, draft }) => ({
    dirty: computed(() => {
      const s = saved();
      const d = draft();
      return !!s && !!d && JSON.stringify(s) !== JSON.stringify(d);
    }),
  })),
  withMethods((store, backend = inject(BACKEND), toasts = inject(Toasts)) => ({
    async load(): Promise<void> {
      patchState(store, { loading: true, error: null });
      try {
        const settings = await backend.settings();
        patchState(store, { saved: settings, draft: structuredClone(settings), loading: false });
      } catch (e) {
        patchState(store, { loading: false, error: toCommandError(e) });
      }
    },

    /** Reads the caches' sizes and hit counts; a failure shows inline and keeps the last good numbers. */
    async loadCacheInfo(): Promise<void> {
      try {
        patchState(store, { cacheInfo: await backend.cacheInfo(), cacheError: null });
      } catch (e) {
        patchState(store, { cacheError: toCommandError(e) });
      }
    },

    /** Empties caches (a command, applied at once and not part of the draft) and refreshes the numbers. */
    async clearCache(scope: CacheClearScope): Promise<number | undefined> {
      if (store.clearing()) return undefined;
      patchState(store, { clearing: true });
      try {
        const removed = await backend.clearCache(scope);
        toasts.ok(removed > 0 ? `Removed ${removed} cached ${removed === 1 ? 'entry' : 'entries'}.` : 'Nothing to remove: the cache was already empty.');
        patchState(store, { clearing: false });
        patchState(store, { cacheInfo: await backend.cacheInfo(), cacheError: null });
        return removed;
      } catch (e) {
        patchState(store, { clearing: false });
        toasts.error(toCommandError(e));
        return undefined;
      }
    },

    edit<S extends Section>(section: S, values: Partial<Settings[S]>): void {
      const d = store.draft();
      if (d) patchState(store, { draft: editSettings(d, section, values), saveError: null });
    },

    discard(): void {
      const s = store.saved();
      patchState(store, { draft: s && structuredClone(s), saveError: null });
    },

    /** Saves the draft; the server validates it and returns what it kept. */
    async save(): Promise<boolean> {
      const d = store.draft();
      if (!d || store.saving()) return false;
      patchState(store, { saving: true, saveError: null });
      try {
        const settings = await backend.saveSettings(d);
        patchState(store, { saved: settings, draft: structuredClone(settings), saving: false });
        toasts.ok('Settings saved.');
        return true;
      } catch (e) {
        const error = toCommandError(e);
        patchState(store, { saving: false, saveError: error });
        toasts.error(error);
        return false;
      }
    },

    /** Applies a memory preset on the server and shows its values (other unsaved edits stay). */
    async applyPreset(preset: Exclude<Preset, 'custom'>): Promise<void> {
      if (store.saving()) return;
      patchState(store, { saving: true });
      try {
        const settings = await backend.applyPreset(preset);
        const d = store.draft();
        patchState(store, { saved: settings, draft: d ? { ...d, memory: structuredClone(settings.memory) } : structuredClone(settings), saving: false });
        toasts.ok(`${PRESET_LABEL[preset]} preset applied.`);
      } catch (e) {
        patchState(store, { saving: false });
        toasts.error(toCommandError(e));
      }
    },
  })),
);

export type SettingsStore = InstanceType<typeof SettingsStore>;
