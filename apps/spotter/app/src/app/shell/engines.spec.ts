import { TestBed } from '@angular/core/testing';
import { BrowserEngines, diagnosticsBackend } from './engines';

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

  it('serves Report a problem in the browser with Spotter’s files and the Logging section', async () => {
    const preview = await diagnosticsBackend().preview(null, 'The list is empty');
    expect(preview.preview.files.map((f) => f.kind)).toEqual(expect.arrayContaining(['system-info', 'settings', 'log']));
    expect(JSON.stringify(preview)).not.toMatch(/shell demo/i);
    const engines = TestBed.runInInjectionContext(() => new BrowserEngines());
    const sections = (await engines.settings.overview()).sections.map((s) => s.id);
    expect(sections).toContain('pd.logging');
  });
});
