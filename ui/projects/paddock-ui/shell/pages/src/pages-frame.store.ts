import { computed, inject, InjectionToken } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import { sidebarFit, sidebarMode, SIDEBAR_SIZES, type SidebarSizes } from './sidebar-plan';

/** Where the frame keeps the user's sidebar choice (a UI preference of this machine; `null`: kept for the session only). */
export interface PagesFrameStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export const PAGES_FRAME_KEY = 'paddock.pages.sidebar';
export const PAGES_FRAME_VERSION = 1;

export const PAGES_FRAME_STORAGE = new InjectionToken<PagesFrameStorage | null>('PADDOCK_PAGES_FRAME_STORAGE', {
  providedIn: 'root',
  factory: () => {
    try {
      return globalThis.localStorage ?? null;
    } catch {
      return null;
    }
  },
});

/** The sizes the frame plans the sidebar with (`provideShellPages({ sidebar })`). */
export const PAGES_SIDEBAR_SIZES = new InjectionToken<SidebarSizes>('PADDOCK_PAGES_SIDEBAR_SIZES', { providedIn: 'root', factory: () => SIDEBAR_SIZES });

/** Whether the user collapsed the sidebar, from storage; anything missing, corrupt or of another version: not collapsed. */
export function loadCollapsed(storage: Pick<PagesFrameStorage, 'getItem'> | null): boolean {
  try {
    const parsed = JSON.parse(storage?.getItem(PAGES_FRAME_KEY) ?? 'null') as { version?: unknown; collapsed?: unknown } | null;
    return parsed?.version === PAGES_FRAME_VERSION && parsed.collapsed === true;
  } catch {
    return false;
  }
}

/** Keeps the choice; a storage that is unavailable or full only costs the persistence. */
export function saveCollapsed(collapsed: boolean, storage: Pick<PagesFrameStorage, 'setItem'> | null): boolean {
  if (!storage) return false;
  try {
    storage.setItem(PAGES_FRAME_KEY, JSON.stringify({ version: PAGES_FRAME_VERSION, collapsed }));
    return true;
  } catch {
    return false;
  }
}

interface PagesFrameState {
  /** The frame's width (px), measured. */
  width: number;
  /** The user collapsed the expanded sidebar to the rail (kept). */
  collapsed: boolean;
  /** The sidebar is open over the page (rail or overlay mode). */
  drawerOpen: boolean;
}

/**
 * The pages frame's interaction state (provided by `pd-shell-frame`, rule 32): the measured width, the user's collapse
 * choice and whether the sidebar is open over the page. The mode follows from the layout plan (`sidebarFit`).
 */
export const PagesFrameStore = signalStore(
  withState<PagesFrameState>(() => ({
    width: typeof globalThis.innerWidth === 'number' && globalThis.innerWidth > 0 ? globalThis.innerWidth : 1280,
    collapsed: loadCollapsed(inject(PAGES_FRAME_STORAGE)),
    drawerOpen: false,
  })),
  withComputed((store, sizes = inject(PAGES_SIDEBAR_SIZES)) => {
    const fit = computed(() => sidebarFit(store.width(), sizes));
    const mode = computed(() => sidebarMode(fit(), store.collapsed()));
    return {
      fit,
      mode,
      /** The sidebar is drawn over the page now. */
      drawer: computed(() => store.drawerOpen() && mode() !== 'expanded'),
      /** The sidebar shows the pages' names (expanded, or open over the page). */
      labelled: computed(() => mode() === 'expanded' || store.drawerOpen()),
    };
  }),
  withMethods((store, storage = inject(PAGES_FRAME_STORAGE)) => {
    const setCollapsed = (collapsed: boolean): void => {
      patchState(store, { collapsed, drawerOpen: false });
      if (!saveCollapsed(collapsed, storage)) console.warn('[paddock-pages] the sidebar choice could not be stored; it lasts for this session');
    };
    return {
      /** The frame was resized; a drawer closes when the sidebar fits beside the page again. */
      measure(width: number): void {
        if (!Number.isFinite(width) || width <= 0 || width === store.width()) return;
        patchState(store, { width });
        if (store.mode() === 'expanded') patchState(store, { drawerOpen: false });
      },
      /** The title bar's button: collapse the expanded sidebar, expand the collapsed one, or open/close it over the page. */
      toggle(): void {
        if (store.mode() === 'expanded') setCollapsed(true);
        else if (store.fit() === 'expanded') setCollapsed(false);
        else patchState(store, { drawerOpen: !store.drawerOpen() });
      },
      setCollapsed,
      closeDrawer(): void {
        if (store.drawerOpen()) patchState(store, { drawerOpen: false });
      },
    };
  }),
);

export type PagesFrameStoreType = InstanceType<typeof PagesFrameStore>;
