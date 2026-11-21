import { toCommandError, toItemError } from './command-error';

describe('toCommandError', () => {
  it('keeps an upstream error with the host it names', () => {
    const e = toCommandError({ code: 'upstream', message: 'the download failed: cannot reach huggingface.co.' });
    expect(e).toEqual({ code: 'upstream', message: 'the download failed: cannot reach huggingface.co.' });
  });

  it('turns an unknown shape into an internal error with its text', () => {
    expect(toCommandError('boom')).toEqual({ code: 'internal', message: 'boom' });
  });
});

describe('toItemError', () => {
  it('keeps the message and says what the code stands for, with the code default retry', () => {
    expect(toItemError({ code: 'busy', message: 'The server is busy.' })).toEqual({ code: 'busy', message: 'The server is busy.', retryable: true });
    expect(toItemError({ code: 'serverUnavailable', message: 'Pitwall is not running.' })).toEqual({ code: 'unavailable', message: 'Pitwall is not running.', retryable: true });
    expect(toItemError({ code: 'badRequest', message: 'Too many ids.' })).toEqual({ code: 'invalid', message: 'Too many ids.', retryable: false });
    expect(toItemError({ code: 'forbidden', message: 'Not allowed.' })).toMatchObject({ code: 'denied', retryable: false });
  });

  it('turns an unknown shape into an internal item error with its text', () => {
    expect(toItemError('boom')).toEqual({ code: 'internal', message: 'boom', retryable: false });
  });
});
