import type { Arrangement, PreviewReply, Cells, Dir, Edit, GridGesture, KindInfo, Moves, PreviewRequest, Rect } from '@decryp7/paddock-ui/widgets';
import { type GridView, type WidgetGridBackend } from '@decryp7/paddock-ui/widgets';
import cases from '../../../paddock-ui/widgets/generated/grid-cases.json';

/** The sample widget of the grid pages: only its kind. */
export interface GridSample {
  readonly kind: string;
}
type View = GridView<GridSample, string, string>;
type Item = { readonly id: string; readonly rect: Rect };

/** One golden scenario, written by a Rust test of `paddock-widgets` (ADR 0044 §6): a layout, a gesture and what the engine returned. */
export interface GoldenCase {
  readonly name: string;
  readonly columns: number;
  readonly items: readonly Item[];
  readonly gesture: GridGesture;
  readonly result: { readonly ok: readonly Item[] } | { readonly error: string };
}
export const GOLDEN_CASES = cases as unknown as readonly GoldenCase[];

/** Six sample kinds; the golden cases use a to f. */
export const SAMPLE_KINDS = ['alpha', 'bravo', 'charlie', 'delta', 'echo', 'foxtrot'] as const;
const size = { cells: { widths: { range: { min: 2, max: 12 } }, heights: { range: { min: 2, max: 14 } }, defaultW: 6, defaultH: 4 } } as const;
const kindInfo = (k: string): KindInfo<string> => ({ kind: k, title: k[0]!.toUpperCase() + k.slice(1), summary: `${k} summary`, icon: 'pd:activity', size, maxInstances: 12, refresh: 'onChange' });

/** The sample kind of a placement id: a, b, c ... in turn. */
export const kindOfId = (id: string): string => SAMPLE_KINDS[Math.max(0, id.charCodeAt(id.length - 1) - 97) % SAMPLE_KINDS.length] as string;

/** The placement a gesture is about. */
export const gestureId = (g: GridGesture): string => Object.values(g)[0]!.id;
const same = (a: Rect, b: Rect) => a.x === b.x && a.y === b.y && a.w === b.w && a.h === b.h;
const sameItems = (a: readonly Item[], b: readonly Item[]) => a.length === b.length && a.every((x) => b.some((y) => y.id === x.id && same(x.rect, y.rect)));
const reading = (items: readonly Item[]): Item[] => [...items].sort((p, q) => p.rect.y - q.rect.y || p.rect.x - q.rect.x || p.id.localeCompare(q.id));

/** What the engine says about the placement an arrange edit moved: its place in reading order and the neighbours. */
function outcomeOf(ordered: readonly Item[], id: string | undefined): View['outcome'] {
  const i = ordered.findIndex((x) => x.id === id);
  if (i < 0) return null;
  const r = ordered[i]!.rect;
  return { id: id as string, position: i + 1, of: ordered.length, after: ordered[i - 1]?.id ?? null, before: ordered[i + 1]?.id ?? null, w: r.w, h: r.h, limit: null };
}

/** A view of some rectangles, as the engine would return it (a stand-in: the arrangements below the first are crude). */
export function viewOf(items: readonly Item[], opts: { revision?: number; canUndo?: boolean; canRedo?: boolean; outcomeId?: string } = {}): View {
  const ordered = reading(items);
  const half = (r: Rect): Rect => ({ x: Math.floor(r.x / 2), y: r.y, w: Math.max(1, Math.ceil(r.w / 2)), h: r.h });
  let y = 0;
  const stacked = ordered.map((i) => {
    const rect = { x: 0, y, w: 1, h: i.rect.h };
    y += i.rect.h;
    return { id: i.id, rect };
  });
  const arrangements: Arrangement[] = [
    { minWidth: 840, columns: 12, placements: ordered },
    { minWidth: 560, columns: 6, placements: ordered.map((i) => ({ id: i.id, rect: half(i.rect) })) },
    { minWidth: 0, columns: 1, placements: stacked },
  ];
  const moves = ordered.map((i, n) => {
    const r = i.rect;
    const m: Moves = { left: r.x > 0, right: r.x + r.w < 12, up: r.y > 0, down: true, first: n > 0, last: n < ordered.length - 1, wider: r.w < 12 && r.x + r.w < 12, narrower: r.w > 2, taller: r.h < 14, shorter: r.h > 2 };
    return { id: i.id, moves: m };
  });
  const kinds = [...new Set(ordered.map((i) => kindOfId(i.id)))];
  return {
    layout: { version: 2, placements: ordered.map((i): { id: string; widget: GridSample } & Cells => ({ id: i.id, widget: { kind: kindOfId(i.id) }, ...i.rect })), presets: [] },
    revision: opts.revision ?? 1,
    arrangements,
    moves,
    canRedo: opts.canRedo ?? false,
    outcome: outcomeOf(ordered, opts.outcomeId),
    catalogue: { surface: { id: 'dashboard', noun: 'dashboard', columns: 12, maxPlacements: 32, minPlacements: 0, maxUserPresets: 3 }, kinds: (kinds.length ? kinds : [...SAMPLE_KINDS]).map(kindInfo) },
    builtinPresets: [],
    notes: [],
    canUndo: opts.canUndo ?? false,
    editable: true,
  };
}

/**
 * A stand-in for the engine in the gallery and in browser development (ADR 0044 §6, D6): a gesture that matches a golden case
 * (same layout, same gesture) gets the engine's recorded answer; any other gesture places the widget without reflow. It is never
 * the reference for behaviour; the real engine is `paddock-widgets`.
 */
export class MockGridBackend implements WidgetGridBackend<GridSample, string, string> {
  private items: Item[];
  private revision = 1;
  private added = 0;
  private readonly undo: Item[][] = [];
  private readonly redo: Item[][] = [];

  constructor(items: readonly Item[]) {
    this.items = [...items];
  }

  view(): View {
    return viewOf(this.items, { revision: this.revision, canUndo: this.undo.length > 0, canRedo: this.redo.length > 0 });
  }

  preview(request: PreviewRequest): Promise<PreviewReply<View>> {
    if (request.revision !== this.revision) return Promise.reject('stale');
    return this.compute(request.gesture).then((items) => ({ seq: request.seq, view: viewOf(items, { revision: this.revision, outcomeId: gestureId(request.gesture) }) }));
  }

  edit(edit: Edit<GridSample> | 'undo' | 'redo', revision: number): Promise<View> {
    if (revision !== this.revision) return Promise.reject('stale');
    if (edit === 'undo' || edit === 'redo') {
      const from = edit === 'undo' ? this.undo : this.redo;
      const to = edit === 'undo' ? this.redo : this.undo;
      const prev = from.pop();
      if (!prev) return Promise.reject('nothingToUndo');
      to.push(this.items);
      this.items = prev;
      this.revision++;
      return Promise.resolve(this.view());
    }
    if (typeof edit === 'string') return Promise.reject({ invalid: 'The gallery engine does not do that.' });
    if ('add' in edit) {
      const id = `n${++this.added}`;
      const y = Math.max(0, ...this.items.map((i) => i.rect.y + i.rect.h));
      return this.commit([...this.items, { id, rect: { x: 0, y, w: 6, h: 4 } }]);
    }
    if ('remove' in edit) return this.commit(this.items.filter((i) => i.id !== edit.remove.id));
    if ('arrange' in edit) return this.compute(edit.arrange.gesture).then((items) => this.commit(items, gestureId(edit.arrange.gesture)));
    return Promise.reject({ invalid: 'The gallery engine does not do that.' });
  }

  reload(): Promise<View> {
    return Promise.resolve(this.view());
  }

  private commit(items: readonly Item[], outcomeId?: string): Promise<View> {
    this.undo.push(this.items);
    this.redo.length = 0;
    this.items = [...items];
    this.revision++;
    return Promise.resolve(viewOf(this.items, { revision: this.revision, canUndo: true, canRedo: false, outcomeId }));
  }

  private compute(g: GridGesture): Promise<readonly Item[]> {
    const golden = GOLDEN_CASES.find((c) => sameItems(c.items, this.items) && JSON.stringify(sortKeys(c.gesture)) === JSON.stringify(sortKeys(g)));
    if (golden) return 'ok' in golden.result ? Promise.resolve(golden.result.ok) : Promise.reject({ blocked: 'unchanged' });
    const id = Object.values(g)[0]!.id;
    const me = this.items.find((i) => i.id === id);
    if (!me) return Promise.reject({ invalid: 'Unknown widget.' });
    const r = me.rect;
    const put = (rect: Rect): Promise<readonly Item[]> => {
      const clamped = { ...rect, x: Math.max(0, Math.min(12 - rect.w, rect.x)), y: Math.max(0, rect.y) };
      return same(clamped, r) ? Promise.reject({ blocked: 'unchanged' }) : Promise.resolve(roughSettle(this.items.map((i) => (i.id === id ? { id: i.id, rect: clamped } : i)), id));
    };
    const step = (dir: Dir, by: number) => ({ dx: dir === 'right' ? by : dir === 'left' ? -by : 0, dy: dir === 'down' ? by : dir === 'up' ? -by : 0 });
    if ('place' in g) {
      const t = g.place.rect;
      return put({ ...t, w: Math.min(12, Math.max(2, t.w)), h: Math.min(14, Math.max(2, t.h)) });
    }
    if ('nudge' in g) {
      const { dx, dy } = step(g.nudge.dir, 1);
      return put({ ...r, x: r.x + dx, y: r.y + dy });
    }
    if ('grow' in g) {
      const { dx, dy } = step(g.grow.dir, 1);
      return put({ ...r, w: Math.min(12, Math.max(2, r.w + dx)), h: Math.min(14, Math.max(2, r.h + dy)) });
    }
    if ('toFirst' in g) return put({ ...r, x: 0, y: 0 });
    return put({ ...r, x: 0, y: Math.max(...this.items.map((i) => i.rect.y + i.rect.h)) });
  }
}

const overlaps = (a: Rect, b: Rect) => a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

/**
 * A rough stand-in for the engine's settle (push the overlapped down, then let everything rise): enough for the gallery's board to
 * look like a dashboard after a drop. The real rules (swap, push order, holes) are the engine's and are checked by its own tests.
 */
function roughSettle(items: readonly Item[], movedId: string): Item[] {
  const out = items.map((i) => ({ ...i }));
  for (let pass = 0; pass < 200; pass++) {
    const ordered = reading(out);
    const pair = ordered.flatMap((p) => ordered.filter((q) => q !== p && overlaps(p.rect, q.rect)).map((q) => [p, q] as const))[0];
    if (!pair) break;
    // The moved one stays; otherwise the later one in reading order goes down.
    const [keep, push] = pair[1].id === movedId ? [pair[1], pair[0]] : [pair[0], pair[1]];
    (push as { rect: Rect }).rect = { ...push.rect, y: keep.rect.y + keep.rect.h };
  }
  for (const i of reading(out)) {
    let y = i.rect.y;
    while (y > 0 && !out.some((o) => o !== i && overlaps(o.rect, { ...i.rect, y: y - 1 }))) y--;
    (i as { rect: Rect }).rect = { ...i.rect, y };
  }
  return out;
}

/** The same value with every object's keys in order, so two gestures compare as text whatever order their keys were written in. */
function sortKeys(o: unknown): unknown {
  if (o === null || typeof o !== 'object') return o;
  return Object.fromEntries(Object.entries(o).sort(([a], [b]) => a.localeCompare(b)).map(([k, v]) => [k, sortKeys(v)]));
}
