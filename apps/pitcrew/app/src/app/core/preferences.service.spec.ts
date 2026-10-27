import { TestBed } from '@angular/core/testing';
import { PreferencesService } from './preferences.service';

describe('PreferencesService', () => {
  beforeEach(() => {
    localStorage.clear();
    TestBed.resetTestingModule();
  });

  it('shows every kind by default', () => {
    expect(TestBed.inject(PreferencesService).isEnabled('stalled')).toBe(true);
  });

  it('mutes and unmutes a kind and persists the choice', () => {
    const prefs = TestBed.inject(PreferencesService);
    prefs.setEnabled('stalled', false);
    expect(prefs.isEnabled('stalled')).toBe(false);
    expect(prefs.isEnabled('updated')).toBe(true);
    expect(JSON.parse(localStorage.getItem('pitcrew.notifications') ?? '[]')).toEqual(['stalled']);
    prefs.setEnabled('stalled', true);
    expect(prefs.isEnabled('stalled')).toBe(true);
  });

  it('restores muted kinds from storage', () => {
    localStorage.setItem('pitcrew.notifications', '["diskLow"]');
    expect(TestBed.inject(PreferencesService).isEnabled('diskLow')).toBe(false);
  });

  it('ignores damaged storage', () => {
    localStorage.setItem('pitcrew.notifications', '[1]');
    expect(TestBed.inject(PreferencesService).isEnabled('diskLow')).toBe(true);
  });
});
