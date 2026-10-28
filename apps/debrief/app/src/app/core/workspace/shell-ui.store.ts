import { patchState, signalStore, withMethods, withState } from '@ngrx/signals';
import { clampExplorerWidth, EXPLORER_WIDTH } from './ui-prefs.store';

interface ShellUiState {
  sidebarOpen: boolean;
  /** Files are dragged over the window (the drop overlay shows). */
  dropHover: boolean;
  /** Bumped to ask the search box to take focus. */
  focusSearchNonce: number;
  /** The widest the layout plan lets the Explorer be right now, px (set by `NarrowWindow`; the user's width is shown within it). */
  explorerRoom: number;
}

/** Transient shell flags: the sidebar, the drop overlay and focus requests for the search box. */
export const ShellUiStore = signalStore(
  { providedIn: 'root' },
  withState<ShellUiState>({ sidebarOpen: true, dropHover: false, focusSearchNonce: 0, explorerRoom: EXPLORER_WIDTH.max }),
  withMethods((store) => ({
    toggleSidebar: () => patchState(store, (s) => ({ sidebarOpen: !s.sidebarOpen })),
    setDropHover: (dropHover: boolean) => patchState(store, { dropHover }),
    setExplorerRoom: (room: number) => patchState(store, { explorerRoom: clampExplorerWidth(room) }),
    focusSearch: () => patchState(store, (s) => ({ focusSearchNonce: s.focusSearchNonce + 1 })),
  })),
);
