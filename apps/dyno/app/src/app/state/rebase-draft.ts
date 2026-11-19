import type { Settings } from '../core/api.types';

/**
 * Carries the edits of a settings draft onto a copy that was reloaded after a stale refusal: every top-level field the draft
 * (`attempted`) changed against the copy it was read from (`base`) replaces the reloaded one; fields it did not touch come from the
 * reloaded copy, so what another window changed is kept. The reloaded revision stays, so saving the result is not stale again.
 */
export function rebaseDraft(base: Settings | null, attempted: Settings, reloaded: Settings): Settings {
  const out = structuredClone(reloaded) as unknown as Record<string, unknown>;
  const from = (base ?? {}) as Record<string, unknown>;
  for (const [key, value] of Object.entries(attempted)) {
    if (key === 'revision') continue;
    if (JSON.stringify(value) !== JSON.stringify(from[key])) out[key] = structuredClone(value);
  }
  return out as unknown as Settings;
}
