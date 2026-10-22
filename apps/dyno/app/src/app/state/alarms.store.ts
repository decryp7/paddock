import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import type { Alarm, AlarmChange, AlarmFilter, AlarmStats, CommandError, Settings, ShelveFor } from '../core/api.types';
import { BACKEND, type Unsubscribe } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { mergeAlarms, needsAcknowledge, prependChange, sortOpen } from '../features/alarms/alarms-model';
import { Notifier } from '../shared/notifier';
import { SettingsStore } from './settings.store';

/** Every open alarm, shelved ones included (the page marks them). */
export const OPEN_FILTER: AlarmFilter = { states: [], minSeverity: null, rule: null, includeShelved: true, limit: 200 };
export const HISTORY_LIMIT = 200;
const DETAIL_HISTORY_LIMIT = 50;

interface AlarmsState {
  alarms: Alarm[];
  loaded: boolean;
  error: CommandError | null;
  history: AlarmChange[];
  historyLoaded: boolean;
  historyError: CommandError | null;
  stats: AlarmStats | null;
  statsError: CommandError | null;
  selectedId: number | null;
  detailHistory: AlarmChange[];
  saving: boolean;
}

/**
 * Alarms: the open list, the change log, the statistics and the selected alarm. The engine owns every state change; this store
 * only calls it and applies what comes back (and what the agent pushes) so the table stays current.
 */
export const AlarmsStore = signalStore(
  { providedIn: 'root' },
  withState<AlarmsState>({
    alarms: [],
    loaded: false,
    error: null,
    history: [],
    historyLoaded: false,
    historyError: null,
    stats: null,
    statsError: null,
    selectedId: null,
    detailHistory: [],
    saving: false,
  }),
  withComputed((s) => ({
    unacknowledged: computed(() => s.alarms().filter(needsAcknowledge)),
    selected: computed(() => s.alarms().find((a) => a.id === s.selectedId()) ?? null),
    mutedRules: computed(() => new Set(s.alarms().filter((a) => a.muted).map((a) => a.rule))),
  })),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier), settings = inject(SettingsStore)) => {
    const apply = (changed: Alarm[]) => patchState(store, (s) => ({ alarms: mergeAlarms(s.alarms, changed) }));
    /** Runs an engine command, applies the alarms it answers, and tells the user when it fails. */
    const run = async (command: () => Promise<Alarm[]>): Promise<boolean> => {
      try {
        apply(await command());
        return true;
      } catch (e) {
        notifier.error(toCommandError(e));
        return false;
      }
    };
    const loadDetailHistory = async (id: number | null): Promise<void> => {
      if (id === null) return patchState(store, { detailHistory: [] });
      try {
        const detailHistory = await backend.alarmHistory(id, DETAIL_HISTORY_LIMIT);
        if (store.selectedId() === id) patchState(store, { detailHistory });
      } catch {
        if (store.selectedId() === id) patchState(store, { detailHistory: [] });
      }
    };
    return {
      async load(): Promise<void> {
        patchState(store, { error: null });
        try {
          patchState(store, { alarms: sortOpen(await backend.alarms(OPEN_FILTER)), loaded: true });
        } catch (e) {
          patchState(store, { error: toCommandError(e) });
        }
      },
      async loadHistory(): Promise<void> {
        patchState(store, { historyError: null });
        try {
          patchState(store, { history: await backend.alarmHistory(null, HISTORY_LIMIT), historyLoaded: true });
        } catch (e) {
          patchState(store, { historyError: toCommandError(e) });
        }
      },
      async loadStats(): Promise<void> {
        patchState(store, { statsError: null });
        try {
          patchState(store, { stats: await backend.alarmStats() });
        } catch (e) {
          patchState(store, { statsError: toCommandError(e) });
        }
      },
      /** Selects an alarm for the detail panel (null closes it) and fetches its change log. */
      async select(id: number | null): Promise<void> {
        patchState(store, { selectedId: id, detailHistory: [] });
        await loadDetailHistory(id);
      },
      acknowledge: (ids: number[]) => (ids.length === 0 ? Promise.resolve(true) : run(() => backend.acknowledgeAlarms(ids))),
      acknowledgeAll: () => run(() => backend.acknowledgeAlarms(store.unacknowledged().map((a) => a.id))),
      shelve: (id: number, until: ShelveFor) => run(() => backend.shelveAlarm(id, until)),
      unshelve: (id: number) => run(() => backend.unshelveAlarm(id)),
      mute: (rule: string, muted: boolean) => run(() => backend.muteRule(rule, muted)),
      note: (id: number, text: string) => run(() => backend.noteAlarm(id, text)),
      /** Applies a pushed change: the open list, the log and the selected alarm's log. */
      receive(c: AlarmChange): void {
        apply([c.alarm]);
        patchState(store, (s) => ({
          history: s.historyLoaded ? prependChange(s.history, c, HISTORY_LIMIT) : s.history,
          detailHistory: s.selectedId === c.alarm.id ? prependChange(s.detailHistory, c, DETAIL_HISTORY_LIMIT) : s.detailHistory,
        }));
      },
      /**
       * Saves the whole settings with `change` applied; the engine validates. Answers its error (for inline display) or null
       * when it was saved and the settings store holds the stored copy.
       */
      async updateSettings(change: (s: Settings) => Settings): Promise<CommandError | null> {
        const current = settings.settings();
        if (!current) return { code: 'internal', message: 'Settings are not loaded yet. Try again in a moment.' };
        patchState(store, { saving: true });
        try {
          await backend.saveSettings(change(current));
          await settings.load();
          return null;
        } catch (e) {
          return toCommandError(e);
        } finally {
          patchState(store, { saving: false });
        }
      },
    };
  }),
  withHooks((store) => {
    const backend = inject(BACKEND);
    let stop: Unsubscribe | null = null;
    return {
      onInit() {
        stop = backend.onAlarm((c) => store.receive(c));
      },
      onDestroy() {
        stop?.();
      },
    };
  }),
);
