import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withHooks, withMethods, withState } from '@ngrx/signals';
import type { CommandError, DiskChildren, DiskNode, DiskScanStatus, DiskTarget, RingNode, Volume } from '../core/api.types';
import { BACKEND, type Unsubscribe } from '../core/backend';
import { toCommandError } from '../core/command-error';
import { type Crumb, capacityStrip, chainTo, levelRows, outcome, volumeGroup } from '../features/disk-map/disk-map-model';

export type DiskView = 'chart' | 'table';

/** Rows asked for per page (the engine's maximum). */
export const PAGE = 256;
/** The chart and the list fold children under this share of the folder into "smaller items"; the table shows everything. */
export const FOLD_BELOW = 0.005;
/** Rings the chart draws below the centre, the share of the centre under which a folder is folded, and the node cap of one answer. */
export const RINGS = 4;
export const RING_MIN_SHARE = 0.005;
export const RING_MAX_NODES = 1500;

interface DiskMapState {
  volumes: Volume[];
  volumesLoaded: boolean;
  volumesError: CommandError | null;
  /** The engine's kept scans have been asked for once (until then a stored result may still be coming). */
  restored: boolean;
  /** Every scan the engine keeps, newest status each. */
  kept: DiskScanStatus[];
  /** The volume the page is about (its id), or null for a folder scan. */
  selectedVolumeId: string | null;
  /** The scan on screen. */
  status: DiskScanStatus | null;
  startError: CommandError | null;
  starting: boolean;
  /** From the scan's root to the centre; the last is the centre. */
  trail: Crumb[];
  level: DiskChildren | null;
  /** The rings under the centre (one request: the levels the chart draws), or null for the table or before the first answer. */
  rings: RingNode | null;
  /** Why the rings could not be read (the list still works). */
  ringsError: CommandError | null;
  /** The scan's root folder node id, once read. */
  rootId: number | null;
  levelError: CommandError | null;
  levelLoading: boolean;
  view: DiskView;
}

const initial: DiskMapState = {
  volumes: [],
  volumesLoaded: false,
  volumesError: null,
  restored: false,
  kept: [],
  selectedVolumeId: null,
  status: null,
  startError: null,
  starting: false,
  trail: [],
  level: null,
  rings: null,
  ringsError: null,
  rootId: null,
  levelError: null,
  levelLoading: false,
  view: 'chart',
};

/**
 * The disk map: the volumes, the scan on screen (a job in the engine, pushed as status events), and one level of its tree at a
 * time. The engine owns every size and state; this store asks for the level under the centre and applies what comes back.
 */
export const DiskMapStore = signalStore(
  { providedIn: 'root' },
  withState(initial),
  withComputed((s) => ({
    centre: computed(() => s.trail().at(-1) ?? null),
    outcome: computed(() => (s.status() ? outcome(s.status()!.state) : null)),
    selectedVolume: computed(() => s.volumes().find((v) => v.id === s.selectedVolumeId()) ?? null),
    /** The volume of the scan on screen, else the selected one. */
    volume: computed(() => s.status()?.volume ?? s.volumes().find((v) => v.id === s.selectedVolumeId()) ?? null),
    /** The folders above the centre, root first, for the chart's way back up. */
    ancestors: computed(() => s.trail().slice(0, -1).map((c) => ({ id: c.node ?? s.rootId() ?? 0, name: c.name, bytes: c.bytes }))),
    rows: computed(() => (s.level() ? levelRows(s.level()!) : [])),
    capacity: computed(() => {
      const st = s.status();
      const vol = st?.volume ?? s.volumes().find((v) => v.id === s.selectedVolumeId()) ?? null;
      return st ? capacityStrip(vol, st.progress, st.state, st.hiddenBytes) : vol ? capacityStrip(vol, { bytes: 0, items: 0, folders: 0, totalBytes: null, unreadable: 0 }, 'cancelled') : null;
    }),
  })),
  withMethods((store, backend = inject(BACKEND)) => {
    let token = 0;
    const keep = (status: DiskScanStatus) =>
      patchState(store, (s) => ({ kept: [...s.kept.filter((k) => k.scan !== status.scan), status].sort((a, b) => a.scan - b.scan) }));

    /** Reads the centre's level for the list and, in the chart view, its rings in one more request; an older answer never replaces a newer one. */
    const refresh = async (): Promise<void> => {
      const status = store.status();
      const centre = store.centre();
      if (!status || !centre) return;
      const mine = ++token;
      const table = store.view() === 'table';
      patchState(store, { levelLoading: true });
      try {
        let ringsError: CommandError | null = null;
        const [level, rings] = await Promise.all([
          backend.diskChildren({ scan: status.scan, node: centre.node, offset: 0, limit: PAGE, minFraction: table ? 0 : FOLD_BELOW }),
          table ? Promise.resolve(null) : backend.diskRings({ scan: status.scan, node: centre.node, rings: RINGS, minShare: RING_MIN_SHARE, maxNodes: RING_MAX_NODES }).then((r) => r.root).catch((e) => {
                ringsError = toCommandError(e);
                return null;
              }),
        ]);
        if (mine === token) {
          patchState(store, (st) => ({
            level,
            rings,
            ringsError,
            rootId: st.trail.length === 1 ? level.node.id : st.rootId,
            trail: st.trail.map((c, i) => (i === st.trail.length - 1 ? { ...c, bytes: level.node.bytes } : c)),
            levelError: null,
          }));
        }
      } catch (e) {
        if (mine === token) patchState(store, { levelError: toCommandError(e) });
      } finally {
        if (mine === token) patchState(store, { levelLoading: false });
      }
    };

    const show = (status: DiskScanStatus | null) => {
      token++;
      patchState(store, { status, trail: status ? [{ node: null, name: status.name, bytes: 0 }] : [], level: null, rings: null, ringsError: null, rootId: null, levelError: null, levelLoading: false });
      return refresh();
    };

    /** Starts a scan (a job; resolves at once) and shows it. */
    const scan = async (target: DiskTarget): Promise<boolean> => {
      if (store.starting()) return false;
      patchState(store, { starting: true, startError: null });
      try {
        const status = await backend.startDiskScan(target);
        keep(status);
        await show(status);
        return true;
      } catch (e) {
        patchState(store, { startError: toCommandError(e) });
        return false;
      } finally {
        patchState(store, { starting: false });
      }
    };

    const goTo = async (index: number): Promise<void> => {
      if (index < 0 || index >= store.trail().length - 1) return;
      patchState(store, (s) => ({ trail: s.trail.slice(0, index + 1) }));
      await refresh();
    };

    return {
      /** The volumes mounted now changed (pushed by the engine): keeps the choice when its volume is still there. */
      volumesChanged(volumes: Volume[]): void {
        patchState(store, (s) => ({
          volumes,
          volumesLoaded: true,
          selectedVolumeId:
            s.selectedVolumeId === null || volumes.some((v) => v.id === s.selectedVolumeId) ? s.selectedVolumeId : (volumes.find((v) => volumeGroup(v) === 'internal') ?? volumes[0])?.id ?? null,
        }));
      },
      async loadVolumes(): Promise<void> {
        patchState(store, { volumesError: null });
        try {
          const volumes = await backend.diskVolumes();
          patchState(store, (s) => ({
            volumes,
            volumesLoaded: true,
            selectedVolumeId: s.selectedVolumeId ?? (volumes.find((v) => volumeGroup(v) === 'internal') ?? volumes[0])?.id ?? null,
          }));
        } catch (e) {
          patchState(store, { volumesError: toCommandError(e) });
        }
      },
      /** Adopts the newest scan the engine kept for the selected volume (a window that opens late asks once). */
      async restore(): Promise<void> {
        try {
          const kept = await backend.diskScans();
          patchState(store, { kept });
          if (store.status()) return;
          const latest = [...kept].reverse().find((k) => !store.selectedVolumeId() || (k.volume?.id ?? null) === store.selectedVolumeId()) ?? null;
          if (latest) await show(latest);
        } catch {
          // Nothing kept is shown as "not scanned"; a scan can still be started.
        } finally {
          patchState(store, { restored: true });
        }
      },
      /** Makes a volume the one the page is about, showing its kept scan when there is one. */
      async selectVolume(id: string): Promise<void> {
        patchState(store, { selectedVolumeId: id, startError: null });
        const kept = [...store.kept()].reverse().find((k) => k.volume?.id === id) ?? null;
        await show(kept);
      },
      /** Asks for a folder in the OS dialog and scans it; false when nothing was chosen or the scan did not start. */
      async chooseFolder(): Promise<boolean> {
        try {
          const path = await backend.pickFolder();
          if (!path) return false;
          patchState(store, { selectedVolumeId: null });
          return await scan({ folder: { path } });
        } catch (e) {
          patchState(store, { startError: toCommandError(e) });
          return false;
        }
      },
      scan,
      /** Scans the same target again (Rescan, Retry). */
      async rescan(): Promise<boolean> {
        const s = store.status();
        const id = store.selectedVolumeId();
        return scan(s?.target ?? (id ? { volume: { id } } : { folder: { path: '/' } }));
      },
      async cancel(): Promise<void> {
        const s = store.status();
        if (!s) return;
        try {
          await backend.cancelDiskScan(s.scan);
        } catch (e) {
          patchState(store, { startError: toCommandError(e) });
        }
      },
      /** A pushed status: the highest `seq` of the scan on screen wins, and the level follows what was counted. */
      receive(status: DiskScanStatus): void {
        const known = store.kept().find((k) => k.scan === status.scan);
        if (known && known.seq >= status.seq) return;
        keep(status);
        const shown = store.status();
        if (!shown || shown.scan !== status.scan) return;
        patchState(store, { status });
        void refresh();
      },
      refresh,
      async setView(view: DiskView): Promise<void> {
        if (store.view() === view) return;
        patchState(store, { view });
        await refresh();
      },
      /** Makes a folder the centre. */
      async open(node: DiskNode): Promise<void> {
        if (node.kind !== 'folder' || !node.hasChildren) return;
        patchState(store, (s) => ({ trail: [...s.trail, { node: node.id, name: node.name, bytes: node.bytes }] }));
        await refresh();
      },
      /** Goes up to the crumb at `index` (0 is the scan's root). */
      goTo,
      /** Makes the folder with this node id the centre, opening every folder on the way (a click on an outer ring). */
      async openTo(node: number): Promise<void> {
        const rings = store.rings();
        if (!rings || rings.node.id === node) return;
        const chain = chainTo(rings, node);
        if (!chain) return;
        patchState(store, (s) => ({ trail: [...s.trail, ...chain.map((c) => ({ node: c.id, name: c.name, bytes: c.bytes }))] }));
        await refresh();
      },
      async up(): Promise<void> {
        await goTo(store.trail().length - 2);
      },
      async root(): Promise<void> {
        await goTo(0);
      },
      /** The next page of the level. */
      async loadMore(): Promise<void> {
        const status = store.status();
        const level = store.level();
        const centre = store.centre();
        if (!status || !level || !centre || level.nextOffset === null) return;
        const mine = token;
        try {
          const next = await backend.diskChildren({ scan: status.scan, node: centre.node, offset: level.nextOffset, limit: PAGE, minFraction: store.view() === 'table' ? 0 : FOLD_BELOW });
          if (mine === token) patchState(store, { level: { ...next, children: [...level.children, ...next.children], offset: 0 } });
        } catch (e) {
          patchState(store, { levelError: toCommandError(e) });
        }
      },
      /** The full path of a node, for Copy path. */
      async pathOf(node: number): Promise<string | null> {
        const status = store.status();
        if (!status) return null;
        return backend.diskPath(status.scan, node);
      },
      /** Shows a node in Finder, Explorer or the file manager. */
      async reveal(node: number): Promise<boolean> {
        const status = store.status();
        if (!status) return false;
        try {
          await backend.diskReveal(status.scan, node);
          return true;
        } catch (e) {
          patchState(store, { startError: toCommandError(e) });
          return false;
        }
      },
      /** Opens the Full Disk Access pane (macOS). */
      async openFullDiskAccessSettings(): Promise<void> {
        try {
          await backend.openFullDiskAccessSettings();
        } catch (e) {
          patchState(store, { startError: toCommandError(e) });
        }
      },
      /** Reads again only the folders the scan could not open; the scan runs again. */
      async readAgain(): Promise<boolean> {
        const s = store.status();
        if (!s) return false;
        try {
          const status = await backend.retryDiskScan(s.scan);
          keep(status);
          await show(status);
          return true;
        } catch (e) {
          patchState(store, { startError: toCommandError(e) });
          return false;
        }
      },
      dismissStartError(): void {
        patchState(store, { startError: null });
      },
      reset(): void {
        token++;
        patchState(store, initial);
      },
    };
  }),
  withHooks((store) => {
    const backend = inject(BACKEND);
    const stops: Unsubscribe[] = [];
    return {
      onInit() {
        stops.push(backend.onDiskScan((s) => store.receive(s)), backend.onDiskVolumes((v) => store.volumesChanged(v)));
      },
      onDestroy() {
        stops.forEach((stop) => stop());
      },
    };
  }),
);
