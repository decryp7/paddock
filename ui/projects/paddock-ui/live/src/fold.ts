/** How a link looks to a window (`paddock_live::Link`, camelCase JSON). */
export type LiveLink =
  | { readonly state: 'connecting' }
  | { readonly state: 'connected' }
  | { readonly state: 'reconnecting'; readonly attempt: number; readonly retryInMs: number; readonly reason: string }
  | { readonly state: 'down'; readonly reason: string }
  | { readonly state: 'stopped' };

/** One message from Rust (`paddock_live::Frame`). */
export type LiveFrame<S, D> =
  | { readonly kind: 'snapshot'; readonly seq: number; readonly state: S; readonly link: LiveLink }
  | { readonly kind: 'delta'; readonly from: number; readonly seq: number; readonly delta: D }
  | { readonly kind: 'link'; readonly link: LiveLink };

/** What a window holds of a feed: the folded state, the sequence number it stands for, and the link. */
export interface LiveFolded<S> {
  readonly state: S;
  readonly seq: number;
  readonly link: LiveLink;
}

/**
 * What folding a frame did. `applied`: the folded value changed. `duplicate`: the frame was already included (an old delta, or
 * a link that did not change). `stale`: a fetched snapshot older than what the window already folded. `gap`: a delta skipped
 * numbers, so the window must ask for a snapshot.
 */
export type FoldOutcome = 'applied' | 'duplicate' | 'stale' | 'gap';

/** The result of {@link foldFrame}: the folded value (the same object when nothing changed) and what happened. */
export interface Folding<S> {
  readonly folded: LiveFolded<S>;
  readonly outcome: FoldOutcome;
}

/** Where a frame came from: pushed in order as an event, or answered by `live_snapshot` (which may be older than the events). */
export type FrameSource = 'event' | 'fetched';

/** A window before its first snapshot: the app's empty state, sequence 0, connecting. */
export function initialFolded<S>(empty: S): LiveFolded<S> {
  return { state: empty, seq: 0, link: { state: 'connecting' } };
}

/** Whether two links are the same. */
export function sameLink(a: LiveLink, b: LiveLink): boolean {
  if (a.state !== b.state) return false;
  switch (a.state) {
    case 'reconnecting': {
      const o = b as typeof a;
      return a.attempt === o.attempt && a.retryInMs === o.retryInMs && a.reason === o.reason;
    }
    case 'down':
      return a.reason === (b as typeof a).reason;
    default:
      return true;
  }
}

/**
 * Folds one frame (pure; the store and the specs share it).
 *
 * - A snapshot replaces the state and the link. A pushed snapshot is always newer; a fetched one is ignored when its number is
 *   below the window's (the events got ahead of the answer).
 * - A delta covers `from..=seq`. It is applied when `from` is at most the next number and `seq` is beyond the window's
 *   (a merged delta that overlaps what the window has is applied: merges are "latest wins", so repeating a part is harmless);
 *   one wholly included is a duplicate; one that skips numbers is a gap.
 * - A link frame replaces the link.
 */
export function foldFrame<S, D>(
  cur: LiveFolded<S>,
  frame: LiveFrame<S, D>,
  apply: (state: S, delta: D) => S,
  source: FrameSource = 'event',
): Folding<S> {
  switch (frame.kind) {
    case 'snapshot':
      if (source === 'fetched' && frame.seq < cur.seq) return { folded: cur, outcome: 'stale' };
      return { folded: { state: frame.state, seq: frame.seq, link: frame.link }, outcome: 'applied' };
    case 'delta':
      if (frame.seq <= cur.seq) return { folded: cur, outcome: 'duplicate' };
      if (frame.from > cur.seq + 1) return { folded: cur, outcome: 'gap' };
      return { folded: { ...cur, state: apply(cur.state, frame.delta), seq: frame.seq }, outcome: 'applied' };
    case 'link':
      if (sameLink(cur.link, frame.link)) return { folded: cur, outcome: 'duplicate' };
      return { folded: { ...cur, link: frame.link }, outcome: 'applied' };
  }
}

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const isCount = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0;

/** Reads a link from untrusted JSON, or null. */
export function parseLink(raw: unknown): LiveLink | null {
  if (!isRecord(raw)) return null;
  switch (raw['state']) {
    case 'connecting':
    case 'connected':
    case 'stopped':
      return { state: raw['state'] };
    case 'reconnecting':
      return isCount(raw['attempt']) && isCount(raw['retryInMs']) && typeof raw['reason'] === 'string'
        ? { state: 'reconnecting', attempt: raw['attempt'], retryInMs: raw['retryInMs'], reason: raw['reason'] }
        : null;
    case 'down':
      return typeof raw['reason'] === 'string' ? { state: 'down', reason: raw['reason'] } : null;
    default:
      return null;
  }
}

/**
 * Reads a frame from an event payload or a command answer (the edge: parse, then trust the type). The structure is checked
 * (kind, numbers, link); the app's state and delta are the app's own types and pass through. Anything else is null and dropped.
 */
export function parseFrame<S, D>(raw: unknown): LiveFrame<S, D> | null {
  if (!isRecord(raw)) return null;
  switch (raw['kind']) {
    case 'snapshot': {
      const link = parseLink(raw['link']);
      return isCount(raw['seq']) && link && 'state' in raw ? { kind: 'snapshot', seq: raw['seq'], state: raw['state'] as S, link } : null;
    }
    case 'delta':
      return isCount(raw['from']) && isCount(raw['seq']) && raw['from'] <= raw['seq'] && 'delta' in raw
        ? { kind: 'delta', from: raw['from'], seq: raw['seq'], delta: raw['delta'] as D }
        : null;
    case 'link': {
      const link = parseLink(raw['link']);
      return link ? { kind: 'link', link } : null;
    }
    default:
      return null;
  }
}
