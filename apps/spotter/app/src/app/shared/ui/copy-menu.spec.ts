import { TestBed } from '@angular/core/testing';
import { CopyMenu, CopyMenuHost } from './copy-menu';

describe('the Copy menu', () => {
  afterEach(() => vi.restoreAllMocks());

  it('opens at the pointer, copies and closes; an unavailable clipboard just closes', async () => {
    const menu = TestBed.inject(CopyMenu);
    await menu.copy(); // nothing open
    const writeText = vi.fn().mockRejectedValue(new Error('denied'));
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    menu.open(1, 2, 'abc');
    await menu.copy();
    expect(writeText).toHaveBeenCalledWith('abc');
    expect(menu.state()).toBeNull();
  });

  it('renders the item, focuses it and closes on Escape or an outside click, not on a click inside', async () => {
    const fixture = TestBed.createComponent(CopyMenuHost);
    const menu = TestBed.inject(CopyMenu);
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('button')).toBeNull();
    menu.open(10, 20, 'x');
    fixture.detectChanges();
    await fixture.whenStable();
    const button = fixture.nativeElement.querySelector('button') as HTMLButtonElement;
    expect(button.textContent).toContain('Copy');
    expect(fixture.nativeElement.querySelector('.menu').style.left).toBe('10px');
    const inside = document.createElement('sp-copy-menu');
    inside.appendChild(document.createElement('span'));
    document.body.appendChild(inside);
    inside.firstElementChild!.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    inside.remove();
    expect(menu.state()).not.toBeNull();
    document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    expect(menu.state()).toBeNull();
    menu.open(1, 1, 'y');
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(menu.state()).toBeNull();
    menu.open(1, 1, 'z');
    fixture.detectChanges();
    (fixture.nativeElement.querySelector('button') as HTMLButtonElement).dispatchEvent(new Event('blur'));
    expect(menu.state()).toBeNull();
  });
});
