import { TestBed } from '@angular/core/testing';
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
});
