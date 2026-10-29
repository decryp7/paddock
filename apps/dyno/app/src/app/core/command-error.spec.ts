import type { ErrorCode } from './api.types';
import { errorHint, isNotBuilt, toCommandError } from './command-error';

describe('toCommandError', () => {
  it('passes a well-formed error through', () => {
    expect(toCommandError({ code: 'busy', message: 'wait', extra: 1 })).toEqual({ code: 'busy', message: 'wait' });
  });
  it('turns unknown codes or shapes into internal', () => {
    expect(toCommandError({ code: 'weird', message: 'x' })).toEqual({ code: 'internal', message: 'Something went wrong.' });
    expect(toCommandError({ code: 'busy' })).toEqual({ code: 'internal', message: 'Something went wrong.' });
    expect(toCommandError({ code: 5, message: 'x' }).code).toBe('internal');
  });
  it('keeps the text of errors and strings', () => {
    expect(toCommandError(new Error('boom'))).toEqual({ code: 'internal', message: 'boom' });
    expect(toCommandError('plain')).toEqual({ code: 'internal', message: 'plain' });
  });
  it('copes with null and undefined', () => {
    expect(toCommandError(null).code).toBe('internal');
    expect(toCommandError(undefined).message).toBe('Something went wrong.');
  });
});

describe('errorHint', () => {
  it('has a hint for actionable codes', () => {
    for (const c of ['agentUnavailable', 'accessDenied', 'timeout', 'busy', 'notFound'] as ErrorCode[]) {
      expect(errorHint(c)).toEqual(expect.any(String));
    }
  });
  it('has none for the rest', () => {
    for (const c of ['unsupported', 'badRequest', 'internal'] as ErrorCode[]) expect(errorHint(c)).toBeNull();
  });
});

describe('isNotBuilt', () => {
  it('detects unsupported and not-built messages', () => {
    expect(isNotBuilt(null)).toBe(false);
    expect(isNotBuilt({ code: 'unsupported', message: 'x' })).toBe(true);
    expect(isNotBuilt({ code: 'internal', message: 'Cleanup is not built yet' })).toBe(true);
    expect(isNotBuilt({ code: 'internal', message: 'NOT implemented' })).toBe(true);
    expect(isNotBuilt({ code: 'internal', message: 'disk on fire' })).toBe(false);
  });
});
