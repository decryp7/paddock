import { TestBed } from '@angular/core/testing';
import { DOCUMENT } from '@angular/core';
import { PlatformService, readPlatform } from './platform';

describe('readPlatform', () => {
  it('reads known platforms and vibrancy', () => {
    expect(readPlatform({ platform: 'windows', vibrancy: 'mica' } as DOMStringMap)).toEqual({ platform: 'windows', vibrancy: 'mica' });
    expect(readPlatform({ platform: 'macos', vibrancy: 'vibrancy' } as DOMStringMap)).toEqual({ platform: 'macos', vibrancy: 'vibrancy' });
    expect(readPlatform({ platform: 'linux' } as DOMStringMap)).toEqual({ platform: 'linux', vibrancy: 'none' });
  });
  it('falls back to browser / none', () => {
    expect(readPlatform({})).toEqual({ platform: 'browser', vibrancy: 'none' });
    expect(readPlatform({ platform: 'beos', vibrancy: 'glass' } as DOMStringMap)).toEqual({ platform: 'browser', vibrancy: 'none' });
  });
});

describe('PlatformService', () => {
  function make(dataset: Record<string, string>) {
    TestBed.configureTestingModule({ providers: [{ provide: DOCUMENT, useValue: { documentElement: { dataset } } }] });
    return TestBed.inject(PlatformService);
  }
  it('uses the command key and overlay title bar on macOS', () => {
    const p = make({ platform: 'macos' });
    expect(p.mod).toBe('⌘');
    expect(p.overlayTitleBar).toBe(true);
  });
  it('uses Ctrl elsewhere', () => {
    const p = make({});
    expect(p.platform).toBe('browser');
    expect(p.mod).toBe('Ctrl');
    expect(p.overlayTitleBar).toBe(false);
  });
});
