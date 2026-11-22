import type { CommandError as ItemError, ErrorCode as ItemErrorCode } from '@decryp7/paddock-ui/ipc';
import type { CommandError, ErrorCode } from './api.types';

const CODES: ReadonlySet<ErrorCode> = new Set<ErrorCode>([
  'serverUnavailable',
  'unauthorized',
  'forbidden',
  'notFound',
  'badRequest',
  'conflict',
  'insufficientMemory',
  'busy',
  'notAvailable',
  'upstream',
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

/** The item code each management code stands for (a bulk call's own rejection, seen as one item's failure). */
const ITEM_CODE: Record<ErrorCode, ItemErrorCode> = {
  serverUnavailable: 'unavailable',
  unauthorized: 'denied',
  forbidden: 'denied',
  notFound: 'notFound',
  badRequest: 'invalid',
  conflict: 'failed',
  insufficientMemory: 'limit',
  busy: 'busy',
  notAvailable: 'unavailable',
  upstream: 'failed',
  notImplemented: 'unsupported',
  cancelled: 'cancelled',
  internal: 'internal',
};

/** Item codes a later call can succeed after (the shared default of `paddock_commands::ErrorCode::retryable_by_default`). */
const RETRYABLE: ReadonlySet<ItemErrorCode> = new Set<ItemErrorCode>(['busy', 'unavailable', 'timeout', 'io']);

/**
 * A rejection of a whole bulk call as the failure of each of its items: an item error with the code the management code stands
 * for, the same message and the code's default `retryable`.
 */
export function toItemError(e: unknown): ItemError {
  const { code, message } = toCommandError(e);
  const item = ITEM_CODE[code];
  return { code: item, message, retryable: RETRYABLE.has(item) };
}
