import { PAGE_ROUTE, SETTINGS_PAGE_ID, destination } from './navigate';
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
