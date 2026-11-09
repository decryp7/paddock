import { toCommandError } from './command-error';

describe('toCommandError', () => {
  it('keeps an upstream error with the host it names', () => {
    const e = toCommandError({ code: 'upstream', message: 'the download failed: cannot reach huggingface.co.' });
    expect(e).toEqual({ code: 'upstream', message: 'the download failed: cannot reach huggingface.co.' });
  });

  it('turns an unknown shape into an internal error with its text', () => {
    expect(toCommandError('boom')).toEqual({ code: 'internal', message: 'boom' });
  });
});
