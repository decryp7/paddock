import { InjectionToken, computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { CacheClearScope, CacheInfo, CommandError, Preset, Settings } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Toasts } from '../../core/toasts';
import { PRESET_LABEL } from '../../shared/labels';

type Section = keyof Settings;

/** How long the page waits after the last edit before it saves (a burst of typing or toggling is one save). */
export const SETTINGS_SAVE_DELAY = new InjectionToken<number>('PitwallSettingsSaveDelay', { factory: () => 500 });

interface SettingsState {
  /** What the server has. */
  saved: Settings | null;
  /** The form: `saved` plus the edits not saved yet (they are saved a moment after they are made). */
  draft: Settings | null;
  loading: boolean;
  saving: boolean;
  error: CommandError | null;
  /** The last save's error, shown inline with Retry; the edit stays in the form until it is saved or reverted. */
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

/**
 * Settings: the saved values and the form's draft. Every edit is saved by itself a moment later (debounced, so a burst is
 * one save); the server validates and returns what it kept. A refused or failed save keeps the edit and its error until the
 * user retries, edits again or reverts. `flush` saves at once (the window closing).
 */
export const SettingsStore = signalStore(
  withState(initial),
  withComputed(({ saved, draft }) => ({
    dirty: computed(() => {
      const s = saved();
      const d = draft();
      return !!s && !!d && JSON.stringify(s) !== JSON.stringify(d);
    }),
  })),
  withMethods((store, backend = inject(BACKEND), toasts = inject(Toasts), delay = inject(SETTINGS_SAVE_DELAY)) => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    let inflight: Promise<boolean> | null = null;
    const same = (a: Settings | null, b: Settings | null) => JSON.stringify(a) === JSON.stringify(b);
    const cancelTimer = () => {
      if (timer !== null) clearTimeout(timer);
      timer = null;
    };

    /** Sends the draft as it is now; edits made while it is on the way stay in the draft and are saved next. */
    async function send(): Promise<boolean> {
      const sent = store.draft();
      if (!sent) return false;
      patchState(store, { saving: true, saveError: null });
      try {
        const kept = await backend.saveSettings(sent);
        const now = store.draft();
        patchState(store, { saved: kept, draft: same(now, sent) ? structuredClone(kept) : now, saving: false });
        return true;
      } catch (e) {
        patchState(store, { saving: false, saveError: toCommandError(e) });
        return false;
      }
    }

    /** Saves now: after a save already on the way, and only when the draft differs from what the server has. */
    async function save(): Promise<boolean> {
      cancelTimer();
      while (inflight) await inflight;
      if (!store.draft()) return false;
      if (same(store.saved(), store.draft())) return true;
      inflight = send();
      try {
        return await inflight;
      } finally {
        inflight = null;
      }
    }

    return {
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

      /** Changes the form and saves it a moment later (a newer edit restarts the wait). */
      edit<S extends Section>(section: S, values: Partial<Settings[S]>): void {
        const d = store.draft();
        if (!d) return;
        patchState(store, { draft: editSettings(d, section, values), saveError: null });
        cancelTimer();
        timer = setTimeout(() => {
          timer = null;
          void save();
        }, delay);
      },

      /** Puts the form back to what the server has (after a refused save). */
      revert(): void {
        cancelTimer();
        const s = store.saved();
        patchState(store, { draft: s && structuredClone(s), saveError: null });
      },

      save,

      /** Saves an edit still waiting for its moment (the window closes or hides); nothing when all is saved or a save failed. */
      flush(): Promise<boolean> {
        if (timer === null && !inflight) return Promise.resolve(!store.saveError());
        return save();
      },

      /** Applies a memory preset on the server (after any edit waiting to be saved) and shows its values. */
      async applyPreset(preset: Exclude<Preset, 'custom'>): Promise<void> {
        if (store.saving()) return;
        if (timer !== null) await save();
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
    };
  }),
);

export type SettingsStore = InstanceType<typeof SettingsStore>;
