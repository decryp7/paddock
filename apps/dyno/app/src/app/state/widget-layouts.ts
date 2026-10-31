import { DestroyRef, Injectable, inject, signal, type Signal } from '@angular/core';
import type { CommandError } from '../core/api.types';
import { BACKEND } from '../core/backend';
import { toCommandError } from '../core/command-error';
import type { DynoSurface, Edit, SurfaceEdit, SurfaceView, DashboardWidget, MenuBarItem, PopoverWidget } from '../core/generated/widgets';
import type { DashboardView, MenuBarView, PopoverView } from '../core/widget-types';

/**
 * The three widget layouts of Dyno (family ADR 0035) as the engine returns them. Nothing here knows a limit, an id or a
 * rule: an edit goes to the engine, the answer replaces the view, and a change made in another window (`dyno://widget-layout`)
 * is read again. A surface is read the first time something asks for it (`use`).
 */
@Injectable({ providedIn: 'root' })
export class WidgetLayouts {
  private readonly backend = inject(BACKEND);
  private readonly _popover = signal<PopoverView | null>(null);
  private readonly _dashboard = signal<DashboardView | null>(null);
  private readonly _menuBar = signal<MenuBarView | null>(null);
  private readonly _error = signal<CommandError | null>(null);
  private readonly wanted = new Set<DynoSurface>();
  private stop: (() => void) | null = null;

  /** The popup's layout, or null before it is read. */
  readonly popover: Signal<PopoverView | null> = this._popover.asReadonly();
  /** The dashboard's layout. */
  readonly dashboard: Signal<DashboardView | null> = this._dashboard.asReadonly();
  /** The menu-bar items' layout. */
  readonly menuBar: Signal<MenuBarView | null> = this._menuBar.asReadonly();
  /** Why a layout could not be read. */
  readonly error: Signal<CommandError | null> = this._error.asReadonly();

  constructor() {
    inject(DestroyRef).onDestroy(() => this.stop?.());
  }

  /** Reads `surface` now and keeps it current while this window lives. */
  use(surface: DynoSurface): Promise<void> {
    this.wanted.add(surface);
    this.stop ??= this.backend.onWidgetLayout((s) => {
      if (this.wanted.has(s)) void this.read(s);
    });
    return this.read(surface);
  }

  /** Sends one edit of a surface. Resolves null when it was applied, or the engine's reason it was refused (the view is then read again). */
  async edit(surface: 'popover', edit: Edit<PopoverWidget>): Promise<CommandError | null>;
  async edit(surface: 'dashboard', edit: Edit<DashboardWidget>): Promise<CommandError | null>;
  async edit(surface: 'menuBar', edit: Edit<MenuBarItem>): Promise<CommandError | null>;
  async edit(surface: DynoSurface, edit: Edit<never>): Promise<CommandError | null> {
    try {
      this.apply(await this.backend.editWidgetLayout({ [surface]: edit } as unknown as SurfaceEdit));
      return null;
    } catch (e) {
      void this.read(surface);
      return toCommandError(e);
    }
  }

  private async read(surface: DynoSurface): Promise<void> {
    try {
      this.apply(await this.backend.widgetLayout(surface));
      this._error.set(null);
    } catch (e) {
      this._error.set(toCommandError(e));
    }
  }

  private apply(view: SurfaceView): void {
    if ('popover' in view) this._popover.set(view.popover);
    else if ('dashboard' in view) this._dashboard.set(view.dashboard);
    else this._menuBar.set(view.menuBar);
  }
}
