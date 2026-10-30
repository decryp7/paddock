/**
 * The text selection at one moment, to give back later: what a native context menu does, which keeps the selection it opened
 * on. WebKit drops the selection when focus moves into an app-drawn menu, so the menu saves it on open and calls `restore()`
 * when it closes by Escape, by its own action (Copy) or by the app, never when a press elsewhere dismissed it.
 */
export interface PreservedSelection {
  /**
   * Puts the saved ranges back. It does nothing when nothing was selected, when a text field has the focus (its caret is the
   * user's), when the user selected something else meanwhile, or when the saved text has left the page.
   */
  restore(): void;
}

/** Is this element a place where the caret belongs to the user (a text field, a select, editable content)? */
export function isEditableElement(el: Element | null): boolean {
  if (!el) return false;
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) return true;
  return el instanceof HTMLElement && (el.isContentEditable || el.getAttribute('contenteditable') === 'true' || el.getAttribute('contenteditable') === '');
}

const NOTHING: PreservedSelection = { restore: () => undefined };

/** Saves `doc`'s selection if it holds text (a collapsed caret is not saved); see {@link PreservedSelection}. */
export function preserveSelection(doc: Document = document): PreservedSelection {
  const sel = doc.getSelection();
  if (!sel || sel.rangeCount === 0 || sel.isCollapsed) return NOTHING;
  const saved = Array.from({ length: sel.rangeCount }, (_, i) => sel.getRangeAt(i).cloneRange());
  return {
    restore: () => {
      const now = doc.getSelection();
      if (!now || isEditableElement(doc.activeElement)) return;
      if (now.rangeCount > 0 && !now.isCollapsed) return;
      // Ranges follow the DOM: one whose text was removed has collapsed onto the parent, and holds nothing to give back.
      const live = saved.filter((r) => !r.collapsed && r.startContainer.isConnected && r.endContainer.isConnected);
      if (live.length === 0) return;
      now.removeAllRanges();
      for (const r of live) now.addRange(r);
    },
  };
}
