import type { LayoutArea } from '@decryp7/paddock-ui';
import type { ViewDef } from '@decryp7/paddock-ui/workbench';

/** A view's home area (ADR 0011): `document`, `analysis`, `side` or `status`. */
export type ViewArea = LayoutArea;

/** The most tabs the analysis panel keeps; opening one more closes the least recently used inactive tab. */
export const ANALYSIS_TAB_CAP = 8;

/** A view's home area: the one it declares (every `ViewDef` declares a layout); a panel without a view is a document. */
export function viewArea(view: Pick<ViewDef, 'layout'> | undefined | null): ViewArea {
  return view?.layout.area ?? 'document';
}

/**
 * Which tab of a full analysis panel to close for a new one: the least recently used tab that is neither active nor
 * pinned (`mru` lists ids most recent first; ids not in it count as oldest). Null while the panel has room.
 */
export function analysisTabToClose(tabs: readonly string[], active: string | null, pinned: ReadonlySet<string>, mru: readonly string[], cap = ANALYSIS_TAB_CAP): string | null {
  if (tabs.length < cap) return null;
  const candidates = tabs.filter((id) => id !== active && !pinned.has(id));
  if (!candidates.length) return null;
  const rank = (id: string) => {
    const i = mru.indexOf(id);
    return i < 0 ? Number.POSITIVE_INFINITY : i;
  };
  return candidates.reduce((oldest, id) => (rank(id) > rank(oldest) ? id : oldest));
}
