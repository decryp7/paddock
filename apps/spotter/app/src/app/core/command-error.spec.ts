import { toCommandError } from './command-error';

describe('toCommandError', () => {
  it('passes a well-formed error through and keeps its problems', () => {
    expect(toCommandError({ code: 'invalid', message: 'bad', problems: ['a', 1, 'b'] })).toEqual({ code: 'invalid', message: 'bad', problems: ['a', 'b'] });
    expect(toCommandError({ code: 'notFound', message: 'gone' })).toEqual({ code: 'notFound', message: 'gone', problems: [] });
  });

  it('turns anything else into internal', () => {
    expect(toCommandError(new Error('boom'))).toEqual({ code: 'internal', message: 'boom', problems: [] });
    expect(toCommandError('plain')).toEqual({ code: 'internal', message: 'plain', problems: [] });
    for (const v of [null, undefined, 42, { code: 'nope', message: 'm' }, { code: 'invalid' }]) {
      expect(toCommandError(v).message).toBe('Something went wrong.');
    }
  });
});
