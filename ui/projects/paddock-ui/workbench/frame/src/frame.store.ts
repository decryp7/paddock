import { computed, inject, InjectionToken } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withProps, withState } from '@ngrx/signals';
import { planLayout, type OpenView, type ViewLayout } from '@decryp7/paddock-ui';
import { EXPLORER_PLAN, type ExplorerVerdict } from '@decryp7/paddock-ui/workbench';

/** The Explorer column as the layout plan sees it (Debrief's `EXPLORER_LAYOUT`): a left column, priority 80. */
export const EXPLORER_LAYOUT: ViewLayout = { area: 'side', edge: 'left', min: { width: 200, height: 200 }, pref: { width: 272, height: 400 }, priority: 80 };
/** What the stage needs beside the Explorer when no dock answers for it: one document at its conservative minimum. */
export const STAGE_LAYOUT: ViewLayout = { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 100 };
/** The activity bar's width, px. */
export const ACTIVITY_BAR_PX = 48;
/** Grow-back margin: a closed Explorer returns only when this much more room than its minimum need is there. */
export const EXPLORER_MARGIN_PX = 48;
/** Explorer widths the user may drag to, px. */
export const EXPLORER_MIN_PX = EXPLORER_LAYOUT.min.width;
export const EXPLORER_MAX_PX = 640;

/**
 * The frame's own plan for the Explorer when no dock answers (`EXPLORER_PLAN` null): the Explorer at `explorerWidth`
 * beside a minimal document in `frameWidth` (minus the activity bar). Pure; same `planLayout` as the dock (ADR 0011).
 */
export function fallbackVerdict(open: boolean, explorerWidth: number, frameWidth: number): ExplorerVerdict {
  const fits = (margin: number): boolean => {
    const explorer: OpenView = { id: 'pd.explorer', layout: { ...EXPLORER_LAYOUT, pref: { ...EXPLORER_LAYOUT.pref, width: Math.max(EXPLORER_MIN_PX, explorerWidth) } } };
    const stage: OpenView = { id: 'pd.stage', layout: STAGE_LAYOUT };
    const plan = planLayout({ width: Math.max(0, frameWidth - ACTIVITY_BAR_PX - margin), height: 600 }, [explorer, stage]);
    return !plan.hidden.some((h) => h.id === explorer.id) && !plan.tabbed.some((t) => t.id === explorer.id);
  };
  if (open) return fits(0) ? 'keep' : 'hide';
  return fits(EXPLORER_MARGIN_PX) ? 'show' : 'hide';
}

/** Where the frame keeps the Explorer's section and width (a per-viewer convenience; null: this session only). */
export interface FrameStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export const WORKBENCH_FRAME_KEY = 'paddock.workbench.frame';
export const WORKBENCH_FRAME_VERSION = 1;

export const WORKBENCH_FRAME_STORAGE = new InjectionToken<FrameStorage | null>('PADDOCK_WORKBENCH_FRAME_STORAGE', {
  providedIn: 'root',
  factory: () => {
    try {
      return globalThis.localStorage ?? null;
    } catch {
      return null;
    }
  },
});

/** What is kept between runs. */
export interface FramePrefs {
  /** The section shown, or null when the user closed the side bar. `undefined` (first run): the first section. */
  readonly explorer?: string | null;
  readonly explorerWidth: number;
}

/** Parses stored prefs at the edge; anything missing, corrupt or of another version gives the defaults. */
export function loadFramePrefs(storage: Pick<FrameStorage, 'getItem'> | null): FramePrefs {
  const defaults: FramePrefs = { explorerWidth: EXPLORER_LAYOUT.pref.width };
  try {
    const v = JSON.parse(storage?.getItem(WORKBENCH_FRAME_KEY) ?? 'null') as { version?: unknown; explorer?: unknown; explorerWidth?: unknown } | null;
    if (v?.version !== WORKBENCH_FRAME_VERSION) return defaults;
    const width = typeof v.explorerWidth === 'number' && Number.isFinite(v.explorerWidth) ? clampWidth(v.explorerWidth) : defaults.explorerWidth;
    const explorer = v.explorer === null ? null : typeof v.explorer === 'string' && v.explorer.length <= 128 ? v.explorer : undefined;
    return explorer === undefined ? { explorerWidth: width } : { explorer, explorerWidth: width };
  } catch {
    return defaults;
  }
}

export function saveFramePrefs(prefs: FramePrefs, storage: Pick<FrameStorage, 'setItem'> | null): boolean {
  if (!storage) return false;
  try {
    storage.setItem(WORKBENCH_FRAME_KEY, JSON.stringify({ version: WORKBENCH_FRAME_VERSION, explorer: prefs.explorer ?? null, explorerWidth: prefs.explorerWidth }));
    return true;
  } catch {
    return false;
  }
}

export function clampWidth(width: number): number {
  return Math.round(Math.min(EXPLORER_MAX_PX, Math.max(EXPLORER_MIN_PX, width)));
}

interface FrameState {
  /** The Explorer section the user wants shown (null: closed by the user). */
  wanted: string | null;
  /** Closed by the plan for want of room (the user's choice is kept in `wanted`). */
  closedByPlan: boolean;
  /** The user reopened it while there was no room: it stays until it fits again. */
  keptByUser: boolean;
  explorerWidth: number;
  frameWidth: number;
}

/**
 * The workbench frame's state (plan W4): which Explorer section the side bar shows, its width, and the narrow-window rule
 * of Debrief's `NarrowWindow` by the layout plan, never by width constants: the Explorer closes when the plan has no room
 * for it beside what is docked, and comes back with a margin so it cannot flap; one the user opened by hand stays until
 * it fits again, one the user closed never reopens by itself.
 */
export const WorkbenchFrameStore = signalStore(
  { protectedState: true },
  withProps(() => ({ _storage: inject(WORKBENCH_FRAME_STORAGE), _plan: inject(EXPLORER_PLAN) })),
  withState<FrameState>({ wanted: null, closedByPlan: false, keptByUser: false, explorerWidth: EXPLORER_LAYOUT.pref.width, frameWidth: 0 }),
  withComputed((store) => ({
    /** The section on show (null: the side bar is closed). */
    explorerShown: computed(() => (store.closedByPlan() ? null : store.wanted())),
  })),
  withMethods((store) => {
    const persist = (): void => void saveFramePrefs({ explorer: store.wanted(), explorerWidth: store.explorerWidth() }, store._storage);
    const verdict = (open: boolean): ExplorerVerdict =>
      store._plan ? store._plan.verdict(open, store.explorerWidth()) : fallbackVerdict(open, store.explorerWidth(), store.frameWidth());
    return {
      /** Starts from the stored prefs; `first` is the section a first run shows (the first contributed one). */
      init(first: string | null): void {
        const prefs = loadFramePrefs(store._storage);
        patchState(store, { wanted: prefs.explorer === undefined ? first : prefs.explorer, explorerWidth: prefs.explorerWidth });
      },
      /** The window's frame width changed (or the dock re-planned): applies the plan's verdict. */
      evaluate(frameWidth = store.frameWidth()): void {
        patchState(store, { frameWidth });
        if (store.wanted() === null || !(frameWidth > 0)) return;
        if (store.closedByPlan()) {
          if (verdict(false) === 'show') patchState(store, { closedByPlan: false });
          return;
        }
        const v = verdict(true);
        if (v === 'keep') patchState(store, { keptByUser: false });
        else if (v === 'hide' && !store.keptByUser()) patchState(store, { closedByPlan: true });
      },
      /** Shows a section (the user asked: kept even when the plan has no room). */
      show(section: string): void {
        const reopened = store.closedByPlan() || store.wanted() === null;
        patchState(store, { wanted: section, closedByPlan: false });
        if (reopened && store.frameWidth() > 0 && verdict(true) === 'hide') patchState(store, { keptByUser: true });
        persist();
      },
      /** Closes the side bar (the user's choice). */
      hide(): void {
        patchState(store, { wanted: null, closedByPlan: false, keptByUser: false });
        persist();
      },
      /** Sets the Explorer's width (dragged or keyed), clamped. */
      setWidth(width: number): void {
        patchState(store, { explorerWidth: clampWidth(width) });
        persist();
      },
    };
  }),
);

export type WorkbenchFrameStoreType = InstanceType<typeof WorkbenchFrameStore>;
