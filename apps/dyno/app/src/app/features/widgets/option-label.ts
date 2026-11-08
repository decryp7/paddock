import { defaultKindOf } from '@decryp7/paddock-ui/widgets';
import type { LayoutView, Placement } from '../../core/generated/widgets';

/** The accessible name of a row's option control: "Volume of Disk space, position 2 of 4" (the kind's title and place, never an internal id). */
export function optionLabel<S, W, P, K extends string>(what: string, view: LayoutView<S, W, P, K> | null, p: Placement<W, P>): string {
  if (view === null) return what;
  const kind = defaultKindOf(p.widget);
  const title = view.catalogue.kinds.find((k) => k.kind === kind)?.title ?? kind;
  const at = view.layout.placements.findIndex((x) => x.id === p.id);
  return `${what} of ${title}, position ${at + 1} of ${view.layout.placements.length}`;
}
