import { inject } from '@angular/core';
import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import type {
  BootReport,
  ChangeTimeline,
  CleanupOutcome,
  CommandError,
  DeleteMode,
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
import { DEFAULT_MIN_MIB, defaultSelection, mibToBytes } from '../features/extras/files-model';
import type { ExtrasTab } from '../features/extras/extras-model';
import { formatBytes, plural } from '../shared/format';
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
  /** File ids chosen for removal. */
  selected: string[];
  removing: boolean;
  removeError: CommandError | null;
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
 * selection, removal through the cleanup policy) and the maintenance windows (saved through the settings store).
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
    selected: [],
    removing: false,
    removeError: null,
  }),
  withMethods((store, backend = inject(BACKEND), notifier = inject(Notifier), settings = inject(SettingsStore)) => {
    /** Reads one resource; a slower answer for a request that was replaced does not overwrite the newer one. */
    const tokens: Partial<Record<ResourceKey | 'finder', number>> = {};
    async function read<T>(key: ResourceKey | 'finder', fetch: () => Promise<T>, after?: (data: T) => Partial<ExtrasState>): Promise<void> {
      const mine = (tokens[key] = (tokens[key] ?? 0) + 1);
      const current = store[key]() as Resource<T>;
      patchState(store, { [key]: { ...current, loading: true, error: null } } as Partial<ExtrasState>);
      try {
        const data = await fetch();
        if (mine === tokens[key]) patchState(store, { [key]: { data, error: null, loading: false }, ...(after?.(data) ?? {}) } as Partial<ExtrasState>);
      } catch (e) {
        if (mine === tokens[key]) patchState(store, { [key]: { ...current, error: toCommandError(e), loading: false } } as Partial<ExtrasState>);
      }
    }

    const search = (): Promise<void> => {
      const folder = store.folder();
      if (!folder) return Promise.resolve();
      return read('finder', () => backend.findFiles({ folder, mode: store.mode(), minBytes: mibToBytes(store.minMib()) }), (r) => ({
        selected: [...defaultSelection(r)],
        removeError: null,
      }));
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
      toggleFile(id: string): void {
        const now = store.selected();
        patchState(store, { selected: now.includes(id) ? now.filter((x) => x !== id) : [...now, id] });
      },
      /** Back to the suggestion: every copy but the oldest, nothing else. */
      resetSelection(): void {
        const r = store.finder().data;
        patchState(store, { selected: r ? [...defaultSelection(r)] : [] });
      },
      clearSelection(): void {
        patchState(store, { selected: [] });
      },
      /** Removes the selected files through the cleanup policy; called only after the person confirmed. */
      async remove(mode: DeleteMode): Promise<CleanupOutcome | null> {
        const result = store.finder().data;
        if (!result || store.removing() || store.selected().length === 0) return null;
        patchState(store, { removing: true, removeError: null });
        try {
          const outcome = await backend.removeFoundFiles(result.scanId, store.selected(), mode);
          patchState(store, { removing: false });
          notifier.show(
            outcome.deleted ? `Moved ${plural(outcome.deleted, 'file')} (${formatBytes(outcome.freedBytes)})` : 'Nothing was moved',
            outcome.failed.length ? 'warn' : 'ok',
          );
          await search();
          return outcome;
        } catch (e) {
          patchState(store, { removing: false, removeError: toCommandError(e) });
          return null;
        }
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
