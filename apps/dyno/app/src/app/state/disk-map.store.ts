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
const DAY = 86_400_000;

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
  /**
   * A Rescan in progress (ADR 0029, D9): the new pass `scan` replaces the map `of`, which stays on screen until the pass is
   * done. Cancel or a failure keeps the old map.
   */
  refreshing: { scan: number; of: number } | null;
  /** The Rescan that failed, so the page says so next to the map that was kept. */
  refreshFailure: DiskScanStatus | null;
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
  refreshing: null,
  refreshFailure: null,
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
    /** The status of the Rescan in progress (its numbers are the strip's), else null. */
    refreshStatus: computed(() => {
      const r = s.refreshing();
      return r ? (s.kept().find((k) => k.scan === r.scan) ?? null) : null;
    }),
    /** Whether the page shows the map a Rescan will replace (so it shows the Rescan's strip, not the growing tree). */
    refreshingThis: computed(() => s.refreshing() !== null && s.refreshing()!.of === s.status()?.scan),
    capacity: computed(() => {
      const st = s.status();
      const vol = st?.volume ?? s.volumes().find((v) => v.id === s.selectedVolumeId()) ?? null;
      return st ? capacityStrip(vol, st.progress, st.state, st.hiddenBytes) : vol ? capacityStrip(vol, { bytes: 0, items: 0, folders: 0, totalBytes: null, unreadable: 0 }, 'cancelled') : null;
    }),
  })),
  withMethods((store, backend = inject(BACKEND)) => {
    let token = 0;
    let opened = false;
    const keep = (status: DiskScanStatus) =>
      patchState(store, (s) => ({ kept: [...s.kept.filter((k) => k.scan !== status.scan), status].sort((a, b) => a.scan - b.scan) }));
    /**
     * The status a reply to a start or retry stands for: the reply, unless a newer status of the same scan was pushed while the
     * request was out. Replies and pushed statuses travel apart, so the last status of a short pass can arrive before the reply.
     * Only what was pushed during the request counts (`before` is the kept list when it was sent): an agent that started again
     * numbers its scans from 1, and an old scan with the same number is not newer.
     */
    const adopt = (reply: DiskScanStatus, before: readonly DiskScanStatus[]): DiskScanStatus => {
      const pushed = store.kept().find((k) => k.scan === reply.scan);
      if (pushed && pushed.seq > reply.seq && !before.includes(pushed)) return pushed;
      keep(reply);
      return reply;
    };

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

    /** A pushed status during a scan asks for a fresh read: at most one read in flight and one waiting, however fast the scan reports. */
    let reading = false;
    let pending = false;
    const refreshSoon = async (): Promise<void> => {
      if (reading) {
        pending = true;
        return;
      }
      reading = true;
      try {
        do {
          pending = false;
          await refresh();
        } while (pending);
      } finally {
        reading = false;
      }
    };

    const show = (status: DiskScanStatus | null) => {
      token++;
      patchState(store, { status, trail: status ? [{ node: null, name: status.name, bytes: 0 }] : [], level: null, rings: null, ringsError: null, rootId: null, levelError: null, levelLoading: false });
      return refresh();
    };

    /** What happens when the Rescan's pass ends: done swaps to it, cancelled or failed keeps the map that was on screen. */
    const settleRefresh = (status: DiskScanStatus): void => {
      const r = store.refreshing();
      if (!r || status.scan !== r.scan) return;
      const how = outcome(status.state);
      if (how === 'running') return;
      patchState(store, { refreshing: null, refreshFailure: how === 'failed' ? status : null });
      const shown = store.status();
      if (how === 'done') {
        // Showing the growing tree already: it just finished, keep the place in it. Otherwise swap to the new map.
        if (shown?.scan !== status.scan) void show(status);
        return;
      }
      // Watching the growing tree of a Rescan that did not finish: back to the map that was kept.
      if (shown?.scan === status.scan) void show(store.kept().find((k) => k.scan === r.of) ?? null);
    };

    /** Starts a scan (a job; resolves at once) and shows it. */
    const scan = async (target: DiskTarget): Promise<boolean> => {
      if (store.starting()) return false;
      patchState(store, { starting: true, startError: null });
      try {
        const before = store.kept();
        await show(adopt(await backend.startDiskScan(target), before));
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
          // A Rescan still running shows the map it replaces, with its strip (D9), as it did before the window was closed.
          const replaced = latest && latest.replaces !== null && outcome(latest.state) === 'running' ? (kept.find((k) => k.scan === latest.replaces) ?? null) : null;
          if (latest && replaced) {
            patchState(store, { refreshing: { scan: latest.scan, of: replaced.scan } });
            await show(replaced);
          } else if (latest) await show(latest);
        } catch {
          // Nothing kept is shown as "not scanned"; a scan can still be started.
        } finally {
          patchState(store, { restored: true });
        }
      },
      /**
       * The opt-in refresh of a saved map (`disk.savedMaps.refreshOnOpen`): at the first look at the page, when the map on screen
       * was read back from the cache and is older than the setting says. Never on a timer, never in the background. Once per run
       * of the page; returns whether a Rescan was started.
       */
      async refreshOnOpen(now: number): Promise<boolean> {
        if (opened) return false;
        opened = true;
        const s = store.status();
        if (!s || typeof s.origin === 'string' || s.scannedAt === null || store.refreshing()) return false;
        try {
          const when = (await backend.settings()).disk?.savedMaps?.refreshOnOpen ?? 'off';
          const limit = when === 'olderThanDay' ? DAY : when === 'olderThanWeek' ? 7 * DAY : null;
          return limit !== null && now - s.scannedAt > limit ? await this.rescan() : false;
        } catch {
          // The setting could not be read: nothing is refreshed by itself.
          return false;
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
      /**
       * Scans the same target again (Rescan, Retry). From a finished map the new pass replaces it: the map stays on screen,
       * readable, with the pass's progress beside it, and the page swaps when the pass is done (D9). Anything else (a partial
       * or failed map, no map) shows the new scan at once.
       */
      async rescan(): Promise<boolean> {
        const s = store.status();
        const id = store.selectedVolumeId();
        if (!s || outcome(s.state) !== 'done') return scan(s?.target ?? (id ? { volume: { id } } : { folder: { path: '/' } }));
        if (store.starting() || store.refreshing()) return false;
        patchState(store, { starting: true, startError: null, refreshFailure: null });
        try {
          const before = store.kept();
          const reply = adopt(await backend.startDiskScan(s.target, s.scan), before);
          patchState(store, { refreshing: { scan: reply.scan, of: s.scan } });
          // A short pass can end before its reply arrives.
          settleRefresh(reply);
          return true;
        } catch (e) {
          patchState(store, { startError: toCommandError(e) });
          return false;
        } finally {
          patchState(store, { starting: false });
        }
      },
      /** Shows the growing tree of the Rescan in progress instead of the map it will replace. */
      async showProgress(): Promise<void> {
        const rs = store.refreshStatus();
        if (rs) await show(rs);
      },
      /** Forgets the saved map on screen (its row in the cache) and clears the page. */
      async forgetSaved(): Promise<boolean> {
        const s = store.status();
        if (!s) return false;
        try {
          await backend.forgetSavedDiskMap(s.scan);
          patchState(store, (st) => ({ kept: st.kept.filter((k) => k.scan !== s.scan), refreshing: st.refreshing?.of === s.scan ? null : st.refreshing }));
          await show(null);
          return true;
        } catch (e) {
          patchState(store, { startError: toCommandError(e) });
          return false;
        }
      },
      async cancel(): Promise<void> {
        const s = store.status();
        // The pass to stop is the Rescan's when one runs for the map on screen; the map itself is not running.
        const target = store.refreshing()?.scan ?? s?.scan;
        if (target === undefined) return;
        try {
          await backend.cancelDiskScan(target);
        } catch (e) {
          patchState(store, { startError: toCommandError(e) });
        }
      },
      /** A pushed status: the highest `seq` of the scan on screen wins, and the level follows what was counted. */
      receive(status: DiskScanStatus): void {
        const known = store.kept().find((k) => k.scan === status.scan);
        if (known && known.seq >= status.seq) return;
        keep(status);
        settleRefresh(status);
        const shown = store.status();
        if (!shown || shown.scan !== status.scan) return;
        patchState(store, { status });
        void refreshSoon();
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
          const before = store.kept();
          await show(adopt(await backend.retryDiskScan(s.scan), before));
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
