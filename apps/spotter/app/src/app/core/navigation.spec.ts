import { isShownRoute, listenForRoutes } from './navigation';
import { MockBackend } from './mock-backend';

describe('routes the tray asks the main window for', () => {
  it('accepts the dashboard, the watches and one watch, and nothing else', () => {
    for (const ok of ['/', '/watches', '/watches/new', '/watches/w-3', '/watches/w-3/edit']) expect(isShownRoute(ok), ok).toBe(true);
    for (const bad of ['/settings', '//evil', '/watches/../x', 'https://x.example', '/watches/a b', `/watches/${'a'.repeat(300)}`]) expect(isShownRoute(bad), bad).toBe(false);
  });

  it('shows a route the tray sends while the window is open, and ignores an unknown one', async () => {
    const backend = new MockBackend();
    const shown: string[] = [];
    const stop = await listenForRoutes(backend, (r) => shown.push(r));
    backend.navigate('/watches/w-3');
    backend.navigate('/settings');
    expect(shown).toEqual(['/watches/w-3']);
    stop();
    backend.navigate('/watches');
    expect(shown).toEqual(['/watches/w-3']);
  });

  it('shows the route that was asked for before the window could listen, once', async () => {
    const backend = new MockBackend();
    backend.pendingRoute = () => Promise.resolve('/watches/w-7');
    const shown: string[] = [];
    await listenForRoutes(backend, (r) => shown.push(r));
    expect(shown).toEqual(['/watches/w-7']);
  });
});
