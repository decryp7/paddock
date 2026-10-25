import { inject, Injectable, signal } from '@angular/core';
import type { DockviewApi } from 'dockview';
import { PANEL_WINDOWS, type PanelWindowsPort } from '@decryp7/paddock-ui/workbench';
import { DockViews } from './dock-views';
import { AutoHideState, EMPTY_AUTO_HIDE } from './auto-hide';
import { DockSlot, Rect, Side } from './dock-geometry';
import { DropGuides } from './drop-guides';
import type { DisplacedView, UserSizes } from './layout-store';
import { viewKind, ViewKind } from './view-kind';

/** An inline problem or hint shown over the dock (a pop-out failed; a layout was fitted), with an optional action. */
export interface DockNotice {
  /** Which cause it belongs to: a new notice of the same key replaces the old; `withdraw(key)` clears it. */
  readonly key: string;
  readonly message: string;
  /** A one-line form for a small window's toast ("Layout fitted"); the full message stays in Notifications. */
  readonly short?: string;
  /** The one action (Retry, Undo). */
  readonly action?: { readonly label: string; readonly run: () => void };
}

export function rectOf(el: Element | null | undefined): Rect | null {
  const r = el?.getBoundingClientRect?.();
  return r ? { x: r.left, y: r.top, width: r.width, height: r.height } : null;
}

/**
 * The state the dock's services share: the dockview instance and its container, which panels are docked, the
 * auto-hidden tool windows and the pinned tabs. Internal to the dock: components use `DockService`.
 */
@Injectable()
export class DockContext {
  readonly views = inject(DockViews);
  private readonly windowsPort = inject(PANEL_WINDOWS);

  api: DockviewApi | null = null;
  container: HTMLElement | null = null;
  guides: DropGuides | null = null;
  /** While a layout is being built or restored: changes are not persisted. */
  restoring = false;
  /** Panels by most recent use (Ctrl+Tab, the document well). */
  mru: string[] = [];

  readonly apiReady = signal(false);
  /** The inline notice over the dock, if any. */
  readonly notice = signal<DockNotice | null>(null);
  /** Ids of the panels in the layout (docked or floating). */
  readonly docked = signal<ReadonlySet<string>>(new Set());
  /** Ids of the panels showing now: each group's active tab (the others are tabs behind it). */
  readonly shown = signal<ReadonlySet<string>>(new Set());
  readonly autoHide = signal<AutoHideState>(EMPTY_AUTO_HIDE);
  /** The side each tool window was last auto-hidden to (it goes there again). */
  /** Views the layout plan moved away for lack of room, by id (they return when there is room; persisted in the extras). */
  readonly displaced = new Map<string, DisplacedView>();
  /** The sizes the user dragged views to (persisted in the extras). */
  userSizes: UserSizes = {};
  readonly autoHideSides = signal<Readonly<Record<string, Side>>>({});
  /** Tabs pinned to the left of their strip (they survive "Close all"). */
  readonly pinned = signal<ReadonlySet<string>>(new Set());
  /** Ticks whenever dockview reports a layout change (a sash moved, a group floated, moved or was maximised). */
  readonly layoutVersion = signal(0);

  /** Where each popped-out panel was docked, so it returns there (`PANEL_WINDOWS.onReturned`). */
  readonly poppedSlots = new Map<string, DockSlot>();

  /** The pop-out windows (absent in a pop-out window itself and in a frame without `workbench/panels`). */
  windows(): PanelWindowsPort | null {
    return this.windowsPort;
  }

  /** Whether this panel can move to a window of its own: a pop-out service is there and the view allows it. */
  canPopOut(id: string): boolean {
    return this.windowsPort !== null && this.views.find(id)?.popOut !== false;
  }

  kindOf(id: string): ViewKind {
    return viewKind(this.views.find(id));
  }

  isAutoHidden(id: string): boolean {
    return this.autoHide().items.some((i) => i.id === id);
  }

  refreshOpen(): void {
    this.docked.set(new Set((this.api?.panels ?? []).map((p) => p.id)));
    this.refreshShown();
  }

  /** Re-reads which tab is in front in each group. */
  refreshShown(): void {
    const ids = (this.api?.groups ?? []).flatMap((g) => (g.activePanel ? [g.activePanel.id] : []));
    const next = new Set(ids);
    const prev = this.shown();
    if (next.size !== prev.size || ids.some((id) => !prev.has(id))) this.shown.set(next);
  }

  /** Marks the tabs of pinned panels (dockview knows nothing about pinning). */
  markPinnedTabs(): void {
    const pinned = this.pinned();
    this.container?.querySelectorAll<HTMLElement>('.pd-dock-tab[data-panel-id]').forEach((t) => {
      const on = pinned.has(t.dataset['panelId'] ?? '');
      t.classList.toggle('pinned', on);
      t.closest('.dv-tab')?.classList.toggle('pd-dock-pinned-tab', on);
    });
  }

  /** The root rectangle of the dock, or null before attach. */
  rootRect(): Rect | null {
    return this.container ? rectOf(this.container) : null;
  }
}
