import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { CommandError, PluginInfo, Settings } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { Notifier } from '../shared/notifier';
import { rebaseDraft } from './rebase-draft';

interface SettingsState {
  settings: Settings | null;
  error: CommandError | null;
  saving: boolean;
  /** Why the last save failed (null after a good save); the page can show it next to the button. */
  saveError: CommandError | null;
  /** After a stale refusal: the copy the window had read and the one it tried to save, until the page has rebased its edits onto the reloaded settings. */
  stale: { base: Settings | null; attempted: Settings } | null;
  plugins: PluginInfo[];
  pluginsLoaded: boolean;
  pluginsError: CommandError | null;
}

/** Settings (the appearance is the shell's, not part of these) and the plugins list. */
export const SettingsStore = signalStore(
  { providedIn: 'root' },
  withState<SettingsState>({ settings: null, error: null, saving: false, saveError: null, stale: null, plugins: [], pluginsLoaded: false, pluginsError: null }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier)) => {
    const load = async (): Promise<void> => {
      patchState(store, { error: null });
      try {
        const settings = await backend.settings();
        patchState(store, { settings });
      } catch (e) {
        patchState(store, { error: toCommandError(e) });
      }
    };
    /** Saves in order (see `save`). */
    let inFlight: Promise<void> | null = null;
    /** The revisions this window's own saves made: a settings revision that is one of them differs from a copy's base by this window's saves only. */
    const ownRevisions = new Set<number>();
    const saveNow = async (settings: Settings, base: Settings | null, message: string | null): Promise<boolean> => {
      // A copy taken before an earlier save of this window finished lacks that save's change: its own edits (against its base) are put
      // onto the settings as they are now, so the earlier change is not undone. A revision another window made is not carried: that save
      // is refused as stale below.
      const current = store.settings();
      const rebased = base && current && current.revision !== base.revision && ownRevisions.has(current.revision) ? rebaseDraft(base, settings, current) : settings;
      patchState(store, { saving: true, saveError: null, stale: null });
      try {
        const saved = await backend.saveSettings(rebased);
        ownRevisions.add(saved.revision);
        patchState(store, { settings: saved, saving: false, saveError: null });
        if (message) notifier.show(message);
        return true;
      } catch (e) {
        const saveError = toCommandError(e);
        patchState(store, { saving: false, saveError });
        if (saveError.code === 'stale') {
          // Another window or request changed the settings after this copy was read: show the current ones instead of overwriting
          // them, keep what was attempted for the page to carry over (the page says so when it does), and say what was done here.
          patchState(store, { stale: { base: store.settings(), attempted: settings } });
          await load();
          notifier.show('The settings changed in another window and were reloaded. Try again.', 'warn');
          return false;
        }
        notifier.error(saveError);
        return false;
      }
    };
    return {
      load,
      async loadPlugins(): Promise<void> {
        patchState(store, { pluginsError: null });
        try {
          patchState(store, { plugins: await backend.plugins(), pluginsLoaded: true });
        } catch (e) {
          patchState(store, { pluginsError: toCommandError(e) });
        }
      },
      /** The page took the rebase of its edits; nothing is left to carry over. */
      clearStale(): void {
        patchState(store, { stale: null });
      },
      /**
       * Saves the whole settings object; the engine validates it and returns what it stored. Saves run one after the other: a second
       * save of the same window waits for the first and carries its own edits onto the settings the first made, so two quick saves never refuse each
       * other as "another window". A save of a copy that another window or request outdated is refused as stale: the store reloads,
       * keeps what was attempted in `stale` for the page to carry over, and says so.
       */
      save(settings: Settings, message: string | null = 'Settings saved'): Promise<boolean> {
        // With no save in flight it starts at once; otherwise it waits for the one in flight.
        const base = store.settings();
        const run: Promise<boolean> = inFlight ? inFlight.then(() => saveNow(settings, base, message)) : saveNow(settings, base, message);
        const mine = run.then(
          () => undefined,
          () => undefined,
        );
        inFlight = mine;
        void mine.then(() => {
          if (inFlight === mine) inFlight = null;
        });
        return run;
      },
    };
  }),
);
