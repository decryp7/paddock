import { ERROR_CODES, type CommandError, type ErrorCode } from '../generated/ipc';

/**
 * What a command rejects with, as the UI handles it (plan X28). The wire type `CommandError` is generated from
 * `paddock_commands::CommandError`; this file adds the checks and conversions around it. Components react to `code` and
 * `retryable`, never to the text.
 */

/** Whether `value` has the wire shape of a command error: a known `code`, a `message` and a boolean `retryable`. */
export function isCommandError(value: unknown): value is CommandError {
  if (typeof value !== 'object' || value === null) return false;
  const o = value as Record<string, unknown>;
  return typeof o['code'] === 'string' && (ERROR_CODES as readonly string[]).includes(o['code']) && typeof o['message'] === 'string' && typeof o['retryable'] === 'boolean';
}

const OPTIONAL = ['hint', 'field', 'cause', 'kind'] as const;

/** A copy of a wire error that keeps only the known fields (what arrives from the webview boundary is not trusted). */
function normalised(e: CommandError): CommandError {
  const out: CommandError = { code: e.code, message: e.message, retryable: e.retryable };
  for (const key of OPTIONAL) {
    const v = (e as unknown as Record<string, unknown>)[key];
    if (typeof v === 'string') out[key] = v;
  }
  return out;
}

/**
 * Any rejection as a typed error: a wire error is kept (known fields only), an `Error` or a string becomes `internal` /
 * `failed` with its text, and anything else `internal` with a calm sentence. Never throws.
 */
export function toCommandError(e: unknown): CommandError {
  if (isCommandError(e)) return normalised(e);
  if (e instanceof Error) return { code: 'internal', message: e.message || 'Something went wrong.', retryable: false };
  if (typeof e === 'string') return { code: 'failed', message: e || 'Something went wrong.', retryable: false };
  return { code: 'internal', message: 'Something went wrong.', retryable: false };
}

/** A command error as an `Error`, for code that throws and catches (`message` is the user's text). */
export class CommandFailure extends Error implements CommandError {
  readonly code: ErrorCode;
  readonly retryable: boolean;
  readonly hint?: string;
  readonly field?: string;
  readonly kind?: string;
  /** The service outage this is a symptom of (`health:store`). */
  override readonly cause?: string;

  constructor(error: CommandError) {
    super(error.message);
    this.name = 'CommandFailure';
    this.code = error.code;
    this.retryable = error.retryable;
    if (error.hint !== undefined) this.hint = error.hint;
    if (error.field !== undefined) this.field = error.field;
    if (error.kind !== undefined) this.kind = error.kind;
    if (error.cause !== undefined) this.cause = error.cause;
  }
}

/** A rejection as thrown to callers: a {@link CommandFailure} for a wire error, anything else as it was. */
export function fromRejection(e: unknown): unknown {
  return isCommandError(e) ? new CommandFailure(normalised(e)) : e;
}

/**
 * Whether `e` is a symptom of a service outage that service health reports once (`cause` starts with `health:`): no
 * panel, banner or toast repeats it.
 */
export function isServiceOutage(e: unknown): boolean {
  const cause = isCommandError(e) || e instanceof CommandFailure ? e.cause : undefined;
  return typeof cause === 'string' && cause.startsWith('health:');
}

/** Whether the engine does not offer the operation (here): the UI says so calmly, without Retry. */
export function isUnsupported(e: unknown): boolean {
  return toCommandError(e).code === 'unsupported';
}
