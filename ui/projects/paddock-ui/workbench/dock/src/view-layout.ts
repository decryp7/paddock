import type { ViewLayout } from '@decryp7/paddock-ui';
import { defaultViewLayout } from '@decryp7/paddock-ui';
import type { ViewDef } from '@decryp7/paddock-ui/workbench';
import { viewArea } from './view-area';

/** The view's declared constraints, else the conservative default for its area (a panel whose view is gone). */
export function viewLayout(view: Pick<ViewDef, 'layout'> | undefined | null): ViewLayout {
  return view?.layout ?? defaultViewLayout(viewArea(view));
}

/**
 * The row of the main column a view shares with others: analysis views share one row below the documents, every other
 * document sits in the one main row.
 */
export function viewRow(view: Pick<ViewDef, 'layout'>): string {
  return viewArea(view) === 'analysis' ? 'analysis' : 'main';
}
