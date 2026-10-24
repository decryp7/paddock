import type { Health, KindView, PackEntry, PacksView, Problem } from '../generated/packs';

// Payloads from the engine are parsed once, at this edge: what is not the expected shape is dropped here, and what passes
// is rebuilt from known fields only (an unknown extra field never travels on).

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

function parseProblem(v: unknown): Problem | null {
  return isObject(v) && typeof v['file'] === 'string' && typeof v['message'] === 'string' ? { file: v['file'], message: v['message'] } : null;
}

function parseHealth(v: unknown): Health | null {
  if (!isObject(v)) return null;
  if (v['level'] === 'ok') return { level: 'ok' };
  if ((v['level'] === 'notes' || v['level'] === 'degraded') && typeof v['message'] === 'string') return { level: v['level'], message: v['message'] };
  return null;
}

function parseEntry(v: unknown): PackEntry | null {
  return isObject(v) && typeof v['name'] === 'string' && (v['origin'] === 'builtin' || v['origin'] === 'user') ? { name: v['name'], origin: v['origin'] } : null;
}

/** A list where every element must parse; null when any does not (a half-read list would hide a problem). */
function all<T>(v: unknown, parse: (x: unknown) => T | null): T[] | null {
  if (!Array.isArray(v)) return null;
  const out: T[] = [];
  for (const x of v) {
    const p = parse(x);
    if (p === null) return null;
    out.push(p);
  }
  return out;
}

function parseKind(v: unknown): KindView | null {
  if (!isObject(v) || typeof v['id'] !== 'string' || typeof v['label'] !== 'string' || typeof v['revision'] !== 'number') return null;
  const packs = all(v['packs'], parseEntry);
  const problems = all(v['problems'], parseProblem);
  const warnings = all(v['warnings'], parseProblem);
  const health = parseHealth(v['health']);
  if (!packs || !problems || !warnings || !health) return null;
  return { id: v['id'], label: v['label'], revision: v['revision'], packs, problems, warnings, health };
}

/** A `PacksView` from `packs_status`, `packs_reload` or the `paddock://packs` event, or null when the payload is not one. */
export function parsePacksView(v: unknown): PacksView | null {
  if (!isObject(v)) return null;
  const kinds = all(v['kinds'], parseKind);
  return kinds ? { kinds } : null;
}

/** A message for the user from anything a command rejected with. */
export function packsMessage(e: unknown, fallback: string): string {
  if (e instanceof Error && e.message) return e.message;
  return typeof e === 'string' && e ? e : fallback;
}
