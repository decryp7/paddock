// Pure helpers for the lists a live fold keeps (Pitwall's tail and event feed, Dyno's sparklines): every one returns a new
// list, or the same list when nothing changes, and none grows past its bound.

/** Appends `item`, keeping the last `max` (oldest first). */
export function appendBounded<T>(list: readonly T[], item: T, max: number): T[] {
  const keep = Math.max(1, Math.floor(max));
  const next = list.length >= keep ? list.slice(list.length - keep + 1) : list.slice();
  next.push(item);
  return next;
}

/** How {@link appendThrottled} spaces and bounds a tail. */
export interface ThrottleOptions<T> {
  /** The time of an item, in milliseconds. */
  readonly at: (item: T) => number;
  /** At most one item per this many milliseconds. */
  readonly stepMs: number;
  /** Most items kept. */
  readonly max: number;
}

/**
 * Appends `item` unless it is less than `stepMs` after the last one (then the list itself is returned), keeping the last
 * `max`: the live tail of a chart that is pushed faster than it is drawn.
 */
export function appendThrottled<T>(list: readonly T[], item: T, options: ThrottleOptions<T>): readonly T[] {
  const last = list[list.length - 1];
  if (last !== undefined && options.at(item) - options.at(last) < options.stepMs) return list;
  return appendBounded(list, item, options.max);
}

/** Replaces the item with the same key in place, or puts `item` first (newest first), keeping at most `max` when given. */
export function upsertBy<T, K>(list: readonly T[], item: T, key: (item: T) => K, max = Number.POSITIVE_INFINITY): T[] {
  const k = key(item);
  const i = list.findIndex((x) => key(x) === k);
  if (i >= 0) {
    const copy = list.slice();
    copy[i] = item;
    return copy;
  }
  return [item, ...list].slice(0, max);
}

/** Puts `item` first unless an item with its key is there already (then the list itself), keeping at most `max`. */
export function prependUnique<T, K>(list: readonly T[], item: T, key: (item: T) => K, max: number): readonly T[] {
  const k = key(item);
  if (list.some((x) => key(x) === k)) return list;
  return [item, ...list].slice(0, Math.max(1, Math.floor(max)));
}
