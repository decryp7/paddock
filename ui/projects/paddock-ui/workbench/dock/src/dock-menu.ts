import { Injectable, signal } from '@angular/core';

/** One entry of a dock menu (the panel menu, the auto-hide side choice). */
export interface DockMenuItem {
  readonly label: string;
  readonly disabled?: boolean;
  readonly run: () => void;
}

/** An open dock menu: where it is, what it holds, and what gets the focus back when it closes. */
export interface DockMenuState {
  readonly x: number;
  readonly y: number;
  readonly items: readonly DockMenuItem[];
  readonly anchor: HTMLElement | null;
}

/**
 * The small popup menus of the dock (panel menu, auto-hide side). They stay in the app (rule 17: menus are not OS
 * windows); `pd-dock` renders `state` as a keyboard-operable menu.
 */
@Injectable()
export class DockMenu {
  private readonly current = signal<DockMenuState | null>(null);
  readonly state = this.current.asReadonly();

  open(x: number, y: number, items: readonly DockMenuItem[], anchor: HTMLElement | null = null): void {
    this.current.set(items.length ? { x, y, items, anchor } : null);
  }

  /** Closes the menu; `restoreFocus` gives the focus back to the element it opened from. */
  close(restoreFocus = false): void {
    const anchor = this.current()?.anchor;
    this.current.set(null);
    if (restoreFocus) anchor?.focus?.();
  }

  /** Runs an entry (unless disabled) and closes. */
  choose(item: DockMenuItem): void {
    if (item.disabled) return;
    this.close();
    item.run();
  }
}
