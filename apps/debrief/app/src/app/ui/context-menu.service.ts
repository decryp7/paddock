import { Injectable, signal } from '@angular/core';

export interface MenuEntry {
  label: string;
  run: () => void;
  disabled?: boolean;
  /** Shown under the label (typically why a disabled item is unavailable). */
  hint?: string;
  /** Entries of one group sit together; a separator is drawn where the group changes. */
  group?: string;
  /** A key binding ("Mod+C") shown right-aligned, in this platform's notation. */
  shortcut?: string;
}

export interface OpenMenu {
  x: number;
  y: number;
  items: readonly MenuEntry[];
  /** Element to focus again when the menu closes without running an item. */
  returnFocus: HTMLElement | null;
  /** The text selection when it opened (copies of its ranges), given back when it closes. */
  selection: readonly Range[];
}

/**
 * Opens the one context menu the app shows at a time; rendered by `debrief-context-menu-host`. Closed, it gives back the text
 * selection it opened on, as a native menu keeps it: WebKit (WKWebView) drops the selection when focus moves into the menu.
 * A selection the user made meanwhile stays.
 */
@Injectable({ providedIn: 'root' })
export class ContextMenuService {
  readonly menu = signal<OpenMenu | null>(null);

  open(x: number, y: number, items: readonly MenuEntry[], returnFocus: HTMLElement | null = document.activeElement as HTMLElement | null): void {
    if (items.length) this.menu.set({ x, y, items, returnFocus, selection: saveSelection() });
  }

  /** Closes the menu; `restoreFocus` is false when an item ran (its action decides where focus goes). */
  close(restoreFocus = true): void {
    const menu = this.menu();
    this.menu.set(null);
    if (restoreFocus) menu?.returnFocus?.focus?.();
    // After the focus moved back (in WebKit that can clear the selection again).
    if (menu) restoreSelection(menu.selection);
  }
}

function saveSelection(): Range[] {
  const sel = document.getSelection();
  if (!sel) return [];
  return Array.from({ length: sel.rangeCount }, (_, i) => sel.getRangeAt(i).cloneRange());
}

/** Puts the saved ranges back unless the user selected something else meanwhile (a selection that is not just a caret). */
function restoreSelection(saved: readonly Range[]): void {
  const sel = document.getSelection();
  if (!sel || saved.length === 0 || (sel.rangeCount > 0 && !sel.isCollapsed)) return;
  const live = saved.filter((r) => r.startContainer.isConnected && r.endContainer.isConnected);
  if (live.length === 0) return;
  sel.removeAllRanges();
  for (const r of live) sel.addRange(r);
}
