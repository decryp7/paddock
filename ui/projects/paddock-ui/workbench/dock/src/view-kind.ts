import type { ViewDef } from '@decryp7/paddock-ui/workbench';

/**
 * Visual Studio's two kinds of panels: tool windows dock around the edges (and can auto-hide); documents live in
 * the central document well and are what you work on.
 */
export type ViewKind = 'tool' | 'document';

/** A view whose home is a side or status area is a tool window; documents and analysis views are documents. */
export function viewKind(view: Pick<ViewDef, 'layout'> | undefined | null): ViewKind {
  const area = view?.layout.area;
  return area === 'side' || area === 'status' ? 'tool' : 'document';
}
