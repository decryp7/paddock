import { appConfig, mockOptionsFrom } from './app.config';

describe('mockOptionsFrom', () => {
  it('defaults to simulated latency, live ticks and streaming delay', () => {
    expect(mockOptionsFrom('')).toEqual({ latencyMs: 120, autoTick: true, streamDelayMs: 28, empty: false, askConsent: false, serverDown: false, startFailed: false, updateAvailable: false, modelUpdates: false, crashed: false, installing: false, noLeftovers: false, quitAsk: false, failing: [] });
  });

  it('mock-fast removes the delays', () => {
    const o = mockOptionsFrom('?mock-fast');
    expect(o.latencyMs).toBe(0);
    expect(o.streamDelayMs).toBe(0);
  });

  it('mock-static stops live updates and mock-empty starts empty', () => {
    const o = mockOptionsFrom('?mock-static&mock-empty');
    expect(o.autoTick).toBe(false);
    expect(o.empty).toBe(true);
  });

  it('mock-updates offers model updates', () => {
    expect(mockOptionsFrom('?mock-updates').modelUpdates).toBe(true);
  });

  it('mock-consent and mock-update switch on the first-run question and an available update', () => {
    const o = mockOptionsFrom('?mock-consent&mock-update');
    expect(o.askConsent).toBe(true);
    expect(mockOptionsFrom('?mock-down&mock-startfailed')).toMatchObject({ serverDown: true, startFailed: true });
    expect(o.updateAvailable).toBe(true);
  });

  it('mock-install and mock-quit show the llama.cpp install and the quit question', () => {
    const o = mockOptionsFrom('?mock-install&mock-quit&mock-clean');
    expect(o.installing).toBe(true);
    expect(o.quitAsk).toBe(true);
    expect(o.noLeftovers).toBe(true);
  });

  it('mock-fail lists the calls that fail', () => {
    expect(mockOptionsFrom('?mock-fail=overview,history').failing).toEqual(['overview', 'history']);
    expect(mockOptionsFrom('?mock-fail=').failing).toEqual([]);
  });
});

describe('appConfig', () => {
  it('provides the router, icons, native feel and the backend', () => {
    expect(appConfig.providers.length).toBeGreaterThanOrEqual(5);
  });
});
