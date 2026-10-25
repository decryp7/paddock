import { clampToViewport, hideUntilPlaced, isAtOrigin, placeMenu } from './menu-placement';

describe('menu placement', () => {
  it('keeps an element hidden until it is revealed, and restores what it had', () => {
    const el = document.createElement('div');
    el.style.visibility = 'visible';
    const reveal = hideUntilPlaced(el);
    expect(el.style.visibility).toBe('hidden');
    reveal();
    expect(el.style.visibility).toBe('visible');
    const plain = document.createElement('div');
    hideUntilPlaced(plain)();
    expect(plain.style.visibility).toBe('');
  });

  it('knows the origin, where an unpositioned overlay is painted', () => {
    expect(isAtOrigin({ left: 0, top: 0 })).toBe(true);
    expect(isAtOrigin({ left: 0, top: 12 })).toBe(false);
    expect(isAtOrigin({ left: 5, top: 0 })).toBe(false);
  });

  it('moves a menu just far enough to stay in the window', () => {
    const vp = { width: 400, height: 300 };
    expect(clampToViewport(50, 60, 100, 80, vp)).toEqual({ x: 50, y: 60 });
    expect(clampToViewport(380, 290, 100, 80, vp)).toEqual({ x: 296, y: 216 });
    expect(clampToViewport(-20, -5, 100, 80, vp)).toEqual({ x: 4, y: 4 });
    expect(clampToViewport(10, 10, 900, 900, vp)).toEqual({ x: 4, y: 4 });
  });

  it('places a menu where it was asked to open, inside the window, and marks it placed', () => {
    const menu = document.createElement('ul');
    menu.getBoundingClientRect = () => ({ width: 120, height: 90 }) as DOMRect;
    placeMenu(menu, 390, 20, { width: 400, height: 300 });
    expect([menu.style.left, menu.style.top, 'placed' in menu.dataset]).toEqual(['276px', '20px', true]);
  });
});
