import type { CommandError, ErrorCode } from './api.types';

const CODES: ReadonlySet<ErrorCode> = new Set<ErrorCode>([
  'agentUnavailable',
  'notFound',
  'accessDenied',
  'unsupported',
  'badRequest',
  'timeout',
  'busy',
  'internal',
  'stale',
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

/** A short next step for an error, shown under its message. */
export function errorHint(code: ErrorCode): string | null {
  switch (code) {
    case 'agentUnavailable':
      return 'The Dyno agent is not running. It restarts automatically; try again in a moment.';
    case 'accessDenied':
      return 'Windows or macOS refused. This reading needs administrator rights.';
    case 'timeout':
      return 'The request took too long. Try again.';
    case 'busy':
      return 'Dyno is busy with another task. Try again when it finishes.';
    case 'stale':
      return 'The layout changed in another window. It was reloaded; try again.';
    case 'notFound':
      return 'It no longer exists (the process may have exited).';
    default:
      return null;
  }
}

/** The engine answers "not built yet" for features that are still to come: pages hide those panels instead of showing an error. */
export function isNotBuilt(e: { code: string; message: string } | null): boolean {
  return e !== null && (e.code === 'unsupported' || /not (built|implemented)/i.test(e.message));
}
