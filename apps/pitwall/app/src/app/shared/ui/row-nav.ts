/** Keyboard paths shared by every table whose rows open a detail panel (pure, so each is unit-tested). */

/** The row to move to for an arrow, Home or End key, or null for any other key. */
export function rowTarget(rows: readonly HTMLElement[], current: HTMLElement | null, key: string): HTMLElement | null {
  if (!rows.length) return null;
  const i = current ? rows.indexOf(current) : -1;
  switch (key) {
    case 'ArrowDown':
      return rows[Math.min(rows.length - 1, i + 1)] ?? null;
    case 'ArrowUp':
      return rows[Math.max(0, i < 0 ? 0 : i - 1)] ?? null;
    case 'Home':
      return rows[0] ?? null;
    case 'End':
      return rows[rows.length - 1] ?? null;
    default:
      return null;
  }
}

/** Shift+F10 or the Menu key: the keyboard's right-click. */
export function isMenuKey(e: Pick<KeyboardEvent, 'key' | 'shiftKey'>): boolean {
  return e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey);
}

/** Moves focus between the rows (`tr[tabindex]`) of the table a keydown happened in; true when it moved. */
export function moveRowFocus(e: KeyboardEvent): boolean {
  const row = (e.target as HTMLElement | null)?.closest<HTMLElement>('tr[tabindex]') ?? null;
  if (!row || e.target !== row) return false;
  const rows = Array.from(row.closest('tbody')?.querySelectorAll<HTMLElement>('tr[tabindex]') ?? []);
  const to = rowTarget(rows, row, e.key);
  if (!to) return false;
  e.preventDefault();
  to.focus();
  return true;
}

/** Whether a click on a row came from a control inside it (a button, link or field), which must not open the row. */
export function fromControl(e: Event): boolean {
  return !!(e.target as HTMLElement | null)?.closest('button, a, select, input, textarea, label');
}
