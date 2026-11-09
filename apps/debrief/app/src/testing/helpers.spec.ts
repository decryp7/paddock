import { settleUntil } from './helpers';

describe('settleUntil', () => {
  it('returns once the condition holds, however many turns that takes', async () => {
    let ready = false;
    setTimeout(() => (ready = true), 30);
    await settleUntil(undefined, () => expect(ready).toBe(true));
    expect(ready).toBe(true);
  });

  it('fails with the condition’s own error when it never holds', async () => {
    await expect(settleUntil(undefined, () => expect('never').toBe('ready'), 50)).rejects.toThrow(/expected 'never' to be 'ready'/);
  });
});
