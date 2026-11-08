import { createBackend, mockOptionsFrom, resetBrowserBackend } from './backend-factory';

describe('mockOptionsFrom', () => {
  it('defaults to simulated latency, live ticks and streaming delay', () => {
    expect(mockOptionsFrom('')).toEqual({ latencyMs: 120, autoTick: true, streamDelayMs: 28, empty: false, askConsent: false, serverDown: false, startFailed: false, modelUpdates: false, crashed: false, installing: false, noLeftovers: false, quitAsk: false, chat: true, failing: [] });
  });

  it('mock-fast removes the delays; mock-static and mock-empty stop the ticks and start empty', () => {
    expect(mockOptionsFrom('?mock-fast')).toMatchObject({ latencyMs: 0, streamDelayMs: 0 });
    expect(mockOptionsFrom('?mock-static&mock-empty')).toMatchObject({ autoTick: false, empty: true });
  });

  it('switches on the demo states by name', () => {
    expect(mockOptionsFrom('?mock-consent&mock-updates&mock-down&mock-startfailed&mock-crash')).toMatchObject({ askConsent: true, modelUpdates: true, serverDown: true, startFailed: true, crashed: true });
    expect(mockOptionsFrom('?mock-install&mock-quit&mock-clean&mock-nochat')).toMatchObject({ installing: true, quitAsk: true, noLeftovers: true, chat: false });
  });

  it('mock-fail lists the calls that fail', () => {
    expect(mockOptionsFrom('?mock-fail=overview,history').failing).toEqual(['overview', 'history']);
    expect(mockOptionsFrom('?mock-fail=').failing).toEqual([]);
  });
});

describe('createBackend', () => {
  afterEach(() => resetBrowserBackend());

  it('gives the browser build one mock per page, so the window and its in-page dialogs share it', async () => {
    const a = createBackend('?mock-fast&mock-static');
    expect(createBackend('?mock-empty')).toBe(a);
    expect((await a.apps()).length).toBeGreaterThan(0);
    resetBrowserBackend();
    expect(createBackend('?mock-fast&mock-static&mock-empty')).not.toBe(a);
  });
});
