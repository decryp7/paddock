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
 * Opens the one context menu the app shows at a time; rendered by `debrief-context-menu-host`. Closed by Escape, an item or
 * the app, it gives back the text selection it opened on, as a native menu keeps it: WebKit (WKWebView) drops the selection
 * when focus moves into the menu. A press elsewhere leaves the selection alone, a focused text field keeps its caret, and a
 * selection the user made meanwhile stays.
 */
@Injectable({ providedIn: 'root' })
export class ContextMenuService {
  readonly menu = signal<OpenMenu | null>(null);

  open(x: number, y: number, items: readonly MenuEntry[], returnFocus: HTMLElement | null = document.activeElement as HTMLElement | null): void {
    if (items.length) this.menu.set({ x, y, items, returnFocus, selection: saveSelection() });
  }

  /**
   * Closes the menu; `restoreFocus` is false when an item ran (its action decides where focus goes), `restoreSelection` false
   * when a press elsewhere dismissed it.
   */
  close(restoreFocus = true, restoreSelection = true): void {
    const menu = this.menu();
    this.menu.set(null);
    if (restoreFocus) menu?.returnFocus?.focus?.();
    // After the focus moved back (in WebKit that can clear the selection again).
    if (menu && restoreSelection) restoreSavedSelection(menu.selection);
  }
}

// Debrief does not build on @decryp7/paddock-ui, whose `preserveSelection` this mirrors (same guards); keep the two in step.

/** Saves a selection that holds text; a collapsed caret is not saved. */
function saveSelection(): Range[] {
  const sel = document.getSelection();
  if (!sel || sel.rangeCount === 0 || sel.isCollapsed) return [];
  return Array.from({ length: sel.rangeCount }, (_, i) => sel.getRangeAt(i).cloneRange());
}

/** A text field, a select or editable content: its caret is the user's. */
function isEditable(el: Element | null): boolean {
  if (!el) return false;
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) return true;
  return el instanceof HTMLElement && (el.isContentEditable || el.getAttribute('contenteditable') === 'true' || el.getAttribute('contenteditable') === '');
}

/** Puts the saved ranges back unless a text field has the focus or the user selected something else meanwhile. */
function restoreSavedSelection(saved: readonly Range[]): void {
  const sel = document.getSelection();
  if (!sel || saved.length === 0 || isEditable(document.activeElement)) return;
  if (sel.rangeCount > 0 && !sel.isCollapsed) return;
  // Ranges follow the DOM: one whose text was removed has collapsed onto the parent, and holds nothing to give back.
  const live = saved.filter((r) => !r.collapsed && r.startContainer.isConnected && r.endContainer.isConnected);
  if (live.length === 0) return;
  sel.removeAllRanges();
  for (const r of live) sel.addRange(r);
}
