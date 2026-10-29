import { TestBed } from '@angular/core/testing';
import { BrowserEngines } from './engines';

describe('the browser build’s engines', () => {
  it('breaks the connection the way Rust reports it: down, with Check now as its Retry repair', async () => {
    const engines = TestBed.runInInjectionContext(() => new BrowserEngines());
    engines.breakSources();
    const snapshot = await engines.health.snapshot();
    const sources = snapshot.services.find((s) => s.service === 'sp.sources');
    expect(sources).toMatchObject({ state: 'down', repair: 'retry' });
    // The wording is the user's, never the mock's own.
    expect(JSON.stringify(sources)).not.toMatch(/mock/i);
  });
});
