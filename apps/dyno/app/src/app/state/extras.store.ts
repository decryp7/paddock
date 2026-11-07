import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
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
import type { JobOutput, JobSpec } from '../core/api.types';
import { JobStarter, progressOf, type JobRun } from '../core/job-starter';
import { JobsStore } from '@decryp7/paddock-ui/shell/jobs';
import { DEFAULT_MIN_MIB, mibToBytes } from '../features/extras/files-model';
import type { ExtrasTab } from '../features/extras/extras-model';
import { fileName, formatStorage } from '../shared/format';
import { Notifier } from '../shared/notifier';
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
  /** The running search's job id (its progress is the shell's job status). */
  finderJobId: number | null;
  /** The running read of each tab that runs OS tools (a job each), by the tab's resource. */
  readJobIds: Partial<Record<ResourceKey, number>>;
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
    finderJobId: null,
    readJobIds: {},
    finderCancelled: false,
  }),
  withComputed(({ finderJobId, readJobIds }, jobs = inject(JobsStore)) => ({
    /** The running search: its phase, its count, and whether it has been quiet long enough to say "Still working". */
    finderJob: computed(() => progressOf(jobs, finderJobId())),
    /** The running read of the tab `key` (sleep, boot, apps or software), or null. */
    readJob: computed(() => (key: ResourceKey) => progressOf(jobs, readJobIds()[key] ?? null)),
  })),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier), settings = inject(SettingsStore), starter = inject(JobStarter)) => {
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
    /** The tabs that run OS tools read through a job each, so a slow tool shows progress and can be cancelled. */
    const READ_JOBS = {
      sleep: ['sleepBlockers', (o: JobOutput) => ('sleep' in o ? o.sleep : null)],
      boot: ['bootAnalysis', (o: JobOutput) => ('boot' in o ? o.boot : null)],
      apps: ['appUsage', (o: JobOutput) => ('appUsage' in o ? o.appUsage : null)],
      software: ['softwareInventory', (o: JobOutput) => ('software' in o ? o.software : null)],
    } as const;
    const readRuns: Partial<Record<ResourceKey, JobRun>> = {};
    /** Runs the job of tab `key` and returns what it produced; a cancelled read says so (the tab shows it with Retry). */
    async function readJob<K extends keyof typeof READ_JOBS>(key: K): Promise<NonNullable<ReturnType<(typeof READ_JOBS)[K][1]>>> {
      const [spec, pick] = READ_JOBS[key] as readonly [JobSpec, (o: JobOutput) => unknown];
      const run = (readRuns[key] = starter.run(spec, (id) => patchState(store, { readJobIds: { ...store.readJobIds(), [key]: id } })));
      try {
        const out = await run.result;
        const value = out ? pick(out) : null;
        if (value === null) throw { code: 'internal', message: 'Stopped before it finished. Retry to read it again.' } satisfies CommandError;
        return value as never;
      } finally {
        delete readRuns[key];
        const { [key]: _gone, ...rest } = store.readJobIds();
        patchState(store, { readJobIds: rest });
      }
    }

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
      patchState(store, { finder: { ...current, loading: true, error: null }, finderJobId: null, finderCancelled: false });
      const run = (finderRun = starter.run(
        { findFiles: { query: { folder, mode: store.mode(), minBytes: mibToBytes(store.minMib()) } } },
        (finderJobId) => {
          if (mine === tokens['finder']) patchState(store, { finderJobId });
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
          patchState(store, { finderJobId: null });
        }
      }
    };

    return {
      loadChanges: () => read('changes', () => backend.changes({ hours: store.hours() })),
      loadSleep: () => read('sleep', () => readJob('sleep')),
      loadBoot: () => read('boot', () => readJob('boot')),
      loadApps: () => read('apps', () => readJob('apps')),
      loadGpu: () => read('gpu', () => backend.gpuProcesses()),
      loadSoftware: () => read('software', () => readJob('software')),
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
      /** Stops the read of the tab `key` that runs OS tools; the tab shows that it stopped, with Retry. */
      async cancelRead(key: ResourceKey): Promise<void> {
        await readRuns[key]?.cancel();
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
            notifier.show(`Saved ${fileName(saved.path)} (${formatStorage(saved.bytes)})`);
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
