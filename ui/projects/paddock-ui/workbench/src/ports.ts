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
