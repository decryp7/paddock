import type { CommandError, ErrorCode } from './api.types';

const CODES: ReadonlySet<ErrorCode> = new Set<ErrorCode>([
  'serverUnavailable',
  'unauthorized',
  'forbidden',
  'notFound',
  'badRequest',
  'conflict',
  'offline',
  'insufficientMemory',
  'busy',
  'notAvailable',
  'notImplemented',
  'cancelled',
  'internal',
]);

/** Any rejection from a command as a typed error (unknown shapes become `internal` with their text). */
export function toCommandError(e: unknown): CommandError {
  if (typeof e === 'object' && e !== null) {
    const { code, message } = e as Record<string, unknown>;
    if (typeof code === 'string' && CODES.has(code as ErrorCode) && typeof message === 'string') {
      return { code: code as ErrorCode, message };
    }
  }
  const message = e instanceof Error ? e.message : typeof e === 'string' ? e : 'Something went wrong.';
  return { code: 'internal', message };
}
