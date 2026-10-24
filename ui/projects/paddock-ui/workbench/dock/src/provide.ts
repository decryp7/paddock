import { type EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { DOCK_PORT, EXPLORER_PLAN } from '@decryp7/paddock-ui/workbench';
import { DockAutoHide } from './dock-auto-hide';
import { DockContext } from './dock-context';
import { provideDockContributions } from './dock-contributions';
import { DockKeys } from './dock-keys';
import { DockLayout } from './dock-layout';
import { DockMenu } from './dock-menu';
import { DOCK_OPTIONS, type DockOptions } from './dock-options';
import { DockPersistence } from './dock-persistence';
import { DockPlacement } from './dock-placement';
import { DockPolicy } from './dock-policy';
import { DOCK_LAYOUT_STORE, InMemoryDockLayoutStore } from './dock-store';
import { DockViews } from './dock-views';
import { WorkbenchDock } from './dock.service';
import { Perspectives } from './perspectives';
import { WindowLayouts } from './window-layouts';

/**
 * The workbench dock for a frame's environment (not root: one dock per window): `WorkbenchDock` and its services, the
 * layout store (`options.store`, default in memory), the dock's commands, keys and Window-menu entries, and the core's
 * `DOCK_PORT` and `EXPLORER_PLAN` pointing at the dock. The frame places `<pd-dock />` and composes `WORKBENCH_PORTS` from
 * `DOCK_PORT`; this provides none of it. Needs the optional peer `dockview` (loaded on first use).
 */
export function provideWorkbenchDock(options: DockOptions = {}): EnvironmentProviders {
  return makeEnvironmentProviders([
    { provide: DOCK_OPTIONS, useValue: options },
    { provide: DOCK_LAYOUT_STORE, useValue: options.store ?? new InMemoryDockLayoutStore() },
    DockViews,
    DockContext,
    DockMenu,
    DockLayout,
    DockPlacement,
    DockPersistence,
    DockAutoHide,
    DockPolicy,
    DockKeys,
    WorkbenchDock,
    WindowLayouts,
    Perspectives,
    { provide: DOCK_PORT, useExisting: WorkbenchDock },
    { provide: EXPLORER_PLAN, useExisting: WorkbenchDock },
    provideDockContributions(),
  ]);
}
