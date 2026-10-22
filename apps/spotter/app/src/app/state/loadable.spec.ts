import { idle, pending, settle } from './loadable';

describe('loadable', () => {
  it('idle has nothing', () => {
    expect(idle<number>()).toEqual({ value: null, loading: false, error: null });
  });

  it('pending keeps the last good value', () => {
    expect(pending<number>(undefined)).toEqual({ value: null, loading: true, error: null });
    expect(pending({ value: 3, loading: false, error: { code: 'unavailable', message: 'x', problems: [] } })).toEqual({ value: 3, loading: true, error: null });
  });

  it('settle returns the value', async () => {
    expect(await settle(undefined, async () => 5)).toEqual({ value: 5, loading: false, error: null });
  });

  it('settle keeps the previous value next to the error', async () => {
    const prev = { value: 1, loading: false, error: null };
    const out = await settle<number>(prev, async () => {
      throw { code: 'unavailable', message: 'later', problems: [] };
    });
    expect(out).toEqual({ value: 1, loading: false, error: { code: 'unavailable', message: 'later', problems: [] } });
    const none = await settle<number>(undefined, () => Promise.reject(new Error('boom')));
    expect(none).toEqual({ value: null, loading: false, error: { code: 'internal', message: 'boom', problems: [] } });
  });
});
