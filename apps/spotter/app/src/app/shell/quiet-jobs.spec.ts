import { isQuietSuccess } from './quiet-jobs';

describe('quiet jobs', () => {
  it('does not announce the success of a source test or a check', () => {
    for (const message of ['Testing the source: done', 'Checking Lumen CLI: done', 'Checking every watch: done']) {
      expect(isQuietSuccess({ level: 'success', message }), message).toBe(true);
    }
  });

  it('announces a failure, and the success of any other job', () => {
    expect(isQuietSuccess({ level: 'error', message: 'Testing the source failed: the share is offline' })).toBe(false);
    expect(isQuietSuccess({ level: 'success', message: 'Downloading Lumen CLI: done' })).toBe(false);
    expect(isQuietSuccess({ level: 'success', message: 'Installing the browser: done' })).toBe(false);
  });
});
