import { rememberSettingsAnchor, takeSettingsAnchor } from './settings-anchor';

describe('settings anchor', () => {
  it('hands a section over once', () => {
    rememberSettingsAnchor('popup-widgets');
    expect(takeSettingsAnchor()).toBe('popup-widgets');
    expect(takeSettingsAnchor()).toBeNull();
  });
  it('forgets one when asked for the top', () => {
    rememberSettingsAnchor('popup-widgets');
    rememberSettingsAnchor(null);
    expect(takeSettingsAnchor()).toBeNull();
  });
});
