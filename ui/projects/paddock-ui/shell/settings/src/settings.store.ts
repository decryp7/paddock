import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import { errorMessage, withRequestStatus } from '@decryp7/paddock-ui/shell/store';
import type { SectionView, SetResult, SettingsChanged, SettingsOverview, SettingValue } from '../generated/settings';
import { SettingsBackend, type SettingsChanges, type Unlisten } from './settings-backend';

/** An engine refusal of one field, with the value it refused (the message shows while that value is in the control). */
export interface Refusal {
  /** Empty: about the whole section. */
  readonly key: string;
  readonly message: string;
  readonly value: SettingValue | null | undefined;
}

interface SettingsState {
  overview: SettingsOverview | null;
  /** The engine's refusals of the last change, by section. */
  refusals: Readonly<Record<string, readonly Refusal[]>>;
  /** The page shown (a page or section id); null: the first one. */
  selected: string | null;
  query: string;
  showAdvanced: boolean;
  /** What the last action did ("Greeting saved."), for the status line. */
  notice: string | null;
  /** A change the engine could not make (not a refusal): unknown section, apply or save failed. */
  actionError: string | null;
  /** Labels of the restart-only settings changed in this window. */
  restartChanges: readonly string[];
}

const without = <T>(map: Readonly<Record<string, T>>, id: string): Record<string, T> => {
  const { [id]: _drop, ...rest } = map;
  return rest;
};

/**
 * The Settings window's state: the engine's sections with their values, the engine's refusals, and what the window shows
 * (page, search, advanced). Values are validated, applied and saved by the engine; unsaved edits live in the window's
 * forms (`SettingsDrafts`), never here. Provided per Settings window by `provideSettingsWindow`; other windows' changes
 * arrive through the backend's events.
 */
export const SettingsStore = signalStore(
  withState<SettingsState>({ overview: null, refusals: {}, selected: null, query: '', showAdvanced: false, notice: null, actionError: null, restartChanges: [] }),
  // A failed read retries itself; a change event while in error reads every section again (ADR 0058).
  withRequestStatus({ retry: true }),
  withComputed((store) => ({
    sections: computed<readonly SectionView[]>(() => store.overview()?.sections ?? []),
    restartPending: computed(() => store.overview()?.restartPending ?? false),
    problem: computed(() => store.overview()?.problem ?? null),
  })),
  withMethods((store, backend = inject(SettingsBackend)) => {
    const sectionOf = (id: string) => store.sections().find((s) => s.id === id);
    const replace = (id: string, values: Readonly<Record<string, SettingValue>>, restartPending?: boolean) => {
      const overview = store.overview();
      if (!overview) return;
      patchState(store, {
        overview: {
          ...overview,
          restartPending: restartPending ?? overview.restartPending,
          sections: overview.sections.map((s) => (s.id === id ? { ...s, values: { ...values } } : s)),
        },
      });
    };
    /** Takes a change's answer: the new values, or the refusals with the values they refused. */
    const take = (id: string, sent: SettingsChanges, r: SetResult, what: string): boolean => {
      if (!r.applied) {
        patchState(store, (st) => ({ refusals: { ...st.refusals, [id]: r.errors.map((e) => ({ ...e, value: sent[e.key] })) }, notice: null }));
        return false;
      }
      const section = sectionOf(id);
      // The engine says whether a restart is needed; the change event may have updated the values already.
      const restart = r.restartRequired ? (section?.fields.filter((f) => f.restartRequired && f.key in sent).map((f) => f.label) ?? []) : [];
      replace(id, r.values, r.restartRequired ? true : undefined);
      patchState(store, (st) => ({
        refusals: without(st.refusals, id),
        notice: $localize`:@@pd.settings.store.saved:${what}:what: saved.`,
        actionError: null,
        restartChanges: [...new Set([...st.restartChanges, ...restart])],
      }));
      return true;
    };
    const reads = store._reads.define({
      read: () => backend.overview(),
      apply: (overview: SettingsOverview) => patchState(store, { overview }),
      message: (e) => errorMessage(e, $localize`:@@pd.settings.store.load-failed:The settings could not be loaded.`),
    });
    return {
      /** (Re)reads every section; the Retry of a failed load. */
      async load(): Promise<void> {
        await reads.load();
      },
      section: (id: string): SectionView | undefined => sectionOf(id),
      select: (selected: string) => patchState(store, { selected, query: '' }),
      setQuery: (query: string) => patchState(store, { query }),
      setShowAdvanced: (showAdvanced: boolean) => patchState(store, { showAdvanced }),
      dismissNotice: () => patchState(store, { notice: null, actionError: null }),
      /**
       * The text in the window's footer (null clears it): the store's own "Saved" and "Reset" go there, and a page with its
       * own content says what it did the same way ("Folder moved"), so there is one place to look.
       */
      setNotice: (notice: string | null) => patchState(store, { notice }),
      /** The engine's refusal of `key` while the control still holds the refused value, else null. */
      refusalOf(id: string, key: string, value: SettingValue | undefined): string | null {
        const r = (store.refusals()[id] ?? []).find((x) => x.key === key);
        return r && JSON.stringify(r.value) === JSON.stringify(value) ? r.message : null;
      },
      /** Refusals about a whole section (no key, or a key it does not have). */
      sectionRefusals(id: string): readonly string[] {
        const keys = new Set(sectionOf(id)?.fields.map((f) => f.key) ?? []);
        return (store.refusals()[id] ?? []).filter((r) => !keys.has(r.key)).map((r) => r.message);
      },
      /** Sends `changes` of section `id`; true when the engine applied them. A failure is the action error, never a throw. */
      async apply(id: string, changes: SettingsChanges): Promise<boolean> {
        const section = sectionOf(id);
        const keys = Object.keys(changes);
        if (!section || !keys.length) return true;
        const what = keys.length === 1 ? (section.fields.find((f) => f.key === keys[0])?.label ?? section.title) : section.title;
        try {
          const r = await backend.set(id, changes);
          return take(id, changes, r, what);
        } catch (e) {
          patchState(store, { actionError: errorMessage(e, $localize`:@@pd.settings.store.set-failed:The settings could not be changed.`), notice: null });
          return false;
        }
      },
      /** Puts section `id`, or its field `key`, back to the default. */
      async reset(id: string, key?: string): Promise<boolean> {
        const section = sectionOf(id);
        if (!section) return false;
        const label = key === undefined ? section.title : (section.fields.find((f) => f.key === key)?.label ?? key);
        try {
          const r = await backend.reset(id, key);
          const sent: SettingsChanges = Object.fromEntries((key === undefined ? section.fields.map((f) => f.key) : [key]).map((k) => [k, null]));
          const ok = take(id, sent, r, label);
          if (ok && key === undefined) patchState(store, { notice: $localize`:@@pd.settings.store.reset-done:${section.title}:section: reset to defaults.` });
          return ok;
        } catch (e) {
          patchState(store, { actionError: errorMessage(e, $localize`:@@pd.settings.store.reset-failed:The settings could not be reset.`), notice: null });
          return false;
        }
      },
      /** A change from any window (the engine's event); while the overview is in error, it is read again. */
      _receive(change: SettingsChanged): void {
        replace(change.section, change.values, change.restartPending);
        reads.event();
      },
    };
  }),
  withHooks((store, backend = inject(SettingsBackend)) => {
    let off: Unlisten | undefined;
    let destroyed = false;
    return {
      onInit() {
        // Listen first, then read: a change during the read arrives as an event after it and wins.
        void backend
          .listen((c) => store._receive(c))
          .then((u) => (destroyed ? u() : (off = u)))
          .catch((e: unknown) => console.warn(`[paddock-settings] cannot listen for settings changes: ${errorMessage(e, 'unknown error')}`));
        void store.load();
      },
      onDestroy() {
        destroyed = true;
        off?.();
      },
    };
  }),
);

export type SettingsStoreType = InstanceType<typeof SettingsStore>;
