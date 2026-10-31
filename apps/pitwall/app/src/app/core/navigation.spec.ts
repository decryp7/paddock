import { SETTINGS_GENERAL_ROUTE, SETTINGS_TRAY_POPUP_ROUTE, settingsPageOf } from './navigation';

describe('the routes the tray sends the main window to', () => {
  it('maps the two Settings routes to the ids of Pitwall\'s Settings pages and nothing else', () => {
    expect(settingsPageOf(SETTINGS_GENERAL_ROUTE)).toBe('pw.general');
    expect(settingsPageOf(SETTINGS_TRAY_POPUP_ROUTE)).toBe('pw.tray');
    for (const other of ['/', '/models', '/settings/other', '/settings/', '/Settings', '/settings/tray-popup/x', '']) expect(settingsPageOf(other), other).toBeNull();
  });

  it('are plain routes the pending-route slot accepts', () => {
    for (const r of [SETTINGS_GENERAL_ROUTE, SETTINGS_TRAY_POPUP_ROUTE]) expect(r).toMatch(/^\/[a-z/-]+$/);
  });
});
