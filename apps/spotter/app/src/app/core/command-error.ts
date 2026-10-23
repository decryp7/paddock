import type { CommandError } from './api.types';

const CODES: ReadonlySet<CommandError['code']> = new Set(['badRequest', 'notFound', 'invalid', 'unavailable', 'internal', 'cancelled']);

/** Any rejection from a command as a typed error (unknown shapes become `internal` with their text). */
export function toCommandError(e: unknown): CommandError {
  if (typeof e === 'object' && e !== null) {
    const { code, message, problems } = e as Record<string, unknown>;
    if (typeof code === 'string' && CODES.has(code as CommandError['code']) && typeof message === 'string') {
      const list = Array.isArray(problems) ? problems.filter((p): p is string => typeof p === 'string') : [];
      return { code: code as CommandError['code'], message, problems: list };
    }
  }
  const message = e instanceof Error ? e.message : typeof e === 'string' ? e : 'Something went wrong.';
  return { code: 'internal', message, problems: [] };
}

/** Whether a rejection is the user's own Cancel (not a failure to report). */
export function isCancelled(e: unknown): boolean {
  return toCommandError(e).code === 'cancelled';
}
