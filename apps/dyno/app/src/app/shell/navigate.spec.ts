import { MockBackend } from '../core/mock-backend';
import { PAGE_ROUTE, SETTINGS_PAGE_ID, destination, followNavigation } from './navigate';
import { NAV } from './nav';

describe('destination', () => {
  it('sends every page to a route the main window has', () => {
    const paths = NAV.map((n) => n.path);
    for (const route of Object.values(PAGE_ROUTE)) expect(paths).toContain(route);
    expect(destination({ page: 'backups' })).toEqual({ route: '/backup' });
  });
  it('opens Dyno\'s page of Settings for a settings section', () => {
    expect(destination({ settings: 'popupWidgets' })).toEqual({ settings: SETTINGS_PAGE_ID });
    expect(destination({ settings: 'general' })).toEqual({ settings: SETTINGS_PAGE_ID });
  });
});

describe('followNavigation', () => {
  const setup = () => {
    const mock = new MockBackend({ latencyMs: 0 });
    const routes: string[] = [];
    const pages: string[] = [];
    const stop = followNavigation(mock, { route: (u) => routes.push(u), settings: (p) => pages.push(p) });
    return { mock, routes, pages, stop };
  };

  it('opens what was waiting when the window started, once', async () => {
    const mock = new MockBackend({ latencyMs: 0 });
    mock.navigate({ page: 'alarms' });
    const routes: string[] = [];
    followNavigation(mock, { route: (u) => routes.push(u), settings: null });
    await vi.waitFor(() => expect(routes).toEqual(['/alarms']));
    expect(await mock.takeNavigation()).toBeNull();
  });

  it('follows each event: a page by route, Settings by its page', async () => {
    const { mock, routes, pages } = setup();
    mock.navigate({ page: 'disk' });
    await vi.waitFor(() => expect(routes).toEqual(['/disk-map']));
    mock.navigate({ settings: 'popupWidgets' });
    await vi.waitFor(() => expect(pages).toEqual([SETTINGS_PAGE_ID]));
  });

  it('does nothing when nothing is waiting, and stops when told to', async () => {
    const { mock, routes, stop } = setup();
    await mock.takeNavigation();
    stop();
    mock.navigate({ page: 'issues' });
    await mock.takeNavigation();
    expect(routes).toEqual([]);
  });
});
