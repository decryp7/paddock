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
  it('a press on Copy keeps the focus where it is, so WebKit does not blur the item and close the menu before the click', async () => {
    const fixture = TestBed.createComponent(CopyMenuHost);
    const menu = TestBed.inject(CopyMenu);
    fixture.detectChanges();
    menu.open(1, 2, 'x');
    await fixture.whenStable();
    const button = fixture.nativeElement.querySelector('button') as HTMLButtonElement;
    const press = new MouseEvent('mousedown', { bubbles: true, cancelable: true });
    button.dispatchEvent(press);
    expect(press.defaultPrevented).toBe(true);
    menu.close();
  });

  it('gives back the selection it opened on when it closes, as a native menu keeps it (WebKit drops it on focus)', async () => {
    const fixture = TestBed.createComponent(CopyMenuHost);
    const menu = TestBed.inject(CopyMenu);
    fixture.detectChanges();
    const p = document.createElement('p');
    p.textContent = 'keep me selected';
    const other = document.createElement('p');
    other.textContent = 'something else';
    document.body.append(p, other);
    const select = (el: Element) => {
      const r = document.createRange();
      r.selectNodeContents(el);
      getSelection()!.removeAllRanges();
      getSelection()!.addRange(r);
    };
    try {
      select(p);
      menu.open(1, 2, 'keep me selected');
      await fixture.whenStable();
      expect(fixture.nativeElement.querySelector('button')).not.toBeNull();
      getSelection()!.removeAllRanges(); // what WebKit does when the menu takes the focus
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
      await fixture.whenStable();
      expect(getSelection()!.toString()).toBe('keep me selected');

      // A selection the user made while it was open is theirs: it stays.
      menu.open(1, 2, 'keep me selected');
      await fixture.whenStable();
      select(other);
      menu.close();
      await fixture.whenStable();
      expect(getSelection()!.toString()).toBe('something else');
    } finally {
      getSelection()!.removeAllRanges();
      p.remove();
      other.remove();
    }
  });
  it('leaves the selection to a press elsewhere, and never takes the caret from a text field', async () => {
    const fixture = TestBed.createComponent(CopyMenuHost);
    const menu = TestBed.inject(CopyMenu);
    fixture.detectChanges();
    const p = document.createElement('p');
    p.textContent = 'keep me selected';
    const input = document.createElement('input');
    document.body.append(p, input);
    const r = document.createRange();
    r.selectNodeContents(p);
    try {
      getSelection()!.removeAllRanges();
      getSelection()!.addRange(r);
      menu.open(1, 2, 'keep me selected');
      await fixture.whenStable();
      getSelection()!.removeAllRanges(); // WebKit, as the item took the focus
      input.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })); // the user clicks into a field
      input.focus();
      await fixture.whenStable();
      expect(menu.state()).toBeNull();
      expect(document.activeElement).toBe(input);
      expect(getSelection()!.toString()).toBe('');

      // Closed by the app while a field has the focus: the field keeps its caret too.
      getSelection()!.removeAllRanges();
      getSelection()!.addRange(r);
      menu.open(1, 2, 'keep me selected');
      await fixture.whenStable();
      input.focus();
      getSelection()!.collapse(input, 0);
      menu.close();
      await fixture.whenStable();
      expect(document.activeElement).toBe(input);
      expect(getSelection()!.toString()).toBe('');
    } finally {
      getSelection()!.removeAllRanges();
      p.remove();
      input.remove();
    }
  });
});
