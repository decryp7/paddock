import { TestBed } from '@angular/core/testing';
import { ContextMenuHost } from './context-menu-host';
import { ContextMenuService } from './context-menu.service';

describe('ContextMenuService', () => {
  let p: HTMLElement;
  let other: HTMLElement;
  const select = (el: Element) => {
    const r = document.createRange();
    r.selectNodeContents(el);
    getSelection()!.removeAllRanges();
    getSelection()!.addRange(r);
  };

  beforeEach(() => {
    p = document.createElement('p');
    p.textContent = 'keep me selected';
    other = document.createElement('p');
    other.textContent = 'something else';
    document.body.append(p, other);
  });
  afterEach(() => {
    getSelection()?.removeAllRanges();
    p.remove();
    other.remove();
  });

  it('gives back the selection it opened on when it closes, as a native menu keeps it (WebKit drops it on focus)', () => {
    const menus = TestBed.inject(ContextMenuService);
    select(p);
    menus.open(1, 2, [{ label: 'Copy', run: () => undefined }], null);
    getSelection()!.removeAllRanges(); // what WebKit does when the menu takes the focus
    menus.close();
    expect(getSelection()!.toString()).toBe('keep me selected');

    select(p);
    menus.open(1, 2, [{ label: 'Copy', run: () => undefined }], null);
    getSelection()!.removeAllRanges();
    menus.close(false); // an item ran
    expect(getSelection()!.toString()).toBe('keep me selected');
  });

  it('keeps a selection the user made while it was open, and restores nothing it never saw', () => {
    const menus = TestBed.inject(ContextMenuService);
    select(p);
    menus.open(1, 2, [{ label: 'Copy', run: () => undefined }], null);
    select(other);
    menus.close();
    expect(getSelection()!.toString()).toBe('something else');

    getSelection()!.removeAllRanges();
    menus.open(1, 2, [{ label: 'Copy', run: () => undefined }], null);
    menus.close();
    expect(getSelection()!.toString()).toBe('');
    menus.close(); // closing a closed menu does nothing
    expect(menus.menu()).toBeNull();
  });
  it('never takes the caret from a text field (also in a shadow root), saves no caret, and restores past a checkbox', () => {
    const menus = TestBed.inject(ContextMenuService);
    const input = document.createElement('input');
    document.body.append(input);
    try {
      // The menu was opened from a field (its returnFocus): closing it gives the field its focus and caret back, untouched.
      select(p);
      menus.open(1, 2, [{ label: 'Copy', run: () => undefined }], input);
      getSelection()!.removeAllRanges();
      menus.close();
      expect(document.activeElement).toBe(input);
      expect(getSelection()!.toString()).toBe('');

      // A caret is not a selection to give back.
      input.blur();
      getSelection()!.collapse(p.firstChild!, 3);
      menus.open(1, 2, [{ label: 'Copy', run: () => undefined }], null);
      expect(menus.menu()!.selection).toEqual([]);
      menus.close();

      // Asked to leave the selection alone, it does.
      select(p);
      menus.open(1, 2, [{ label: 'Copy', run: () => undefined }], null);
      getSelection()!.removeAllRanges();
      menus.close(true, false);
      expect(getSelection()!.toString()).toBe('');

      // A checkbox has the focus: it owns no caret, so the selection comes back.
      const box = document.createElement('input');
      box.type = 'checkbox';
      document.body.append(box);
      select(p);
      menus.open(1, 2, [{ label: 'Copy', run: () => undefined }], box);
      getSelection()!.removeAllRanges();
      menus.close();
      expect(getSelection()!.toString()).toBe('keep me selected');
      box.remove();

      // A text field inside a shadow root keeps its caret.
      const shadowHost = document.createElement('div');
      document.body.append(shadowHost);
      const inner = document.createElement('input');
      shadowHost.attachShadow({ mode: 'open' }).append(inner);
      select(p);
      menus.open(1, 2, [{ label: 'Copy', run: () => undefined }], null);
      inner.focus();
      getSelection()!.removeAllRanges();
      menus.close(false);
      expect(getSelection()!.toString()).toBe('');
      shadowHost.remove();
    } finally {
      input.remove();
    }
  });
  it('a press on the backdrop closes the menu and gives the selection back: the backdrop swallowed it, nothing else changed', async () => {
    const menus = TestBed.inject(ContextMenuService);
    const fixture = TestBed.createComponent(ContextMenuHost);
    select(p);
    menus.open(1, 2, [{ label: 'Copy', run: () => undefined }], null);
    await fixture.whenStable();
    const backdrop = fixture.nativeElement.querySelector('.backdrop') as HTMLElement;
    getSelection()!.removeAllRanges(); // WebKit, as the menu took the focus
    backdrop.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    expect(menus.menu()).toBeNull();
    expect(getSelection()!.toString()).toBe('keep me selected');
  });
});
