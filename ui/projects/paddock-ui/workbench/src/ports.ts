import { InjectionToken, signal, type Signal } from '@angular/core';

/** How a view is shown: focused (default) or opened in the background, and optionally as a given instance. */
export interface ShowViewOptions {
  /** An instance key for a view with `instances` (`src/main.rs`). */
  readonly key?: string;
  /** Open without taking focus (a view opened by `autoOpen`). */
  readonly background?: boolean;
}

/**
 * What the workbench's parts ask of the dock and the side bar, so the activity bar, the palette, the Explorer and an app's
 * context need not import the dock (dockview stays in the dock's chunk). The workbench frame provides the real ports;
 * without one, the default logs a warning and does nothing (degrade, don't fail).
 */
export interface WorkbenchPorts {
  /** Opens (if needed) and focuses a view or a view's instance. Resolves false when the view is unknown or not offered. */
  showView(id: string, options?: ShowViewOptions): Promise<boolean>;
  /** Closes a panel (a view, or `<view>#<key>`). */
  closeView(panel: string): void;
  /** The panels that are open now (view ids and `<view>#<key>`). */
  readonly openPanels: Signal<readonly string[]>;
  /** The panel that has focus. */
  readonly activePanel: Signal<string | null>;
  /** Switches to a perspective; resolves false when it is unknown. */
  applyPerspective(id: string): Promise<boolean>;
  /** The perspective in use. */
  readonly activePerspective: Signal<string | null>;
  /** Shows the side bar with an Explorer section expanded and focused. */
  showExplorer(id: string): void;
  /** Closes the side bar. */
  hideExplorer(): void;
  /** The Explorer section shown in the side bar, or null when the side bar is closed. */
  readonly explorerShown: Signal<string | null>;
  /** Opens the command palette, optionally with a prefix typed (`>`, `@`). */
  openPalette(prefix?: string): void;
}

const NONE = signal<readonly string[]>([]).asReadonly();
const NULL = signal<string | null>(null).asReadonly();
const warn = (what: string): void => console.warn(`[paddock-workbench] no workbench frame in this window; cannot ${what}`);

/** The ports of a window without a workbench frame: each logs a warning and does nothing. */
export function defaultWorkbenchPorts(): WorkbenchPorts {
  return {
    showView: (id) => {
      warn(`show view ${id}`);
      return Promise.resolve(false);
    },
    closeView: (id) => warn(`close view ${id}`),
    openPanels: NONE,
    activePanel: NULL,
    applyPerspective: (id) => {
      warn(`apply perspective ${id}`);
      return Promise.resolve(false);
    },
    activePerspective: NULL,
    showExplorer: (id) => warn(`show Explorer section ${id}`),
    hideExplorer: () => warn('hide the side bar'),
    explorerShown: NULL,
    openPalette: () => warn('open the palette'),
  };
}

export const WORKBENCH_PORTS = new InjectionToken<WorkbenchPorts>('PADDOCK_WORKBENCH_PORTS', { providedIn: 'root', factory: defaultWorkbenchPorts });

/** Where a pop-out window should appear: screen coordinates of its top-left corner (logical px), and its size. */
export interface PopOutPlacement {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  /** The scale factor of the display `x` and `y` are logical px of (what the drag ghost reports); without it Rust works it out. */
  readonly scale?: number;
}

/**
 * Pop-out windows as the dock sees them (plan W3): `workbench/panels` provides the real one over `paddock-panels`; the
 * dock moves a panel out with `popOut` and takes it back when `returned` reports it (the window was closed or docked
 * back). Without a provider panels cannot leave the window (the dock hides the action).
 */
export interface PanelWindowsPort {
  /** Moves a panel to its own OS window; resolves false when it could not (the panel then stays docked). */
  popOut(panel: string, placement?: PopOutPlacement): Promise<boolean>;
  /** Brings a popped-out panel's window to the front. */
  focus(panel: string): void;
  /** Panels living in their own windows now. */
  readonly poppedOut: Signal<readonly string[]>;
  /** Called with a panel whose window closed or asked to dock back; returns the unsubscribe function. */
  onReturned(listener: (panel: string) => void): () => void;
  /**
   * Called when a pop-out dragged over this window changes its look (`RedockLook`): it turns translucent over the
   * window so the drop guides show, back to opaque when it leaves or the drag ends; `outline` when the system reduces
   * transparency (the guides then get a clear outline). Returns the unsubscribe function.
   */
  onRedock(listener: (look: RedockLook, panel: string) => void): () => void;
}

/** How a pop-out window being dragged over its host looks: see `PanelWindowsPort.onRedock`. */
export type RedockLook = 'opaque' | 'translucent' | 'outline';

/** Provided by `workbench/panels`; optional (null: no pop-outs in this window, as in a pop-out window itself). */
export const PANEL_WINDOWS = new InjectionToken<PanelWindowsPort | null>('PADDOCK_PANEL_WINDOWS', { providedIn: 'root', factory: () => null });

/** The dock's part of `WORKBENCH_PORTS` (plan W2a): `workbench/dock` provides it; the frame composes the rest. */
export type DockPort = Pick<WorkbenchPorts, 'showView' | 'closeView' | 'openPanels' | 'activePanel' | 'applyPerspective' | 'activePerspective'>;

/** Provided by `provideWorkbenchDock`; null in a window without a dock. */
export const DOCK_PORT = new InjectionToken<DockPort | null>('PADDOCK_DOCK_PORT', { providedIn: 'root', factory: () => null });

/** Whether the Explorer keeps its column (`keep`), should close for want of room (`hide`), or fits again (`show`). */
export type ExplorerVerdict = 'keep' | 'hide' | 'show';

/**
 * The layout plan's say on the Explorer column (ADR 0011; Debrief's `NarrowWindow`): the dock plans the Explorer as a
 * left column beside what is docked and answers here; without a dock the frame plans it against a minimal document.
 */
export interface ExplorerPlan {
  verdict(open: boolean, width: number): ExplorerVerdict;
  /** Called after each re-plan of the window (a resize, a view opened); returns the unsubscribe function. */
  onWindowPlan(listener: () => void): () => void;
}

/** Provided by the dock; null falls back to the frame's own plan. */
export const EXPLORER_PLAN = new InjectionToken<ExplorerPlan | null>('PADDOCK_EXPLORER_PLAN', { providedIn: 'root', factory: () => null });
