import type { Settings } from '../core/api.types';

type Plain = Record<string, unknown>;
const isPlain = (v: unknown): v is Plain => typeof v === 'object' && v !== null && !Array.isArray(v);
const same = (a: unknown, b: unknown): boolean => JSON.stringify(a) === JSON.stringify(b);

/** A three-way merge of one value: what `attempted` changed against `base` goes onto `reloaded`; the rest is `reloaded`'s. */
function merge(base: unknown, attempted: unknown, reloaded: unknown): unknown {
  if (same(attempted, base)) return structuredClone(reloaded);
  if (isPlain(base) && isPlain(attempted) && isPlain(reloaded)) {
    const out: Plain = {};
    for (const key of new Set([...Object.keys(reloaded), ...Object.keys(attempted)])) {
      if (!(key in attempted)) continue;
      out[key] = key in reloaded ? merge(base[key], attempted[key], reloaded[key]) : structuredClone(attempted[key]);
    }
    // A key only the reloaded copy has (another window added it) stays.
    for (const key of Object.keys(reloaded)) if (!(key in attempted)) out[key] = structuredClone(reloaded[key]);
    return out;
  }
  return structuredClone(attempted);
}

/**
 * Carries the edits of a settings draft onto a copy that was reloaded after a stale refusal, or onto the settings as this window's
 * earlier save left them: a deep three-way merge of `attempted` (the draft), `base` (the copy it was read from) and `reloaded`.
 * It recurses while all three are plain objects, and takes the draft's value at the first leaf or array that differs from the base;
 * what the draft did not change comes from the reloaded copy, so another window's change, also inside the same field, is kept. The
 * reloaded revision stays, so saving the result is not stale again.
 */
export function rebaseDraft(base: Settings | null, attempted: Settings, reloaded: Settings): Settings {
  const out = merge(base ?? {}, attempted, reloaded) as Plain;
  out['revision'] = (reloaded as unknown as Plain)['revision'];
  return out as unknown as Settings;
}
