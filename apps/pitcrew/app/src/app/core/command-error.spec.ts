import { toCommandError } from './command-error';

describe('toCommandError', () => {
  it('passes a well-formed error through', () => {
    expect(toCommandError({ code: 'busy', message: 'try later' })).toEqual({ code: 'busy', message: 'try later' });
  });

  it('keeps only code and message', () => {
    expect(toCommandError({ code: 'badRequest', message: 'x', extra: 1 })).toEqual({ code: 'badRequest', message: 'x' });
  });

  it('turns an unknown code into internal', () => {
    expect(toCommandError(new Error('boom'))).toEqual({ code: 'internal', message: 'boom' });
    expect(toCommandError({ code: 'nope', message: 'm' })).toEqual({ code: 'internal', message: 'Something went wrong.' });
  });

  it('uses a string rejection as the message', () => {
    expect(toCommandError('plain')).toEqual({ code: 'internal', message: 'plain' });
  });

  it('falls back to a generic message for anything else', () => {
    for (const v of [null, undefined, 42, { code: 'busy' }, { code: 1, message: 'x' }]) {
      expect(toCommandError(v)).toEqual({ code: 'internal', message: 'Something went wrong.' });
    }
  });
});
