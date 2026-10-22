import { TestBed } from '@angular/core/testing';
import { ViewPrefs } from './view-prefs';

describe('ViewPrefs', () => {
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
});
