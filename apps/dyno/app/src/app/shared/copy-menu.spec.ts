import { TestBed } from '@angular/core/testing';
import { CopyMenu, CopyMenuHost } from './copy-menu';

describe('CopyMenu service', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('opens at a position with its text and closes', () => {
    const menu = TestBed.inject(CopyMenu);
    expect(menu.state()).toBeNull();
    menu.open(10, 20, 'hello');
    expect(menu.state()).toEqual({ x: 10, y: 20, text: 'hello' });
    menu.close();
    expect(menu.state()).toBeNull();
  });
  it('copy writes the text to the clipboard and closes', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    const menu = TestBed.inject(CopyMenu);
    menu.open(0, 0, 'selected text');
    await menu.copy();
    expect(writeText).toHaveBeenCalledWith('selected text');
    expect(menu.state()).toBeNull();
  });
  it('copy with nothing open does nothing', async () => {
    const writeText = vi.fn();
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    await TestBed.inject(CopyMenu).copy();
    expect(writeText).not.toHaveBeenCalled();
  });
  it('a failing or missing clipboard only closes the menu', async () => {
    vi.stubGlobal('navigator', { clipboard: { writeText: vi.fn().mockRejectedValue(new Error('denied')) } });
    const menu = TestBed.inject(CopyMenu);
    menu.open(0, 0, 'x');
    await expect(menu.copy()).resolves.toBeUndefined();
    expect(menu.state()).toBeNull();
    vi.stubGlobal('navigator', {});
    menu.open(0, 0, 'x');
    await expect(menu.copy()).resolves.toBeUndefined();
    expect(menu.state()).toBeNull();
  });
});

describe('CopyMenuHost', () => {
  function host() {
    const f = TestBed.createComponent(CopyMenuHost);
    f.detectChanges();
    return { f, menu: TestBed.inject(CopyMenu), el: f.nativeElement as HTMLElement };
  }

  it('renders nothing until opened, then a Copy item at the pointer', async () => {
    const { f, menu, el } = host();
    expect(el.querySelector('[role="menu"]')).toBeNull();
    menu.open(30, 40, 'abc');
    f.detectChanges();
    const m = el.querySelector<HTMLElement>('[role="menu"]')!;
    expect(m.style.left).toBe('30px');
    expect(m.style.top).toBe('40px');
    expect(el.querySelector('[role="menuitem"]')?.textContent).toBe('Copy');
  });
  it('focuses the item when it opens', async () => {
    const { f, menu, el } = host();
    menu.open(0, 0, 'abc');
    f.detectChanges();
    await f.whenStable();
    expect(document.activeElement).toBe(el.querySelector('button'));
  });
  it('clicking Copy copies and closes', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    const { f, menu, el } = host();
    menu.open(0, 0, 'picked');
    f.detectChanges();
    el.querySelector('button')!.click();
    await f.whenStable();
    expect(writeText).toHaveBeenCalledWith('picked');
    expect(menu.state()).toBeNull();
    vi.unstubAllGlobals();
  });
  it('Escape closes it', () => {
    const { f, menu } = host();
    menu.open(0, 0, 'x');
    f.detectChanges();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(menu.state()).toBeNull();
  });
  it('a mouse press elsewhere closes it, one inside does not', () => {
    const { f, menu } = host();
    menu.open(0, 0, 'x');
    f.detectChanges();
    const inside = document.createElement('dy-copy-menu');
    document.body.append(inside);
    inside.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    expect(menu.state()).not.toBeNull();
    inside.remove();
    document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    expect(menu.state()).toBeNull();
  });
});
