import { routes } from './app.routes';

describe('routes', () => {
  it('opens the compact dashboard for the tray dropdown and for every menu-bar item popover (regression: #/popover/<id> fell through to the main window)', () => {
    const paths = routes.map((r) => r.path);
    expect(paths).toContain('tray');
    expect(paths).toContain('popover/:id');
    expect(routes.find((r) => r.path === 'popover/:id')?.loadComponent).toBeTypeOf('function');
  });
});
