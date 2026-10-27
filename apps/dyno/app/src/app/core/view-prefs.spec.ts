import { TestBed } from '@angular/core/testing';
import { ViewPrefs } from './view-prefs';

describe('ViewPrefs', () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => {
    vi.restoreAllMocks();
    localStorage.clear();
  });

  it('remembers the CPU view across instances', () => {
    expect(TestBed.inject(ViewPrefs).cpuView()).toBe('overall');
    TestBed.inject(ViewPrefs).setCpuView('cores');
    expect(localStorage.getItem('dyno.cpuView')).toBe('cores');
    TestBed.resetTestingModule();
    expect(TestBed.inject(ViewPrefs).cpuView()).toBe('cores');
  });

  it('works when storage is blocked', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    const p = TestBed.inject(ViewPrefs);
    expect(p.cpuView()).toBe('overall');
    p.setCpuView('cores');
    expect(p.cpuView()).toBe('cores');
  });

  it('remembers what the startup manager hides, and tolerates damage and blocked storage', () => {
    expect(TestBed.inject(ViewPrefs).startupFilter()).toEqual({ hideOs: false, hideSigned: false });
    TestBed.inject(ViewPrefs).setStartupFilter({ hideOs: true, hideSigned: false });
    TestBed.resetTestingModule();
    expect(TestBed.inject(ViewPrefs).startupFilter()).toEqual({ hideOs: true, hideSigned: false });
    TestBed.resetTestingModule();
    localStorage.setItem('dyno.startupFilter', '{nope');
    expect(TestBed.inject(ViewPrefs).startupFilter()).toEqual({ hideOs: false, hideSigned: false });
    TestBed.resetTestingModule();
    localStorage.setItem('dyno.startupFilter', '{"hideOs":"yes","hideSigned":true}');
    expect(TestBed.inject(ViewPrefs).startupFilter()).toEqual({ hideOs: false, hideSigned: true });
    TestBed.resetTestingModule();
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    const p = TestBed.inject(ViewPrefs);
    p.setStartupFilter({ hideOs: true, hideSigned: true });
    expect(p.startupFilter()).toEqual({ hideOs: true, hideSigned: true });
  });
});
