import { TestBed } from '@angular/core/testing';
import { BrowserWindowControl, WINDOW_CONTROL } from './window-control';

describe('BrowserWindowControl', () => {
  it('navigates to the route as a hash', async () => {
    const nav = vi.fn();
    await new BrowserWindowControl(nav).openMain('/models');
    expect(nav).toHaveBeenCalledWith('#/models');
  });

  it('defaults to the root route', async () => {
    const nav = vi.fn();
    await new BrowserWindowControl(nav).openMain();
    expect(nav).toHaveBeenCalledWith('#/');
  });

  it('sets location.hash by default', async () => {
    const before = location.hash;
    await new BrowserWindowControl().openMain('/apps');
    expect(location.hash).toBe('#/apps');
    location.hash = before;
  });

  it('has nothing to listen to and returns a harmless stop function', () => {
    const stop = new BrowserWindowControl(vi.fn()).onNavigate();
    expect(() => stop()).not.toThrow();
  });

  it('is what the injection token provides outside Tauri', () => {
    expect(TestBed.inject(WINDOW_CONTROL)).toBeInstanceOf(BrowserWindowControl);
  });
});
