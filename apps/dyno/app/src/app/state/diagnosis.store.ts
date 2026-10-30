import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type { Anomaly, CommandError, Diagnosis } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';

/** Most earlier diagnoses kept for the Why slow page (this session only; the engine does not store them). */
export const MAX_DIAGNOSIS_HISTORY = 10;

interface DiagnosisState {
  diagnosis: Diagnosis | null;
  /** The diagnoses run since the app opened, newest first (the first is `diagnosis`). */
  history: Diagnosis[];
  running: boolean;
  error: CommandError | null;
  anomalies: Anomaly[];
  anomaliesLoaded: boolean;
  anomaliesError: CommandError | null;
}

/** "Why is my computer slow?" (the engine's diagnosis, run on demand) and the anomalies it found in the history. */
export const DiagnosisStore = signalStore(
  { providedIn: 'root' },
  withState<DiagnosisState>({ diagnosis: null, history: [], running: false, error: null, anomalies: [], anomaliesLoaded: false, anomaliesError: null }),
  withMethods((store, backend = inject(BACKEND)) => ({
    async run(): Promise<void> {
      patchState(store, { running: true, error: null });
      try {
        const diagnosis = await backend.whySlow();
        patchState(store, (s) => ({ diagnosis, history: [diagnosis, ...s.history].slice(0, MAX_DIAGNOSIS_HISTORY), running: false }));
      } catch (e) {
        patchState(store, { running: false, error: toCommandError(e) });
      }
    },
    /** Runs the diagnosis once (pages call it freely; the button re-runs it). */
    async ensure(): Promise<void> {
      if (!store.diagnosis() && !store.running()) await this.run();
    },
    async loadAnomalies(): Promise<void> {
      patchState(store, { anomaliesError: null });
      try {
        const anomalies = await backend.anomalies();
        patchState(store, { anomalies, anomaliesLoaded: true });
      } catch (e) {
        patchState(store, { anomaliesError: toCommandError(e) });
      }
    },
  })),
);
