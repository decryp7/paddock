import { causeOf } from './browser-engine';

describe('causeOf (mirrors health::cause_of in Rust)', () => {
  const lost = { state: 'unavailable', message: 'not running' } as const;

  it('says a service that was never installed is not installed, and names the OS prompt', () => {
    const c = causeOf(lost, false)!;
    expect(c.code).toBe('pc.service.not-installed');
    expect(c.message).toContain('not installed');
    expect(c.hint).toMatch(/UAC|System Settings/);
  });

  it('keeps an installed service that does not answer as unreachable, and a mismatch as its own cause', () => {
    expect(causeOf(lost, true)!.code).toBe('pc.service.unavailable');
    expect(causeOf({ state: 'mismatch', message: 'x' }, true)!.code).toBe('pc.service.mismatch');
  });

  it('is no outage while connecting or connected', () => {
    expect(causeOf({ state: 'connecting' }, true)).toBeNull();
    expect(causeOf({ state: 'connected', serviceVersion: '1' }, true)).toBeNull();
  });
});
