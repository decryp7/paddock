import { TestBed } from '@angular/core/testing';
import { BACKEND } from './core/backend';
import { MockBackend } from './core/mock-backend';
import { appConfig } from './app.config';
import { routes } from './app.routes';

describe('appConfig', () => {
  afterEach(() => window.history.replaceState(null, '', '/'));

  async function backendFor(search: string) {
    window.history.replaceState(null, '', `/${search}`);
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: appConfig.providers });
    const backend = TestBed.inject(BACKEND);
    expect(backend).toBeInstanceOf(MockBackend);
    return backend;
  }

  it('uses the demo mock in the browser build', async () => {
    expect(await (await backendFor('')).listWatches()).toHaveLength(8);
  });

  it('lets ?mock= pick the scenario and ignores unknown ones', async () => {
    expect(await (await backendFor('?mock=empty')).listWatches()).toEqual([]);
    await expect((await backendFor('?mock=broken')).listWatches()).rejects.toMatchObject({ code: 'unavailable' });
    expect(await (await backendFor('?mock=bogus')).listWatches()).toHaveLength(8);
  });

  it('sets the window chrome at start-up without touching the backend', async () => {
    await backendFor('');
    expect(document.documentElement.dataset['platform']).toBeDefined();
  });
});

describe('routes', () => {
  it('lazy loads every page and falls back to the dashboard', async () => {
    const paths = routes.map((r) => r.path);
    expect(paths).toEqual(['', 'watches', 'watches/new', 'watches/:id/edit', 'watches/:id', 'settings', 'tray', '**']);
    for (const r of routes.filter((x) => x.loadComponent)) expect(await r.loadComponent!()).toBeTruthy();
    expect(routes.at(-1)).toMatchObject({ redirectTo: '' });
    expect(routes.every((r) => !r.title || String(r.title).endsWith('Spotter'))).toBe(true);
  });

  it('puts the specific watch routes before the id route', () => {
    const paths = routes.map((r) => r.path);
    expect(paths.indexOf('watches/new')).toBeLessThan(paths.indexOf('watches/:id'));
  });
});
