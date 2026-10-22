import { routes } from './app.routes';

describe('routes', () => {
  const lazy = routes.filter((r) => r.loadComponent);

  it('has a titled route for every screen, then a catch-all back to the dashboard', () => {
    expect(routes.map((r) => r.path)).toEqual(['', 'models', 'engines', 'apps', 'requests', 'playground', 'benchmarks', 'settings', 'tray', '**']);
    expect(routes.at(-1)).toMatchObject({ path: '**', redirectTo: '' });
    for (const r of lazy) expect(String(r.title)).toMatch(/Pitwall$/);
    expect(routes[0]).toMatchObject({ pathMatch: 'full' });
  });

  it.each(lazy.map((r) => [r.path || '(dashboard)', r] as const))('lazy-loads the %s component', async (_name, route) => {
    const component = await (route.loadComponent as () => Promise<unknown>)();
    expect(typeof component).toBe('function');
  });
});
