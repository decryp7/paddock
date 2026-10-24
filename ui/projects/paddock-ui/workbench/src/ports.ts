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
  /** The Explorer section shown in the side bar, or null when the side bar is closed. */
  readonly explorerShown: Signal<string | null>;
  /** Opens the command palette, optionally with a prefix typed (`>`, `@`). */
  openPalette(prefix?: string): void;
}

const NONE = signal<readonly string[]>([]).asReadonly();
const NULL = signal<string | null>(null).asReadonly();
const warn = (what: string): void => console.warn(`[paddock-workbench] no workbench frame in this window; cannot ${what}`);

export const WORKBENCH_PORTS = new InjectionToken<WorkbenchPorts>('PADDOCK_WORKBENCH_PORTS', {
  providedIn: 'root',
  factory: () => ({
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
    explorerShown: NULL,
    openPalette: () => warn('open the palette'),
  }),
});

/** Where a pop-out window should appear: screen coordinates of its top-left corner (logical px), and its size. */
export interface PopOutPlacement {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
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
}

/** Provided by `workbench/panels`; optional (null: no pop-outs in this window, as in a pop-out window itself). */
export const PANEL_WINDOWS = new InjectionToken<PanelWindowsPort | null>('PADDOCK_PANEL_WINDOWS', { providedIn: 'root', factory: () => null });
