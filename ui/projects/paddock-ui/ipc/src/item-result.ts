import type { CommandError, ItemResult } from '../generated/ipc';
import { CommandFailure, isCommandError, toCommandError } from './command-error';

/**
 * One result per item for bulk commands (PRINCIPLES, "Bulk APIs"): the wire type `ItemResult<T>` (`{ ok: T } | { err: CommandError }`)
 * is generated from `paddock_commands::ItemResult`; this file adds the checks around it. A bulk command answers one result per item in
 * request order; its own rejection (a `CommandError`) is only for the whole call (a batch over its bound, a refusal).
 */

/** Whether the item succeeded (its `ok` may be `null`: a unit reply such as a delete). */
export function isItemOk<T>(r: ItemResult<T>): r is { ok: T } {
  return 'ok' in r;
}

const bad = (message: string): CommandFailure => new CommandFailure({ code: 'internal', message, retryable: false });

/** The value check that accepts every value: pass it to `parseItemResults` to say so, when the caller checks the values itself. */
export const acceptAny = (_v: unknown): _v is unknown => true;

/**
 * The reply of a bulk command for `count` items, checked at the edge: an array of exactly `count` item results, each with exactly one of
 * `ok` or `err`, every error a known `CommandError` (known fields only), and every value accepted by `isValue` (required: `acceptAny`
 * says that any value is fine). Anything else throws an `internal` `CommandFailure`: the backend broke its contract, which is a bug, not
 * one item's failure.
 */
export function parseItemResults<T>(reply: unknown, count: number, isValue: (v: unknown) => v is T): ItemResult<T>[] {
  if (typeof isValue !== 'function') throw bad('parseItemResults needs a value check (or acceptAny).');
  if (!Array.isArray(reply)) throw bad('The bulk reply is not a list of item results.');
  if (reply.length !== count) throw bad(`The bulk reply has ${reply.length} results for ${count} items.`);
  return reply.map((r: unknown, i): ItemResult<T> => {
    if (typeof r !== 'object' || r === null) throw bad(`Item ${i} of the bulk reply is not an item result.`);
    const o = r as Record<string, unknown>;
    const keys = Object.keys(o);
    if (keys.length !== 1 || (keys[0] !== 'ok' && keys[0] !== 'err')) throw bad(`Item ${i} of the bulk reply has neither one "ok" nor one "err".`);
    if (keys[0] === 'err') {
      if (!isCommandError(o['err'])) throw bad(`Item ${i} of the bulk reply has an error of an unknown shape.`);
      return { err: toCommandError(o['err']) };
    }
    if (!isValue(o['ok'])) throw bad(`Item ${i} of the bulk reply has a value of an unexpected shape.`);
    return { ok: o['ok'] as T };
  });
}

/** The items that succeeded, with their values, and the ones that failed, with their errors: one summary instead of a toast per item. */
export interface SplitItems<I, T> {
  readonly ok: { item: I; value: T }[];
  readonly failed: { item: I; error: CommandError }[];
}

/** Pairs each of `items` with its result (same order and length, or it throws). */
export function splitItemResults<I, T>(items: readonly I[], results: readonly ItemResult<T>[]): SplitItems<I, T> {
  if (items.length !== results.length) throw bad(`The bulk reply has ${results.length} results for ${items.length} items.`);
  const out: SplitItems<I, T> = { ok: [], failed: [] };
  items.forEach((item, i) => {
    const r = results[i] as ItemResult<T>; // the lengths are equal
    if (isItemOk(r)) out.ok.push({ item, value: r.ok });
    else out.failed.push({ item, error: r.err });
  });
  return out;
}
