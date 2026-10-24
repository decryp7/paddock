import { describe, expect, it } from 'vitest';
import { ERROR_CODES } from '../generated/ipc';
import { CommandFailure, fromRejection, isCommandError, isServiceOutage, isUnsupported, toCommandError } from './command-error';

const wire = { code: 'busy', message: 'Already running.', retryable: true } as const;

describe('isCommandError', () => {
  it('accepts the wire shape with every known code', () => {
    for (const code of ERROR_CODES) expect(isCommandError({ code, message: 'm', retryable: false })).toBe(true);
  });
  it('refuses anything else', () => {
    for (const v of [null, undefined, 'x', 3, [], {}, { code: 'nope', message: 'm', retryable: false }, { code: 'busy', message: 1, retryable: false }, { code: 'busy', message: 'm' }, new Error('x')]) {
      expect(isCommandError(v)).toBe(false);
    }
  });
});

describe('toCommandError', () => {
  it('keeps a wire error and only its known fields', () => {
    const e = toCommandError({ ...wire, hint: 'Wait.', field: 'f', cause: 'health:x', kind: 'k', extra: 1, detail: { big: true } });
    expect(e).toEqual({ ...wire, hint: 'Wait.', field: 'f', cause: 'health:x', kind: 'k' });
    expect(toCommandError({ ...wire, hint: 5 })).toEqual(wire);
  });
  it('turns an Error into internal, a string into failed, anything else into a calm sentence', () => {
    expect(toCommandError(new Error('boom'))).toEqual({ code: 'internal', message: 'boom', retryable: false });
    expect(toCommandError(new Error(''))).toEqual({ code: 'internal', message: 'Something went wrong.', retryable: false });
    expect(toCommandError('disk full')).toEqual({ code: 'failed', message: 'disk full', retryable: false });
    expect(toCommandError('')).toEqual({ code: 'failed', message: 'Something went wrong.', retryable: false });
    for (const v of [undefined, null, 42, {}, { code: 'busy' }]) expect(toCommandError(v)).toEqual({ code: 'internal', message: 'Something went wrong.', retryable: false });
  });
});

describe('CommandFailure and fromRejection', () => {
  it('is an Error carrying the fields', () => {
    const f = new CommandFailure({ ...wire, hint: 'h', field: 'f', kind: 'k', cause: 'health:x' });
    expect(f).toBeInstanceOf(Error);
    expect([f.name, f.message, f.code, f.retryable, f.hint, f.field, f.kind, f.cause]).toEqual(['CommandFailure', 'Already running.', 'busy', true, 'h', 'f', 'k', 'health:x']);
    const bare = new CommandFailure(wire);
    expect([bare.hint, bare.field, bare.kind, bare.cause]).toEqual([undefined, undefined, undefined, undefined]);
  });
  it('wraps a wire error and leaves other rejections alone', () => {
    expect(fromRejection(wire)).toBeInstanceOf(CommandFailure);
    const other = new TypeError('x');
    expect(fromRejection(other)).toBe(other);
    expect(fromRejection('text')).toBe('text');
  });
});

describe('classifiers', () => {
  it('recognises a service outage by its cause', () => {
    expect(isServiceOutage({ ...wire, cause: 'health:store' })).toBe(true);
    expect(isServiceOutage(new CommandFailure({ ...wire, cause: 'health:store' }))).toBe(true);
    expect(isServiceOutage({ ...wire, cause: 'other' })).toBe(false);
    expect(isServiceOutage(wire)).toBe(false);
    expect(isServiceOutage('x')).toBe(false);
  });
  it('recognises an unsupported operation', () => {
    expect(isUnsupported({ code: 'unsupported', message: 'm', retryable: false })).toBe(true);
    expect(isUnsupported(wire)).toBe(false);
    expect(isUnsupported(undefined)).toBe(false);
  });
});
