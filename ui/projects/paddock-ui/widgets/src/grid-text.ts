import type { Block, Dir, GridGesture, Moves, Outcome, Rect } from '../generated/widgets';

/** One of the engine's `Moves` flags, which is also one entry of a widget's ⋯ menu and one key. */
export type MoveName = keyof Moves;

export interface MoveAction {
  /** The menu item's verb and direction: the widget's name goes after the verb ("Move Alpha left"). */
  readonly label: (title: string) => string;
  /** The key that does it, shown in the menu. */
  readonly key: string;
  /** What the engine's refusal would be, for the words when its `moves` flag says the key does nothing. */
  readonly block: Block | 'first' | 'last';
  /** The intent sent to the engine. */
  readonly gesture: (id: string) => GridGesture;
}

const nudge = (dir: Dir) => (id: string): GridGesture => ({ nudge: { id, dir } });
const grow = (dir: Dir) => (id: string): GridGesture => ({ grow: { id, dir } });

/** The ten gestures in menu order (ADR 0044 §5), each with the intent it sends. The engine decides which of them work. */
export const MOVE_ACTIONS: Readonly<Record<MoveName, MoveAction>> = {
  left: { label: (t) => `Move ${t} left`, key: '←', block: { edge: 'left' }, gesture: nudge('left') },
  right: { label: (t) => `Move ${t} right`, key: '→', block: { edge: 'right' }, gesture: nudge('right') },
  up: { label: (t) => `Move ${t} up`, key: '↑', block: { edge: 'up' }, gesture: nudge('up') },
  down: { label: (t) => `Move ${t} down`, key: '↓', block: { edge: 'down' }, gesture: nudge('down') },
  first: { label: (t) => `Move ${t} to first place`, key: 'Home', block: 'first', gesture: (id) => ({ toFirst: { id } }) },
  last: { label: (t) => `Move ${t} to the end`, key: 'End', block: 'last', gesture: (id) => ({ toLast: { id } }) },
  wider: { label: (t) => `Make ${t} wider`, key: '⇧→', block: 'largest', gesture: grow('right') },
  narrower: { label: (t) => `Make ${t} narrower`, key: '⇧←', block: 'smallest', gesture: grow('left') },
  taller: { label: (t) => `Make ${t} taller`, key: '⇧↓', block: 'largest', gesture: grow('down') },
  shorter: { label: (t) => `Make ${t} shorter`, key: '⇧↑', block: 'smallest', gesture: grow('up') },
};
/** The names in menu order. */
export const MOVE_NAMES = Object.keys(MOVE_ACTIONS) as readonly MoveName[];

/** The gesture a key stands for in edit mode, or null: arrows nudge, Shift with arrows grows, Home and End go first and last. */
export function moveOfKey(key: string, shift: boolean): MoveName | null {
  if (key === 'Home') return shift ? null : 'first';
  if (key === 'End') return shift ? null : 'last';
  const plain: Record<string, MoveName> = { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' };
  const grown: Record<string, MoveName> = { ArrowLeft: 'narrower', ArrowRight: 'wider', ArrowUp: 'shorter', ArrowDown: 'taller' };
  return (shift ? grown : plain)[key] ?? null;
}

/** What a refused gesture says, in words: the engine's reason with the widget's name. */
export function blockText(title: string, block: Block): string {
  if (block === 'largest') return `${title} can't get any larger.`;
  if (block === 'smallest') return `${title} can't get any smaller.`;
  if (block === 'unchanged') return `${title} stays where it is.`;
  const side = { left: 'left', right: 'right', up: 'top', down: 'bottom' }[block.edge];
  return `${title} is already at the ${side} edge.`;
}

/** True when the engine answered that the view the gesture was computed on is out of date. */
export const isStale = (e: unknown): boolean => e === 'stale';

/** What a key says when the engine's `moves` flag for it is false: the same words a refused gesture would get. */
export function cantText(title: string, move: MoveName): string {
  const b = MOVE_ACTIONS[move].block;
  if (b === 'first') return `${title} is already first.`;
  if (b === 'last') return `${title} is already last.`;
  return blockText(title, b);
}

/** The block the engine reported, or null for any other answer. */
export function blockOf(e: unknown): Block | null {
  return e !== null && typeof e === 'object' && 'blocked' in e ? (e as { blocked: Block }).blocked : null;
}

/** A sentence for a refused or failed edit; `title` names the widget the edit was about. */
export function errorText(e: unknown, title: string): string {
  const block = blockOf(e);
  if (block) return blockText(title, block);
  if (e !== null && typeof e === 'object' && 'sizeNotAllowed' in e) return `${title} can't be that size.`;
  if (e !== null && typeof e === 'object' && 'invalid' in e) return String((e as { invalid: unknown }).invalid);
  return "Couldn't change the layout. Try again.";
}

/** "6 by 6, position 5 of 15": the description a screen reader hears after the widget's name. */
export const sizeText = (r: Rect, position: number, of: number): string => `${r.w} by ${r.h}, position ${position} of ${of}`;

/**
 * What a finished gesture announces, from the engine's `Outcome` (ADR 0044 §5): "Memory moved right, after CPU. Position 6 of 15."
 * or "Memory resized to 4 by 6." `previous` and `next` are the titles of `outcome.after` and `outcome.before`; `before` is the stored size, to tell a drop that
 * resized from one that only moved. A resize that stopped at the kind's limit says so.
 */
export function outcomeText(title: string, o: Outcome, previous: string | null, next: string | null, g: GridGesture, before: Rect | undefined): string {
  const grew = 'grow' in g;
  const resized = grew || (before !== undefined && (before.w !== o.w || before.h !== o.h));
  const moved = !grew;
  const size = `${title} resized to ${o.w} by ${o.h}.${o.limit ? ' ' + blockText(title, o.limit) : ''}`;
  if (resized && !moved) return size;
  // First in reading order has nothing before it: say what follows instead.
  const where = previous !== null ? `, after ${previous}` : next !== null ? `, before ${next}` : '';
  const base = `${title} moved ${howOf(g)}${where}. Position ${o.position} of ${o.of}.`;
  return resized ? `${base} ${size}` : base;
}

/** The word for how a gesture moved a widget, for `outcomeText`. */
export function howOf(g: GridGesture): string {
  if ('nudge' in g) return g.nudge.dir;
  if ('toFirst' in g) return 'to first';
  if ('toLast' in g) return 'to last';
  return 'here';
}
