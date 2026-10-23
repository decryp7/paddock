import { DestroyRef, computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { CommandError, CounterHistory, CounterHit, CounterObject, CounterObjectList, CounterReading, CounterSet, Settings } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { MAX_PINNED, MAX_SETS, MAX_SET_NAME, MAX_WATCHED, readingNumber, withPath, withoutPath } from '../features/counters/counters-model';
import { MAX_WIDGETS, newWidget } from '../features/widgets/widget-model';
import { Notifier } from '../shared/notifier';
import { SettingsStore } from './settings.store';

/** Counters are read this often while the page or a dashboard widget needs them. */
export const COUNTERS_POLL_MS = 2000;
/** Most results asked for in a search (the engine's cap is 200). */
export const SEARCH_LIMIT = 100;
/** Points kept per counter for dashboard widgets. */
const TRAIL = 60;

export const RANGES = [
  { minutes: 5, label: '5 min' },
  { minutes: 15, label: '15 min' },
  { minutes: 60, label: '1 h' },
] as const;

interface CountersState {
  objects: CounterObjectList | null;
  objectsError: CommandError | null;
  opened: CounterObject | null;
  openError: CommandError | null;
  query: string;
  hits: CounterHit[] | null;
  searchError: CommandError | null;
  /** The counters charted. */
  watching: string[];
  readings: Record<string, CounterReading>;
  history: CounterHistory | null;
  minutes: number;
  pollError: CommandError | null;
  /** Counters that dashboard widgets show (read while a dashboard is on screen). */
  tracked: string[];
  /** The last values per counter, for dashboard graphs. */
  trail: Record<string, number[]>;
}

/**
 * The performance counters browser: the object list, search, the counters being charted with their readings and
 * history, and the user's pinned counters and saved sets (kept in Settings, validated by the engine). Reading happens
 * only while something needs it: `use()` returns the function that releases it, and polling stops with the last user.
 */
export const CountersStore = signalStore(
  { providedIn: 'root' },
  withState<CountersState>({
    objects: null,
    objectsError: null,
    opened: null,
    openError: null,
    query: '',
    hits: null,
    searchError: null,
    watching: [],
    readings: {},
    history: null,
    minutes: 5,
    pollError: null,
    tracked: [],
    trail: {},
  }),
  withComputed((s, settings = inject(SettingsStore)) => ({
    pinned: computed(() => settings.settings()?.counters?.pinned ?? []),
    sets: computed(() => settings.settings()?.counters?.sets ?? []),
    /** Whether the chart has anything to show yet. */
    charted: computed(() => s.watching().length > 0),
    /** The paths read on the next tick: charted ones first, then the dashboard's. */
    wanted: computed(() => [...new Set([...s.watching(), ...s.tracked()])].slice(0, MAX_WATCHED)),
  })),
  withMethods((store, backend = inject(BACKEND), settings = inject(SettingsStore), notifier = inject(Notifier), destroyRef = inject(DestroyRef, { optional: true })) => {
    let users = 0;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let running = false;
    let searchToken = 0;

    const stop = () => {
      if (timer !== null) clearTimeout(timer);
      timer = null;
    };

    /** Reads what is wanted now; the chart's history is read only while counters are charted. */
    const tick = async (): Promise<void> => {
      if (running) return;
      running = true;
      try {
        const paths = store.wanted();
        if (paths.length > 0) {
          const values = await backend.counterValues(paths);
          const readings = Object.fromEntries(values.map((v) => [v.path, v]));
          const trail = { ...store.trail() };
          for (const v of values) {
            const n = readingNumber(v);
            if (n !== null) trail[v.path] = [...(trail[v.path] ?? []), n].slice(-TRAIL);
          }
          patchState(store, { readings: { ...store.readings(), ...readings }, trail, pollError: null });
        }
        if (store.watching().length > 0) {
          patchState(store, { history: await backend.counterHistory(store.watching(), store.minutes()), pollError: null });
        }
      } catch (e) {
        patchState(store, { pollError: toCommandError(e) });
      } finally {
        running = false;
      }
      if (users > 0) timer = setTimeout(() => void tick(), COUNTERS_POLL_MS);
    };

    const saveCounters = async (patch: (c: { pinned: string[]; sets: CounterSet[] }) => { pinned: string[]; sets: CounterSet[] }, message: string | null): Promise<boolean> => {
      const s = settings.settings();
      if (!s) return false;
      const current = { pinned: s.counters?.pinned ?? [], sets: s.counters?.sets ?? [] };
      return settings.save({ ...s, counters: patch(current) } satisfies Settings, message);
    };

    destroyRef?.onDestroy(stop);

    return {
      /** Starts reading (a page or a dashboard needs it); call the returned function when done. */
      use(): () => void {
        users++;
        if (users === 1) {
          stop();
          void tick();
        }
        let released = false;
        return () => {
          if (released) return;
          released = true;
          users = Math.max(0, users - 1);
          if (users === 0) stop();
        };
      },
      tick,
      /** The counters dashboard widgets show; read while a dashboard is on screen. */
      track(paths: string[]): void {
        const next = [...new Set(paths)].slice(0, MAX_WATCHED);
        if (JSON.stringify(next) !== JSON.stringify(store.tracked())) patchState(store, { tracked: next });
      },
      async loadObjects(): Promise<void> {
        patchState(store, { objectsError: null });
        try {
          patchState(store, { objects: await backend.counterObjects() });
        } catch (e) {
          patchState(store, { objectsError: toCommandError(e) });
        }
      },
      async open(name: string): Promise<void> {
        patchState(store, { openError: null });
        try {
          patchState(store, { opened: await backend.counterObject(name) });
        } catch (e) {
          patchState(store, { openError: toCommandError(e), opened: null });
        }
      },
      closeObject(): void {
        patchState(store, { opened: null, openError: null });
      },
      /** Searches the catalog; an empty query goes back to the object list. Older answers never overwrite newer ones. */
      async search(query: string): Promise<void> {
        const q = query.trim();
        patchState(store, { query, searchError: null });
        const token = ++searchToken;
        if (!q) {
          patchState(store, { hits: null });
          return;
        }
        try {
          const hits = await backend.counterSearch(q, SEARCH_LIMIT);
          if (token === searchToken) patchState(store, { hits });
        } catch (e) {
          if (token === searchToken) patchState(store, { searchError: toCommandError(e), hits: null });
        }
      },
      async watch(path: string): Promise<void> {
        patchState(store, { watching: withPath(store.watching(), path, MAX_WATCHED) });
        await tick();
      },
      async unwatch(path: string): Promise<void> {
        patchState(store, { watching: withoutPath(store.watching(), path) });
        if (store.watching().length === 0) patchState(store, { history: null });
        else await tick();
      },
      async setWatching(paths: string[]): Promise<void> {
        patchState(store, { watching: [...new Set(paths)].slice(0, MAX_WATCHED), history: null });
        await tick();
      },
      async setMinutes(minutes: number): Promise<void> {
        patchState(store, { minutes });
        await tick();
      },
      isPinned(path: string): boolean {
        return store.pinned().includes(path);
      },
      /** Pins or unpins a counter (kept in settings, so it survives a restart). */
      async togglePin(path: string): Promise<boolean> {
        if (store.pinned().includes(path)) return saveCounters((c) => ({ ...c, pinned: withoutPath(c.pinned, path) }), 'Counter unpinned');
        if (store.pinned().length >= MAX_PINNED) {
          notifier.show(`You can pin up to ${MAX_PINNED} counters. Unpin one first.`, 'bad');
          return false;
        }
        return saveCounters((c) => ({ ...c, pinned: withPath(c.pinned, path, MAX_PINNED) }), 'Counter pinned');
      },
      /** Saves the charted counters as a named set (the same name replaces the set). */
      async saveSet(name: string): Promise<boolean> {
        const clean = name.trim();
        const paths = store.watching();
        if (!clean || clean.length > MAX_SET_NAME || paths.length === 0) return false;
        if (!store.sets().some((x) => x.name === clean) && store.sets().length >= MAX_SETS) {
          notifier.show(`You can keep up to ${MAX_SETS} sets. Delete one first.`, 'bad');
          return false;
        }
        return saveCounters((c) => ({ ...c, sets: [...c.sets.filter((x) => x.name !== clean), { name: clean, paths }] }), `Saved the set “${clean}”`);
      },
      /** Deletes a set; the toast offers Undo (the set is saved again as it was). */
      async deleteSet(name: string): Promise<boolean> {
        const gone = store.sets().find((x) => x.name === name);
        const ok = await saveCounters((c) => ({ ...c, sets: c.sets.filter((x) => x.name !== name) }), null);
        if (ok && gone) {
          notifier.show(`Deleted the set “${name}”`, 'ok', { label: 'Undo', run: () => void saveCounters((c) => ({ ...c, sets: [...c.sets, gone] }), `Restored the set “${name}”`) });
        }
        return ok;
      },
      async applySet(name: string): Promise<void> {
        const set = store.sets().find((x) => x.name === name);
        if (set) await this.setWatching(set.paths);
      },
      /** Adds a counter to the dashboard as a graph widget (saved right away, like pinning). */
      async addToDashboard(path: string): Promise<boolean> {
        const s = settings.settings();
        const layout = s?.dashboard;
        if (!s || !layout) return false;
        if (layout.widgets.length >= MAX_WIDGETS) {
          notifier.show(`The dashboard holds up to ${MAX_WIDGETS} widgets. Remove one first.`, 'bad');
          return false;
        }
        const widget = newWidget('graph', { perfCounter: path }, layout.widgets);
        return settings.save({ ...s, dashboard: { ...layout, widgets: [...layout.widgets, widget] } }, 'Added to the dashboard');
      },
    };
  }),
);
