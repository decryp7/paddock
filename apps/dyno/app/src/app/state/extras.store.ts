import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type {
  BootReport,
  ChangeTimeline,
  CommandError,
  FinderMode,
  FinderResult,
  GpuReport,
  InventoryFormat,
  MaintenanceSettings,
  MaintenanceStatus,
  SavedInventory,
  SleepReport,
  SoftwareInventory,
  UsageReport,
} from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { JobRunner, type JobProgress, type JobRun } from '../core/job-runner';
import { DEFAULT_MIN_MIB, mibToBytes } from '../features/extras/files-model';
import type { ExtrasTab } from '../features/extras/extras-model';
import { formatBytes } from '../shared/format';
import { Notifier } from '../shared/notifier';
import { fileName } from './diagnostics.store';
import { SettingsStore } from './settings.store';

/** One tab's data: nothing until it is first opened, then the engine's answer or its error (with Retry). */
export interface Resource<T> {
  data: T | null;
  error: CommandError | null;
  loading: boolean;
}

const idle = <T>(): Resource<T> => ({ data: null, error: null, loading: false });

interface ExtrasState {
  changes: Resource<ChangeTimeline>;
  /** The window of "What changed", in hours. */
  hours: number;
  sleep: Resource<SleepReport>;
  boot: Resource<BootReport>;
  apps: Resource<UsageReport>;
  gpu: Resource<GpuReport>;
  software: Resource<SoftwareInventory>;
  maintenance: Resource<MaintenanceStatus>;
  /** Saving the maintenance windows: the engine's refusal, shown next to the form. */
  maintenanceSaving: boolean;
  maintenanceError: CommandError | null;
  exporting: boolean;
  saved: SavedInventory | null;
  saveError: CommandError | null;
  // The file finder.
  folder: string | null;
  mode: FinderMode;
  minMib: number;
  finder: Resource<FinderResult>;
  /** The running search (a job): its phase, its count, and whether it has stood still for a while. */
  finderJob: JobProgress | null;
  /** The last search was stopped with Cancel; what was shown before stays. */
  finderCancelled: boolean;
}

type ResourceKey = 'changes' | 'sleep' | 'boot' | 'apps' | 'gpu' | 'software' | 'maintenance';

/** The tab that reads each resource; the Files tab and the Maintenance form have their own methods. */
const TAB_RESOURCE: Partial<Record<ExtrasTab, ResourceKey>> = {
  changes: 'changes',
  sleep: 'sleep',
  boot: 'boot',
  apps: 'apps',
  gpu: 'gpu',
  inventory: 'software',
  maintenance: 'maintenance',
};

/**
 * The Extras page: one read-only resource per tab, loaded when the tab is first opened, plus the file finder (search,
 * listing only) and the maintenance windows (saved through the settings store).
 */
export const ExtrasStore = signalStore(
  { providedIn: 'root' },
  withState<ExtrasState>({
    changes: idle<ChangeTimeline>(),
    hours: 72,
    sleep: idle<SleepReport>(),
    boot: idle<BootReport>(),
    apps: idle<UsageReport>(),
    gpu: idle<GpuReport>(),
    software: idle<SoftwareInventory>(),
    maintenance: idle<MaintenanceStatus>(),
    maintenanceSaving: false,
    maintenanceError: null,
    exporting: false,
    saved: null,
    saveError: null,
    folder: null,
    mode: 'both',
    minMib: DEFAULT_MIN_MIB,
    finder: idle<FinderResult>(),
    finderJob: null,
    finderCancelled: false,
  }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier), settings = inject(SettingsStore), jobs = inject(JobRunner)) => {
    /** Reads one resource; a slower answer for a request that was replaced does not overwrite the newer one. */
    const tokens: Partial<Record<ResourceKey | 'finder', number>> = {};
    async function read<T>(key: ResourceKey | 'finder', fetch: () => Promise<T>): Promise<void> {
      const mine = (tokens[key] = (tokens[key] ?? 0) + 1);
      const current = store[key]() as Resource<T>;
      patchState(store, { [key]: { ...current, loading: true, error: null } } as Partial<ExtrasState>);
      try {
        const data = await fetch();
        if (mine === tokens[key]) patchState(store, { [key]: { data, error: null, loading: false } } as Partial<ExtrasState>);
      } catch (e) {
        if (mine === tokens[key]) patchState(store, { [key]: { ...current, error: toCommandError(e), loading: false } } as Partial<ExtrasState>);
      }
    }

    /**
     * Searches the chosen folder as a job: the page shows the walk's growing count and then the comparison's progress, with
     * Cancel. A new search (another mode or size) first stops the running one. A cancelled search keeps the earlier result.
     */
    let finderRun: JobRun | null = null;
    const search = async (): Promise<void> => {
      const folder = store.folder();
      if (!folder) return;
      const previous = finderRun;
      const mine = (tokens['finder'] = (tokens['finder'] ?? 0) + 1);
      if (previous) {
        await previous.cancel().catch(() => undefined);
        await previous.result.catch(() => undefined);
      }
      if (mine !== tokens['finder']) return;
      const current = store.finder();
      patchState(store, { finder: { ...current, loading: true, error: null }, finderJob: null, finderCancelled: false });
      const run = (finderRun = jobs.run(
        { findFiles: { query: { folder, mode: store.mode(), minBytes: mibToBytes(store.minMib()) } } },
        (finderJob) => {
          if (mine === tokens['finder']) patchState(store, { finderJob });
        },
      ));
      try {
        const out = await run.result;
        if (mine !== tokens['finder']) return;
        if (out === null) patchState(store, { finder: { ...current, loading: false }, finderCancelled: true });
        else if ('finder' in out) patchState(store, { finder: { data: out.finder, error: null, loading: false } });
      } catch (e) {
        if (mine === tokens['finder']) patchState(store, { finder: { ...current, error: toCommandError(e), loading: false } });
      } finally {
        if (mine === tokens['finder']) {
          finderRun = null;
          patchState(store, { finderJob: null });
        }
      }
    };

    return {
      loadChanges: () => read('changes', () => backend.changes({ hours: store.hours() })),
      loadSleep: () => read('sleep', () => backend.sleepBlockers()),
      loadBoot: () => read('boot', () => backend.bootAnalysis()),
      loadApps: () => read('apps', () => backend.appUsage()),
      loadGpu: () => read('gpu', () => backend.gpuProcesses()),
      loadSoftware: () => read('software', () => backend.softwareInventory()),
      loadMaintenance: () => read('maintenance', () => backend.maintenance()),

      /** Reads a tab the first time it opens (later visits show what is there; Retry and the refresh buttons read again). */
      async ensure(tab: ExtrasTab): Promise<void> {
        const key = TAB_RESOURCE[tab];
        if (!key) return;
        const r = store[key]() as Resource<unknown>;
        if (r.data !== null || r.loading) return;
        await this.reload(tab);
      },
      async reload(tab: ExtrasTab): Promise<void> {
        switch (tab) {
          case 'changes':
            return this.loadChanges();
          case 'sleep':
            return this.loadSleep();
          case 'boot':
            return this.loadBoot();
          case 'apps':
            return this.loadApps();
          case 'gpu':
            return this.loadGpu();
          case 'inventory':
            return this.loadSoftware();
          case 'maintenance':
            return this.loadMaintenance();
          case 'files':
            return search();
        }
      },

      async setHours(hours: number): Promise<void> {
        if (hours === store.hours()) return;
        patchState(store, { hours });
        await this.loadChanges();
      },

      // ------------------------------------------------------------------------------------------ files
      /** Asks for a folder through the shell's dialog and searches it; a cancelled dialog changes nothing. */
      async chooseFolder(): Promise<void> {
        try {
          const folder = await backend.pickFolder();
          if (!folder) return;
          patchState(store, { folder });
          await search();
        } catch (e) {
          patchState(store, { finder: { ...store.finder(), error: toCommandError(e) } });
        }
      },
      search,
      /** Stops the search in progress; the earlier result stays. */
      async cancelSearch(): Promise<void> {
        try {
          await finderRun?.cancel();
        } catch (e) {
          patchState(store, { finder: { ...store.finder(), error: toCommandError(e) } });
        }
      },
      async setMode(mode: FinderMode): Promise<void> {
        if (mode === store.mode()) return;
        patchState(store, { mode });
        await search();
      },
      async setMinMib(minMib: number): Promise<void> {
        if (minMib === store.minMib()) return;
        patchState(store, { minMib });
        await search();
      },
      // -------------------------------------------------------------------------------------- inventory
      /** Shows the OS save dialog through the shell and writes the report; a cancelled dialog changes nothing. */
      async exportReport(format: InventoryFormat): Promise<void> {
        if (store.exporting()) return;
        patchState(store, { exporting: true, saveError: null });
        try {
          const saved = await backend.saveInventoryReport(format);
          patchState(store, { exporting: false });
          if (saved) {
            patchState(store, { saved });
            notifier.show(`Saved ${fileName(saved.path)} (${formatBytes(saved.bytes)})`);
          }
        } catch (e) {
          patchState(store, { exporting: false, saveError: toCommandError(e) });
        }
      },

      // ------------------------------------------------------------------------------------- maintenance
      /** Saves the windows through the settings (the engine validates them); true when saved. */
      async saveMaintenance(next: MaintenanceSettings): Promise<boolean> {
        if (store.maintenanceSaving()) return false;
        if (!settings.settings()) await settings.load();
        const current = settings.settings();
        if (!current) {
          patchState(store, { maintenanceError: settings.error() ?? { code: 'internal', message: 'The settings could not be read.' } });
          return false;
        }
        patchState(store, { maintenanceSaving: true, maintenanceError: null });
        const ok = await settings.save({ ...current, maintenance: next }, 'Maintenance windows saved');
        patchState(store, { maintenanceSaving: false, maintenanceError: ok ? null : settings.saveError() });
        if (ok) await this.loadMaintenance();
        return ok;
      },
    };
  }),
);
